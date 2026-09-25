mod common;

use common::{ids, msg, msgs};
use serde_json::json;
use tgsum_core::{flatten_text, group_by_topic, is_forum, resolve_name, strip_service};

#[test]
fn flatten_text_handles_plain_string() {
    assert_eq!(
        flatten_text(&msg(json!({ "id": 1, "type": "message", "text": "hi" }))),
        "hi"
    );
}

#[test]
fn flatten_text_prefers_text_entities() {
    let m = msg(json!({
        "id": 1, "type": "message",
        "text": ["Check ", { "type": "bold", "text": "this" }],
        "text_entities": [
            { "type": "plain", "text": "Check " },
            { "type": "bold", "text": "this" },
            { "type": "link", "text": "http://x" },
        ],
    }));
    assert_eq!(flatten_text(&m), "Check thishttp://x");
}

#[test]
fn flatten_text_flattens_array_text_without_entities() {
    // Old exports have no `text_entities`.
    let m =
        msg(json!({ "id": 1, "type": "message", "text": ["a", { "type": "bold", "text": "b" }] }));
    assert_eq!(flatten_text(&m), "ab");
}

#[test]
fn flatten_text_ignores_empty_entities() {
    let m = msg(json!({ "id": 1, "type": "message", "text": "fallback", "text_entities": [] }));
    assert_eq!(flatten_text(&m), "fallback");
}

#[test]
fn resolve_name_uses_from() {
    let m = msg(json!({ "id": 1, "type": "message", "from": "Alice", "from_id": "user1" }));
    assert_eq!(resolve_name(&m), "Alice");
}

#[test]
fn resolve_name_falls_back_to_from_id() {
    let m = msg(json!({ "id": 1, "type": "message", "from": null, "from_id": "user9" }));
    assert_eq!(resolve_name(&m), "user9");
}

#[test]
fn resolve_name_accepts_numeric_from_id_and_actor() {
    // Old exports: bare integer `from_id`; service messages use `actor`.
    assert_eq!(resolve_name(&msg(json!({ "id": 1, "from_id": 42 }))), "42");
    assert_eq!(
        resolve_name(&msg(json!({ "id": 1, "type": "service", "actor": "Bob" }))),
        "Bob"
    );
    assert_eq!(resolve_name(&msg(json!({ "id": 1 }))), "unknown");
}

#[test]
fn strip_service_drops_service_keeps_real_messages() {
    let m = msgs(json!([
        { "id": 1, "type": "service", "action": "pin_message" },
        { "id": 2, "type": "message", "text": "real" },
    ]));
    assert_eq!(ids(&strip_service(m)), ["2"]);
}

#[test]
fn group_by_topic_walks_replies_up_to_topic_created() {
    let m = msgs(json!([
        { "id": 100, "type": "service", "action": "topic_created", "title": "Bugs" },
        { "id": 101, "type": "message", "text": "in bugs", "reply_to_message_id": 100 },
        { "id": 102, "type": "message", "text": "reply in bugs", "reply_to_message_id": 101 },
        { "id": 5, "type": "message", "text": "general msg" },
    ]));
    assert!(is_forum(&m));
    let groups = group_by_topic(&m);
    let bugs = groups.iter().find(|g| g.topic_id == "100").unwrap();
    assert_eq!(bugs.title, "Bugs");
    assert_eq!(
        bugs.messages
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["101", "102"]
    );
    let general = groups.iter().find(|g| g.topic_id == "1").unwrap();
    assert_eq!(
        general
            .messages
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["5"]
    );
}

#[test]
fn group_by_topic_survives_reply_cycles_and_orphans() {
    let m = msgs(json!([
        { "id": 100, "type": "service", "action": "topic_created" },
        { "id": 7, "type": "message", "reply_to_message_id": 8 },
        { "id": 8, "type": "message", "reply_to_message_id": 7 },
        { "id": 9, "type": "message", "reply_to_message_id": 12345 },
    ]));
    let groups = group_by_topic(&m);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].topic_id, "1");
    assert_eq!(groups[0].messages.len(), 3);
    assert!(is_forum(&m));
}

#[test]
fn untitled_topic_gets_a_placeholder_title() {
    let m = msgs(json!([
        { "id": 100, "type": "service", "action": "topic_created" },
        { "id": 101, "type": "message", "reply_to_message_id": 100 },
    ]));
    assert_eq!(group_by_topic(&m)[0].title, "Untitled");
}
