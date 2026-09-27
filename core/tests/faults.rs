//! Deterministic fault/property corpus. No accounts or host application data.
#[path = "faults/process.rs"]
mod process;
#[path = "faults/schema.rs"]
mod schema;

use std::fs;
use std::io::{self, Read};

use serde_json::json;
use tgsum_core::snapshot::{SnapshotStore, SourceScope};

struct Fragmented<'a> {
    bytes: &'a [u8],
    offset: usize,
    chunk: usize,
    fail_at: Option<usize>,
}

impl Read for Fragmented<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.fail_at == Some(self.offset) {
            return Err(io::Error::other("synthetic transport failure"));
        }
        let boundary = self.fail_at.unwrap_or(self.bytes.len());
        let n = out
            .len()
            .min(self.chunk)
            .min(self.bytes.len() - self.offset)
            .min(boundary - self.offset);
        out[..n].copy_from_slice(&self.bytes[self.offset..self.offset + n]);
        self.offset += n;
        Ok(n)
    }
}

fn source() -> SourceScope {
    SourceScope::telegram("synthetic-fault-corpus", "9007199254740993")
}

fn encode(texts: &[String]) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "id": 9_007_199_254_740_993u64, "type": "private_group", "name": "Synthetic",
        "messages": texts.iter().enumerate().map(|(i, text)| json!({
            "id": u64::MAX - i as u64, "type": "message", "text": text,
        })).collect::<Vec<_>>()
    }))
    .unwrap()
}

