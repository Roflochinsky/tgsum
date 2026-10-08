use super::source;
use serde_json::{json, Value};
use std::{fs, path::Path};
use tgsum_core::project::{Project, ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SnapshotStore;

fn manifest(root: &Path, p: &Project) -> std::path::PathBuf {
    root.join(&p.project_id)
        .join("revisions")
        .join(format!("{:020}.json", p.revision))
}

fn connected(store: &ProjectStore) -> Project {
    let project = store.create("Synthetic Жλ").unwrap();
    store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "source".into(),
                connector_id: "telegram_json".into(),
                scope: source(),
                archive_path: None,
                latest_snapshot_id: None,
                selection: Default::default(),
            }),
        )
        .unwrap()
}

#[test]
fn all_project_schema_upgrades_preserve_old_bytes_and_reject_corrupt_heads() {
    for schema in 1..=11 {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(root.path());
        let project = connected(&store);
        let path = manifest(root.path(), &project);
        let mut legacy = serde_json::to_value(&project).unwrap();
        legacy["schema_version"] = schema.into();
        if schema == 1 {
            let object = legacy.as_object_mut().unwrap();
            object.remove("analysis_run");
            object.remove("baselines");
            object["sources"][0]
                .as_object_mut()
                .unwrap()
                .remove("selection");
        }
        let bytes = serde_json::to_vec(&legacy).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert_eq!(store.open(&project.project_id).unwrap(), project);
        assert_eq!(fs::read(&path).unwrap(), bytes);

        let mut mutations = Vec::new();
        for version in [
            json!(0),
            json!(12),
            json!(u64::MAX),
            json!("9"),
            Value::Null,
        ] {
            let mut value = legacy.clone();
            value["schema_version"] = version;
            mutations.push(value);
        }
        for (pointer, replacement) in [
            ("/project_id", json!("project-elsewhere")),
            ("/revision", json!(project.revision + 1)),
            ("/name", json!("")),
            ("/settings/max_tokens", json!(0)),
            ("/sources/0/scope/account_local_id", json!("")),
            ("/sources/0/scope/conversation_id", json!(["1"])),
        ] {
            let mut value = legacy.clone();
            *value.pointer_mut(pointer).unwrap() = replacement;
            mutations.push(value);
        }
        let mut duplicate = legacy.clone();
        let source = duplicate["sources"][0].clone();
        duplicate["sources"].as_array_mut().unwrap().push(source);
        mutations.push(duplicate);
        for value in mutations {
            let malformed = serde_json::to_vec(&value).unwrap();
            fs::write(&path, &malformed).unwrap();
            assert!(
                store.open(&project.project_id).is_err(),
                "schema {schema}: {value}"
            );
            assert!(store
                .update(
                    &project.project_id,
                    project.revision,
                    ProjectChange::Rename("must not publish".into())
                )
                .is_err());
            assert_eq!(fs::read(&path).unwrap(), malformed);
            assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 2);
        }
        fs::write(&path, &bytes).unwrap();
        let upgraded = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::Rename("Upgraded".into()),
            )
            .unwrap();
        assert_eq!(upgraded.schema_version, 11);
        assert_eq!(upgraded.revision, project.revision + 1);
        assert_eq!(store.open(&project.project_id).unwrap(), upgraded);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn manifest_size_boundary_is_enforced_for_reads_and_failed_publication() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = connected(&store);
    let path = manifest(root.path(), &project);
    let bytes = fs::read(&path).unwrap();
    let mut padded = bytes.clone();
    padded.resize(1 << 20, b' ');
    fs::write(&path, &padded).unwrap();
    assert_eq!(store.open(&project.project_id).unwrap(), project);
    padded.push(b' ');
    fs::write(&path, &padded).unwrap();
    assert!(store
        .open(&project.project_id)
        .unwrap_err()
        .to_string()
        .contains("1 MiB"));
    assert_eq!(fs::read(&path).unwrap(), padded);
    fs::write(&path, &bytes).unwrap();

    let mut oversized = project.sources[0].clone();
    oversized.scope.account_local_id = "x".repeat(1 << 20);
    let error = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(oversized),
        )
        .unwrap_err();
    assert!(error.to_string().contains("1 MiB"));
    assert_eq!(store.open(&project.project_id).unwrap(), project);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 2);
}

#[test]
fn legacy_snapshot_corruption_never_migrates_or_rewrites_its_file() {
    let root = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(root.path());
    let path = root.path().join("before.json");
    let original = include_bytes!("../fixtures/snapshot-v1.json");
    let legacy: Value = serde_json::from_slice(original).unwrap();
    let mut corruptions = Vec::new();
    for (pointer, value) in [
        ("/schema_version", json!(0)),
        ("/schema_version", json!(2)),
        ("/schema_version", json!(3)),
        ("/schema_version", json!(u64::MAX)),
        ("/snapshot_id", json!("elsewhere")),
        ("/source/platform", json!("future")),
        ("/messages/0/key/message_id", json!("")),
        ("/messages/0/key/message_id", json!("id\n")),
        ("/messages/0/key/source/conversation_id", json!("elsewhere")),
    ] {
        let mut value_copy = legacy.clone();
        *value_copy.pointer_mut(pointer).unwrap() = value;
        corruptions.push(serde_json::to_vec(&value_copy).unwrap());
    }
    let mut duplicate = legacy.clone();
    duplicate["messages"][1]["key"] = duplicate["messages"][0]["key"].clone();
    corruptions.push(serde_json::to_vec(&duplicate).unwrap());
    for len in 0..original.len() {
        // The fixture ends with }, so every proper prefix is incomplete.
        corruptions.push(original[..len].to_vec());
    }
    for bytes in corruptions {
        fs::write(&path, &bytes).unwrap();
        assert!(store.load("before").is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    fs::write(&path, original).unwrap();
    let migrated = store.load("before").unwrap();
    assert_eq!(migrated.schema_version, 2);
    assert_eq!(migrated.messages.len(), 4);
    assert_eq!(migrated.messages[3].key.message_id, u64::MAX.to_string());
    assert_eq!(fs::read(&path).unwrap(), original);
}
