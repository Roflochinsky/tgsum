//! tgsum core: Telegram Desktop export (`result.json`) → AI-ready Markdown.
//!
//! Two streaming passes over the export: [`index_reader`] builds a light
//! chat/topic index, [`extract_reader`] pulls out only the selected chats and
//! topics, [`format_unit`] turns each into Markdown and [`write_units`] saves
//! the files. Nothing leaves the machine: no network, no LLM.

pub mod bridge;
pub mod connector;
mod de;
pub mod extract;
pub mod format;
pub mod index;
pub mod model;
pub mod output;
pub mod progress;
pub mod project;
pub mod snapshot;
pub mod stream;
pub mod text;

use std::path::{Path, PathBuf};

pub use extract::{extract_reader, extract_selection};
pub use format::{est_tokens, format_unit, safe_name, Formatted, DEFAULT_MAX_TOKENS};
pub use index::{index_reader, stream_index};
pub use model::{
    flatten_text, group_by_topic, is_forum, resolve_name, strip_service, ChatIndex, ExtractedUnit,
    Group, Message, MessageMeta, RawChat, RawMessage, Selection, TopicIndex, GENERAL_TOPIC_ID,
};
pub use output::write_units;
pub use progress::{cancelled, is_cancelled, ProgressReader};
pub use stream::stream_chats;

/// File name Telegram Desktop gives a machine-readable export.
pub const EXPORT_FILE_NAME: &str = "result.json";

/// The export file for a user-supplied path: the file itself, or
/// `result.json` inside a dropped export folder. `None` if neither exists.
pub fn resolve_export_path(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let inner = path.join(EXPORT_FILE_NAME);
    inner.is_file().then_some(inner)
}
