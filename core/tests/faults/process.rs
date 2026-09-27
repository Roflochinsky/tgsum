use super::{encode, source};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use tgsum_core::connector::{
    ArchiveImporter, ConnectorDescriptor, ConversationObservation, TelegramJson,
};
use tgsum_core::project::{Project, ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::{Snapshot, SnapshotStore, SourceScope};

const ROOT: &str = "TGSUM_FAULT_WORKER_ROOT";
const MODE: &str = "TGSUM_FAULT_WORKER_MODE";
const WORKER: &str = "TGSUM_FAULT_WORKER_ID";
const DEADLINE: Duration = Duration::from_secs(30);

struct Worker {
    child: Child,
    log: PathBuf,
}
impl Worker {
    fn start(root: &Path, mode: &str, id: usize) -> Self {
        let path = root.join(format!("worker-{id}.log"));
        let log = File::create(&path).unwrap();
        Self {
            child: Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "process::subprocess_entry", "--nocapture"])
                .env(ROOT, root)
                .env(MODE, mode)
                .env(WORKER, id.to_string())
                .stdin(Stdio::null())
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap(),
            log: path,
        }
    }

    fn log_text(&self) -> String {
        let mut text = String::new();
        File::open(&self.log)
            .unwrap()
            .take(64 * 1024)
            .read_to_string(&mut text)
            .unwrap();
        text
    }

    fn wait(&mut self) -> ExitStatus {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(
                    matches!(status.code(), Some(0 | 73 | 74)),
                    "worker failed: {}",
                    self.log_text()
                );
                return status;
            }
            assert!(start.elapsed() < DEADLINE, "synthetic worker timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Reap our own child even when an assertion or startup deadline fails.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn project(root: &Path) -> (ProjectStore, Project) {
    let store = ProjectStore::new(root.join("projects"));
    let project = store.create("Before worker").unwrap();
    let project = store
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
        .unwrap();
    fs::write(root.join("project-id"), &project.project_id).unwrap();
    (store, project)
}

fn manifest(root: &Path, project: &Project) -> Vec<u8> {
    fs::read(
        root.join("projects")
            .join(&project.project_id)
            .join("revisions")
            .join(format!("{:020}.json", project.revision)),
    )
    .unwrap()
}

struct ExitAfterStaging;
impl ArchiveImporter for ExitAfterStaging {
    fn descriptor(&self) -> ConnectorDescriptor {
        TelegramJson.descriptor()
    }
    fn normalize(
        &self,
        reader: &mut dyn Read,
        scope: &SourceScope,
        emit: &mut dyn FnMut(ConversationObservation) -> io::Result<()>,
    ) -> io::Result<()> {
        TelegramJson.normalize(reader, scope, emit)?;
        // The public store callback has serialized/flushed the observation.
        // Exit without unwinding: the NamedTempFile destructor cannot clean up.
        // This models process termination, not a kernel crash or power failure.
        std::process::exit(73)
    }
}

