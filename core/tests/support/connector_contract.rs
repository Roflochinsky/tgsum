//! Reusable offline contracts. A new importer supplies its native encoding of
//! the documented corpus; all assertions use public store/bridge interfaces.
use std::fs;
use std::io::{self, Cursor, Read};

use tgsum_core::bridge::{ClientStatus, ClientUpdate, ExportJob, ExportRequest, ExportState};
use tgsum_core::connector::{ArchiveImporter, ConnectorDescriptor, SourceAcquisition};
use tgsum_core::snapshot::{
    CoverageLevel, IdentityQuality, SnapshotStore, SourceScope, TimezoneStatus,
};

pub const KEEP: &str = "9007199254740993";
pub const MISSING: &str = "9007199254740994";
pub const CREATED: &str = "3";

pub struct Corpus<'a> {
    pub source: SourceScope,
    pub before: &'a [u8],
    pub after: &'a [u8],
    pub malformed: &'a [u8],
    pub duplicate: &'a [u8],
    pub coverage: CoverageLevel,
    /// Only provide when the native format actually exposes tombstones.
    pub tombstone: Option<&'a [u8]>,
}

struct Fault<'a> {
    input: Cursor<&'a [u8]>,
    at: usize,
}

/// Formats without stable IDs must preserve repeated records without silently
/// matching them across snapshots. Positions/content hashes are not native IDs.
pub fn snapshot_local_contract(importer: &dyn ArchiveImporter, corpus: &Corpus<'_>) {
    assert!(!importer.descriptor().capabilities.stable_message_ids);
    let root = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(root.path());
    let import = |id| {
        store
            .import(
                importer,
                id,
                &corpus.source,
                &mut Cursor::new(corpus.before),
            )
            .unwrap()
    };
    let first = import("first");
    let second = import("second");
    assert_eq!(first.messages.len(), 2);
    assert_eq!(first.messages[0].text, first.messages[1].text);
    assert_ne!(first.messages[0].key, first.messages[1].key);
    assert_eq!(first.coverage.level, corpus.coverage);
    assert_eq!(store.load("first").unwrap(), first);
    for message in first.messages.iter().chain(&second.messages) {
        assert_eq!(
            message.metadata.as_ref().unwrap().identity_quality,
            IdentityQuality::SnapshotLocal
        );
    }
    assert!(second.diff(&first).is_err());
    assert_eq!(first.diff(&first).unwrap().unchanged, 2);
}
impl Read for Fault<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let remaining = self.at.saturating_sub(self.input.position() as usize);
        if remaining == 0 {
            return Err(io::Error::other("synthetic interrupted input"));
        }
        let count = buffer.len().min(remaining);
        self.input.read(&mut buffer[..count])
    }
}

