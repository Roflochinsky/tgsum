//! Domain model: raw export records, the chat/topic index, and the pure rules
//! for names, text and forum-topic grouping.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::de;

/// Topic id of a forum's implicit "General" topic.
pub const GENERAL_TOPIC_ID: &str = "1";

/// One message from `result.json`, reduced to the fields tgsum uses.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RawMessage {
    #[serde(default, deserialize_with = "de::string")]
    pub id: String,
    /// `type == "service"`: joins, pins, calls, topic creation…
    #[serde(rename = "type", default, deserialize_with = "de::is_service")]
    pub service: bool,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub date: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub from: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub from_id: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub actor: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub actor_id: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub action: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub reply_to_message_id: Option<String>,
    /// Flattened `text` (a string or an array of runs).
    #[serde(default, deserialize_with = "de::text")]
    pub text: Option<String>,
    /// Flattened `text_entities`; `None` unless it is a non-empty array.
    #[serde(default, deserialize_with = "de::text_entities")]
    pub text_entities: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub media_type: Option<String>,
    /// A `photo` is attached.
    #[serde(default, deserialize_with = "de::truthy")]
    pub photo: bool,
    /// A `file` is attached.
    #[serde(default, deserialize_with = "de::truthy")]
    pub file: bool,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub file_name: Option<String>,
    #[serde(default, deserialize_with = "de::number")]
    pub duration_seconds: Option<f64>,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub sticker_emoji: Option<String>,
}

/// The slice of a message the index pass needs. Skipping text and media keeps
/// memory low while a whole chat is held for topic grouping.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct MessageMeta {
    #[serde(default, deserialize_with = "de::string")]
    pub id: String,
    #[serde(rename = "type", default, deserialize_with = "de::is_service")]
    pub service: bool,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub date: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub action: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string")]
    pub reply_to_message_id: Option<String>,
}

/// What indexing and topic grouping read from a message.
pub trait Message {
    fn id(&self) -> &str;
    fn is_service(&self) -> bool;
    fn action(&self) -> Option<&str>;
    fn title(&self) -> Option<&str>;
    fn reply_to(&self) -> Option<&str>;
    fn date(&self) -> Option<&str>;
}

macro_rules! impl_message {
    ($($t:ty),*) => {$(
        impl Message for $t {
            fn id(&self) -> &str {
                &self.id
            }
            fn is_service(&self) -> bool {
                self.service
            }
            fn action(&self) -> Option<&str> {
                self.action.as_deref()
            }
            fn title(&self) -> Option<&str> {
                self.title.as_deref()
            }
            fn reply_to(&self) -> Option<&str> {
                self.reply_to_message_id.as_deref()
            }
            fn date(&self) -> Option<&str> {
                self.date.as_deref()
            }
        }
    )*};
}

impl_message!(RawMessage, MessageMeta);

/// One element of `chats.list[]`, or the root of a per-chat export.
#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "M: Deserialize<'de>"))]
pub struct RawChat<M> {
    #[serde(default, deserialize_with = "de::opt_string")]
    pub name: Option<String>,
    #[serde(rename = "type", default, deserialize_with = "de::string")]
    pub kind: String,
    #[serde(deserialize_with = "de::id")]
    pub id: String,
    pub messages: Vec<M>,
}

impl<M> RawChat<M> {
    /// The chat's name, or a placeholder for nameless chats (deleted accounts).
    pub fn display_name(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("(no name {})", self.id),
        }
    }
}

/// A forum topic in the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicIndex {
    /// `"1"` for General, otherwise the id of the `topic_created` message.
    pub topic_id: String,
    pub title: String,
    pub count: usize,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
}

/// A chat in the index built by the first pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatIndex {
    pub chat_id: String,
    pub name: String,
    /// Telegram chat type: `personal_chat`, `private_supergroup`, …
    #[serde(rename = "type")]
    pub kind: String,
    pub count: usize,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    /// Empty unless the chat is a forum.
    pub topics: Vec<TopicIndex>,
}

