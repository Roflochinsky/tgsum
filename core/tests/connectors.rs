use std::fs;
use std::io::{self, BufRead, BufReader, Cursor, Read};

use tgsum_core::bridge::{ClientStatus, ClientUpdate, ExportJob, ExportRequest, ExportState};
use tgsum_core::connector::{
    ArchiveImporter, AttachmentAccess, ConnectorCapabilities, ConnectorDescriptor, ConnectorKind,
    ConversationObservation, CredentialKind, MessageObservation, RefreshMethod, SourceAcquisition,
    TelegramJson,
};
use tgsum_core::snapshot::{
    CanonicalMessage, Coverage, CoverageLevel, DeletionState, IdentityQuality, MessageKey,
    SnapshotStore, SourceScope,
};

fn scope() -> SourceScope {
    SourceScope {
        platform: "synthetic".into(),
        account_local_id: "fixture".into(),
        conversation_id: "chosen".into(),
    }
}

fn descriptor() -> ConnectorDescriptor {
    ConnectorDescriptor {
        id: "synthetic_lines",
        revision: "fixture-1",
        platform: "synthetic",
        kind: ConnectorKind::LocalData,
        format_id: "synthetic_tab_records",
        capabilities: ConnectorCapabilities {
            history: true,
            refresh: RefreshMethod::Reimport,
            stable_message_ids: true,
            attachments: AttachmentAccess::None,
            credentials: CredentialKind::None,
        },
    }
}

fn observation(source: SourceScope, rows: &[(&str, &str)]) -> ConversationObservation {
    ConversationObservation {
        messages: rows
            .iter()
            .map(|(id, text)| {
                MessageObservation::native_present(CanonicalMessage::new(
                    MessageKey {
                        source: source.clone(),
                        message_id: (*id).into(),
                    },
                    *text,
                ))
            })
            .collect(),
        source,
        title: Some("Synthetic conversation".into()),
        kind: "group".into(),
        coverage: Coverage::unknown("Synthetic fixture does not claim full history"),
    }
}

struct Lines;
impl ArchiveImporter for Lines {
    fn descriptor(&self) -> ConnectorDescriptor {
        descriptor()
    }
    fn normalize(
        &self,
        reader: &mut dyn Read,
        source: &SourceScope,
        emit: &mut dyn FnMut(ConversationObservation) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut selected = observation(source.clone(), &[]);
        for line in BufReader::new(reader).lines() {
            let line = line?;
            let (id, text) = line.split_once('\t').ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "missing record separator")
            })?;
            selected
                .messages
                .push(MessageObservation::native_present(CanonicalMessage::new(
                    MessageKey {
                        source: source.clone(),
                        message_id: id.into(),
                    },
                    text,
                )));
        }
        emit(selected)
    }
}

#[test]
fn second_adapter_uses_the_same_atomic_store_and_diff() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let first = store
        .import(
            &Lines,
            "first",
            &scope(),
            &mut Cursor::new("a\tsame text\nb\tsame text\n"),
        )
        .unwrap();
    let second = store
        .import(
            &Lines,
            "second",
            &scope(),
            &mut Cursor::new("a\tedited\nc\tnew\n"),
        )
        .unwrap();
    assert_eq!(store.load("first").unwrap(), first);
    assert_eq!(first.messages.len(), 2);
    assert_eq!(
        first.metadata.as_ref().unwrap().connector_id,
        "synthetic_lines"
    );
    assert_eq!(
        first.metadata.as_ref().unwrap().acquisition_method,
        "local_data"
    );
    assert_eq!(first.coverage.level, CoverageLevel::Unknown);
    let delta = second.diff(&first).unwrap();
    assert_eq!(delta.created[0].message_id, "c");
    assert_eq!(delta.edited[0].message_id, "a");
    assert_eq!(delta.missing[0].message_id, "b");
    assert!(delta.deleted.is_empty());
    assert_eq!(
        store
            .import(
                &Lines,
                "first",
                &scope(),
                &mut Cursor::new("a\treplacement\n")
            )
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(store.load("first").unwrap(), first);
    assert!(store
        .import(
            &Lines,
            "wrong",
            &SourceScope::telegram("a", "1"),
            &mut Cursor::new("a\ttext\n")
        )
        .is_err());
    assert!(store
        .import(
            &Lines,
            "duplicate",
            &scope(),
            &mut Cursor::new("a\tfirst\na\tsecond\n")
        )
        .is_err());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

enum FailureMode {
    LateFailure,
    Twice,
    WrongScope,
    IgnoredRejection,
    NoEmission,
    UnreadInput,
}
struct Broken(FailureMode);
impl ArchiveImporter for Broken {
    fn descriptor(&self) -> ConnectorDescriptor {
        descriptor()
    }
    fn normalize(
        &self,
        reader: &mut dyn Read,
        source: &SourceScope,
        emit: &mut dyn FnMut(ConversationObservation) -> io::Result<()>,
    ) -> io::Result<()> {
        if !matches!(self.0, FailureMode::UnreadInput) {
            io::copy(reader, &mut io::sink())?;
        }
        match self.0 {
            FailureMode::NoEmission => Ok(()),
            FailureMode::LateFailure => {
                emit(observation(source.clone(), &[("a", "staged")]))?;
                Err(io::Error::other("failed after selected records"))
            }
            FailureMode::Twice => {
                emit(observation(source.clone(), &[]))?;
                emit(observation(source.clone(), &[]))
            }
            FailureMode::WrongScope | FailureMode::IgnoredRejection => {
                let mut wrong = source.clone();
                wrong.account_local_id = "another".into();
                let result = emit(observation(wrong, &[]));
                if matches!(self.0, FailureMode::IgnoredRejection) {
                    Ok(())
                } else {
                    result
                }
            }
            FailureMode::UnreadInput => emit(observation(source.clone(), &[])),
        }
    }
}

#[test]
fn failed_or_invalid_adapter_observations_never_publish() {
    for failure in [
        FailureMode::LateFailure,
        FailureMode::Twice,
        FailureMode::WrongScope,
        FailureMode::IgnoredRejection,
        FailureMode::NoEmission,
        FailureMode::UnreadInput,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = SnapshotStore::new(dir.path());
        assert!(store
            .import(
                &Broken(failure),
                "retry",
                &scope(),
                &mut Cursor::new("synthetic input")
            )
            .is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(store
            .import(&Lines, "retry", &scope(), &mut Cursor::new("a\tvalid\n"))
            .is_ok());
    }
}

#[test]
fn explicit_tombstones_are_separate_from_missing_records() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let first = store
        .publish_observation(
            descriptor(),
            "before",
            &scope(),
            observation(scope(), &[("a", "present"), ("b", "present")]),
        )
        .unwrap();
    let mut next = observation(scope(), &[("a", "")]);
    next.messages[0].deletion_state = DeletionState::Deleted;
    let second = store
        .publish_observation(descriptor(), "after", &scope(), next)
        .unwrap();
    let delta = second.diff(&first).unwrap();
    assert_eq!(delta.deleted[0].message_id, "a");
    assert_eq!(delta.missing[0].message_id, "b");
    assert!(delta.edited.is_empty());
    assert_eq!(second.diff(&second).unwrap().unchanged, 1);
}

#[test]
fn snapshot_local_ids_cannot_silently_match_across_imports() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let mut descriptor = descriptor();
    descriptor.capabilities.stable_message_ids = false;
    assert!(store
        .publish_observation(
            descriptor,
            "native-lie",
            &scope(),
            observation(scope(), &[("1", "text")])
        )
        .is_err());
    let mut snapshots = Vec::new();
    for id in ["first", "second"] {
        let mut observed = observation(scope(), &[("1", "text"), ("2", "text")]);
        for message in &mut observed.messages {
            message.identity_quality = IdentityQuality::SnapshotLocal;
        }
        snapshots.push(
            store
                .publish_observation(descriptor, id, &scope(), observed)
                .unwrap(),
        );
    }
    assert_eq!(snapshots[0].messages.len(), 2);
    assert!(snapshots[1].diff(&snapshots[0]).is_err());
    assert_eq!(snapshots[0].diff(&snapshots[0]).unwrap().unchanged, 2);
}