pub fn archive_contract(importer: &dyn ArchiveImporter, corpus: &Corpus<'_>) {
    let root = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(root.path());
    let import = |id, bytes| store.import(importer, id, &corpus.source, &mut Cursor::new(bytes));
    let before = import("before", corpus.before).unwrap();
    assert_eq!(
        before.messages.len(),
        2,
        "same text must not merge identities"
    );
    assert_eq!(before.coverage.level, corpus.coverage);
    assert_eq!(store.load("before").unwrap(), before);
    let keep = before
        .messages
        .iter()
        .find(|m| m.key.message_id == KEEP)
        .unwrap();
    let missing = before
        .messages
        .iter()
        .find(|m| m.key.message_id == MISSING)
        .unwrap();
    assert_eq!(keep.text, missing.text);
    assert_eq!(keep.timestamp.as_deref(), Some("1970-01-01T03:00:00+03:00"));
    assert_eq!(
        keep.metadata
            .as_ref()
            .unwrap()
            .timestamp
            .utc
            .unwrap()
            .seconds,
        0
    );
    assert_eq!(missing.timestamp.as_deref(), Some("2026-06-18T10:00:00"));
    assert_eq!(
        missing.metadata.as_ref().unwrap().timestamp.timezone_status,
        TimezoneStatus::UnknownTimezone
    );
    assert!(missing.metadata.as_ref().unwrap().timestamp.utc.is_none());
    for message in &before.messages {
        assert_eq!(message.key.source, corpus.source);
        assert_eq!(
            message.metadata.as_ref().unwrap().identity_quality,
            IdentityQuality::Native
        );
    }
    let repeat = import("repeat", corpus.before).unwrap();
    let delta = repeat.diff(&before).unwrap();
    assert_eq!(delta.unchanged, 2);
    assert!(
        delta.created.is_empty()
            && delta.edited.is_empty()
            && delta.missing.is_empty()
            && delta.deleted.is_empty()
    );

    let after = import("after", corpus.after).unwrap();
    let delta = after.diff(&before).unwrap();
    assert_eq!(
        delta
            .created
            .iter()
            .map(|k| k.message_id.as_str())
            .collect::<Vec<_>>(),
        [CREATED]
    );
    assert_eq!(
        delta
            .edited
            .iter()
            .map(|k| k.message_id.as_str())
            .collect::<Vec<_>>(),
        [KEEP]
    );
    assert_eq!(
        delta
            .missing
            .iter()
            .map(|k| k.message_id.as_str())
            .collect::<Vec<_>>(),
        [MISSING]
    );
    assert!(
        delta.deleted.is_empty(),
        "absence must never become deletion"
    );
    assert_eq!(after.coverage.level, corpus.coverage);
    assert!(
        import("before", corpus.after).is_err(),
        "immutable checkpoint overwritten"
    );
    assert_eq!(store.load("before").unwrap(), before);

    let mut other = corpus.source.clone();
    other.account_local_id.push_str("-other");
    let other = store
        .import(
            importer,
            "other-account",
            &other,
            &mut Cursor::new(corpus.before),
        )
        .unwrap();
    assert_ne!(before.messages[0].key, other.messages[0].key);
    assert!(other.diff(&before).is_err());
    let mut wrong = corpus.source.clone();
    wrong.platform.push_str("-wrong");
    assert!(store
        .import(
            importer,
            "wrong-platform",
            &wrong,
            &mut Cursor::new(corpus.before)
        )
        .is_err());
    assert!(store.load("wrong-platform").is_err());

    for (id, bytes) in [
        ("malformed", corpus.malformed),
        ("duplicate", corpus.duplicate),
    ] {
        assert!(import(id, bytes).is_err());
        assert!(
            store.load(id).is_err(),
            "invalid attempt published a checkpoint"
        );
        assert_eq!(store.load("before").unwrap(), before);
        assert!(
            import(id, corpus.after).is_ok(),
            "failed publication prevented retry"
        );
    }
    // Fail during input and at the EOF probe, including after selected records.
    for at in [corpus.before.len() / 2, corpus.before.len()] {
        let mut reader = Fault {
            input: Cursor::new(corpus.before),
            at,
        };
        assert!(store
            .import(importer, "io-failure", &corpus.source, &mut reader)
            .is_err());
        assert!(store.load("io-failure").is_err());
        assert_eq!(store.load("before").unwrap(), before);
    }
    assert!(import("io-failure", corpus.before).is_ok());
    if let Some(bytes) = corpus.tombstone {
        let deleted = import("explicit-deletion", bytes).unwrap();
        let delta = deleted.diff(&before).unwrap();
        assert_eq!(
            delta
                .deleted
                .iter()
                .map(|k| k.message_id.as_str())
                .collect::<Vec<_>>(),
            [KEEP]
        );
        assert_eq!(
            delta
                .missing
                .iter()
                .map(|k| k.message_id.as_str())
                .collect::<Vec<_>>(),
            [MISSING]
        );
        assert!(delta.edited.is_empty());
    }
}

