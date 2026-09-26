use std::{fs, io::Cursor};

use serde_json::json;
use tgsum_core::snapshot::{
    CoverageGap, CoverageLevel, DeletionState, IdentityQuality, RecordLocatorKind, SnapshotStore,
    SourceScope, TimeRange, TimestampInfo, TimezoneStatus, UtcInstant,
};

const BEFORE: &[u8] = include_bytes!("fixtures/telegram-single-before.json");
const V1: &[u8] = include_bytes!("fixtures/snapshot-v1.json");

fn source() -> SourceScope {
    SourceScope::telegram("synthetic-account", "9007199254740993")
}

#[test]
fn old_snapshot_migrates_in_memory_without_rewriting_or_inventing_provenance() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("before.json"), V1).unwrap();
    let store = SnapshotStore::new(dir.path());
    let old = store.load("before").unwrap();
    assert_eq!(old.schema_version, 2);
    let provenance = old.metadata.as_ref().unwrap();
    assert_eq!(provenance.migrated_from_schema, Some(1));
    assert_eq!(provenance.imported_at, None);
    assert_eq!(old.coverage.level, CoverageLevel::Unknown);
    assert!(old.coverage.range.is_none());
    assert_eq!(old.messages[3].key.message_id, "18446744073709551615");
    assert_eq!(
        old.messages[0]
            .metadata
            .as_ref()
            .unwrap()
            .timestamp
            .timezone_status,
        TimezoneStatus::UnknownTimezone
    );
    assert_eq!(
        old.messages[0]
            .metadata
            .as_ref()
            .unwrap()
            .provenance
            .locator_kind,
        RecordLocatorKind::MigratedCanonicalMessages
    );
    assert_eq!(
        old.messages[0]
            .metadata
            .as_ref()
            .unwrap()
            .provenance
            .raw_digest,
        None
    );
    let current = store
        .import_telegram("current", &source(), Cursor::new(BEFORE))
        .unwrap();
    let delta = current.diff(&old).unwrap();
    assert_eq!(delta.unchanged, 4);
    assert!(delta.edited.is_empty());
    assert_eq!(
        current.metadata.as_ref().unwrap().content_digest,
        provenance.content_digest
    );
    assert_eq!(fs::read(dir.path().join("before.json")).unwrap(), V1);
}

#[test]
fn revisions_ignore_observation_and_order_but_detect_content_changes() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let first = store
        .import_telegram("first", &source(), Cursor::new(BEFORE))
        .unwrap();
    let mut archive: serde_json::Value = serde_json::from_slice(BEFORE).unwrap();
    archive["messages"].as_array_mut().unwrap().reverse();
    let reordered = store
        .import_telegram(
            "reordered",
            &source(),
            Cursor::new(serde_json::to_vec(&archive).unwrap()),
        )
        .unwrap();
    assert_eq!(reordered.diff(&first).unwrap().unchanged, 4);
    assert_eq!(
        first.metadata.as_ref().unwrap().content_digest,
        reordered.metadata.as_ref().unwrap().content_digest
    );
    for (index, message) in reordered.messages.iter().enumerate() {
        let meta = message.metadata.as_ref().unwrap();
        assert_eq!(meta.provenance.snapshot_id, "reordered");
        assert_eq!(meta.provenance.record_ordinal, index as u64);
        assert_eq!(meta.identity_quality, IdentityQuality::Native);
        assert_eq!(meta.deletion_state, DeletionState::Present);
        assert!(meta.revision_id.starts_with("sha256:"));
    }
    archive["messages"][0]["text"] = json!("new text");
    let changed = store
        .import_telegram(
            "changed",
            &source(),
            Cursor::new(serde_json::to_vec(&archive).unwrap()),
        )
        .unwrap();
    assert_eq!(
        changed.diff(&first).unwrap().edited[0].message_id,
        "18446744073709551615"
    );
    assert_ne!(
        changed.metadata.as_ref().unwrap().content_digest,
        first.metadata.as_ref().unwrap().content_digest
    );
}

