use std::fs;
use std::time::{Duration, Instant};

use tgsum_core::assisted::AssistedExportSettings;
use tgsum_core::export_inbox::ExportInbox;
use tgsum_core::project::{ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;

#[test]
fn observes_only_selected_folder_and_debounces_candidates_without_importing() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("selected");
    fs::create_dir(&directory).unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("result.json"), b"other").unwrap();
    let direct = directory.join("result.json");
    fs::write(&direct, b"partial").unwrap();
    let nested = directory.join("ChatExport_2026-09-27");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("result.json"), b"nested").unwrap();
    let unrelated = directory.join("Unrelated");
    fs::create_dir(&unrelated).unwrap();
    fs::write(unrelated.join("result.json"), b"unrelated").unwrap();

    let store = ProjectStore::new(root.path().join("projects"));
    let project = store.create("Selected").unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "chat".into(),
                connector_id: "telegram_json".into(),
                scope: SourceScope::telegram("work", "42"),
                archive_path: None,
                latest_snapshot_id: None,
                selection: Default::default(),
            }),
        )
        .unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::AssistedExport {
                source_id: "chat".into(),
                settings: Some(AssistedExportSettings {
                    directory,
                    client: None,
                }),
            },
        )
        .unwrap();
    let t0 = Instant::now();
    let mut inbox = ExportInbox::new(Duration::from_secs(2));
    assert!(inbox.poll(&project, "chat", t0).unwrap().is_empty());
    assert!(inbox
        .poll(&project, "chat", t0 + Duration::from_secs(1))
        .unwrap()
        .is_empty());
    let candidates = inbox
        .poll(&project, "chat", t0 + Duration::from_secs(2))
        .unwrap();
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().any(|c| c.path == direct));
    assert!(candidates
        .iter()
        .any(|c| c.path == nested.join("result.json")));
    assert!(project.sources[0].latest_snapshot_id.is_none());
    assert_eq!(store.open(&project.project_id).unwrap(), project);

    fs::write(&direct, b"still partial and growing").unwrap();
    let changed = inbox
        .poll(&project, "chat", t0 + Duration::from_secs(3))
        .unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].path, nested.join("result.json"));
    fs::remove_file(nested.join("result.json")).unwrap();
    assert!(inbox
        .poll(&project, "chat", t0 + Duration::from_secs(4))
        .unwrap()
        .is_empty());
    assert_eq!(
        inbox
            .poll(&project, "chat", t0 + Duration::from_secs(5))
            .unwrap()
            .len(),
        1
    );
}

#[cfg(unix)]
#[test]
fn ignores_symlinked_export_candidate() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("selected");
    fs::create_dir(&folder).unwrap();
    fs::write(root.path().join("outside.json"), b"outside").unwrap();
    symlink(root.path().join("outside.json"), folder.join("result.json")).unwrap();
    let store = ProjectStore::new(root.path().join("projects"));
    let project = store.create("Selected").unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "chat".into(),
                connector_id: "telegram_json".into(),
                scope: SourceScope::telegram("work", "42"),
                archive_path: None,
                latest_snapshot_id: None,
                selection: Default::default(),
            }),
        )
        .unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::AssistedExport {
                source_id: "chat".into(),
                settings: Some(AssistedExportSettings {
                    directory: folder,
                    client: None,
                }),
            },
        )
        .unwrap();
    let now = Instant::now();
    let mut inbox = ExportInbox::new(Duration::ZERO);
    assert!(inbox.poll(&project, "chat", now).unwrap().is_empty());
}
