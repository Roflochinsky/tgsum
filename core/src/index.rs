//! Pass 1: a light index of every chat and forum topic (names, counts, dates).

use std::fs::File;
use std::io::{self, Read};
use std::ops::ControlFlow;
use std::path::Path;

use crate::model::{
    group_by_topic, is_forum, ChatIndex, Message, MessageMeta, RawChat, TopicIndex,
};
use crate::stream::stream_chats;

/// Indexes the export at `path`.
pub fn stream_index(path: &Path) -> io::Result<Vec<ChatIndex>> {
    index_reader(File::open(path)?)
}

/// Indexes an export read from `reader`.
pub fn index_reader(reader: impl Read) -> io::Result<Vec<ChatIndex>> {
    let mut out = Vec::new();
    stream_chats(reader, |chat: RawChat<MessageMeta>| {
        out.push(index_chat(chat));
        ControlFlow::Continue(())
    })?;
    Ok(out)
}

fn index_chat(chat: RawChat<MessageMeta>) -> ChatIndex {
    let content = chat.messages.iter().filter(|m| !m.is_service());
    let count = content.clone().count();
    let (first_date, last_date) = date_range(content);
    let topics = if is_forum(&chat.messages) {
        group_by_topic(&chat.messages)
            .into_iter()
            .map(|g| {
                let (first_date, last_date) = date_range(g.messages.iter().copied());
                TopicIndex {
                    count: g.messages.len(),
                    topic_id: g.topic_id,
                    title: g.title,
                    first_date,
                    last_date,
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    ChatIndex {
        chat_id: chat.id.clone(),
        name: chat.display_name(),
        kind: chat.kind,
        count,
        first_date,
        last_date,
        topics,
    }
}

/// First and last non-empty dates, in message order.
fn date_range<'a, M: Message + 'a>(
    msgs: impl Iterator<Item = &'a M>,
) -> (Option<String>, Option<String>) {
    let mut first = None;
    let mut last = None;
    for d in msgs.filter_map(|m| m.date()).filter(|d| !d.is_empty()) {
        first.get_or_insert(d);
        last = Some(d);
    }
    (first.map(str::to_owned), last.map(str::to_owned))
}