#[test]
fn short_reads_preserve_generated_unicode_and_exact_native_identities() {
    let root = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(root.path());
    // Deterministic generated strings include escape syntax, multibyte scalars,
    // combining marks, repeated content and empty bodies. No RNG dependency.
    let alphabet = [
        "",
        "Ж",
        "λ",
        "👩🏽‍💻",
        "e\u{301}",
        "\n",
        "\\",
        "\"",
        "\0",
        "same",
        "\u{80}\u{7ff}\u{800}\u{d7ff}\u{e000}\u{ffff}\u{10000}\u{10ffff}",
    ];
    for seed in 0..24 {
        let texts = (0..seed)
            .map(|i| {
                (0..i + 1)
                    .map(|j| alphabet[(seed * 7 + i * 3 + j * 11) % alphabet.len()])
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let bytes = encode(&texts);
        let reference = store
            .import_telegram(&format!("reference-{seed}"), &source(), bytes.as_slice())
            .unwrap();
        for chunk in [1, 2, 3, 7, 31, 8192] {
            let id = format!("fragmented-{seed}-{chunk}");
            let snapshot = store
                .import_telegram(
                    &id,
                    &source(),
                    Fragmented {
                        bytes: &bytes,
                        offset: 0,
                        chunk,
                        fail_at: None,
                    },
                )
                .unwrap();
            assert_eq!(snapshot.messages.len(), texts.len());
            for (index, message) in snapshot.messages.iter().enumerate() {
                assert_eq!(
                    message.key.message_id,
                    (u64::MAX - index as u64).to_string()
                );
                assert_eq!(message.text, texts[index]);
            }
            let diff = snapshot.diff(&reference).unwrap();
            assert_eq!(diff.unchanged, texts.len());
            assert!(diff.created.is_empty() && diff.edited.is_empty() && diff.missing.is_empty());
            assert_eq!(store.load(&id).unwrap(), snapshot);
        }
    }
}

#[test]
fn every_truncation_and_read_failure_boundary_preserves_the_committed_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(root.path());
    let bytes = encode(&["Жλ👩🏽‍💻\n\"\\".into(), "same".into()]);
    let committed = store
        .import_telegram("committed", &source(), bytes.as_slice())
        .unwrap();
    let original = fs::read(root.path().join("committed.json")).unwrap();
    // The compact JSON has no trailing whitespace: every proper prefix is
    // incomplete, including splits within UTF-8 and string escape sequences.
    for boundary in 0..bytes.len() {
        assert!(
            store
                .import_telegram("attempt", &source(), &bytes[..boundary])
                .is_err(),
            "truncation {boundary}"
        );
        assert!(!root.path().join("attempt.json").exists());
    }
    for boundary in 0..=bytes.len() {
        let input = Fragmented {
            bytes: &bytes,
            offset: 0,
            chunk: 7,
            fail_at: Some(boundary),
        };
        assert!(
            store.import_telegram("attempt", &source(), input).is_err(),
            "read fault {boundary}"
        );
        assert!(!root.path().join("attempt.json").exists());
    }
    assert_eq!(
        fs::read(root.path().join("committed.json")).unwrap(),
        original
    );
    assert_eq!(store.load("committed").unwrap(), committed);
    assert_eq!(
        fs::read_dir(root.path()).unwrap().count(),
        1,
        "failed attempts clean staging files"
    );
    let retried = store
        .import_telegram("attempt", &source(), bytes.as_slice())
        .unwrap();
    assert_eq!(retried.diff(&committed).unwrap().unchanged, 2);
}

#[test]
fn bounded_byte_mutations_never_publish_invalid_json_or_change_committed_data() {
    let root = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(root.path());
    let bytes = encode(&["Unicode Жλ; quote \" and slash \\".into(), "same".into()]);
    store
        .import_telegram("baseline", &source(), bytes.as_slice())
        .unwrap();
    let baseline = fs::read(root.path().join("baseline.json")).unwrap();
    let mut accepted = 0;
    let mut rejected = 0;
    // A small, reproducible mutation corpus in the normal gate: every byte is
    // replaced with NUL/invalid UTF-8/a delimiter/a digit, then removed. This
    // probes arbitrary malformed boundaries, including numeric identities.
    for offset in 0..bytes.len() {
        for mutation in [Some(0), Some(0xff), Some(b'}'), Some(b'0'), None] {
            let mut input = bytes.clone();
            if let Some(byte) = mutation {
                input[offset] = byte;
            } else {
                input.remove(offset);
            }
            let valid_json = serde_json::from_slice::<serde_json::Value>(&input).is_ok();
            let index = tgsum_core::index_reader(input.as_slice());
            let result = store.import_telegram("mutant", &source(), input.as_slice());
            if !valid_json {
                assert!(
                    index.is_err() && result.is_err(),
                    "invalid JSON at {offset}: {mutation:?}; index_ok={}, snapshot_ok={}",
                    index.is_ok(),
                    result.is_ok()
                );
            }
            match result {
                Ok(snapshot) => {
                    accepted += 1;
                    assert_eq!(snapshot.source, source());
                    assert_eq!(store.load("mutant").unwrap(), snapshot);
                    let reference = store
                        .import_telegram(
                            "fragmented-mutant",
                            &source(),
                            Fragmented {
                                bytes: &input,
                                offset: 0,
                                chunk: 1,
                                fail_at: None,
                            },
                        )
                        .unwrap();
                    let diff = reference.diff(&snapshot).unwrap();
                    assert_eq!(diff.unchanged, snapshot.messages.len());
                    assert!(
                        diff.created.is_empty()
                            && diff.edited.is_empty()
                            && diff.missing.is_empty()
                    );
                    fs::remove_file(root.path().join("mutant.json")).unwrap();
                    fs::remove_file(root.path().join("fragmented-mutant.json")).unwrap();
                }
                Err(_) => {
                    rejected += 1;
                    assert!(!root.path().join("mutant.json").exists());
                }
            }
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        }
    }
    assert!(accepted > 0 && rejected > 0, "exercise both outcomes");
    assert_eq!(
        fs::read(root.path().join("baseline.json")).unwrap(),
        baseline
    );
}

#[test]
fn ignored_fields_validate_utf8_across_chunks_but_extraction_can_stop_before_bad_tail() {
    let root = tempfile::tempdir().unwrap();
    let store = SnapshotStore::new(root.path());
    for bad in [
        &[0xff][..],
        &[0x80],
        &[0xc0, 0xaf],
        &[0xed, 0xa0, 0x80],
        &[0xf4, 0x90, 0x80, 0x80],
        &[0xe2, 0x82],
    ] {
        let mut bytes =
            br#"{"id":9007199254740993,"type":"private_group","messages":[],"extra":""#.to_vec();
        bytes.extend_from_slice(bad);
        bytes.extend_from_slice(br#""}"#);
        for chunk in [1, 2, 3, 4, 7, 1 << 18] {
            let read = || Fragmented {
                bytes: &bytes,
                offset: 0,
                chunk,
                fail_at: None,
            };
            assert!(
                tgsum_core::index_reader(read()).is_err(),
                "ignored UTF-8 {bad:?}, chunk {chunk}"
            );
            assert!(store.import_telegram("invalid", &source(), read()).is_err());
            assert!(!root.path().join("invalid.json").exists());
        }
    }
    let mut bytes =
        br#"{"chats":{"list":[{"id":9007199254740993,"type":"private_group","messages":[]},"#
            .to_vec();
    bytes.push(0xff);
    let selection = tgsum_core::Selection {
        chat_id: source().conversation_id,
        topic_ids: Vec::new(),
    };
    // Buffer fill may already read the bad tail. Deliver valid prefix bytes
    // before the deferred encoding error so a deliberate early stop survives.
    assert!(tgsum_core::extract_reader(bytes.as_slice(), &[selection]).is_ok());
    assert!(tgsum_core::index_reader(bytes.as_slice()).is_err());
}
