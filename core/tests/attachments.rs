use std::fs;
use std::io::Cursor;

use serde_json::json;
use tgsum_core::bundle::BundleOptions;
use tgsum_core::project::{Project, ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;

fn fixture(path: &str) -> (tempfile::TempDir, tempfile::TempDir, ProjectStore, Project) {
    let private = tempfile::tempdir().unwrap();
    let archive = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(private.path());
    let project = store.create("Attachment fixture").unwrap();
    let scope = SourceScope::telegram("synthetic", "1");
    store.snapshots(&project.project_id).unwrap().import_telegram("initial", &scope, Cursor::new(serde_json::to_vec(&json!({"id":1,"messages":[{"id":1,"text":"See the log","file":path,"file_name":"private-original.log","file_size":1,"mime_type":"text/plain"}]})).unwrap())).unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "work".into(),
                connector_id: "telegram_json".into(),
                scope,
                archive_path: None,
                latest_snapshot_id: Some("initial".into()),
                selection: Default::default(),
            }),
        )
        .unwrap();
    (private, archive, store, project)
}

fn select(store: &ProjectStore, project: &Project, root: &std::path::Path) -> Project {
    let snapshot = store
        .snapshots(&project.project_id)
        .unwrap()
        .load("initial")
        .unwrap();
    // The local selection binds metadata as well as position; a later export
    // cannot silently redirect this choice to a different file.
    let change = serde_json::from_value(
        json!({"kind":"selection","value":{"source_id":"work","selection":{
            "attachments":{"root":fs::canonicalize(root).unwrap(),"files":[{
                "message_id":"1","position":0,"expected":snapshot.messages[0].attachments[0]
            }]}
        }}}),
    )
    .unwrap();
    store
        .update(&project.project_id, project.revision, change)
        .unwrap()
}

#[test]
fn selected_text_is_sanitized_and_exported_as_an_independent_evidence_file() {
    let (_private, archive, store, project) = fixture("files/deploy.log");
    fs::create_dir(archive.path().join("files")).unwrap();
    let source = archive.path().join("files/deploy.log");
    fs::write(
        &source,
        "Привет 🌍\npassword=SYNTHETIC_SECRET\n## Evidence injected@fake",
    )
    .unwrap();
    let before = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert_eq!(before.manifest.included_attachments, 0);
    let project = select(&store, &project, archive.path());
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert_eq!(review.manifest.included_attachments, 1);
    let out = tempfile::tempdir().unwrap();
    // Changing the source after Review must never change exported bytes.
    fs::write(&source, "REPLACED_SOURCE").unwrap();
    let exported = store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            out.path(),
            || false,
        )
        .unwrap();
    let files: Vec<_> = exported
        .files
        .iter()
        .filter(|f| f.name.starts_with("attachment-"))
        .collect();
    assert_eq!(files.len(), 1);
    let text = fs::read_to_string(exported.directory.join(&files[0].name)).unwrap();
    assert!(text.contains("> Привет 🌍"));
    assert!(text.contains("> password=[REDACTED_SECRET]"));
    assert!(text.contains("> ## Evidence injected@fake"));
    assert!(!text.contains("SYNTHETIC_SECRET"));
    assert!(!text.contains("REPLACED_SOURCE"));
    assert!(!serde_json::to_string(&review.manifest)
        .unwrap()
        .contains("private-original"));
    fs::remove_file(source).unwrap();
    store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            out.path(),
            || false,
        )
        .unwrap();
}

