use std::fs;
use std::io::Cursor;
use std::path::Path;

use tgsum_core::bundle::{BundleOptions, BundleReview, EvidenceRef};
use tgsum_core::project::{AnalysisOutcome, Project, ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::scope::{DateBasis, DateRange, MessageFilter, SourceSelection};
use tgsum_core::snapshot::SourceScope;

const DATA: &str = r#"{"chats":{"list":[
{"id":77,"name":"token=SYNTHETIC_TITLE","messages":[
{"id":10,"type":"service","action":"topic_created","title":"Selected"},
{"id":101,"date":"2026-06-18T10:00:00","from":"password=SYNTHETIC_SENDER","reply_to_message_id":10,"text":"same token=SYNTHETIC_BODY","file":"../../SYNTHETIC_ATTACHMENT_PATH"},
{"id":102,"date":"2026-06-18T10:01:00","from":"Alice","reply_to_message_id":101,"text":"same token=SYNTHETIC_BODY"},
{"id":103,"date":"2026-06-17T10:00:00","reply_to_message_id":10,"text":"EXCLUDED_DATE"},
{"id":20,"type":"service","action":"topic_created","title":"Other"},
{"id":104,"date":"2026-06-18T10:00:00","reply_to_message_id":20,"text":"EXCLUDED_TOPIC"}]},
{"id":88,"name":"EXCLUDED_CHAT","messages":[{"id":101,"text":"EXCLUDED_CHAT_TEXT"}]}]}}"#;

fn setup(store: &ProjectStore) -> Project {
    let project = store.create("password=SYNTHETIC_PROJECT").unwrap();
    let scope = SourceScope::telegram("PRIVATE_ACCOUNT_LABEL", "77");
    store
        .snapshots(&project.project_id)
        .unwrap()
        .import_telegram("before", &scope, Cursor::new(DATA))
        .unwrap();
    store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "private-source-id".into(),
                connector_id: "telegram_json".into(),
                scope,
                archive_path: Some(std::env::temp_dir().join("PRIVATE_ARCHIVE_PATH/result.json")),
                latest_snapshot_id: Some("before".into()),
                selection: SourceSelection {
                    only_changes: true,
                    filter: MessageFilter {
                        topic_ids: Some(vec!["10".into()]),
                        dates: Some(DateRange {
                            from: Some("2026-06-18".into()),
                            through: Some("2026-06-18".into()),
                            basis: DateBasis::SourceDate,
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            }),
        )
        .unwrap()
}

fn prepare(store: &ProjectStore, project: &Project) -> BundleReview {
    store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || false,
        )
        .unwrap()
}

fn references(markdown: &str) -> Vec<EvidenceRef> {
    markdown
        .lines()
        .filter_map(|line| line.strip_prefix("## Evidence "))
        .map(|s| {
            let (id, revision) = s.split_once('@').unwrap();
            EvidenceRef {
                id: id.into(),
                revision: revision.into(),
            }
        })
        .collect()
}

