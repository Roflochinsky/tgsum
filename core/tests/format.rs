mod common;

use common::{big_unit, msg, msgs, unit};
use serde_json::json;
use tgsum_core::format_unit;

fn forum_unit() -> tgsum_core::ExtractedUnit {
    unit(
        "Pilot Forum",
        Some("Bugs"),
        msgs(json!([
            { "id": 101, "type": "message", "date": "2026-06-18T09:31:00", "from": "Bob", "from_id": "user2", "text": "found a bug" },
            { "id": 102, "type": "message", "date": "2026-06-18T09:32:00", "from": "Alice", "from_id": "user1", "reply_to_message_id": 101, "text": "fixing it" },
            { "id": 103, "type": "message", "date": "2026-06-18T09:40:00", "from": "Alice", "from_id": "user1", "media_type": "voice_message", "duration_seconds": 42 },
        ])),
    )
}

#[test]
fn header_day_section_replies_and_media_markers() {
    let f = format_unit(&forum_unit(), 100_000);
    assert_eq!(f.filename, "Pilot Forum__Bugs.md");
    assert_eq!(f.parts.len(), 1);
    let md = &f.parts[0];
    assert!(md.contains("# Чат: Pilot Forum / Топик: Bugs"));
    assert!(md.contains("сообщений: 3"));
    assert!(md.contains("## 2026-06-18"));
    assert!(md.contains("[09:31] Bob: found a bug"));
    assert!(md.contains("[09:32] Alice ↳ Bob «found a bug»: fixing it"));
    assert!(md.contains("[09:40] Alice: [voice 0:42]"));
}

#[test]
fn exact_layout() {
    let f = format_unit(&forum_unit(), 100_000);
    assert_eq!(
        f.parts[0],
        "# Чат: Pilot Forum / Топик: Bugs\n\
         # Период: 2026-06-18 — 2026-06-18 | сообщений: 3 | участники: Bob, Alice\n\
         \n\
         ## 2026-06-18\n\
         [09:31] Bob: found a bug\n\
         [09:32] Alice ↳ Bob «found a bug»: fixing it\n\
         [09:40] Alice: [voice 0:42]"
    );
}

#[test]
fn splits_into_parts_by_token_budget_repeating_the_header() {
    let f = format_unit(&big_unit("C", 50, 'x'), 60);
    assert!(f.parts.len() > 1);
    for p in &f.parts {
        assert!(p.contains("# Чат: C"));
    }
}

#[test]
fn repeats_the_day_header_on_every_part_with_that_days_messages() {
    let f = format_unit(&big_unit("D", 50, 'x'), 60);
    assert!(f.parts.len() > 1);
    for p in &f.parts {
        if p.lines().any(|l| l.starts_with("[09:00]")) {
            assert!(p.contains("## 2026-06-18"), "part without day header:\n{p}");
        }
    }
}

#[test]
fn splitting_is_lossless() {
    let f = format_unit(&big_unit("L", 50, 'x'), 60);
    let lines = f
        .parts
        .iter()
        .flat_map(|p| p.lines())
        .filter(|l| l.starts_with("[09:00]"))
        .count();
    assert_eq!(lines, 50);
}

#[test]
fn falls_back_to_chat_when_name_has_no_alphanumerics() {
    let u = unit(
        ":::",
        None,
        vec![msg(
            json!({ "id": 1, "type": "message", "date": "2026-06-18T09:00:00", "from": "X", "text": "hi" }),
        )],
    );
    assert_eq!(format_unit(&u, 100_000).filename, "chat.md");
}

#[test]
fn safe_name_replaces_reserved_characters() {
    let u = unit(
        r#"a/b\c:d*e?f"g<h>i|j"#,
        Some(" Q/A "),
        vec![msg(json!({ "id": 1, "text": "hi" }))],
    );
    assert_eq!(
        format_unit(&u, 100_000).filename,
        "a_b_c_d_e_f_g_h_i_j__Q_A.md"
    );
}

#[test]
fn media_markers() {
    let cases = [
        (
            json!({ "media_type": "video_message", "duration_seconds": 125 }),
            "[video 2:05]",
        ),
        (json!({ "media_type": "audio_file" }), "[audio 0:00]"),
        (
            json!({ "media_type": "sticker", "sticker_emoji": "👍" }),
            "[sticker 👍]",
        ),
        (json!({ "media_type": "animation" }), "[video]"),
        (
            json!({ "photo": "photos/1.jpg", "text": "логи деплоя" }),
            "[photo] логи деплоя",
        ),
        (
            json!({ "file": "files/a.pdf", "file_name": "a.pdf" }),
            "[file: a.pdf]",
        ),
        (json!({ "file": "files/x" }), "[file: attachment]"),
    ];
    for (fields, expected) in cases {
        let mut m =
            json!({ "id": 1, "type": "message", "date": "2026-06-18T09:00:00", "from": "X" });
        m.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let md = &format_unit(&unit("M", None, vec![msg(m)]), 100_000).parts[0];
        assert!(md.ends_with(&format!("[09:00] X: {expected}")), "{md}");
    }
}

#[test]
fn reply_quote_is_cut_to_40_chars() {
    let long = "а".repeat(60);
    let u = unit(
        "Q",
        None,
        msgs(json!([
            { "id": 1, "date": "2026-06-18T09:00:00", "from": "A", "text": long },
            { "id": 2, "date": "2026-06-18T09:01:00", "from": "B", "reply_to_message_id": 1, "text": "ok" },
        ])),
    );
    let md = &format_unit(&u, 100_000).parts[0];
    assert!(md.contains(&format!("[09:01] B ↳ A «{}»: ok", "а".repeat(40))));
}

#[test]
fn messages_without_dates() {
    let u = unit(
        "N",
        None,
        vec![msg(json!({ "id": 1, "from": "A", "text": "hi" }))],
    );
    let md = &format_unit(&u, 100_000).parts[0];
    assert!(md.contains("# Период: n/a | сообщений: 1 | участники: A"));
    assert!(md.contains("## unknown\n[--:--] A: hi"));
}

#[test]
fn empty_unit_has_no_parts() {
    assert!(format_unit(&unit("E", None, vec![]), 100_000)
        .parts
        .is_empty());
}
