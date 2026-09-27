use serde_json::json;
use std::io::Cursor;
use tgsum_core::{
    project::{ProjectChange, ProjectSource, ProjectStore},
    snapshot::SourceScope,
};

#[test]
fn catalog_is_scoped_paginated_and_does_not_probe_or_copy_files() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Catalog").unwrap();
    let scope = SourceScope::telegram("synthetic", "1");
    let messages:Vec<_>=(1..=53).map(|id|json!({"id":id,"file":if id==1{"https://example.invalid/file.txt"}else if id==2{"archive.zip"}else{"not-present.log"},"file_name":format!("local name {id}")})).collect();
    store
        .snapshots(&project.project_id)
        .unwrap()
        .import_telegram(
            "initial",
            &scope,
            Cursor::new(serde_json::to_vec(&json!({"id":1,"messages":messages})).unwrap()),
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
                selection: Default::default(),
            }),
        )
        .unwrap();
    let first = store
        .attachment_catalog(&project.project_id, project.revision, "work", 0, || false)
        .unwrap();
    assert_eq!(first.total, 53);
    assert_eq!(first.items.len(), 50);
    assert_eq!(first.next_offset, Some(50));
    assert!(!first.items[0].eligible);
    assert!(!first.items[1].eligible);
    assert!(first.items[2].eligible); // no root/file has been selected or opened
    let second = store
        .attachment_catalog(&project.project_id, project.revision, "work", 50, || false)
        .unwrap();
    assert_eq!(second.items.len(), 3);
    assert_eq!(second.items[0].choice.message_id, "51");
    assert_eq!(second.next_offset, None);
    let mut selection = project.sources[0].selection.clone();
    selection.filter.topic_ids = Some(vec![]);
    let changed = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Selection {
                source_id: "work".into(),
                selection,
            },
        )
        .unwrap();
    assert!(store
        .attachment_catalog(&project.project_id, project.revision, "work", 0, || false)
        .is_err());
    assert_eq!(
        store
            .attachment_catalog(&project.project_id, changed.revision, "work", 0, || false)
            .unwrap()
            .total,
        0
    );
}