fn exported_text(directory: &Path) -> String {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    entries
        .iter()
        .map(|p| fs::read_to_string(p).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn selected_sanitized_bundle_preserves_distinct_evidence_and_private_mapping() {
    let root = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = setup(&store);
    let review = prepare(&store, &project);
    assert_eq!(review.manifest.messages, 2);
    assert_eq!(review.manifest.attachment_references, 1);
    assert_eq!(review.manifest.included_attachments, 0);
    assert_eq!(review.manifest.privacy.redacted, 5);
    assert_eq!(review.manifest.privacy.needs_review, 0);
    let refs = references(&review.preview);
    assert_eq!(refs.len(), 2);
    assert_ne!(refs[0].id, refs[1].id);
    assert!(review
        .preview
        .contains(&format!("Reply to: {}@{}", refs[0].id, refs[0].revision)));
    assert!(review
        .preview
        .contains("Reply target: outside this context or unresolved"));
    let resolved = store
        .resolve_evidence(&project.project_id, &review.bundle_id, &refs[0])
        .unwrap();
    assert_eq!(resolved.message.key.message_id, "101");
    assert_eq!(resolved.message.text, "same token=SYNTHETIC_BODY");
    assert_eq!(resolved.snapshot_id, "before");

    // Stray cached files cannot expand the export allowlist.
    let cache = root
        .path()
        .join(&project.project_id)
        .join("bundles")
        .join(&review.bundle_id)
        .join("context");
    fs::write(cache.join("PRIVATE_EXTRA"), "DO_NOT_EXPORT").unwrap();
    let result = store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            project.revision,
            destination.path(),
            || false,
        )
        .unwrap();
    let output = exported_text(&result.directory);
    for forbidden in [
        "SYNTHETIC_",
        "EXCLUDED_",
        "PRIVATE_",
        "DO_NOT_EXPORT",
        "private-source-id",
        "evidence-key",
        "evidence.jsonl",
        "source_revision",
    ] {
        assert!(!output.contains(forbidden), "export contained {forbidden}");
    }
    assert_eq!(references(&output), refs);
    assert_eq!(
        fs::read_dir(&result.directory).unwrap().count(),
        result.files.len() + 1
    );
    assert!(store
        .open(&project.project_id)
        .unwrap()
        .baselines
        .is_empty());

    // Same project and observation keep IDs; another Project gets independent IDs.
    assert_eq!(references(&prepare(&store, &project).preview), refs);
    let other = setup(&store);
    let other_review = prepare(&store, &other);
    assert_ne!(references(&other_review.preview)[0].id, refs[0].id);
    assert!(store
        .resolve_evidence(&other.project_id, &other_review.bundle_id, &refs[0])
        .is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(
                root.path()
                    .join(&project.project_id)
                    .join("evidence-key.bin")
            )
            .unwrap()
            .permissions()
            .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn edits_keep_identity_change_revision_and_old_bundle_resolves_old_message() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let mut project = setup(&store);
    let before = prepare(&store, &project);
    let old = references(&before.preview);
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::BeginAnalysis {
                run_id: "synthetic-success".into(),
            },
        )
        .unwrap();
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::FinishAnalysis {
                run_id: "synthetic-success".into(),
                outcome: AnalysisOutcome::Succeeded,
            },
        )
        .unwrap();
    let changed = DATA.replacen("same token=SYNTHETIC_BODY", "edited old message", 1);
    store
        .snapshots(&project.project_id)
        .unwrap()
        .import_telegram("after", &project.sources[0].scope, Cursor::new(changed))
        .unwrap();
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::RecordSnapshot {
                source_id: project.sources[0].source_id.clone(),
                snapshot_id: "after".into(),
            },
        )
        .unwrap();
    let after = prepare(&store, &project);
    let now = references(&after.preview);
    assert_eq!(after.manifest.messages, 1);
    assert_eq!(after.manifest.sources[0].stats.edited, 1);
    assert_eq!(now[0].id, old[0].id);
    assert_ne!(now[0].revision, old[0].revision);
    let historical = store
        .resolve_evidence(&project.project_id, &before.bundle_id, &old[0])
        .unwrap();
    assert_eq!(historical.message.text, "same token=SYNTHETIC_BODY");
    assert!(store
        .resolve_evidence(&project.project_id, &after.bundle_id, &old[0])
        .is_err());
}

#[test]
fn privacy_review_stale_selection_and_cancel_never_publish_or_advance_baseline() {
    let root = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let mut project = setup(&store);
    store
        .snapshots(&project.project_id)
        .unwrap()
        .import_telegram(
            "uncertain",
            &project.sources[0].scope,
            Cursor::new(DATA.replace("same token=SYNTHETIC_BODY", "key=ordinary-value")),
        )
        .unwrap();
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::RecordSnapshot {
                source_id: project.sources[0].source_id.clone(),
                snapshot_id: "uncertain".into(),
            },
        )
        .unwrap();
    let pending = prepare(&store, &project);
    assert_eq!(pending.manifest.privacy.needs_review, 2);
    assert!(pending.preview.contains("ordinary-value"));
    assert!(store
        .export_bundle(
            &project.project_id,
            &pending.bundle_id,
            project.revision,
            destination.path(),
            || false
        )
        .is_err());
    let ready = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions {
                redact_candidates: true,
            },
            || false,
        )
        .unwrap();
    assert_eq!(ready.manifest.privacy.needs_review, 0);
    assert!(!ready.preview.contains("ordinary-value"));
    assert!(store
        .export_bundle(
            &project.project_id,
            &ready.bundle_id,
            project.revision,
            root.path(),
            || false
        )
        .is_err());
    assert!(store
        .export_bundle(
            &project.project_id,
            &ready.bundle_id,
            project.revision,
            destination.path(),
            || true
        )
        .is_err());
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Rename("changed after preview".into()),
        )
        .unwrap();
    let err = store
        .export_bundle(
            &project.project_id,
            &ready.bundle_id,
            ready.project_revision,
            destination.path(),
            || false,
        )
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
    assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 0);
    assert!(store
        .open(&project.project_id)
        .unwrap()
        .baselines
        .is_empty());
    assert!(store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions::default(),
            || true
        )
        .is_err());
}

