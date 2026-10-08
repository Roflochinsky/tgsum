use std::io::{self, Cursor};
use tgsum_core::assisted::{AssistedExportSettings, AssistedImportRequest};
use tgsum_core::project::{ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;

#[test]
fn remembers_export_plan_only_for_the_same_connected_conversation() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path().join("projects"));
    let p = store.create("Pilot").unwrap();
    let mut source = ProjectSource {
        source_id: "pilot".into(),
        connector_id: "telegram_json".into(),
        scope: SourceScope::telegram("work", "42"),
        archive_path: None,
        latest_snapshot_id: None,
        selection: Default::default(),
    };
    let p = store
        .update(
            &p.project_id,
            p.revision,
            ProjectChange::Source(source.clone()),
        )
        .unwrap();
    let settings = AssistedExportSettings {
        directory: root.path().join("future-export"),
        client: Some(root.path().join("chosen-client")),
    };
    let p = store
        .update(
            &p.project_id,
            p.revision,
            ProjectChange::AssistedExport {
                source_id: "pilot".into(),
                settings: Some(settings.clone()),
            },
        )
        .unwrap();
    assert_eq!(
        store.open(&p.project_id).unwrap().assisted_exports["pilot"],
        settings
    );
    // Reconnecting a moved JSON retains the plan; replacing its scope does not.
    source.archive_path = Some(root.path().join("moved.json"));
    let p = store
        .update(
            &p.project_id,
            p.revision,
            ProjectChange::Source(source.clone()),
        )
        .unwrap();
    assert_eq!(p.assisted_exports["pilot"], settings);
    source.scope = SourceScope::telegram("other-account", "42");
    let p = store
        .update(&p.project_id, p.revision, ProjectChange::Source(source))
        .unwrap();
    assert!(p.assisted_exports.is_empty());
    assert!(store
        .update(
            &p.project_id,
            p.revision,
            ProjectChange::AssistedExport {
                source_id: "disconnected".into(),
                settings: Some(settings)
            }
        )
        .is_err());
}

#[test]
fn confirmed_completed_file_atomically_updates_only_the_selected_source() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let p = store.create("Pilot").unwrap();
    let p = store
        .update(
            &p.project_id,
            p.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "pilot".into(),
                connector_id: "telegram_json".into(),
                scope: SourceScope::telegram("work", "42"),
                archive_path: Some(root.path().join("old.json")),
                latest_snapshot_id: None,
                selection: Default::default(),
            }),
        )
        .unwrap();
    let p = store
        .update(
            &p.project_id,
            p.revision,
            ProjectChange::AssistedExport {
                source_id: "pilot".into(),
                settings: Some(AssistedExportSettings {
                    directory: root.path().join("exports"),
                    client: None,
                }),
            },
        )
        .unwrap();
    let mut request = AssistedImportRequest {
        project_id: p.project_id.clone(),
        expected_revision: p.revision,
        source_id: "pilot".into(),
        archive_path: root
            .path()
            .join("exports/ChatExport_2026-09-27/result.json"),
        scope_and_completion_confirmed: false,
    };
    let json = r#"{"id":42,"name":"Pilot","type":"private_group","messages":[{"id":1,"type":"message","text":"New context"}]}"#;
    assert!(store
        .import_assisted_export(
            &request,
            || -> io::Result<Cursor<&str>> { panic!("must not open unconfirmed input") },
            || false
        )
        .is_err());
    request.scope_and_completion_confirmed = true;
    let updated = store
        .import_assisted_export(&request, || Ok(Cursor::new(json)), || false)
        .unwrap();
    assert_eq!(
        updated.sources[0].archive_path,
        Some(request.archive_path.clone())
    );
    assert_eq!(updated.revision, p.revision + 1);
    let snapshots = store.snapshots(&p.project_id).unwrap();
    let snapshot = snapshots
        .load(updated.sources[0].latest_snapshot_id.as_ref().unwrap())
        .unwrap();
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.messages[0].text, "New context");
    assert_eq!(
        store
            .import_assisted_export(&request, || Ok(Cursor::new(json)), || false)
            .unwrap_err()
            .kind(),
        io::ErrorKind::WouldBlock
    );
    request.expected_revision = updated.revision;
    for bad in [
        json.replace("42", "43"),
        format!("{json} trailing"),
        json[..60].to_owned(),
    ] {
        assert!(store
            .import_assisted_export(&request, || Ok(Cursor::new(bad)), || false)
            .is_err());
        assert_eq!(store.open(&p.project_id).unwrap(), updated);
    }
    assert!(store
        .import_assisted_export(&request, || Ok(Cursor::new(json)), || true)
        .is_err());
    assert_eq!(store.open(&p.project_id).unwrap(), updated);

    // Edits and cancellation arriving after validation/while reading must not
    // let a late import replace the saved head or reconnect a changed source.
    let cancelled = std::cell::Cell::new(false);
    let reader = tgsum_core::ProgressReader::new(Cursor::new(json), |_| {
        cancelled.set(true);
        Ok(())
    });
    assert!(store
        .import_assisted_export(&request, || Ok(reader), || cancelled.get())
        .is_err());
    assert_eq!(store.open(&p.project_id).unwrap(), updated);
    let error = store
        .import_assisted_export(
            &request,
            || {
                store.update(
                    &p.project_id,
                    updated.revision,
                    ProjectChange::Rename("Concurrent editor".into()),
                )?;
                Ok(Cursor::new(json))
            },
            || false,
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    let head = store.open(&p.project_id).unwrap();
    assert_eq!(head.name, "Concurrent editor");
    assert_eq!(head.sources, updated.sources);
}

