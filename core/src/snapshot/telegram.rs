//! Telegram acquisition details stay behind the snapshot import interface.

use std::collections::BTreeMap;
use std::io;

use serde::Deserialize;

use super::{
    invalid, Attachment, AttachmentAvailability, CanonicalMessage, Coverage, CoverageLevel,
    MessageKey, Snapshot, SourceScope, SCHEMA_VERSION,
};
use crate::{de, flatten_text, group_by_topic, is_forum, Message, RawChat, RawMessage};

/// Metadata that the legacy Markdown projection deliberately discards.
/// The flattened legacy record is buffered per message by serde, never as an
/// entire export. IDs and attachment paths are decoded before that projection.
#[derive(Deserialize)]
pub(super) struct Record {
    #[serde(deserialize_with = "de::id")]
    id: String,
    #[serde(default, deserialize_with = "de::optional_id")]
    reply_to_message_id: Option<String>,
    #[serde(default)]
    photo: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    file_size: Option<u64>,
    #[serde(default)]
    photo_size: Option<u64>,
    #[serde(default, deserialize_with = "de::opt_string")]
    date_unixtime: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    edited: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    edited_unixtime: Option<String>,
    #[serde(flatten)]
    legacy: RawMessage,
}

impl Message for Record {
    fn id(&self) -> &str {
        &self.id
    }
    fn is_service(&self) -> bool {
        self.legacy.service
    }
    fn action(&self) -> Option<&str> {
        self.legacy.action.as_deref()
    }
    fn title(&self) -> Option<&str> {
        self.legacy.title.as_deref()
    }
    fn reply_to(&self) -> Option<&str> {
        self.reply_to_message_id.as_deref()
    }
    fn date(&self) -> Option<&str> {
        self.legacy.date.as_deref()
    }
}

pub(super) fn normalize(
    snapshot_id: &str,
    source: &SourceScope,
    chat: RawChat<Record>,
) -> io::Result<Snapshot> {
    let mut topics = BTreeMap::new();
    if is_forum(&chat.messages) {
        for group in group_by_topic(&chat.messages) {
            for message in group.messages {
                topics.insert(message.id.clone(), group.topic_id.clone());
            }
        }
    }
    let mut messages = Vec::with_capacity(chat.messages.len());
    for record in chat.messages {
        // Telegram native IDs are decimal. Reject malformed identities rather
        // than allowing the lenient display projection to invent a key.
        let digits = record.id.strip_prefix('-').unwrap_or(&record.id);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("invalid Telegram message ID"));
        }
        let text = flatten_text(&record.legacy).to_owned();
        let mut attachments = Vec::new();
        for (path, kind, size) in [
            (record.photo, "photo", record.photo_size),
            (
                record.file,
                record.legacy.media_type.as_deref().unwrap_or("file"),
                record.file_size,
            ),
        ] {
            if let Some(path) = path.filter(|p| !p.is_empty()) {
                let relative_path = safe_relative_path(&path).then_some(path);
                let availability = if relative_path.is_some() {
                    AttachmentAvailability::UnverifiedReference
                } else {
                    AttachmentAvailability::Unavailable
                };
                attachments.push(Attachment {
                    relative_path,
                    original_name: record.legacy.file_name.clone(),
                    mime_type: record.mime_type.clone(),
                    size,
                    media_type: kind.into(),
                    availability,
                });
            }
        }
        messages.push(CanonicalMessage {
            thread_id: topics.remove(&record.id),
            key: MessageKey {
                source: source.clone(),
                message_id: record.id,
            },
            timestamp: record.legacy.date,
            timestamp_unix: record.date_unixtime,
            sender_id: record.legacy.from_id.or(record.legacy.actor_id),
            sender_name: record.legacy.from.or(record.legacy.actor),
            text,
            reply_to: record.reply_to_message_id,
            edited_at: record.edited,
            edited_unix: record.edited_unixtime,
            is_service: record.legacy.service,
            service_action: record.legacy.action,
            service_title: record.legacy.title,
            attachments,
        });
    }
    let snapshot = Snapshot {
        schema_version: SCHEMA_VERSION,
        snapshot_id: snapshot_id.into(),
        source: source.clone(),
        conversation_title: chat.name,
        conversation_kind: chat.kind,
        coverage: Coverage {
            level: CoverageLevel::Unknown,
            reason: "Telegram JSON identifies the chat but does not prove account ownership or completeness of exported history".into(),
        },
        messages,
    };
    snapshot.validate()?;
    Ok(snapshot)
}

fn safe_relative_path(path: &str) -> bool {
    // Exporter placeholders such as `(File not included...)` are not paths.
    !path.starts_with('(')
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
