use std::fs;
use std::io::{self, Cursor};

use tgsum_core::snapshot::{
    AttachmentAvailability, CoverageLevel, MessageKey, SnapshotStore, SourceScope,
};

const BEFORE: &[u8] = include_bytes!("fixtures/telegram-single-before.json");
const AFTER: &[u8] = include_bytes!("fixtures/telegram-single-after.json");

fn scope() -> SourceScope {
    SourceScope::telegram("synthetic-account", "9007199254740993")
}
fn ids(keys: &[MessageKey]) -> Vec<&str> {
    keys.iter().map(|k| k.message_id.as_str()).collect()
}

#[test]
fn persistent_snapshots_diff_native_identities_and_preserve_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let before = store
        .import_telegram("first", &scope(), Cursor::new(BEFORE))
        .unwrap();
    let after = store
        .import_telegram("second", &scope(), Cursor::new(AFTER))
        .unwrap();
    assert_eq!(before, store.load("first").unwrap());
    assert_eq!(after, store.load("second").unwrap());
    let diff = after.diff(&store.load("first").unwrap()).unwrap();
    assert_eq!(ids(&diff.created), ["4"]);
    assert_eq!(ids(&diff.edited), ["2"]);
    assert_eq!(ids(&diff.missing), ["3"]);
    assert_eq!(diff.unchanged, 2);
    assert_eq!(after.diff(&after).unwrap().unchanged, 4);
    assert_eq!(before.coverage.level, CoverageLevel::Unknown);
    assert_eq!(
        before
            .messages
            .iter()
            .filter(|m| m.text == "Repeated text")
            .count(),
        2
    );
    let attachment = &before.messages[1].attachments[0];
    assert_eq!(
        attachment.relative_path.as_deref(),
        Some("files/deploy.log")
    );
    assert_eq!(attachment.size, Some(42));
    assert_eq!(
        attachment.availability,
        AttachmentAvailability::UnverifiedReference
    );
    assert_eq!(before.messages[1].text, "Deploy tomorrow");
    assert_eq!(
        after.messages[1].reply_to.as_deref(),
        Some("18446744073709551615")
    );
    // A rename/reordering does not turn unchanged message content into edits.
    assert_ne!(before.conversation_title, after.conversation_title);
    let bytes = fs::read(dir.path().join("first.json")).unwrap();
    assert_eq!(
        store
            .import_telegram("first", &scope(), Cursor::new(AFTER))
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read(dir.path().join("first.json")).unwrap(), bytes);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn selected_chat_is_the_only_persisted_scope_and_topics_survive() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let scope = SourceScope::telegram("account", "222");
    let data = include_bytes!("fixtures/sample-export.json");
    let snapshot = store
        .import_telegram("forum", &scope, Cursor::new(data))
        .unwrap();
    assert!(snapshot.messages.iter().all(|m| m.key.source == scope));
    assert!(
        snapshot
            .messages
            .iter()
            .find(|m| m.key.message_id == "100")
            .unwrap()
            .is_service
    );
    assert_eq!(
        snapshot
            .messages
            .iter()
            .find(|m| m.key.message_id == "102")
            .unwrap()
            .thread_id
            .as_deref(),
        Some("100")
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    assert!(
        !String::from_utf8(fs::read(dir.path().join("forum.json")).unwrap())
            .unwrap()
            .contains("Direct with Bob")
    );
    let other = store
        .import_telegram(
            "other-account",
            &SourceScope::telegram("other", "222"),
            Cursor::new(data),
        )
        .unwrap();
    assert!(other.diff(&snapshot).is_err());
}

#[test]
fn invalid_or_incomplete_exports_never_publish_and_allow_retry() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let scope = SourceScope::telegram("a", "1");
    for invalid in [
        r#"{"chats":{"list":[{"id":1,"messages":[]},"#,
        r#"{"id":1,"messages":[]} trailing"#,
        r#"{"id":2,"messages":[]}"#,
        r#"{"chats":{"list":[{"id":1,"messages":[]},{"id":1,"messages":[]}]}}"#,
        r#"{"id":1,"messages":[{"id":2,"text":"a"},{"id":2,"text":"b"}]}"#,
        r#"{"id":1,"messages":[{"text":"missing identity"}]}"#,
        r#"{"id":1,"messages":[{"id":9007199254740993.0}]}"#,
        r#"{"id":1,"messages":[{"id":1,"reply_to_message_id":9007199254740993.0}]}"#,
        r#"{"id":1.0,"messages":[]}"#,
        r#"{"id":1,"messages":[{"id":"1.0"}]}"#,
        r#"{"chats":{"list":[{"id":1}]}}"#,
    ] {
        assert!(
            store
                .import_telegram("retry", &scope, Cursor::new(invalid))
                .is_err(),
            "{invalid}"
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0, "{invalid}");
    }
    for invalid_id in ["../escape", "CON", "NUL", "com1", "LPT9"] {
        assert!(store
            .import_telegram(invalid_id, &scope, Cursor::new(BEFORE))
            .is_err());
    }
    let snapshot = store
        .import_telegram("retry", &scope, Cursor::new(r#"{"id":1,"messages":[]}"#))
        .unwrap();
    assert!(snapshot.messages.is_empty());
}

#[test]
fn read_failure_after_staging_does_not_publish() {
    use std::io::Read;
    struct Failure;
    impl Read for Failure {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(tgsum_core::cancelled())
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(directory.path());
    let prefix = Cursor::new(br#"{"chats":{"list":[{"id":1,"messages":[]},"#);
    let error = store
        .import_telegram(
            "cancelled",
            &SourceScope::telegram("a", "1"),
            prefix.chain(Failure),
        )
        .unwrap_err();
    assert!(tgsum_core::is_cancelled(&error), "{error}");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn concurrent_publication_never_overwrites_an_existing_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let start = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = [BEFORE, AFTER]
        .into_iter()
        .map(|bytes| {
            let store = SnapshotStore::new(directory.path());
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                store.import_telegram("same-run", &scope(), Cursor::new(bytes))
            })
        })
        .collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    let winner = results
        .iter()
        .find_map(|result| result.as_ref().ok())
        .unwrap();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .find_map(|r| r.as_ref().err())
            .unwrap()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(
        &SnapshotStore::new(directory.path())
            .load("same-run")
            .unwrap(),
        winner
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn references_are_metadata_and_changed_attachments_are_edits() {
    let dir = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(dir.path());
    let scope = SourceScope::telegram("a", "1");
    let before = store
        .import_telegram(
            "a",
            &scope,
            Cursor::new(r#"{"id":1,"messages":[{"id":1,"photo":"photos/a.jpg","photo_size":12}]}"#),
        )
        .unwrap();
    for (i, path) in [
        "../tdata/key",
        "/etc/passwd",
        "C:\\private",
        "(File not included. Change data exporting settings to download.)",
    ]
    .iter()
    .enumerate()
    {
        let data =
            serde_json::to_vec(&serde_json::json!({"id":1,"messages":[{"id":1,"photo":path}]}))
                .unwrap();
        let after = store
            .import_telegram(&format!("b{i}"), &scope, Cursor::new(data))
            .unwrap();
        assert_eq!(
            after.messages[0].attachments[0].availability,
            AttachmentAvailability::Unavailable
        );
        assert!(after.messages[0].attachments[0].relative_path.is_none());
        assert_eq!(ids(&after.diff(&before).unwrap().edited), ["1"]);
    }
    assert_eq!(before.messages[0].attachments[0].size, Some(12));
}