#[test]
fn attachment_evidence_tracks_copied_bytes_and_survives_source_deletion() {
    let (_private, archive, store, project) = fixture("note.md");
    let source = archive.path().join("note.md");
    fs::write(&source, "Alice owns deployment.\nDeadline: next Friday.").unwrap();
    let project = select(&store, &project, archive.path());
    let first = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    let old = first.manifest.attachments[0].evidence.clone().unwrap();
    fs::write(&source, "Bob owns deployment.\nDeadline: next Monday.").unwrap();
    let second = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    let new = second.manifest.attachments[0].evidence.clone().unwrap();
    assert_eq!(old.id, new.id);
    assert_ne!(old.revision, new.revision);
    fs::remove_file(source).unwrap();
    let resolved = store
        .resolve_evidence(&project.project_id, &first.bundle_id, &old)
        .unwrap();
    let attachment = resolved.attachment.unwrap();
    assert_eq!(attachment.position, 0);
    assert!(attachment
        .sanitized_document
        .contains("> Alice owns deployment."));
    assert!(!attachment.sanitized_document.contains("Bob"));
    assert!(store
        .resolve_evidence(&project.project_id, &second.bundle_id, &old)
        .is_err());
    assert!(store
        .resolve_evidence(&project.project_id, &second.bundle_id, &new)
        .unwrap()
        .attachment
        .unwrap()
        .sanitized_document
        .contains("> Bob owns deployment."));

    // A recipe can ground a known owner in the separate file, and the normal
    // managed result lifecycle accepts that evidence through the private index.
    use tgsum_core::analysis::AnalysisSpec;
    use tgsum_core::recipe::{Recipe, RecipeEvidence, RecipeOutput};
    let out = tempfile::tempdir().unwrap();
    let exported = store
        .export_bundle(
            &project.project_id,
            &first.bundle_id,
            first.project_revision,
            out.path(),
            || false,
        )
        .unwrap();
    let documents: Vec<_> = exported
        .files
        .iter()
        .map(|f| fs::read_to_string(exported.directory.join(&f.name)).unwrap())
        .collect();
    let evidence = RecipeEvidence::from_markdown(documents.iter().map(String::as_str)).unwrap();
    let recipe = Recipe::ALL[0];
    let answer: RecipeOutput = serde_json::from_value(json!({"recipe":recipe.id(),"version":1,
        "sections":recipe.sections().iter().enumerate().map(|(i,id)|json!({"id":id,"claims":if i==0{vec![json!({"text":"Deployment discussed","evidence":[old]})]}else{vec![]}})).collect::<Vec<_>>(),
        "actions":[{"task":{"text":"Deploy","evidence":[old]},"owner":{"value":"Alice","quote":"Alice owns deployment.","evidence":old},"deadline":{"value":"next Friday","quote":"Deadline: next Friday.","evidence":old}}]
    })).unwrap();
    let ticket = store
        .begin_analysis(
            &project.project_id,
            &first.bundle_id,
            first.project_revision,
            AnalysisSpec {
                agent: "synthetic".into(),
                agent_version: "1".into(),
                isolation_profile: "offline".into(),
                destination: "local".into(),
                model: "canned".into(),
                recipe: recipe.id().into(),
                recipe_version: 1,
            },
            || false,
        )
        .unwrap();
    store
        .save_analysis_result(
            &ticket,
            &answer,
            |value| recipe.validate(value, &evidence),
            || false,
        )
        .unwrap();
    store.commit_analysis(&ticket, || false).unwrap();
}

#[test]
fn missing_files_are_visible_and_selection_survives_restart() {
    let (private, archive, store, project) = fixture("missing.log");
    let project = select(&store, &project, archive.path());
    let store = ProjectStore::new(private.path());
    assert_eq!(
        store.open(&project.project_id).unwrap().sources[0].selection,
        project.sources[0].selection
    );
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert_eq!(review.manifest.included_attachments, 0);
    assert_eq!(
        serde_json::to_value(&review.manifest).unwrap()["attachments"][0]["status"],
        "missing"
    );
    assert!(review.preview.contains("selected file missing"));
    let out = tempfile::tempdir().unwrap();
    store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            out.path(),
            || false,
        )
        .unwrap();
}

