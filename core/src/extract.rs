//! Pass 2: stream the export again and pull out only the selected chats/topics.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read};
use std::ops::ControlFlow;
use std::path::Path;

use crate::model::{group_by_topic, strip_service, ExtractedUnit, RawChat, RawMessage, Selection};
use crate::stream::stream_chats;

/// Extracts `selection` from the export at `path`.
pub fn extract_selection(path: &Path, selection: &[Selection]) -> io::Result<Vec<ExtractedUnit>> {
    extract_reader(File::open(path)?, selection)
}

#[derive(Default)]
struct Wanted {
    whole: bool,
    topic_ids: Vec<String>,
}

/// Extracts `selection` from an export read from `reader`. Units come in file
/// order; within a chat the whole chat comes first, then its topics in
/// selection order. Reading stops as soon as every selected chat was seen.
pub fn extract_reader(
    reader: impl Read,
    selection: &[Selection],
) -> io::Result<Vec<ExtractedUnit>> {
    // Merge picks per chat so several topics of the same chat (and a
    // whole-chat pick) all survive.
    let mut by_chat: HashMap<&str, Wanted> = HashMap::new();
    for s in selection {
        let want = by_chat.entry(s.chat_id.as_str()).or_default();
        if s.topic_ids.is_empty() {
            want.whole = true;
        }
        for t in &s.topic_ids {
            if !want.topic_ids.contains(t) {
                want.topic_ids.push(t.clone());
            }
        }
    }

    let mut units = Vec::new();
    let mut pending: HashSet<&str> = by_chat.keys().copied().collect();
    if pending.is_empty() {
        return Ok(units);
    }
    stream_chats(reader, |chat: RawChat<RawMessage>| {
        let Some(want) = by_chat.get(chat.id.as_str()) else {
            return ControlFlow::Continue(());
        };
        pending.remove(chat.id.as_str());
        extract_chat(chat, want, &mut units);
        if pending.is_empty() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?;
    Ok(units)
}

fn extract_chat(chat: RawChat<RawMessage>, want: &Wanted, units: &mut Vec<ExtractedUnit>) {
    let name = chat.display_name();
    let topics: Vec<ExtractedUnit> = if want.topic_ids.is_empty() {
        Vec::new()
    } else {
        let groups = group_by_topic(&chat.messages);
        want.topic_ids
            .iter()
            .filter_map(|id| groups.iter().find(|g| &g.topic_id == id))
            .map(|g| ExtractedUnit {
                chat_name: name.clone(),
                topic_title: Some(g.title.clone()),
                messages: g.messages.iter().map(|m| (*m).clone()).collect(),
            })
            .collect()
    };
    if want.whole {
        units.push(ExtractedUnit {
            chat_name: name,
            topic_title: None,
            messages: strip_service(chat.messages),
        });
    }
    units.extend(topics);
}