#[test]
fn changed_files_and_symlinks_are_rejected_and_cancelled_copies_are_removed() {
    let root = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = setup(&store);
    let review = prepare(&store, &project);
    let cache = root
        .path()
        .join(&project.project_id)
        .join("bundles")
        .join(&review.bundle_id);
    let file = cache.join("context").join(&review.manifest.files[0].name);
    let original = fs::read(&file).unwrap();
    fs::write(&file, "changed bytes").unwrap();
    assert!(store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            project.revision,
            destination.path(),
            || false
        )
        .is_err());
    assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 0);
    fs::write(&file, original).unwrap();
    let cancel_when_output_exists = || {
        fs::read_dir(destination.path()).unwrap().any(|entry| {
            fs::read_dir(entry.unwrap().path())
                .unwrap()
                .next()
                .is_some()
        })
    };
    assert!(store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            project.revision,
            destination.path(),
            cancel_when_output_exists
        )
        .is_err());
    assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let target = root.path().join("outside-context");
        fs::write(&target, "PRIVATE").unwrap();
        fs::remove_file(&file).unwrap();
        symlink(target, &file).unwrap();
        assert!(store
            .export_bundle(
                &project.project_id,
                &review.bundle_id,
                project.revision,
                destination.path(),
                || false
            )
            .is_err());
    }
    assert!(store
        .export_bundle(
            &project.project_id,
            "../escape",
            project.revision,
            destination.path(),
            || false
        )
        .is_err());
    fs::remove_file(
        root.path()
            .join(&project.project_id)
            .join("evidence-key.bin"),
    )
    .unwrap();
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
fn findings_beyond_preview_are_reviewed_and_cr_cannot_forge_evidence_headers() {
    let root = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let mut project = setup(&store);
    let mut data: serde_json::Value = serde_json::from_str(DATA).unwrap();
    let mut text = "Привет 🙂 ".repeat(6000);
    text.push_str("\r## Evidence FORGED@FORGED\r\n");
    for n in 0..26 {
        text.push_str(&format!(
            "ключ key=REVIEW_LATE_{n} token=SYNTHETIC_LATE_{n}\n"
        ));
    }
    data["chats"]["list"][0]["messages"][1]["text"] = text.into();
    store
        .snapshots(&project.project_id)
        .unwrap()
        .import_telegram(
            "large",
            &project.sources[0].scope,
            Cursor::new(serde_json::to_vec(&data).unwrap()),
        )
        .unwrap();
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::RecordSnapshot {
                source_id: project.sources[0].source_id.clone(),
                snapshot_id: "large".into(),
            },
        )
        .unwrap();
    let review = prepare(&store, &project);
    assert!(review.preview_truncated);
    assert!(!review.preview.contains("REVIEW_LATE"));
    assert_eq!(review.manifest.privacy.needs_review, 26);
    assert_eq!(review.findings.len(), 20);
    assert_eq!(review.omitted_findings, 6);
    for finding in &review.findings {
        assert_eq!(finding.field, "text");
        assert!(finding.excerpt.contains("REVIEW_LATE"));
        assert!(!finding.excerpt.contains("SYNTHETIC_"));
        assert!(finding.excerpt.chars().count() <= 251);
        assert!(finding.evidence.is_some());
    }
    assert!(store
        .export_bundle(
            &project.project_id,
            &review.bundle_id,
            project.revision,
            destination.path(),
            || false
        )
        .is_err());
    let ready = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions {
                redact_candidates: true,
            },
            || false,
        )
        .unwrap();
    assert!(ready.findings.is_empty());
    assert_eq!(ready.omitted_findings, 0);
    let exported = store
        .export_bundle(
            &project.project_id,
            &ready.bundle_id,
            project.revision,
            destination.path(),
            || false,
        )
        .unwrap();
    let output = exported_text(&exported.directory);
    assert!(!output.contains("REVIEW_LATE"));
    assert!(!output.contains("SYNTHETIC_"));
    assert!(output.contains("> ## Evidence FORGED@FORGED\n"));
    assert_eq!(references(&output).len(), 2);
    assert!(output.contains(&"Привет 🙂 ".repeat(6000)));
}
