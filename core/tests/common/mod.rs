#![allow(dead_code)]

use std::path::PathBuf;

use serde_json::Value;
use tgsum_core::{ExtractedUnit, RawMessage};

/// A message built from JSON, exactly as it would appear in `result.json`.
pub fn msg(v: Value) -> RawMessage {
    serde_json::from_value(v).expect("valid message")
}

pub fn msgs(v: Value) -> Vec<RawMessage> {
    serde_json::from_value(v).expect("valid messages")
}

pub fn unit(
    chat_name: &str,
    topic_title: Option<&str>,
    messages: Vec<RawMessage>,
) -> ExtractedUnit {
    ExtractedUnit {
        chat_name: chat_name.to_owned(),
        topic_title: topic_title.map(str::to_owned),
        messages,
    }
}

/// `n` messages on one day, each with a 40-char text of `ch`.
pub fn big_unit(chat_name: &str, n: usize, ch: char) -> ExtractedUnit {
    let messages = (0..n)
        .map(|i| {
            msg(serde_json::json!({
                "id": i, "type": "message", "date": "2026-06-18T09:00:00",
                "from": "X", "from_id": "user1", "text": ch.to_string().repeat(40),
            }))
        })
        .collect();
    unit(chat_name, None, messages)
}

pub fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-export.json")
}

pub fn ids(messages: &[RawMessage]) -> Vec<&str> {
    messages.iter().map(|m| m.id.as_str()).collect()
}