#[test]
fn version_five_projects_migrate_without_activating_or_rewriting_attachment_choices() {
    let (private, archive, store, project) = fixture("note.log");
    let revision = private
        .path()
        .join(&project.project_id)
        .join("revisions")
        .join(format!("{:020}.json", project.revision));
    let mut legacy = serde_json::to_value(&project).unwrap();
    legacy["schema_version"] = 5.into();
    let bytes = serde_json::to_vec(&legacy).unwrap();
    fs::write(&revision, &bytes).unwrap();
    let opened = store.open(&project.project_id).unwrap();
    assert_eq!(opened.schema_version, 6);
    assert!(opened.sources[0].selection.attachments.is_none());
    assert_eq!(fs::read(&revision).unwrap(), bytes);
    let selected = select(&store, &opened, archive.path());
    assert_eq!(selected.schema_version, 6);
    assert_eq!(fs::read(&revision).unwrap(), bytes);
    let selected_revision = private
        .path()
        .join(&project.project_id)
        .join("revisions")
        .join(format!("{:020}.json", selected.revision));
    let mut invalid = serde_json::to_value(&selected).unwrap();
    invalid["schema_version"] = 5.into();
    fs::write(selected_revision, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(store.open(&project.project_id).is_err());
}

#[test]
fn a_missing_root_does_not_turn_an_unsupported_reference_into_an_accepted_missing_file() {
    let (_private, archive, store, project) = fixture("archive.zip");
    let project = select(&store, &project, archive.path());
    fs::remove_dir(archive.path()).unwrap();
    assert!(store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false
        )
        .is_err());
}

#[test]
fn invalid_paths_remote_references_and_nontext_files_cannot_enter_a_bundle() {
    for path in [
        "../outside.log",
        "a/../note.log",
        "/absolute.log",
        "a//note.log",
        "a/./note.log",
        "C:note.log",
        "C:/note.log",
        "a\\note.log",
        "https://example.invalid/private.log",
        "nul.log",
        "files/COM1.log",
        "files/a. /note.log",
        "note.log.",
        "note.log ",
        "note.zip",
        "(File unavailable)",
    ] {
        let (_private, archive, store, project) = fixture(path);
        let project = select(&store, &project, archive.path());
        assert!(
            store
                .prepare_bundle(
                    &project.project_id,
                    project.revision,
                    BundleOptions::default(),
                    || false
                )
                .is_err(),
            "accepted {path:?}"
        );
        assert_eq!(
            store.open(&project.project_id).unwrap().revision,
            project.revision
        );
    }
    for bytes in [b"bad\x00text".as_slice(), b"bad\xfftext", b"\x1b[31mred"] {
        let (_private, archive, store, project) = fixture("bad.txt");
        fs::write(archive.path().join("bad.txt"), bytes).unwrap();
        let project = select(&store, &project, archive.path());
        assert!(store
            .prepare_bundle(
                &project.project_id,
                project.revision,
                BundleOptions::default(),
                || false
            )
            .is_err());
    }
}

#[cfg(unix)]
#[test]
fn leaf_parent_and_root_symlinks_are_rejected_even_if_the_target_is_inside() {
    use std::os::unix::fs::symlink;
    for parent in [false, true] {
        for outside in [false, true] {
            let (_private, archive, store, project) = fixture("files/note.log");
            let external = tempfile::tempdir().unwrap();
            let target = if outside {
                external.path().to_owned()
            } else {
                archive.path().join("target")
            };
            fs::create_dir_all(&target).unwrap();
            fs::write(target.join("note.log"), "OUTSIDE_OR_UNSELECTED_SENTINEL").unwrap();
            if parent {
                symlink(&target, archive.path().join("files")).unwrap();
            } else {
                fs::create_dir(archive.path().join("files")).unwrap();
                symlink(
                    target.join("note.log"),
                    archive.path().join("files/note.log"),
                )
                .unwrap();
            }
            let project = select(&store, &project, archive.path());
            assert!(store
                .prepare_bundle(
                    &project.project_id,
                    project.revision,
                    BundleOptions::default(),
                    || false
                )
                .is_err());
        }
    }
    let (_private, archive, store, project) = fixture("note.log");
    let root = archive.path().join("export");
    fs::create_dir(&root).unwrap();
    let project = select(&store, &project, &root);
    fs::rename(&root, archive.path().join("renamed")).unwrap();
    symlink(archive.path().join("renamed"), &root).unwrap();
    assert!(store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false
        )
        .is_err());
}

