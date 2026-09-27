//! Adversarial filesystem fixtures for the public Project/bundle lifecycle.
//! Every input, substitute target, private store and output is under one TempDir.
use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;

use serde_json::json;
use tgsum_core::{
    attachments::{AttachmentChoice, AttachmentSelection},
    bundle::{BundleOptions, BundleReview},
    project::{Project, ProjectChange, ProjectSource, ProjectStore},
    scope::SourceSelection,
    snapshot::SourceScope,
};

struct Fixture {
    root: tempfile::TempDir,
    archive: PathBuf,
    store: ProjectStore,
    project: Project,
}

impl Fixture {
    fn new(files: &[(&str, &[u8])]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("archive");
        fs::create_dir(&archive).unwrap();
        for (name, bytes) in files {
            let path = archive.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let store = ProjectStore::new(root.path().join("projects"));
        let project = store.create("Packager fixture").unwrap();
        let scope = SourceScope::telegram("synthetic", "1");
        let archive_json = json!({"id":1,"messages":files.iter().enumerate().map(|(index,(name,_))|json!({
            "id":index+1,"text":"Attached","file":name,"file_name":"same-original-name.log","file_size":1
        })).collect::<Vec<_>>()});
        let snapshot = store
            .snapshots(&project.project_id)
            .unwrap()
            .import_telegram(
                "initial",
                &scope,
                Cursor::new(serde_json::to_vec(&archive_json).unwrap()),
            )
            .unwrap();
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
                    selection: SourceSelection {
                        attachments: Some(AttachmentSelection {
                            root: fs::canonicalize(&archive).unwrap(),
                            files: snapshot
                                .messages
                                .iter()
                                .map(|m| AttachmentChoice {
                                    message_id: m.key.message_id.clone(),
                                    position: 0,
                                    expected: m.attachments[0].clone(),
                                })
                                .collect(),
                        }),
                        ..Default::default()
                    },
                }),
            )
            .unwrap();
        Self {
            root,
            archive,
            store,
            project,
        }
    }

    fn prepare(&self) -> BundleReview {
        self.store
            .prepare_bundle(
                &self.project.project_id,
                self.project.revision,
                BundleOptions::default(),
                || false,
            )
            .unwrap()
    }

    fn bundles(&self) -> BTreeSet<PathBuf> {
        let path = self
            .root
            .path()
            .join("projects")
            .join(&self.project.project_id)
            .join("bundles");
        match fs::read_dir(path) {
            Ok(entries) => entries.map(|entry| entry.unwrap().path()).collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
            Err(e) => panic!("cannot inspect owned fixture: {e}"),
        }
    }

    /// Observe an actual artifact, not a cancellation-callback invocation count.
    fn new_artifact_exists(&self, before: &BTreeSet<PathBuf>) -> bool {
        self.bundles().difference(before).any(|directory| {
            fs::metadata(directory.join("context/attachment-00001.md")).is_ok_and(|m| m.len() > 0)
        })
    }

    fn assert_unchanged_after_failure(&self, bundles: &BTreeSet<PathBuf>) {
        assert_eq!(
            &self.bundles(),
            bundles,
            "failed draft left private files behind"
        );
        let project = self.store.open(&self.project.project_id).unwrap();
        assert_eq!(project, self.project);
    }

    fn exported_text(&self, review: &BundleReview) -> String {
        let export = self
            .store
            .export_bundle(
                &self.project.project_id,
                &review.bundle_id,
                review.project_revision,
                &self.root.path().join("out"),
                || false,
            )
            .unwrap();
        export
            .files
            .iter()
            .map(|file| fs::read_to_string(export.directory.join(&file.name)).unwrap())
            .collect()
    }
}

