use std::fs;

use tgsum_core::bridge::{ClientStatus, ClientUpdate, ExportJob, ExportRequest, ExportState};
use tgsum_core::snapshot::{Snapshot, SnapshotStore, SourceScope};

const BEFORE: &[u8] = include_bytes!("fixtures/telegram-single-before.json");
const AFTER: &[u8] = include_bytes!("fixtures/telegram-single-after.json");

/// Only writes checked-in synthetic bytes and emits simulated client events.
struct SimulatedClient {
    dir: tempfile::TempDir,
}

impl SimulatedClient {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }
    fn job(&self, run_id: &str) -> ExportJob {
        ExportJob::new(ExportRequest {
            run_id: run_id.into(),
            source: SourceScope::telegram("synthetic", "9007199254740993"),
            archive_path: self.dir.path().join(format!("{run_id}.json")),
        })
        .unwrap()
    }
    fn emit(
        job: &mut ExportJob,
        store: &SnapshotStore,
        status: ClientStatus,
    ) -> std::io::Result<Option<Snapshot>> {
        job.apply(
            ClientUpdate {
                request: job.request().clone(),
                status,
            },
            store,
        )
    }
    fn finish(
        job: &mut ExportJob,
        store: &SnapshotStore,
        archive: &[u8],
    ) -> std::io::Result<Snapshot> {
        Self::emit(job, store, ClientStatus::Exporting)?;
        fs::write(&job.request().archive_path, archive)?;
        Ok(Self::emit(job, store, ClientStatus::Completed)?.unwrap())
    }
}

#[test]
fn two_simulated_exports_produce_persistent_deterministic_diff() {
    let client = SimulatedClient::new();
    let data = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(data.path());
    let mut first = client.job("first");
    SimulatedClient::emit(
        &mut first,
        &store,
        ClientStatus::NeedsUserAction("Choose the synthetic chat".into()),
    )
    .unwrap();
    assert!(matches!(first.state(), ExportState::NeedsUserAction(_)));
    let before = SimulatedClient::finish(&mut first, &store, BEFORE).unwrap();
    assert_eq!(first.state(), &ExportState::Ready);
    let mut second = client.job("second");
    let after = SimulatedClient::finish(&mut second, &store, AFTER).unwrap();
    let diff = after.diff(&before).unwrap();
    assert_eq!(
        (
            diff.created.len(),
            diff.edited.len(),
            diff.missing.len(),
            diff.unchanged
        ),
        (1, 1, 1, 2)
    );
    assert_eq!(
        diff,
        store
            .load("second")
            .unwrap()
            .diff(&store.load("first").unwrap())
            .unwrap()
    );
    assert!(SimulatedClient::emit(&mut second, &store, ClientStatus::Completed).is_err());
}

#[test]
fn completion_must_match_attempt_account_chat_and_destination() {
    let client = SimulatedClient::new();
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let mut job = client.job("request");
    fs::write(&job.request().archive_path, BEFORE).unwrap();
    assert!(SimulatedClient::emit(&mut job, &store, ClientStatus::Completed).is_err());
    SimulatedClient::emit(&mut job, &store, ClientStatus::Exporting).unwrap();
    for field in 0..4 {
        let mut request = job.request().clone();
        match field {
            0 => request.run_id = "old-request".into(),
            1 => request.source.account_local_id = "another-account".into(),
            2 => request.source.conversation_id = "another-chat".into(),
            _ => request.archive_path = client.dir.path().join("another-file.json"),
        }
        assert!(job
            .apply(
                ClientUpdate {
                    request,
                    status: ClientStatus::Completed
                },
                &store
            )
            .is_err());
        assert_eq!(job.state(), &ExportState::Exporting);
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    assert!(
        SimulatedClient::emit(&mut job, &store, ClientStatus::Completed)
            .unwrap()
            .is_some()
    );
}

#[test]
fn partial_wrong_scope_failed_and_cancelled_exports_leave_no_snapshot() {
    let client = SimulatedClient::new();
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    for (id, bytes) in [
        ("partial", &BEFORE[..BEFORE.len() / 2]),
        ("wrong-chat", br#"{"id":1,"messages":[]}"#.as_slice()),
    ] {
        let mut job = client.job(id);
        assert!(SimulatedClient::finish(&mut job, &store, bytes).is_err());
        assert!(matches!(job.state(), ExportState::Failed(_)));
        assert!(SimulatedClient::emit(&mut job, &store, ClientStatus::Exporting).is_err());
    }
    for status in [
        ClientStatus::Failed("Client unavailable".into()),
        ClientStatus::Cancelled,
    ] {
        let mut job = client.job("aborted");
        SimulatedClient::emit(&mut job, &store, ClientStatus::Exporting).unwrap();
        SimulatedClient::emit(&mut job, &store, status).unwrap();
        assert!(SimulatedClient::emit(&mut job, &store, ClientStatus::Completed).is_err());
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    let mut retry = client.job("retry-fresh-run");
    SimulatedClient::finish(&mut retry, &store, BEFORE).unwrap();
    assert_eq!(retry.state(), &ExportState::Ready);
}