/// One pick from the chat list.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub chat_id: String,
    /// Empty = the whole chat; otherwise these forum topics.
    #[serde(default)]
    pub topic_ids: Vec<String>,
}

/// A chat or topic pulled out by the second pass, ready to format.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExtractedUnit {
    pub chat_name: String,
    pub topic_title: Option<String>,
    pub messages: Vec<RawMessage>,
}

/// The flattened message text: `text_entities` when present, else `text`.
pub fn flatten_text(m: &RawMessage) -> &str {
    m.text_entities
        .as_deref()
        .or(m.text.as_deref())
        .unwrap_or("")
}

/// Author name: `from`, then `from_id`, then `actor`/`actor_id` for service
/// messages, else `"unknown"`.
pub fn resolve_name(m: &RawMessage) -> &str {
    if let Some(from) = m.from.as_deref().filter(|s| !s.is_empty()) {
        return from;
    }
    if let Some(id) = m.from_id.as_deref() {
        return id;
    }
    if let Some(actor) = m.actor.as_deref().filter(|s| !s.is_empty()) {
        return actor;
    }
    if let Some(id) = m.actor_id.as_deref() {
        return id;
    }
    "unknown"
}

/// Drops service messages (not content; topic titles are read separately by
/// [`group_by_topic`]).
pub fn strip_service<M: Message>(msgs: Vec<M>) -> Vec<M> {
    msgs.into_iter().filter(|m| !m.is_service()).collect()
}

/// Whether the chat is a forum (has at least one created topic).
pub fn is_forum<M: Message>(msgs: &[M]) -> bool {
    msgs.iter().any(is_topic_root)
}

fn is_topic_root<M: Message>(m: &M) -> bool {
    m.is_service() && m.action() == Some("topic_created")
}

/// Messages of one forum topic.
#[derive(Debug)]
pub struct Group<'a, M> {
    pub topic_id: String,
    pub title: String,
    pub messages: Vec<&'a M>,
}

/// Splits a forum's content messages into topics. The export has no topic
/// field, so each message walks its reply chain up to a `topic_created`
/// message; anything that never reaches one belongs to General (`"1"`).
/// Groups come in order of their first message.
pub fn group_by_topic<'a, M: Message>(msgs: &'a [M]) -> Vec<Group<'a, M>> {
    let mut by_id: HashMap<&str, &M> = HashMap::with_capacity(msgs.len());
    let mut titles: HashMap<&str, &str> = HashMap::from([(GENERAL_TOPIC_ID, "General")]);
    for m in msgs {
        by_id.insert(m.id(), m);
        if is_topic_root(m) {
            titles.insert(m.id(), m.title().unwrap_or("Untitled"));
        }
    }

    let resolve_topic = |m: &'a M| -> &'a str {
        let mut cur = Some(m);
        let mut seen = HashSet::new();
        while let Some(c) = cur {
            if c.is_service() && titles.contains_key(c.id()) {
                return c.id(); // hit a topic root
            }
            let Some(parent) = c.reply_to() else { break };
            if titles.contains_key(parent) {
                return parent; // parent is a topic root
            }
            if !seen.insert(parent) {
                break; // cycle guard
            }
            cur = by_id.get(parent).copied();
        }
        GENERAL_TOPIC_ID
    };

    let mut groups: Vec<Group<'a, M>> = Vec::new();
    let mut slot: HashMap<&str, usize> = HashMap::new();
    for m in msgs.iter().filter(|m| !m.is_service()) {
        let topic_id = resolve_topic(m);
        let i = *slot.entry(topic_id).or_insert_with(|| {
            groups.push(Group {
                topic_id: topic_id.to_owned(),
                title: titles
                    .get(topic_id)
                    .copied()
                    .unwrap_or("General")
                    .to_owned(),
                messages: Vec::new(),
            });
            groups.len() - 1
        });
        groups[i].messages.push(m);
    }
    groups
}