#[test]
fn v7_migrates_without_rewriting_and_cannot_smuggle_future_settings() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let p = store.create("Legacy").unwrap();
    let path = root
        .path()
        .join(&p.project_id)
        .join("revisions/00000000000000000000.json");
    let mut value = serde_json::to_value(&p).unwrap();
    value["schema_version"] = 7.into();
    let bytes = serde_json::to_vec(&value).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let opened = store.open(&p.project_id).unwrap();
    assert_eq!(opened.schema_version, 10);
    assert!(opened.assisted_exports.is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    value["assisted_exports"] =
        serde_json::json!({"unconnected":{"directory":root.path(),"client":null}});
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(store.open(&p.project_id).is_err());
}

#[test]
fn stable_import_returns_delta_and_rejects_source_mutation_or_cancel_without_advancing_project() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path().join("projects"));
    let p = store.create("Pilot").unwrap();
    let p = store
        .update(
            &p.project_id,
            p.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "pilot".into(),
                connector_id: "telegram_json".into(),
                scope: SourceScope::telegram("work", "42"),
                archive_path: None,
                latest_snapshot_id: None,
                selection: Default::default(),
            }),
        )
        .unwrap();
    let folder = root.path().join("exports");
    std::fs::create_dir(&folder).unwrap();
    let p = store
        .update(
            &p.project_id,
            p.revision,
            ProjectChange::AssistedExport {
                source_id: "pilot".into(),
                settings: Some(AssistedExportSettings {
                    directory: folder.clone(),
                    client: None,
                }),
            },
        )
        .unwrap();
    let path = folder.join("result.json");
    let first =
        r#"{"id":42,"type":"private_group","messages":[{"id":1,"type":"message","text":"one"}]}"#;
    std::fs::write(&path, first).unwrap();
    let mut request = AssistedImportRequest {
        project_id: p.project_id.clone(),
        expected_revision: p.revision,
        source_id: "pilot".into(),
        archive_path: path.clone(),
        scope_and_completion_confirmed: false,
    };
    assert!(store
        .import_stable_assisted_export(&request, |_, _| panic!("not opened"), || false)
        .is_err());
    request.scope_and_completion_confirmed = true;
    let completed = store
        .import_stable_assisted_export(&request, |_, _| Ok(()), || false)
        .unwrap();
    assert_eq!(completed.delta.created, 1);
    assert_eq!(completed.delta.unchanged, 0);
    let current = completed.project;
    request.expected_revision = current.revision;
    let second = r#"{"id":42,"type":"private_group","messages":[{"id":1,"type":"message","text":"edited"},{"id":2,"type":"message","text":"two"}]}"#;
    std::fs::write(&path, second).unwrap();
    let next = store
        .import_stable_assisted_export(&request, |_, _| Ok(()), || false)
        .unwrap();
    assert_eq!(
        (
            next.delta.created,
            next.delta.edited,
            next.delta.missing,
            next.delta.unchanged
        ),
        (1, 1, 0, 0)
    );
    request.expected_revision = next.project.revision;
    let unchanged = next.project.clone();

    std::fs::write(&path, first).unwrap();
    let mut altered = false;
    let error = store
        .import_stable_assisted_export(
            &request,
            |read, _| {
                if read > 0 && !altered {
                    altered = true;
                    std::fs::write(&path, b"changed while copying, same path and longer").unwrap();
                }
                Ok(())
            },
            || false,
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(store.open(&p.project_id).unwrap(), unchanged);

    std::fs::write(&path, second).unwrap();
    assert!(store
        .import_stable_assisted_export(
            &request,
            |_, _| Err(io::Error::other("disk fault")),
            || false
        )
        .is_err());
    assert_eq!(store.open(&p.project_id).unwrap(), unchanged);
    assert!(store
        .import_stable_assisted_export(&request, |_, _| Ok(()), || true)
        .is_err());
    assert_eq!(store.open(&p.project_id).unwrap(), unchanged);
    std::fs::write(&path, b"{\"id\":42,\"messages\":[").unwrap();
    assert!(store
        .import_stable_assisted_export(&request, |_, _| Ok(()), || false)
        .is_err());
    assert_eq!(store.open(&p.project_id).unwrap(), unchanged);
}
