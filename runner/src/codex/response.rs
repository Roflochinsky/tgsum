use std::collections::HashMap;

use serde::{de::DeserializeOwned, Deserialize};

use super::{
    DecodeError, Decoded, Usage, MAX_EVENTS, MAX_EVENT_BYTES, MAX_OUTPUT_BYTES, MAX_RESULT_BYTES,
};
use crate::RunOutput;

#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum Event {
    #[serde(rename = "thread.started")]
    ThreadStarted { thread_id: String },
    #[serde(rename = "turn.started")]
    TurnStarted {},
    #[serde(rename = "turn.completed")]
    TurnCompleted { usage: Usage },
    #[serde(rename = "turn.failed")]
    TurnFailed { error: serde::de::IgnoredAny },
    #[serde(rename = "error")]
    Error { message: serde::de::IgnoredAny },
    #[serde(rename = "item.started")]
    ItemStarted { item: Item },
    #[serde(rename = "item.updated")]
    ItemUpdated { item: Item },
    #[serde(rename = "item.completed")]
    ItemCompleted { item: Item },
}

// Deliberately supports only text/reasoning/todo items. Unknown payload fields,
// item kinds and duplicate fields fail closed. A new CLI needs a new review.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
    items: Option<Vec<Todo>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Todo {
    #[serde(rename = "text")]
    _text: String,
    #[serde(rename = "completed")]
    _completed: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum ItemKind {
    Agent,
    Reasoning,
    Todo,
}

struct ItemState {
    kind: ItemKind,
    completed: bool,
}

#[derive(Clone, Copy)]
enum Update {
    Started,
    Updated,
    Completed,
}

/// Decode the reviewed single-turn text/plan subset of the exec JSONL stream.
/// `validate` is mandatory and must enforce the trusted recipe's constraints
/// and evidence membership/revisions against the exact reviewed bundle. Use a
/// strict output type (`deny_unknown_fields`); provider schema guidance alone
/// is not local validation. No error includes arbitrary stdout/stderr content.
pub fn decode<T: DeserializeOwned>(
    output: &RunOutput,
    validate: impl FnOnce(&T) -> bool,
) -> Result<Decoded<T>, DecodeError> {
    if !output.process_succeeded() {
        return Err(DecodeError::Process {
            termination: output.termination,
            exit_code: output.exit_code,
        });
    }
    let stream_error = |line, reason| DecodeError::Stream { line, reason };
    if output.stdout.len() > MAX_OUTPUT_BYTES {
        return Err(stream_error(0, "output limit exceeded"));
    }
    let mut thread_started = false;
    let mut turn_started = false;
    let mut usage = None;
    let mut items = HashMap::<String, ItemState>::new();
    let mut final_text = None;
    // strip_suffix permits one final newline, but no blank embedded records.
    let bytes = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
    for (index, bytes) in bytes.split(|b| *b == b'\n').enumerate() {
        let line = index + 1;
        let error = |reason| stream_error(line, reason);
        if line > MAX_EVENTS || bytes.len() > MAX_EVENT_BYTES {
            return Err(error("event limit exceeded"));
        }
        if usage.is_some() {
            return Err(error("event after turn completion"));
        }
        let event: Event =
            serde_json::from_slice(bytes).map_err(|_| error("malformed or unsupported event"))?;
        match event {
            Event::Error { message } | Event::TurnFailed { error: message } => {
                let _ = message;
                return Err(error("CLI reported failure"));
            }
            Event::ThreadStarted { thread_id } => {
                if thread_started || !valid_id(&thread_id) {
                    return Err(error("invalid thread start"));
                }
                thread_started = true;
            }
            Event::TurnStarted {} => {
                if !thread_started || turn_started {
                    return Err(error("invalid turn start"));
                }
                turn_started = true;
            }
            Event::TurnCompleted { usage: tokens } => {
                if !turn_started || final_text.is_none() || items.values().any(|i| !i.completed) {
                    return Err(error("incomplete turn"));
                }
                usage = Some(tokens);
            }
            event => {
                if !turn_started {
                    return Err(error("item outside a turn"));
                }
                let (item, update) = match event {
                    Event::ItemStarted { item } => (item, Update::Started),
                    Event::ItemUpdated { item } => (item, Update::Updated),
                    Event::ItemCompleted { item } => (item, Update::Completed),
                    _ => unreachable!("non-item events handled above"),
                };
                let kind = match item.kind.as_str() {
                    "agent_message" => ItemKind::Agent,
                    "reasoning" => ItemKind::Reasoning,
                    "todo_list" => ItemKind::Todo,
                    _ => return Err(error("tool, error or unsupported item")),
                };
                if !valid_id(&item.id) {
                    return Err(error("invalid item ID"));
                }
                match kind {
                    ItemKind::Agent | ItemKind::Reasoning => {
                        if item.text.is_none() || item.items.is_some() {
                            return Err(error("invalid text item"));
                        }
                    }
                    ItemKind::Todo => {
                        if item.text.is_some() || item.items.is_none() {
                            return Err(error("invalid todo item"));
                        }
                        // Plan text is validated as data and discarded.
                    }
                }
                match (items.get(&item.id), update) {
                    (None, Update::Started | Update::Completed) => {}
                    (Some(state), Update::Updated | Update::Completed)
                        if !state.completed && state.kind == kind => {}
                    _ => return Err(error("invalid item lifecycle")),
                }
                items.insert(
                    item.id,
                    ItemState {
                        kind,
                        completed: matches!(update, Update::Completed),
                    },
                );
                if kind == ItemKind::Agent && matches!(update, Update::Completed) {
                    let text = item.text.expect("checked above");
                    if text.len() > MAX_RESULT_BYTES {
                        return Err(error("result limit exceeded"));
                    }
                    // The last completed agent message is the final answer.
                    // Earlier commentary need not be JSON and is discarded.
                    final_text = Some(text);
                }
            }
        }
    }
    let usage = usage.ok_or_else(|| stream_error(0, "missing turn completion"))?;
    let value: T = serde_json::from_str(
        final_text
            .as_deref()
            .expect("completion requires a message"),
    )
    .map_err(|_| DecodeError::InvalidResult)?;
    if !validate(&value) {
        return Err(DecodeError::RejectedResult);
    }
    Ok(Decoded { value, usage })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
