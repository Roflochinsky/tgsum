//! Deterministic fault/property corpus. No accounts or host application data.
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