struct SimulatedAcquisition {
    active: Option<ExportRequest>,
}
impl SourceAcquisition for SimulatedAcquisition {
    fn descriptor(&self) -> ConnectorDescriptor {
        descriptor()
    }
    fn start(&mut self, request: &ExportRequest) -> io::Result<ClientUpdate> {
        self.active = Some(request.clone());
        Ok(ClientUpdate {
            request: request.clone(),
            status: ClientStatus::Exporting,
        })
    }
    fn poll(&mut self, request: &ExportRequest) -> io::Result<Option<ClientUpdate>> {
        if self.active.as_ref() != Some(request) {
            return Err(io::Error::other("inactive request"));
        }
        fs::write(&request.archive_path, "a\tsynthetic\n")?;
        self.active = None;
        Ok(Some(ClientUpdate {
            request: request.clone(),
            status: ClientStatus::Completed,
        }))
    }
    fn cancel(&mut self, request: &ExportRequest) -> io::Result<ClientUpdate> {
        self.active = None;
        Ok(ClientUpdate {
            request: request.clone(),
            status: ClientStatus::Cancelled,
        })
    }
}

#[test]
fn acquisition_events_use_the_same_bridge_with_a_bound_normalizer() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path().join("snapshots"));
    let request = ExportRequest {
        run_id: "synthetic".into(),
        source: scope(),
        archive_path: dir.path().join("source.txt"),
    };
    let mut job = ExportJob::for_importer(request.clone(), &Lines).unwrap();
    let mut driver = SimulatedAcquisition { active: None };
    let started = driver.start(&request).unwrap();
    assert!(job
        .apply_with_importer(started.clone(), &store, &TelegramJson)
        .is_err());
    assert_eq!(job.state(), &ExportState::WaitingForClient);
    assert!(job
        .apply_with_importer(started, &store, &Lines)
        .unwrap()
        .is_none());
    assert!(!request.archive_path.exists());
    let completed = driver.poll(&request).unwrap().unwrap();
    let snapshot = job
        .apply_with_importer(completed.clone(), &store, &Lines)
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.messages[0].text, "synthetic");
    assert_eq!(job.state(), &ExportState::Ready);
    assert!(job.apply_with_importer(completed, &store, &Lines).is_err());
    assert_eq!(store.load("synthetic").unwrap(), snapshot);
}

#[test]
fn telegram_compatibility_entry_point_matches_common_import() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let source = SourceScope::telegram("fixture", "111");
    let data = include_bytes!("fixtures/sample-export.json");
    let legacy = store
        .import_telegram("legacy-entry", &source, Cursor::new(data))
        .unwrap();
    let common = store
        .import(
            &TelegramJson,
            "common-entry",
            &source,
            &mut Cursor::new(data),
        )
        .unwrap();
    let delta = common.diff(&legacy).unwrap();
    assert_eq!(delta.unchanged, legacy.messages.len());
    assert!(
        delta.created.is_empty()
            && delta.edited.is_empty()
            && delta.deleted.is_empty()
            && delta.missing.is_empty()
    );
}
