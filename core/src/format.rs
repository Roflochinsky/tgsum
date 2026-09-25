//! Markdown formatter: header, day sections, speakers, replies, media markers,
//! and lossless splitting into parts under a token budget.
//!
//! ```text
//! # Чат: Команда / Топик: Релизы
//! # Период: 2026-06-18 — 2026-06-20 | сообщений: 142 | участники: Алиса, Боб
//!
//! ## 2026-06-20
//! [14:02] Алиса: когда катим релиз?
//! [14:05] Вера ↳ Боб «смёржь PR #210»: смёржила
//! ```

use std::collections::{HashMap, HashSet};

use crate::model::{flatten_text, resolve_name, ExtractedUnit, RawMessage};
use crate::text::{js_number, js_trim, utf16_len, utf16_slice};

/// Default token budget per file: a soft cap under a ~100k context.
pub const DEFAULT_MAX_TOKENS: usize = 90_000;

/// Characters of the replied-to message quoted after `↳ Name`.
const QUOTE_LEN: usize = 40;

/// Token estimate: UTF-16 length / 2.5 (Cyrillic-aware; chars/4 under-counts
/// Russian about 2×), rounded up. No tokenizer dependency.
pub fn est_tokens(s: &str) -> usize {
    (utf16_len(s) * 2).div_ceil(5)
}

/// A file-system-safe name: reserved characters become `_`; a name with no
/// letters or digits left falls back to `chat`.
pub fn safe_name(s: &str) -> String {
    let replaced: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    let cleaned = js_trim(&replaced);
    if cleaned.chars().any(char::is_alphanumeric) {
        cleaned.to_owned()
    } else {
        "chat".to_owned()
    }
}

fn day_of(date: Option<&str>) -> &str {
    match date {
        Some(d) if !d.is_empty() => utf16_slice(d, 0, 10),
        _ => "unknown",
    }
}

fn time_of(date: Option<&str>) -> &str {
    match date {
        Some(d) if !d.is_empty() => utf16_slice(d, 11, 16),
        _ => "--:--",
    }
}

fn media_marker(m: &RawMessage) -> Option<String> {
    match m.media_type.as_deref() {
        Some(kind @ ("voice_message" | "video_message" | "audio_file")) => {
            let d = m.duration_seconds.unwrap_or(0.0);
            let kind = match kind {
                "voice_message" => "voice",
                "video_message" => "video",
                _ => "audio",
            };
            let (mm, ss) = (js_number((d / 60.0).floor()), js_number(d % 60.0));
            Some(format!("[{kind} {mm}:{ss:0>2}]"))
        }
        Some("sticker") => Some(format!(
            "[sticker {}]",
            m.sticker_emoji.as_deref().unwrap_or("")
        )),
        Some("animation" | "video_file") => Some("[video]".to_owned()),
        _ if m.photo => Some("[photo]".to_owned()),
        _ if m.file => Some(format!(
            "[file: {}]",
            m.file_name.as_deref().unwrap_or("attachment")
        )),
        _ => None,
    }
}

fn line_for(m: &RawMessage, by_id: &HashMap<&str, &RawMessage>) -> String {
    let name = resolve_name(m);
    let time = time_of(m.date.as_deref());
    let reply = m
        .reply_to_message_id
        .as_deref()
        .and_then(|id| by_id.get(id))
        .map(|target| {
            let quote = utf16_slice(flatten_text(target), 0, QUOTE_LEN);
            format!(" ↳ {} «{quote}»", resolve_name(target))
        })
        .unwrap_or_default();
    let text = flatten_text(m);
    let body = match media_marker(m) {
        Some(marker) if text.is_empty() => marker,
        Some(marker) => format!("{marker} {text}"),
        None => text.to_owned(),
    };
    format!("[{time}] {name}{reply}: {}", js_trim(&body))
}

/// A formatted unit: its file name and one or more parts (each a full file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formatted {
    pub filename: String,
    pub parts: Vec<String>,
}

struct Block {
    day: String,
    text: String,
    is_day_header: bool,
}

/// Formats a unit into Markdown, split into parts of at most `max_tokens`
/// (estimated). No message is ever dropped: every part repeats the unit
/// header, and a part that starts mid-day re-opens with that day's header.
pub fn format_unit(unit: &ExtractedUnit, max_tokens: usize) -> Formatted {
    let msgs = &unit.messages;
    let by_id: HashMap<&str, &RawMessage> = msgs.iter().map(|m| (m.id.as_str(), m)).collect();

    let mut dates = msgs
        .iter()
        .filter_map(|m| m.date.as_deref())
        .filter(|d| !d.is_empty());
    let first = dates.next();
    let period = match (first, dates.next_back().or(first)) {
        (Some(f), Some(l)) => format!("{} — {}", day_of(Some(f)), day_of(Some(l))),
        _ => "n/a".to_owned(),
    };

    let mut seen = HashSet::new();
    let participants: Vec<&str> = msgs
        .iter()
        .map(resolve_name)
        .filter(|name| seen.insert(*name))
        .collect();

    let topic = unit.topic_title.as_deref().filter(|t| !t.is_empty());
    let title_line = match topic {
        Some(t) => format!("# Чат: {} / Топик: {t}", unit.chat_name),
        None => format!("# Чат: {}", unit.chat_name),
    };
    let header = format!(
        "{title_line}\n# Период: {period} | сообщений: {} | участники: {}\n",
        msgs.len(),
        participants.join(", ")
    );

    // Body blocks, each tagged with its day so a new part can re-emit the
    // `## <day>` header for messages it carries over.
    let mut blocks = Vec::with_capacity(msgs.len() + 1);
    let mut cur_day = "";
    for m in msgs {
        let day = day_of(m.date.as_deref());
        if day != cur_day {
            blocks.push(Block {
                day: day.to_owned(),
                text: format!("\n## {day}"),
                is_day_header: true,
            });
            cur_day = day;
        }
        blocks.push(Block {
            day: day.to_owned(),
            text: line_for(m, &by_id),
            is_day_header: false,
        });
    }

    // Pack blocks into parts under the budget.
    let header_cost = est_tokens(&header);
    let mut parts = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut cost = header_cost;
    let mut part_day = String::new(); // day header already present in this part
    for b in blocks {
        let c = est_tokens(&b.text) + 1;
        if !cur.is_empty() && cost + c > max_tokens {
            parts.push(format!("{header}{}", cur.join("\n")));
            cur.clear();
            cost = header_cost;
            part_day.clear();
        }
        if b.is_day_header {
            part_day.clone_from(&b.day);
        } else if part_day != b.day {
            let dh = format!("\n## {}", b.day);
            cost += est_tokens(&dh) + 1;
            cur.push(dh);
            part_day.clone_from(&b.day);
        }
        cur.push(b.text);
        cost += c;
    }
    if !cur.is_empty() {
        parts.push(format!("{header}{}", cur.join("\n")));
    }

    let topic_part = topic
        .map(|t| format!("__{}", safe_name(t)))
        .unwrap_or_default();
    Formatted {
        filename: format!("{}{topic_part}.md", safe_name(&unit.chat_name)),
        parts,
    }
}