#[test]
fn same_basenames_and_duplicate_original_names_keep_distinct_artifacts_and_evidence() {
    let fixture = Fixture::new(&[
        ("a/same.log", b"FIRST_CONTENT"),
        ("b/same.log", b"SECOND_CONTENT"),
    ]);
    let review = fixture.prepare();
    assert_eq!(review.manifest.included_attachments, 2);
    let records = &review.manifest.attachments;
    assert_ne!(records[0].file, records[1].file);
    assert_ne!(records[0].evidence, records[1].evidence);
    for (record, expected) in records.iter().zip(["FIRST_CONTENT", "SECOND_CONTENT"]) {
        let resolved = fixture
            .store
            .resolve_evidence(
                &fixture.project.project_id,
                &review.bundle_id,
                record.evidence.as_ref().unwrap(),
            )
            .unwrap();
        assert!(resolved
            .attachment
            .unwrap()
            .sanitized_document
            .contains(expected));
    }
    let first = fixture.exported_text(&review);
    fs::write(fixture.archive.join("a/same.log"), "CHANGED_ORIGINAL").unwrap();
    fs::remove_file(fixture.archive.join("b/same.log")).unwrap();
    assert_eq!(fixture.exported_text(&review), first);
}

#[test]
fn binary_masquerading_after_a_valid_file_discards_all_staged_documents() {
    let fixture = Fixture::new(&[
        ("first.log", b"SAFE_FIRST"),
        ("looks-like-text.txt", b"PK\x03\x04\0BINARY_SENTINEL"),
    ]);
    let before = fixture.bundles();
    let wrote_first = Cell::new(false);
    let result = fixture.store.prepare_bundle(
        &fixture.project.project_id,
        fixture.project.revision,
        BundleOptions::default(),
        || {
            if fixture.new_artifact_exists(&before) {
                wrote_first.set(true);
            }
            false
        },
    );
    assert!(
        wrote_first.get(),
        "fixture must reach the partially written draft"
    );
    let error = result.err().unwrap().to_string();
    assert!(!error.contains("BINARY_SENTINEL"));
    assert!(!error.contains(fixture.archive.to_str().unwrap()));
    fixture.assert_unchanged_after_failure(&before);
}

#[test]
fn cancellation_after_attachment_write_removes_prepare_and_export_staging() {
    let fixture = Fixture::new(&[("first.log", b"SAFE_FIRST"), ("second.log", b"SAFE_SECOND")]);
    let kept = fixture.prepare();
    let before = fixture.bundles();
    let cancelled = Cell::new(false);
    let result = fixture.store.prepare_bundle(
        &fixture.project.project_id,
        fixture.project.revision,
        BundleOptions::default(),
        || {
            let stop = fixture.new_artifact_exists(&before);
            cancelled.set(cancelled.get() || stop);
            stop
        },
    );
    assert!(cancelled.get());
    assert!(tgsum_core::is_cancelled(&result.err().unwrap()));
    fixture.assert_unchanged_after_failure(&before);

    let output = fixture.root.path().join("cancelled-output");
    fs::create_dir(&output).unwrap();
    let result = fixture.store.export_bundle(
        &fixture.project.project_id,
        &kept.bundle_id,
        kept.project_revision,
        &output,
        || {
            fs::read_dir(&output).unwrap().any(|entry| {
                fs::metadata(entry.unwrap().path().join("attachment-00001.md"))
                    .is_ok_and(|m| m.len() > 0)
            })
        },
    );
    assert!(tgsum_core::is_cancelled(&result.err().unwrap()));
    assert_eq!(fs::read_dir(output).unwrap().count(), 0);
    fixture.assert_unchanged_after_failure(&before);
    assert!(fixture.exported_text(&kept).contains("SAFE_SECOND"));
}

#[test]
fn growing_source_hits_actual_read_budget_without_publishing_a_partial_snapshot() {
    let fixture = Fixture::new(&[("growing.log", &[b'a'; 64 * 1024])]);
    let path = fixture.archive.join("growing.log");
    let before = fixture.bundles();
    let writes = Cell::new(0);
    // Grow at every cooperative checkpoint, including reads. Stop writing at a
    // finite ceiling so a broken guard cannot make the regression loop forever.
    let result = fixture.store.prepare_bundle(
        &fixture.project.project_id,
        fixture.project.revision,
        BundleOptions::default(),
        || {
            if writes.get() < 200 {
                fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .unwrap()
                    .write_all(&[b'b'; 128 * 1024])
                    .unwrap();
                writes.set(writes.get() + 1);
            }
            false
        },
    );
    assert!(result.err().unwrap().to_string().contains("budget"));
    assert!(fs::metadata(path).unwrap().len() > tgsum_core::attachments::MAX_FILE_BYTES);
    fixture.assert_unchanged_after_failure(&before);
}

