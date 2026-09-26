use std::fs;
use std::io::{self, Cursor};
use std::path::Path;
use std::sync::{Arc, Barrier};

use tgsum_core::project::{
    Project, ProjectChange, ProjectEntry, ProjectSettings, ProjectSource, ProjectStore,
    SourceAvailability,
};
use tgsum_core::snapshot::SourceScope;

fn source(path: &Path) -> ProjectSource {
    ProjectSource {
        source_id: "client".into(),
        connector_id: "telegram_export".into(),
        scope: SourceScope::telegram("work", "1"),
        archive_path: Some(path.to_owned()),
        latest_snapshot_id: None,
        selection: Default::default(),
    }
}

fn manifest(root: &Path, project: &Project) -> std::path::PathBuf {
    root.join(&project.project_id)
        .join("revisions")
        .join(format!("{:020}.json", project.revision))
}

#[test]
fn version_one_opens_with_default_scope_without_rewriting_old_revision() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let created = store.create("legacy project").unwrap();
    let project = store
        .update(
            &created.project_id,
            created.revision,
            ProjectChange::Source(source(&root.path().join("export.json"))),
        )
        .unwrap();
    let path = manifest(root.path(), &project);
    let mut legacy = serde_json::to_value(&project).unwrap();
    legacy["schema_version"] = 1.into();
    let object = legacy.as_object_mut().unwrap();
    object.remove("analysis_run");
    object.remove("baselines");
    object["sources"][0]
        .as_object_mut()
        .unwrap()
        .remove("selection");
    let legacy_bytes = serde_json::to_vec(&legacy).unwrap();
    fs::write(&path, &legacy_bytes).unwrap();

    let reopened = store.open(&project.project_id).unwrap();
    assert_eq!(reopened.schema_version, 2);
    assert_eq!(reopened.sources[0].selection, Default::default());
    assert!(reopened.analysis_run.is_none() && reopened.baselines.is_empty());
    assert_eq!(fs::read(&path).unwrap(), legacy_bytes);
    let updated = store
        .update(
            &project.project_id,
            reopened.revision,
            ProjectChange::Rename("upgraded".into()),
        )
        .unwrap();
    assert_eq!(updated.schema_version, 2);
    assert_eq!(updated.revision, reopened.revision + 1);
    assert_eq!(store.open(&project.project_id).unwrap(), updated);
    assert_eq!(fs::read(path).unwrap(), legacy_bytes);
}

#[test]
fn project_survives_restart_and_never_copies_the_original_archive() {
    let root = tempfile::tempdir().unwrap();
    let archives = tempfile::tempdir().unwrap();
    let archive = archives.path().join("result.json");
    fs::write(&archive, "SYNTHETIC PRIVATE ARCHIVE CONTENT").unwrap();
    let store = ProjectStore::new(root.path());
    let created = store.create("  Клиент ACME  ").unwrap();
    assert_eq!(created.name, "Клиент ACME");
    assert_eq!(created.settings.default_agent, "export_only");
    let attached = store
        .update(
            &created.project_id,
            0,
            ProjectChange::Source(source(&archive)),
        )
        .unwrap();
    let renamed = store
        .update(
            &created.project_id,
            attached.revision,
            ProjectChange::Rename("Pilot".into()),
        )
        .unwrap();
    let settings = ProjectSettings {
        max_tokens: None,
        default_recipe: "retro".into(),
        ..Default::default()
    };
    let saved = store
        .update(
            &created.project_id,
            renamed.revision,
            ProjectChange::Settings(settings.clone()),
        )
        .unwrap();
    let reopened = ProjectStore::new(root.path())
        .open(&created.project_id)
        .unwrap();
    assert_eq!(reopened, saved);
    assert_eq!(reopened.settings, settings);
    assert_eq!(
        reopened.sources[0].availability(),
        SourceAvailability::FileAvailable
    );
    fs::rename(&archive, archives.path().join("moved.json")).unwrap();
    let reopened = store.open(&created.project_id).unwrap();
    assert_eq!(
        reopened.sources[0].availability(),
        SourceAvailability::FileMissing
    );
    assert_eq!(
        fs::read_dir(root.path().join(&created.project_id))
            .unwrap()
            .count(),
        1
    );
    for entry in fs::read_dir(root.path().join(&created.project_id).join("revisions")).unwrap() {
        assert!(!fs::read_to_string(entry.unwrap().path())
            .unwrap()
            .contains("SYNTHETIC PRIVATE"));
    }
    assert_eq!(store.open(&created.project_id).unwrap().revision, 3);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(root.path().join(&created.project_id))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(manifest(root.path(), &saved))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn only_matching_project_snapshots_can_be_referenced() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("snapshots").unwrap();
    let selected = source(&root.path().join("not-required.json"));
    let snapshot_store = store.snapshots(&project.project_id).unwrap();
    let data = br#"{"id":1,"messages":[{"id":7,"text":"synthetic"}]}"#;
    snapshot_store
        .import_telegram("correct", &selected.scope, Cursor::new(data))
        .unwrap();
    snapshot_store
        .import_telegram(
            "wrong-account",
            &SourceScope::telegram("personal", "1"),
            Cursor::new(data),
        )
        .unwrap();
    let project = store
        .update(&project.project_id, 0, ProjectChange::Source(selected))
        .unwrap();
    for invalid in ["absent", "../escape", "wrong-account"] {
        assert!(store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::RecordSnapshot {
                    source_id: "client".into(),
                    snapshot_id: invalid.into(),
                }
            )
            .is_err());
        assert_eq!(store.open(&project.project_id).unwrap(), project);
    }
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::RecordSnapshot {
                source_id: "client".into(),
                snapshot_id: "correct".into(),
            },
        )
        .unwrap();
    assert_eq!(
        project.sources[0].latest_snapshot_id.as_deref(),
        Some("correct")
    );
    let mut wrong_scope = project.sources[0].clone();
    wrong_scope.scope.account_local_id = "another".into();
    assert!(store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(wrong_scope)
        )
        .is_err());
    // Configuration edits remain possible even if an old snapshot is damaged.
    // Loading that snapshot still reports its own error; renames never read it.
    fs::write(snapshot_store.directory().join("correct.json"), "broken").unwrap();
    let renamed = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Rename("renamed".into()),
        )
        .unwrap();
    let removed = store
        .update(
            &project.project_id,
            renamed.revision,
            ProjectChange::RemoveSource("client".into()),
        )
        .unwrap();
    assert!(removed.sources.is_empty());
    assert!(snapshot_store.directory().join("correct.json").exists());
}

