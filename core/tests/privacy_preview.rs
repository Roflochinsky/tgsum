use serde_json::json;
use std::{fs, io::Cursor};
use tgsum_core::{
    attachments::{AttachmentChoice, AttachmentSelection},
    bundle::BundleOptions,
    project::{ProjectChange, ProjectSource, ProjectStore},
    scope::SourceSelection,
    snapshot::SourceScope,
};

#[test]
fn review_compares_saved_message_and_verified_file_bytes_without_exporting_originals() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("archive");
    fs::create_dir(&archive).unwrap();
    let source = archive.join("note.log");
    fs::write(&source, "FILE password=SYNTHETIC_FILE_SECRET").unwrap();
    let store = ProjectStore::new(root.path().join("projects"));
    let project = store.create("Comparison").unwrap();
    let scope = SourceScope::telegram("synthetic", "1");
    let snapshot=store.snapshots(&project.project_id).unwrap().import_telegram("initial",&scope,Cursor::new(br#"{"id":1,"messages":[{"id":1,"text":"MESSAGE password=SYNTHETIC_MESSAGE_SECRET","file":"note.log"}]}"#)).unwrap();
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
                        files: vec![AttachmentChoice {
                            message_id: "1".into(),
                            position: 0,
                            expected: snapshot.messages[0].attachments[0].clone(),
                        }],
                    }),
                    ..Default::default()
                },
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
    let page = store
        .review_items(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            0,
            || false,
        )
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.next_offset, None);
    let file = page.items.iter().find(|i| i.kind == "attachment").unwrap();
    let message = page.items.iter().find(|i| i.kind == "message").unwrap();
    let comparison = store
        .preview_evidence(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            &message.reference,
            || false,
        )
        .unwrap();
    assert_eq!(
        comparison.before.as_deref(),
        Some("MESSAGE password=SYNTHETIC_MESSAGE_SECRET")
    );
    assert!(comparison
        .after
        .contains("MESSAGE password=[REDACTED_SECRET]"));
    let comparison = store
        .preview_evidence(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            &file.reference,
            || false,
        )
        .unwrap();
    assert_eq!(
        comparison.before.as_deref(),
        Some("FILE password=SYNTHETIC_FILE_SECRET")
    );
    assert!(comparison.after.contains("FILE password=[REDACTED_SECRET]"));
    assert!(!serde_json::to_value(&review.manifest)
        .unwrap()
        .to_string()
        .contains("SYNTHETIC"));
    fs::write(&source, "CHANGED_SOURCE").unwrap();
    let changed = store
        .preview_evidence(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            &file.reference,
            || false,
        )
        .unwrap();
    assert!(changed.before.is_none());
    assert_eq!(
        serde_json::to_value(changed.before_state).unwrap(),
        json!("file_changed")
    );
    assert_eq!(changed.after, comparison.after);
    fs::remove_file(source).unwrap();
    let missing = store
        .preview_evidence(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            &file.reference,
            || false,
        )
        .unwrap();
    assert!(missing.before.is_none());
    assert_eq!(missing.after, comparison.after);
    assert!(store
        .resolve_evidence(&project.project_id, &review.bundle_id, &file.reference)
        .unwrap()
        .attachment
        .is_some());
}

#[test]
fn comparison_skips_large_other_messages_and_bounds_both_utf8_panes() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let mut project = store.create("Bounded comparison").unwrap();
    let mut settings = project.settings.clone();
    settings.max_tokens = None;
    project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Settings(settings),
        )
        .unwrap();
    let scope = SourceScope::telegram("synthetic", "1");
    let snapshot = json!({"id":1,"messages":[{"id":1,"text":"OTHER_SENTINEL".repeat(10000)},{"id":2,"text":format!("TARGET {}", "界🌍".repeat(10000))}]});
    store
        .snapshots(&project.project_id)
        .unwrap()
        .import_telegram(
            "initial",
            &scope,
            Cursor::new(serde_json::to_vec(&snapshot).unwrap()),
        )
        .unwrap();
    project = store
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
    let review = store
        .prepare_saved_bundle(&project.project_id, project.revision, || false)
        .unwrap();
    assert_eq!(review.manifest.files.len(), 1);
    let page = store
        .review_items(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            0,
            || false,
        )
        .unwrap();
    let reference = &page
        .items
        .iter()
        .find(|i| i.label == "2")
        .unwrap()
        .reference;
    let comparison = store
        .preview_evidence(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            reference,
            || false,
        )
        .unwrap();
    assert!(comparison.before_truncated && comparison.after_truncated);
    assert!(comparison.before.as_ref().unwrap().len() <= 24 * 1024);
    assert!(comparison.after.len() <= 24 * 1024);
    assert!(comparison.after.starts_with("TARGET "));
    assert!(!comparison.after.contains("OTHER_SENTINEL"));
    let error = store
        .preview_evidence(
            &project.project_id,
            &review.bundle_id,
            review.project_revision,
            reference,
            || true,
        )
        .err()
        .unwrap();
    assert!(tgsum_core::is_cancelled(&error));
    store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Rename("Edited after review".into()),
        )
        .unwrap();
    assert_eq!(
        store
            .review_items(
                &project.project_id,
                &review.bundle_id,
                review.project_revision,
                0,
                || false
            )
            .err()
            .unwrap()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        store
            .preview_evidence(
                &project.project_id,
                &review.bundle_id,
                review.project_revision,
                reference,
                || false
            )
            .err()
            .unwrap()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
}