#[test]
fn subprocess_entry() {
    let Some(mode) = std::env::var_os(MODE) else {
        return;
    };
    let root = std::fs::canonicalize(std::env::var_os(ROOT).unwrap()).unwrap();
    assert_eq!(
        root.parent().unwrap(),
        std::env::temp_dir().canonicalize().unwrap()
    );
    assert!(root
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("tgsum-faults-"));
    let id: usize = std::env::var(WORKER).unwrap().parse().unwrap();
    assert!(id < 4);
    let store = ProjectStore::new(root.join("projects"));
    let project = store
        .open(fs::read_to_string(root.join("project-id")).unwrap().trim())
        .unwrap();
    let snapshots = store.snapshots(&project.project_id).unwrap();
    let bytes = encode(&[format!("worker-{id}")]);
    match mode.to_str().unwrap() {
        "crash-staged" => {
            let _ = snapshots.import(
                &ExitAfterStaging,
                "interrupted",
                &source(),
                &mut bytes.as_slice(),
            );
            panic!("worker did not reach the staged observation");
        }
        "crash-published" => {
            snapshots
                .import_telegram("interrupted", &source(), bytes.as_slice())
                .unwrap();
            std::process::exit(74); // no Project checkpoint has been advanced
        }
        "race-snapshot" | "race-project" => {
            fs::write(root.join(format!("ready-{id}")), b"ready").unwrap();
            let start = Instant::now();
            while !root.join("go").exists() {
                assert!(start.elapsed() < DEADLINE, "barrier timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
            let result = if mode == "race-snapshot" {
                snapshots
                    .import_telegram("race", &source(), bytes.as_slice())
                    .map(|_| ())
            } else {
                store
                    .update(
                        &project.project_id,
                        project.revision,
                        ProjectChange::Rename(format!("worker-{id}")),
                    )
                    .map(|_| ())
            };
            let outcome = match result {
                Ok(()) => "won",
                Err(e) if mode == "race-snapshot" && e.kind() == io::ErrorKind::AlreadyExists => {
                    "conflict"
                }
                Err(e) if mode == "race-project" && e.kind() == io::ErrorKind::WouldBlock => {
                    "conflict"
                }
                Err(e) => panic!("unexpected writer error: {e}"),
            };
            fs::write(root.join(format!("outcome-{id}")), outcome).unwrap();
        }
        _ => panic!("unknown synthetic worker mode"),
    }
}

#[test]
fn restart_ignores_staged_files_and_does_not_adopt_uncheckpointed_snapshots() {
    for (mode, code) in [("crash-staged", 73), ("crash-published", 74)] {
        let root = tempfile::Builder::new()
            .prefix("tgsum-faults-")
            .tempdir()
            .unwrap();
        let (store, project) = project(root.path());
        let snapshots = store.snapshots(&project.project_id).unwrap();
        let bytes = encode(&["committed".into()]);
        snapshots
            .import_telegram("committed", &source(), bytes.as_slice())
            .unwrap();
        let project = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::RecordSnapshot {
                    source_id: "source".into(),
                    snapshot_id: "committed".into(),
                },
            )
            .unwrap();
        let previous_manifest = manifest(root.path(), &project);
        let previous_snapshot = fs::read(snapshots.directory().join("committed.json")).unwrap();
        let mut worker = Worker::start(root.path(), mode, 0);
        assert_eq!(worker.wait().code(), Some(code));

        let restarted = ProjectStore::new(root.path().join("projects"));
        assert_eq!(restarted.open(&project.project_id).unwrap(), project);
        assert_eq!(manifest(root.path(), &project), previous_manifest);
        assert_eq!(
            fs::read(snapshots.directory().join("committed.json")).unwrap(),
            previous_snapshot
        );
        if mode == "crash-staged" {
            assert!(snapshots.load("interrupted").is_err());
            let staged = fs::read_dir(snapshots.directory())
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.file_name().unwrap() != "committed.json")
                .collect::<Vec<_>>();
            assert_eq!(
                staged.len(),
                1,
                "process exit must leave the actual staging artifact"
            );
            let uncommitted: Snapshot =
                serde_json::from_slice(&fs::read(&staged[0]).unwrap()).unwrap();
            assert_eq!(uncommitted.snapshot_id, "interrupted");
            // A new import can publish the same intended ID. The old staging
            // name is neither adopted nor silently deleted during restart.
            snapshots
                .import_telegram("interrupted", &source(), bytes.as_slice())
                .unwrap();
            assert!(staged[0].exists());
        } else {
            assert_eq!(
                snapshots.load("interrupted").unwrap().messages[0].text,
                "worker-0"
            );
            assert_eq!(
                snapshots
                    .import_telegram("interrupted", &source(), bytes.as_slice())
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::AlreadyExists
            );
        }
        let updated = restarted
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::RecordSnapshot {
                    source_id: "source".into(),
                    snapshot_id: "interrupted".into(),
                },
            )
            .unwrap();
        assert_eq!(
            updated.sources[0].latest_snapshot_id.as_deref(),
            Some("interrupted")
        );
    }
}

#[test]
fn independent_process_writers_publish_one_winner_without_overwriting_history() {
    for mode in ["race-snapshot", "race-project"] {
        let root = tempfile::Builder::new()
            .prefix("tgsum-faults-")
            .tempdir()
            .unwrap();
        let (store, project) = project(root.path());
        let original = manifest(root.path(), &project);
        let mut workers = (0..4)
            .map(|id| Worker::start(root.path(), mode, id))
            .collect::<Vec<_>>();
        let start = Instant::now();
        while !(0..4).all(|id| root.path().join(format!("ready-{id}")).exists()) {
            for worker in &mut workers {
                assert!(
                    worker.child.try_wait().unwrap().is_none(),
                    "worker exited before barrier: {}",
                    worker.log_text()
                );
            }
            assert!(start.elapsed() < DEADLINE, "workers did not reach barrier");
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(root.path().join("go"), b"go").unwrap();
        for worker in &mut workers {
            assert!(worker.wait().success());
        }
        let outcomes = (0..4)
            .map(|id| fs::read_to_string(root.path().join(format!("outcome-{id}"))).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(outcomes.iter().filter(|s| *s == "won").count(), 1);
        assert_eq!(outcomes.iter().filter(|s| *s == "conflict").count(), 3);
        let winner = outcomes.iter().position(|s| s == "won").unwrap();
        assert_eq!(manifest(root.path(), &project), original);
        if mode == "race-snapshot" {
            let snapshots: SnapshotStore = store.snapshots(&project.project_id).unwrap();
            assert_eq!(
                snapshots.load("race").unwrap().messages[0].text,
                format!("worker-{winner}")
            );
            assert_eq!(fs::read_dir(snapshots.directory()).unwrap().count(), 1);
            assert_eq!(store.open(&project.project_id).unwrap(), project);
        } else {
            let head = store.open(&project.project_id).unwrap();
            assert_eq!(head.revision, project.revision + 1);
            assert_eq!(head.name, format!("worker-{winner}"));
            assert_eq!(
                fs::read_dir(
                    root.path()
                        .join("projects")
                        .join(&project.project_id)
                        .join("revisions")
                )
                .unwrap()
                .count(),
                3
            );
        }
    }
}
