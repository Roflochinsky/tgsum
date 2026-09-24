mod common;

use std::io::{self, Cursor, Read};
use std::ops::ControlFlow;

use common::{fixture, ids};
use tgsum_core::{
    extract_reader, extract_selection, index_reader, is_cancelled, stream_chats, stream_index,
    MessageMeta, ProgressReader, RawChat, Selection,
};

fn pick(chat_id: &str, topic_ids: &[&str]) -> Selection {
    Selection {
        chat_id: chat_id.to_owned(),
        topic_ids: topic_ids.iter().map(|t| t.to_string()).collect(),
    }
}

#[test]
fn index_chats_with_counts_dates_and_forum_topics() {
    let idx = stream_index(&fixture()).unwrap();
    let mut names: Vec<_> = idx.iter().map(|c| c.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["Direct with Bob", "Pilot Forum"]);

    let direct = idx.iter().find(|c| c.chat_id == "111").unwrap();
    assert_eq!(direct.count, 2); // service pin excluded
    assert!(direct.topics.is_empty());
    assert_eq!(direct.first_date.as_deref(), Some("2026-06-18T10:00:00"));
    assert_eq!(direct.last_date.as_deref(), Some("2026-06-19T11:00:00"));
    assert_eq!(direct.kind, "personal_chat");

    let forum = idx.iter().find(|c| c.chat_id == "222").unwrap();
    let mut titles: Vec<_> = forum.topics.iter().map(|t| t.title.as_str()).collect();
    titles.sort();
    assert_eq!(titles, ["Bugs", "General"]);
    let bugs = forum.topics.iter().find(|t| t.title == "Bugs").unwrap();
    assert_eq!(bugs.count, 2); // 101 + 102
    assert_eq!(bugs.topic_id, "100");
    assert_eq!(bugs.last_date.as_deref(), Some("2026-06-20T09:31:00"));
}

#[test]
fn extract_whole_non_forum_chat() {
    let units = extract_selection(&fixture(), &[pick("111", &[])]).unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].chat_name, "Direct with Bob");
    assert_eq!(units[0].topic_title, None);
    assert_eq!(ids(&units[0].messages), ["1", "2"]); // service excluded
}

#[test]
fn extract_single_forum_topic() {
    let units = extract_selection(&fixture(), &[pick("222", &["100"])]).unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].topic_title.as_deref(), Some("Bugs"));
    assert_eq!(ids(&units[0].messages), ["101", "102"]);
}

#[test]
fn extract_keeps_multiple_topics_of_the_same_chat() {
    let units =
        extract_selection(&fixture(), &[pick("222", &["1"]), pick("222", &["100"])]).unwrap();
    let mut titles: Vec<_> = units
        .iter()
        .map(|u| u.topic_title.as_deref().unwrap())
        .collect();
    titles.sort();
    assert_eq!(titles, ["Bugs", "General"]);
}

#[test]
fn extract_whole_chat_plus_topic_of_it() {
    let units = extract_selection(&fixture(), &[pick("222", &["100"]), pick("222", &[])]).unwrap();
    assert_eq!(units.len(), 2);
    assert_eq!(units[0].topic_title, None); // whole chat first
    assert_eq!(ids(&units[0].messages), ["50", "101", "102"]);
    assert_eq!(units[1].topic_title.as_deref(), Some("Bugs"));
}

#[test]
fn extract_nothing_selected_reads_nothing() {
    assert!(extract_reader(Cursor::new(b"not json"), &[])
        .unwrap()
        .is_empty());
}

#[test]
fn extract_stops_reading_after_the_last_selected_chat() {
    // Everything after the selected chat is garbage: reading it would fail.
    let json = r#"{"chats":{"list":[{"id":1,"name":"A","type":"personal_chat","messages":[{"id":1,"type":"message","text":"x"}]}, !!!garbage"#;
    let units = extract_reader(Cursor::new(json), &[pick("1", &[])]).unwrap();
    assert_eq!(units.len(), 1);
}

