#[path = "support/connector_contract.rs"]
mod contract;

use std::io::{self, BufRead, BufReader, Read};

use serde_json::{json, Value};
use tgsum_core::connector::{
    ArchiveImporter, AttachmentAccess, ConnectorCapabilities, ConnectorDescriptor, ConnectorKind,
    ConversationObservation, CredentialKind, MessageObservation, RefreshMethod, TelegramJson,
};
use tgsum_core::registry::{load_importer, ObservedVersion, Operation, Registry};
use tgsum_core::snapshot::{
    CanonicalMessage, Coverage, CoverageLevel, DeletionState, IdentityQuality, MessageKey,
    SourceScope,
};

struct SyntheticJsonLines;
impl ArchiveImporter for SyntheticJsonLines {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            id: "synthetic_json_lines",
            revision: "fixture-1",
            platform: "synthetic",
            kind: ConnectorKind::LocalData,
            format_id: "fixture_jsonl",
            capabilities: ConnectorCapabilities {
                history: true,
                refresh: RefreshMethod::Reimport,
                stable_message_ids: true,
                attachments: AttachmentAccess::None,
                credentials: CredentialKind::None,
            },
        }
    }
    fn normalize(
        &self,
        reader: &mut dyn Read,
        source: &SourceScope,
        emit: &mut dyn FnMut(ConversationObservation) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut messages = Vec::new();
        for line in BufReader::new(reader).lines() {
            let value: Value = serde_json::from_str(&line?)?;
            if value["channel"] != source.conversation_id {
                continue;
            }
            let mut message = CanonicalMessage::new(
                MessageKey {
                    source: source.clone(),
                    message_id: value["native"]
                        .as_str()
                        .ok_or_else(|| io::Error::other("missing native ID"))?
                        .into(),
                },
                value["body"]
                    .as_str()
                    .ok_or_else(|| io::Error::other("missing body"))?,
            );
            message.timestamp = value["when"].as_str().map(str::to_owned);
            let mut observation = MessageObservation::native_present(message);
            if value["tombstone"] == true {
                observation.deletion_state = DeletionState::Deleted;
            }
            messages.push(observation);
        }
        let mut coverage = Coverage::unknown("Synthetic own-message fixture");
        coverage.level = CoverageLevel::OwnMessagesOnly;
        emit(ConversationObservation {
            source: source.clone(),
            title: None,
            kind: "group".into(),
            coverage,
            messages,
        })
    }
}

struct SnapshotLocalFixture;
impl ArchiveImporter for SnapshotLocalFixture {
    fn descriptor(&self) -> ConnectorDescriptor {
        let mut descriptor = SyntheticJsonLines.descriptor();
        descriptor.id = "synthetic_snapshot_local";
        descriptor.capabilities.stable_message_ids = false;
        descriptor
    }
    fn normalize(
        &self,
        reader: &mut dyn Read,
        source: &SourceScope,
        emit: &mut dyn FnMut(ConversationObservation) -> io::Result<()>,
    ) -> io::Result<()> {
        SyntheticJsonLines.normalize(reader, source, &mut |mut conversation| {
            for (index, record) in conversation.messages.iter_mut().enumerate() {
                record.message.key.message_id = index.to_string();
                record.identity_quality = IdentityQuality::SnapshotLocal;
            }
            emit(conversation)
        })
    }
}

type Row<'a> = (&'a str, &'a str, Option<&'a str>, bool);

fn encode(telegram: bool, rows: &[Row<'_>]) -> Vec<u8> {
    let records = rows
        .iter()
        .map(|(id, text, timestamp, deleted)| {
            if telegram {
                // JSON numbers exercise IDs above JavaScript's exact integer range.
                let mut value =
                    json!({"id":id.parse::<u64>().unwrap(),"type":"message","text":text});
                if let Some(timestamp) = timestamp {
                    value["date"] = (*timestamp).into();
                }
                value
            } else {
                json!({"channel":"42","native":id,"body":text,"when":timestamp,"tombstone":deleted})
            }
        })
        .collect::<Vec<_>>();
    if telegram {
        serde_json::to_vec(&json!({"id":42,"type":"private_group","messages":records})).unwrap()
    } else {
        records
            .into_iter()
            .map(|v| v.to_string() + "\n")
            .collect::<String>()
            .into_bytes()
    }
}

fn run_contract(importer: &dyn ArchiveImporter, telegram: bool) {
    use contract::{CREATED, KEEP, MISSING};
    let first = (
        KEEP,
        "same UTF-8 Ж λ",
        Some("1970-01-01T03:00:00+03:00"),
        false,
    );
    let second = (
        MISSING,
        "same UTF-8 Ж λ",
        Some("2026-06-18T10:00:00"),
        false,
    );
    let before = encode(telegram, &[first, second]);
    let after = encode(
        telegram,
        &[
            (KEEP, "edited", first.2, false),
            (CREATED, "new", None, false),
        ],
    );
    let duplicate = encode(telegram, &[first, first]);
    let mut malformed = before.clone();
    malformed.extend_from_slice(b"\nnot valid JSON");
    let tombstone = encode(false, &[(KEEP, "", None, true)]);
    let corpus = contract::Corpus {
        source: SourceScope {
            platform: importer.descriptor().platform.into(),
            account_local_id: "synthetic-account".into(),
            conversation_id: "42".into(),
        },
        before: &before,
        after: &after,
        malformed: &malformed,
        duplicate: &duplicate,
        coverage: if telegram {
            CoverageLevel::Unknown
        } else {
            CoverageLevel::OwnMessagesOnly
        },
        tombstone: if telegram { None } else { Some(&tombstone) },
    };
    if importer.descriptor().capabilities.stable_message_ids {
        contract::archive_contract(importer, &corpus);
    } else {
        contract::snapshot_local_contract(importer, &corpus);
    }
    contract::acquisition_contract(importer, &corpus);
}

#[test]
fn telegram_passes_shared_archive_and_acquisition_contracts() {
    run_contract(&TelegramJson, true);
}

#[test]
fn independent_synthetic_adapter_passes_the_same_contracts_with_explicit_tombstones() {
    run_contract(&SyntheticJsonLines, false);
}

#[test]
fn weak_identity_uses_snapshot_local_contract_without_promising_incremental_matching() {
    run_contract(&SnapshotLocalFixture, false);
}

#[test]
fn passing_contracts_and_registry_flags_do_not_install_an_adapter_or_enable_acquisition() {
    let mut registry =
        Registry::parse(include_bytes!("../../docs/connectors/registry.json")).unwrap();
    let id = "synthetic_fixture_profile";
    registry.connectors[0].id = id.into();
    registry.connectors[0].platform = "synthetic".into();
    registry.connectors[0].enabled_operations =
        vec![Operation::LocalImport, Operation::Acquisition];
    let observed = ObservedVersion {
        format_id: Some("fixture_jsonl"),
        ..Default::default()
    };
    let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 27).unwrap();
    let support = registry.technical_support(id, &observed, today);
    assert!(support.implemented_operations.is_empty() && support.qualifications.is_empty());
    assert!(load_importer(id, &observed).is_err());
    registry.connectors[1]
        .enabled_operations
        .push(Operation::Acquisition);
    let support = registry.technical_support(
        "telegram_single_export",
        &ObservedVersion {
            format_id: Some("telegram_desktop_json"),
            ..Default::default()
        },
        today,
    );
    assert_eq!(support.implemented_operations, [Operation::LocalImport]);
}