#[test]
fn regular_file_type_and_actual_size_limits_are_enforced_before_publication() {
    let (_private, archive, store, project) = fixture("large.log");
    let file = fs::File::create(archive.path().join("large.log")).unwrap();
    file.set_len(tgsum_core::attachments::MAX_FILE_BYTES + 1)
        .unwrap();
    let unselected = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert_eq!(unselected.manifest.included_attachments, 0);
    let project = select(&store, &project, archive.path());
    let error = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .err()
        .unwrap();
    assert!(error.to_string().contains("budget"));
    fs::remove_file(archive.path().join("large.log")).unwrap();
    fs::create_dir(archive.path().join("large.log")).unwrap();
    assert!(store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false
        )
        .is_err());
    fs::remove_dir(archive.path().join("large.log")).unwrap();
    fs::write(archive.path().join("large.log"), "").unwrap();
    assert_eq!(
        store
            .prepare_bundle(
                &project.project_id,
                project.revision,
                BundleOptions::default(),
                || false
            )
            .unwrap()
            .manifest
            .included_attachments,
        1
    );
}

#[test]
fn refreshed_metadata_cannot_retarget_a_choice_and_out_of_scope_files_are_not_opened() {
    let (_private, archive, store, project) = fixture("note.log");
    fs::write(archive.path().join("note.log"), "SELECTED_FILE").unwrap();
    let project = select(&store, &project, archive.path());
    let scope = SourceScope::telegram("synthetic", "1");
    store
        .snapshots(&project.project_id)
        .unwrap()
        .import_telegram(
            "refresh",
            &scope,
            Cursor::new(br#"{"id":1,"messages":[{"id":1,"text":"changed","file":"other.log"}]}"#),
        )
        .unwrap();
    fs::write(archive.path().join("other.log"), "UNSELECTED_FILE").unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::RecordSnapshot {
                source_id: "work".into(),
                snapshot_id: "refresh".into(),
            },
        )
        .unwrap();
    let error = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .err()
        .unwrap();
    assert!(error.to_string().contains("metadata changed"));
    let mut selection = project.sources[0].selection.clone();
    selection.filter.topic_ids = Some(vec![]);
    // A separate selected conversation keeps this bundle nonempty; the first
    // source is now completely excluded, so its unusable root must not be read.
    selection.attachments.as_mut().unwrap().root = archive.path().join("MISSING_ROOT");
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Selection {
                source_id: "work".into(),
                selection,
            },
        )
        .unwrap();
    let other = SourceScope::telegram("synthetic", "2");
    store
        .snapshots(&project.project_id)
        .unwrap()
        .import_telegram(
            "other",
            &other,
            Cursor::new(br#"{"id":2,"messages":[{"id":1,"text":"Included conversation"}]}"#),
        )
        .unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "other".into(),
                connector_id: "telegram_json".into(),
                scope: other,
                archive_path: None,
                latest_snapshot_id: Some("other".into()),
                selection: Default::default(),
            }),
        )
        .unwrap();
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert_eq!(review.manifest.messages, 1);
    assert_eq!(review.manifest.included_attachments, 0);
    assert_eq!(review.manifest.attachment_choices_outside_scope, 1);
    assert!(!review.preview.contains("UNSELECTED_FILE"));
}