#[test]
fn ids_beyond_2_pow_53_stay_exact() {
    let json = r#"{"chats":{"list":[{"id":9007199254740993,"name":"Big","type":"private_supergroup","messages":[
        {"id":18446744073709551615,"type":"message","date":"2026-01-01T00:00:00","text":"a"},
        {"id":-9223372036854775808,"type":"message","reply_to_message_id":18446744073709551615,"text":"b"}]}]}}"#;
    let idx = index_reader(Cursor::new(json)).unwrap();
    assert_eq!(idx[0].chat_id, "9007199254740993");
    let units = extract_reader(Cursor::new(json), &[pick("9007199254740993", &[])]).unwrap();
    assert_eq!(
        ids(&units[0].messages),
        ["18446744073709551615", "-9223372036854775808"]
    );
    assert_eq!(
        units[0].messages[1].reply_to_message_id.as_deref(),
        Some("18446744073709551615")
    );
}

#[test]
fn tolerates_odd_types_and_skips_other_top_level_keys() {
    let json = r#"{
        "about": "x", "personal_information": {"first_name": "A", "list": [1, 2]},
        "left_chats": {"list": [{"id": 5, "name": "left", "messages": []}]},
        "chats": {"about": "chats", "list": [
            {"id": "7", "name": null, "type": "saved_messages", "messages": [
                {"id": 1, "type": "message", "text": 42, "photo": null, "file": "", "duration_seconds": "x"},
                {"id": 2, "type": "message", "text": [null, 5, {"type": "bold"}, {"text": 7}, "ok"]},
                {"id": 3, "type": "service", "action": "topic_created", "title": 123}
            ]}
        ]}
    }"#;
    let idx = index_reader(Cursor::new(json)).unwrap();
    assert_eq!(idx.len(), 1);
    assert_eq!(idx[0].name, "(no name 7)");
    assert_eq!(idx[0].count, 2);
    assert_eq!(idx[0].topics.len(), 1); // forum with only General content
    let units = extract_reader(Cursor::new(json), &[pick("7", &[])]).unwrap();
    let m = &units[0].messages;
    assert_eq!(tgsum_core::flatten_text(&m[0]), "");
    assert!(!m[0].photo && !m[0].file);
    assert_eq!(tgsum_core::flatten_text(&m[1]), "7ok");
}

#[test]
fn not_an_export_is_a_clear_error() {
    for bad in ["", "[1,2]", "{\"chats\": {\"list\": [1]}}", "{\"chats\":"] {
        let err = index_reader(Cursor::new(bad)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{bad}: {err}");
    }
}

#[test]
fn stream_can_stop_early() {
    let json = r#"{"chats":{"list":[{"id":1,"messages":[]},{"id":2,"messages":[]},{"id":3,"messages":[]}]}}"#;
    let mut seen = Vec::new();
    stream_chats(Cursor::new(json), |chat: RawChat<MessageMeta>| {
        seen.push(chat.id);
        if seen.len() == 2 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .unwrap();
    assert_eq!(seen, ["1", "2"]);
}

#[test]
fn progress_reader_reports_bytes_and_cancels() {
    let data = std::fs::read(fixture()).unwrap();
    let mut last = 0;
    let reader = ProgressReader::new(Cursor::new(&data), |n| {
        last = n;
        Ok(())
    });
    index_reader(reader).unwrap();
    assert_eq!(last, data.len() as u64);

    let reader = ProgressReader::new(Cursor::new(&data), |_| Err(tgsum_core::cancelled()));
    let err = index_reader(reader).unwrap_err();
    assert!(is_cancelled(&err), "{err}");

    let mut buf = Vec::new();
    ProgressReader::new(Cursor::new(&data), |_| Ok(()))
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf, data);
}

#[test]
fn resolve_export_path_accepts_file_or_export_folder() {
    let file = fixture();
    assert_eq!(tgsum_core::resolve_export_path(&file), Some(file.clone()));
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(tgsum_core::resolve_export_path(dir.path()), None);
    std::fs::copy(&file, dir.path().join("result.json")).unwrap();
    assert_eq!(
        tgsum_core::resolve_export_path(dir.path()),
        Some(dir.path().join("result.json"))
    );
}