#[cfg(unix)]
#[test]
fn replacing_root_after_first_copy_cannot_redirect_the_retained_directory_handle() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new(&[("first.log", b"SAFE_FIRST"), ("second.log", b"SAFE_SECOND")]);
    let outside = fixture.root.path().join("outside-selected-root");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("second.log"), "UNSELECTED_SENTINEL").unwrap();
    let before = fixture.bundles();
    let switched = Cell::new(false);
    let review = fixture
        .store
        .prepare_bundle(
            &fixture.project.project_id,
            fixture.project.revision,
            BundleOptions::default(),
            || {
                if !switched.get() && fixture.new_artifact_exists(&before) {
                    fs::rename(
                        &fixture.archive,
                        fixture.root.path().join("retained-original"),
                    )
                    .unwrap();
                    symlink(&outside, &fixture.archive).unwrap();
                    switched.set(true);
                }
                false
            },
        )
        .unwrap();
    assert!(switched.get());
    let output = fixture.exported_text(&review);
    assert!(output.contains("SAFE_FIRST") && output.contains("SAFE_SECOND"));
    assert!(!output.contains("UNSELECTED_SENTINEL"));
}

#[cfg(unix)]
#[test]
fn swapping_a_later_parent_or_leaf_for_a_link_aborts_and_removes_the_first_copy() {
    use std::os::unix::fs::symlink;
    for parent in [true, false] {
        let fixture = Fixture::new(&[
            ("first.log", b"SAFE_FIRST"),
            ("later/second.log", b"SAFE_SECOND"),
        ]);
        let outside = fixture.root.path().join("outside-selected-root");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("second.log"), "UNSELECTED_SENTINEL").unwrap();
        let before = fixture.bundles();
        let switched = Cell::new(false);
        let result = fixture.store.prepare_bundle(
            &fixture.project.project_id,
            fixture.project.revision,
            BundleOptions::default(),
            || {
                if !switched.get() && fixture.new_artifact_exists(&before) {
                    let replace =
                        fixture
                            .archive
                            .join(if parent { "later" } else { "later/second.log" });
                    fs::rename(&replace, fixture.root.path().join("parked-original")).unwrap();
                    symlink(
                        if parent {
                            outside.clone()
                        } else {
                            outside.join("second.log")
                        },
                        replace,
                    )
                    .unwrap();
                    switched.set(true);
                }
                false
            },
        );
        assert!(switched.get());
        assert!(result.is_err());
        fixture.assert_unchanged_after_failure(&before);
    }
}

#[test]
fn damaged_reviewed_artifact_blocks_both_export_and_historical_resolution() {
    let fixture = Fixture::new(&[("first.log", b"SAFE_FIRST")]);
    let review = fixture.prepare();
    let private_bundle = fixture.bundles().into_iter().next().unwrap();
    let record = &review.manifest.attachments[0];
    fs::write(
        private_bundle
            .join("context")
            .join(record.file.as_ref().unwrap()),
        "CORRUPTED_REVIEW",
    )
    .unwrap();
    let output = fixture.root.path().join("rejected-export");
    assert!(fixture
        .store
        .export_bundle(
            &fixture.project.project_id,
            &review.bundle_id,
            review.project_revision,
            &output,
            || false
        )
        .is_err());
    assert_eq!(fs::read_dir(output).unwrap().count(), 0);
    assert!(fixture
        .store
        .resolve_evidence(
            &fixture.project.project_id,
            &review.bundle_id,
            record.evidence.as_ref().unwrap()
        )
        .is_err());
    assert_eq!(
        fixture.store.open(&fixture.project.project_id).unwrap(),
        fixture.project
    );
}