#[test]
fn concurrent_editors_cannot_lose_a_committed_update() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("original").unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let results = std::thread::scope(|threads| {
        let handles: Vec<_> = ["first", "second"]
            .into_iter()
            .map(|name| {
                let barrier = barrier.clone();
                let store = &store;
                let id = &project.project_id;
                threads.spawn(move || {
                    barrier.wait();
                    store.update(id, 0, ProjectChange::Rename(name.into()))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .find_map(|r| r.as_ref().err())
            .unwrap()
            .kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(
        store.open(&project.project_id).unwrap(),
        *results.iter().find_map(|r| r.as_ref().ok()).unwrap()
    );
    assert_eq!(
        fs::read_to_string(manifest(root.path(), &project)).unwrap(),
        serde_json::to_string(&project).unwrap()
    );
}

#[test]
fn corruption_and_unknown_versions_are_visible_without_rewriting_data() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let good = store.create("good").unwrap();
    let broken = store.create("broken").unwrap();
    let future = store.create("future").unwrap();
    let broken_path = manifest(root.path(), &broken);
    fs::write(&broken_path, "{unfinished").unwrap();
    let future_path = manifest(root.path(), &future);
    let future_data = r#"{"schema_version":42,"next_generation_field":true}"#;
    fs::write(&future_path, future_data).unwrap();
    assert!(store
        .open(&future.project_id)
        .unwrap_err()
        .to_string()
        .contains("unsupported project schema"));
    let entries = store.list().unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|e| matches!(e, ProjectEntry::Ready { .. }))
            .count(),
        1
    );
    assert_eq!(
        entries
            .iter()
            .filter(|e| matches!(e, ProjectEntry::Unavailable { .. }))
            .count(),
        2
    );
    assert_eq!(store.open(&good.project_id).unwrap(), good);
    assert_eq!(fs::read_to_string(future_path).unwrap(), future_data);
    assert_eq!(fs::read_to_string(broken_path).unwrap(), "{unfinished");
    // Unpublished staging files are ignored; a corrupt committed head is not.
    let revisions = root.path().join(&good.project_id).join("revisions");
    fs::write(revisions.join(".staging"), "incomplete").unwrap();
    assert_eq!(store.open(&good.project_id).unwrap(), good);
    fs::write(revisions.join("00000000000000000001.json"), "incomplete").unwrap();
    assert!(store.open(&good.project_id).is_err());
}

#[test]
fn invalid_changes_and_external_paths_do_not_publish_revisions() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    for name in ["", "  ", "hello\nworld"] {
        assert!(store.create(name).is_err());
    }
    let project = store.create("valid").unwrap();
    let mut invalid_source = source(Path::new("relative.json"));
    let changes = [
        ProjectChange::Rename("".into()),
        ProjectChange::Source(invalid_source.clone()),
        ProjectChange::RemoveSource("unknown".into()),
        ProjectChange::Settings(ProjectSettings {
            max_tokens: Some(0),
            ..Default::default()
        }),
    ];
    for change in changes {
        assert!(store.update(&project.project_id, 0, change).is_err());
        assert_eq!(store.open(&project.project_id).unwrap(), project);
    }
    invalid_source.archive_path = None;
    invalid_source.source_id = "../escape".into();
    assert!(store
        .update(
            &project.project_id,
            0,
            ProjectChange::Source(invalid_source)
        )
        .is_err());
    for invalid in ["../escape", "/outside", "project-../escape", "NUL"] {
        assert!(store.open(invalid).is_err());
        assert!(store.snapshots(invalid).is_err());
    }
    assert_eq!(
        fs::read_dir(root.path().join(&project.project_id).join("revisions"))
            .unwrap()
            .count(),
        1
    );
    fs::write(manifest(root.path(), &project), vec![b' '; (1 << 20) + 1]).unwrap();
    assert!(store
        .open(&project.project_id)
        .unwrap_err()
        .to_string()
        .contains("1 MiB"));
}

#[cfg(unix)]
#[test]
fn project_storage_rejects_symlink_substitutions() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    symlink(outside.path(), root.path().join("project-link")).unwrap();
    assert!(store.open("project-link").is_err());
    let project = store.create("valid").unwrap();
    symlink(
        outside.path(),
        root.path().join(&project.project_id).join("snapshots"),
    )
    .unwrap();
    assert!(store.snapshots(&project.project_id).is_err());
    let path = manifest(root.path(), &project);
    fs::remove_file(&path).unwrap();
    let external = outside.path().join("manifest.json");
    fs::write(&external, serde_json::to_vec(&project).unwrap()).unwrap();
    symlink(external, path).unwrap();
    assert!(store.open(&project.project_id).is_err());
}