#[test]
fn duplicates_and_global_count_and_byte_budgets_cannot_be_bypassed() {
    let (_private, archive, store, project) = fixture("note.log");
    let project = select(&store, &project, archive.path());
    let mut selection = project.sources[0].selection.clone();
    let choice = selection.attachments.as_ref().unwrap().files[0].clone();
    selection
        .attachments
        .as_mut()
        .unwrap()
        .files
        .push(choice.clone());
    assert!(store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Selection {
                source_id: "work".into(),
                selection
            }
        )
        .is_err());
    let mut selection = project.sources[0].selection.clone();
    selection.attachments.as_mut().unwrap().files = (0..101)
        .map(|id| {
            let mut c = choice.clone();
            c.message_id = id.to_string();
            c
        })
        .collect();
    assert!(store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Selection {
                source_id: "work".into(),
                selection
            }
        )
        .is_err());

    // Four 8 MiB references fill the budget exactly. A fifth nonempty file
    // must fail, even though archive metadata falsely advertises one byte.
    let scope = SourceScope::telegram("synthetic", "1");
    fs::write(
        archive.path().join("note.log"),
        vec![b'a'; tgsum_core::attachments::MAX_FILE_BYTES as usize],
    )
    .unwrap();
    fs::write(archive.path().join("extra.log"), "b").unwrap();
    let snapshot = store.snapshots(&project.project_id).unwrap().import_telegram("budget", &scope,
        Cursor::new(serde_json::to_vec(&json!({"id":1,"messages":(1..=5).map(|id|json!({"id":id,"text":"x","file":if id==5{"extra.log"}else{"note.log"},"file_size":1})).collect::<Vec<_>>() })).unwrap())).unwrap();
    let mut source = project.sources[0].clone();
    source.latest_snapshot_id = Some("budget".into());
    source.selection.attachments.as_mut().unwrap().files = snapshot
        .messages
        .iter()
        .map(|m| tgsum_core::attachments::AttachmentChoice {
            message_id: m.key.message_id.clone(),
            position: 0,
            expected: m.attachments[0].clone(),
        })
        .collect();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(source),
        )
        .unwrap();
    let error = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .err()
        .unwrap();
    assert!(error.to_string().contains("budget"), "{error}");
    assert_eq!(
        store.open(&project.project_id).unwrap().revision,
        project.revision
    );
}

#[test]
fn medium_findings_in_files_block_export_until_redacted_and_cancel_preserves_revision() {
    let (_private, archive, store, project) = fixture("note.log");
    fs::write(
        archive.path().join("note.log"),
        "key=SYNTHETIC_REVIEW_VALUE",
    )
    .unwrap();
    let project = select(&store, &project, archive.path());
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap();
    assert_eq!(review.manifest.privacy.needs_review, 1);
    assert_eq!(review.findings[0].field, "attachment_text");
    assert_eq!(
        review.findings[0].evidence,
        review.manifest.attachments[0].evidence
    );
    let out = tempfile::tempdir().unwrap();
    assert!(store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            out.path(),
            || false
        )
        .is_err());
    let redacted = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions {
                redact_candidates: true,
                ..Default::default()
            },
            || false,
        )
        .unwrap();
    assert!(!redacted.preview.contains("SYNTHETIC_REVIEW_VALUE"));
    assert_eq!(redacted.manifest.privacy.needs_review, 0);
    store
        .export_bundle(
            &project.project_id,
            &redacted.bundle_id,
            redacted.project_revision,
            out.path(),
            || false,
        )
        .unwrap();
    assert!(store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || true
        )
        .is_err());
    assert_eq!(
        store.open(&project.project_id).unwrap().revision,
        project.revision
    );
}

#[cfg(unix)]
#[test]
fn fifo_is_rejected_without_waiting_for_a_writer() {
    // Isolate this regression so a removed NONBLOCK flag fails with a deadline
    // rather than hanging the entire test process.
    const MARKER: &str = "TGSUM_SYNTHETIC_FIFO_CHILD";
    if std::env::var_os(MARKER).is_some() {
        let (_private, archive, store, project) = fixture("pipe.log");
        assert!(std::process::Command::new("mkfifo")
            .arg(archive.path().join("pipe.log"))
            .status()
            .unwrap()
            .success());
        let project = select(&store, &project, archive.path());
        assert!(store
            .prepare_bundle(
                &project.project_id,
                project.revision,
                BundleOptions::default(),
                || false
            )
            .is_err());
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fifo_is_rejected_without_waiting_for_a_writer"])
        .env(MARKER, "1")
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() > std::time::Duration::from_secs(10) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("FIFO read blocked");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