#[test]
fn timestamps_never_use_the_host_timezone_and_keep_subsecond_precision() {
    let cases = [
        (None, None, TimezoneStatus::Missing, None),
        (
            Some("2026-01-01T10:00:00"),
            None,
            TimezoneStatus::UnknownTimezone,
            None,
        ),
        (Some("not a date"), None, TimezoneStatus::Invalid, None),
        (
            Some("1970-01-01T03:00:00+03:00"),
            None,
            TimezoneStatus::ExplicitOffset,
            Some(UtcInstant {
                seconds: 0,
                nanoseconds: 0,
            }),
        ),
        (
            Some("1970-01-01T00:00:00.123456789Z"),
            None,
            TimezoneStatus::ExplicitOffset,
            Some(UtcInstant {
                seconds: 0,
                nanoseconds: 123456789,
            }),
        ),
        (
            Some("1970-01-01T03:00:00"),
            Some("0"),
            TimezoneStatus::UnixTime,
            Some(UtcInstant {
                seconds: 0,
                nanoseconds: 0,
            }),
        ),
        (
            None,
            Some("-1"),
            TimezoneStatus::UnixTime,
            Some(UtcInstant {
                seconds: -1,
                nanoseconds: 0,
            }),
        ),
        (
            Some("1970-01-01T00:00:00Z"),
            Some("1"),
            TimezoneStatus::Conflicting,
            None,
        ),
        (None, Some("1.5"), TimezoneStatus::Invalid, None),
        (
            None,
            Some("18446744073709551615"),
            TimezoneStatus::Invalid,
            None,
        ),
        (
            Some("2016-12-31T23:59:60Z"),
            Some("1483228799"),
            TimezoneStatus::Invalid,
            None,
        ),
    ];
    for (raw, unix, expected_status, expected_utc) in cases {
        let result = TimestampInfo::from_export(raw, unix);
        assert_eq!(
            result.timezone_status, expected_status,
            "{raw:?} / {unix:?}"
        );
        assert_eq!(result.utc, expected_utc, "{raw:?} / {unix:?}");
    }
}

#[test]
fn missing_records_remain_missing_even_with_bounded_complete_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let before = store
        .import_telegram("before", &source(), Cursor::new(BEFORE))
        .unwrap();
    let mut after = store
        .import_telegram(
            "after",
            &source(),
            Cursor::new(include_bytes!("fixtures/telegram-single-after.json")),
        )
        .unwrap();
    after.coverage.level = CoverageLevel::Complete;
    assert!(after.diff(&before).is_err());
    let range = TimeRange {
        start: UtcInstant {
            seconds: 0,
            nanoseconds: 0,
        },
        end: UtcInstant {
            seconds: 10,
            nanoseconds: 0,
        },
    };
    after.coverage.range = Some(range.clone());
    after.coverage.evidence = vec!["Synthetic bounded coverage claim for contract testing".into()];
    let delta = after.diff(&before).unwrap();
    assert_eq!(delta.missing[0].message_id, "3");
    assert!(after
        .messages
        .iter()
        .all(|m| m.metadata.as_ref().unwrap().deletion_state == DeletionState::Present));
    after.coverage.known_gaps.push(CoverageGap {
        range: Some(range),
        reason: "Synthetic outage".into(),
    });
    assert!(after.diff(&before).is_err());
    after.coverage.level = CoverageLevel::Partial;
    assert!(after.diff(&before).is_ok());
    after.coverage.range.as_mut().unwrap().start.seconds = 100;
    assert!(after.diff(&before).is_err());
}

#[test]
fn tampered_content_provenance_or_timestamp_metadata_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let original = store
        .import_telegram("saved", &source(), Cursor::new(BEFORE))
        .unwrap();
    let bytes = fs::read(dir.path().join("saved.json")).unwrap();
    let valid: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for (pointer, value) in [
        ("/messages/0/text", json!("tampered")),
        (
            "/messages/0/metadata/provenance/snapshot_id",
            json!("another"),
        ),
        ("/messages/0/metadata/provenance/record_ordinal", json!(9)),
        (
            "/messages/0/metadata/timestamp/timezone_status",
            json!("unix_time"),
        ),
        ("/messages/0/metadata", serde_json::Value::Null),
        ("/metadata/content_digest", json!("sha256:incorrect")),
        ("/schema_version", json!(999)),
    ] {
        let mut changed = valid.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        fs::write(
            dir.path().join("saved.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        assert!(store.load("saved").is_err(), "{pointer}");
    }
    fs::write(dir.path().join("saved.json"), bytes).unwrap();
    assert_eq!(store.load("saved").unwrap(), original);
}