struct ScriptedClient<'a> {
    descriptor: ConnectorDescriptor,
    bytes: &'a [u8],
}
impl SourceAcquisition for ScriptedClient<'_> {
    fn descriptor(&self) -> ConnectorDescriptor {
        self.descriptor
    }
    fn start(&mut self, request: &ExportRequest) -> io::Result<ClientUpdate> {
        // A complete readable file while the client is exporting is not a
        // completion signal and must not advance the published snapshot.
        fs::write(&request.archive_path, self.bytes)?;
        Ok(ClientUpdate {
            request: request.clone(),
            status: ClientStatus::Exporting,
        })
    }
    fn poll(&mut self, request: &ExportRequest) -> io::Result<Option<ClientUpdate>> {
        Ok(Some(ClientUpdate {
            request: request.clone(),
            status: ClientStatus::Completed,
        }))
    }
    fn cancel(&mut self, request: &ExportRequest) -> io::Result<ClientUpdate> {
        Ok(ClientUpdate {
            request: request.clone(),
            status: ClientStatus::Cancelled,
        })
    }
}

/// Qualifies the common lifecycle with this normalizer, not a real client or
/// remote API cursor. Provider-specific acquisition tests remain necessary.
pub fn acquisition_contract(importer: &dyn ArchiveImporter, corpus: &Corpus<'_>) {
    let root = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(root.path().join("snapshots"));
    let request = ExportRequest {
        run_id: "attempt".into(),
        source: corpus.source.clone(),
        archive_path: root.path().join("export"),
    };
    let mut driver = ScriptedClient {
        descriptor: importer.descriptor(),
        bytes: corpus.before,
    };
    let mut job = ExportJob::for_importer(request.clone(), importer).unwrap();
    let completed = driver.poll(&request).unwrap().unwrap();
    assert!(job
        .apply_with_importer(completed.clone(), &store, importer)
        .is_err());
    let started = driver.start(&request).unwrap();
    assert!(job
        .apply_with_importer(started, &store, importer)
        .unwrap()
        .is_none());
    assert!(request.archive_path.exists() && store.load(&request.run_id).is_err());
    for field in ["run", "account", "conversation", "path"] {
        let mut stale = completed.clone();
        match field {
            "run" => stale.request.run_id.push_str("-stale"),
            "account" => stale.request.source.account_local_id.push_str("-stale"),
            "conversation" => stale.request.source.conversation_id.push_str("-stale"),
            _ => stale.request.archive_path = root.path().join("other"),
        }
        assert!(job.apply_with_importer(stale, &store, importer).is_err());
        assert_eq!(job.state(), &ExportState::Exporting);
        assert!(store.load(&request.run_id).is_err());
    }
    let cancel = driver.cancel(&request).unwrap();
    assert!(job
        .apply_with_importer(cancel, &store, importer)
        .unwrap()
        .is_none());
    assert_eq!(job.state(), &ExportState::Cancelled);
    assert!(job
        .apply_with_importer(completed, &store, importer)
        .is_err());
    assert!(store.load(&request.run_id).is_err());

    let mut failed_request = request.clone();
    failed_request.run_id = "failed-archive".into();
    let mut failed_job = ExportJob::for_importer(failed_request.clone(), importer).unwrap();
    driver.bytes = corpus.malformed;
    let started = driver.start(&failed_request).unwrap();
    assert!(failed_job
        .apply_with_importer(started, &store, importer)
        .unwrap()
        .is_none());
    let completed = driver.poll(&failed_request).unwrap().unwrap();
    assert!(failed_job
        .apply_with_importer(completed.clone(), &store, importer)
        .is_err());
    assert!(matches!(failed_job.state(), ExportState::Failed(_)));
    assert!(store.load(&failed_request.run_id).is_err());
    assert!(failed_job
        .apply_with_importer(completed, &store, importer)
        .is_err());

    driver.bytes = corpus.before;
    let mut retry = request.clone();
    retry.run_id = "retry".into();
    let mut job = ExportJob::for_importer(retry.clone(), importer).unwrap();
    let started = driver.start(&retry).unwrap();
    assert!(job
        .apply_with_importer(started, &store, importer)
        .unwrap()
        .is_none());
    let completed = driver.poll(&retry).unwrap().unwrap();
    let snapshot = job
        .apply_with_importer(completed.clone(), &store, importer)
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(job.state(), &ExportState::Ready);
    assert_eq!(store.load("retry").unwrap(), snapshot);
    assert!(job
        .apply_with_importer(completed, &store, importer)
        .is_err());
}
