use super::*;
use std::fs::OpenOptions;
use std::io::{Cursor, Write};

use serde_json::json;

use crate::local_package::PackageSettings;
use crate::project::ProjectChange;
use crate::snapshot::SourceScope;
use crate::telegram_debug::settings::ContinuousSettings;

const CHAT: &str = "9007199254740995";
const SENDER: &str = "9007199254740993";
const SENT: i64 = 1_700_000_000;

#[test]
fn schema_ten_stays_passive_and_cannot_smuggle_client_control() {
    let f = Fixture::new();
    let path = f
        .store
        .directory(&f.project.project_id)
        .unwrap()
        .join("revisions")
        .join(format!("{:020}.json", f.project.revision));
    let mut old = serde_json::to_value(&f.project).unwrap();
    old["schema_version"] = 10.into();
    old["telegram_continuous"]["selected"]
        .as_object_mut()
        .unwrap()
        .remove("manage_client");
    let bytes = serde_json::to_vec(&old).unwrap();
    fs::write(&path, &bytes).unwrap();
    let upgraded = f.store.open(&f.project.project_id).unwrap();
    assert_eq!(upgraded.schema_version, 11);
    assert!(!upgraded.telegram_continuous["selected"].manage_client);
    assert_eq!(
        upgraded.telegram_continuous["selected"].settings,
        f.project.telegram_continuous["selected"].settings
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    old["telegram_continuous"]["selected"]["manage_client"] = true.into();
    let forged = serde_json::to_vec(&old).unwrap();
    fs::write(&path, &forged).unwrap();
    assert!(f.store.open(&f.project.project_id).is_err());
    assert_eq!(fs::read(&path).unwrap(), forged);
    fs::write(&path, bytes).unwrap();
    let mut stopped_settings = upgraded.telegram_continuous["selected"].settings.clone();
    stopped_settings.enabled = false;
    let controlled = f
        .store
        .update(
            &f.project.project_id,
            f.project.revision,
            ProjectChange::TelegramContinuous {
                source_id: "selected".into(),
                settings: stopped_settings,
                manage_client: true,
            },
        )
        .unwrap();
    assert!(controlled.telegram_continuous["selected"].manage_client);
    assert_eq!(f.store.open(&f.project.project_id).unwrap(), controlled);
}

struct Fixture {
    _root: tempfile::TempDir,
    root: PathBuf,
    input: PathBuf,
    logs: PathBuf,
    store: ProjectStore,
    project: Project,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let input = root.join("archive");
        let logs = root.join("DebugLogs");
        fs::create_dir(&input).unwrap();
        fs::create_dir(&logs).unwrap();
        fs::create_dir(input.join("photos")).unwrap();
        fs::write(input.join("photos/image.png"), b"synthetic image").unwrap();
        let archive = serde_json::to_vec(&json!({
            "id": CHAT, "name":"Synthetic Ж 😀", "type":"private_supergroup",
            "messages":[
                {"id":1,"text":"Initial body","from":"Alice","from_id":format!("user{SENDER}"),
                 "date":"2023-11-14T22:13:20Z","date_unixtime":SENT.to_string(),
                 "photo":"photos/image.png","reply_to_message_id":9},
                {"id":2,"text":"Only in bootstrap","from":"Bob","from_id":"user5"},
                {"id":9,"type":"service","action":"topic_created","title":"Topic"}
            ]
        }))
        .unwrap();
        fs::write(input.join("result.json"), &archive).unwrap();
        let store = ProjectStore::new(root.join("projects"));
        let project = store.create("Synthetic continuous").unwrap();
        let scope = SourceScope::telegram("Основной: fixture 😀", CHAT);
        store
            .snapshots(&project.project_id)
            .unwrap()
            .import_telegram("bootstrap", &scope, Cursor::new(archive))
            .unwrap();
        let project = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::Source(ProjectSource {
                    source_id: "selected".into(),
                    connector_id: "telegram_json".into(),
                    scope,
                    archive_path: Some(input.join("result.json")),
                    latest_snapshot_id: Some("bootstrap".into()),
                    selection: Default::default(),
                }),
            )
            .unwrap();
        let settings = ContinuousSettings {
            enabled: true,
            generation: "selected-generation".into(),
            input_directory: logs.clone(),
            peer: TypedPeer {
                kind: PeerKind::Channel,
                id: CHAT.into(),
            },
            self_user_id: None,
            confirmed_single_account: true,
            bootstrap_snapshot_id: "bootstrap".into(),
        };
        let project = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::TelegramContinuous {
                    source_id: "selected".into(),
                    settings,
                    manage_client: false,
                },
            )
            .unwrap();
        Self {
            _root: temporary,
            root,
            input,
            logs,
            store,
            project,
        }
    }

    fn append(&self, text: &str) {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.logs.join("mtp_12_00.txt"))
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    fn capture(&self) -> ContinuousCapture {
        self.store
            .open_telegram_capture(&self.project.project_id, "selected")
            .unwrap()
    }

    fn apply(&self, capture: &mut ContinuousCapture) -> Project {
        self.store
            .apply_telegram_observations(&self.project.project_id, "selected", capture)
            .unwrap()
    }

    fn snapshot(&self, project: &Project) -> Snapshot {
        self.store
            .snapshots(&project.project_id)
            .unwrap()
            .load(project.sources[0].latest_snapshot_id.as_ref().unwrap())
            .unwrap()
    }

    fn stop(&self) -> Project {
        let project = self.store.open(&self.project.project_id).unwrap();
        let mut settings = project.telegram_continuous["selected"].settings.clone();
        settings.enabled = false;
        self.store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::TelegramContinuous {
                    source_id: "selected".into(),
                    settings,
                    manage_client: false,
                },
            )
            .unwrap()
    }

    fn configure_package(&self, source_ids: Vec<String>) {
        let output = self.root.join("packages");
        fs::create_dir(&output).unwrap();
        let project = self.store.open(&self.project.project_id).unwrap();
        self.store
            .configure_local_package(
                &project.project_id,
                project.revision,
                PackageSettings {
                    source_ids,
                    input_directory: self.input.clone(),
                    output_directory: output,
                    automatic: true,
                    include_images: true,
                    include_office: false,
                    github_repository: None,
                    cloud_processing: false,
                },
            )
            .unwrap();
    }
}

fn frame(body: &str) -> String {
    format!("[12:00:00.123 00-0000001] (dc:2_main) Recv: {{ core_message\nbody: {body},\n}} (dc:2,key:123456,session:987654)\n")
}

fn message(id: u32, text: &str, edited: Option<i64>) -> String {
    let text = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    let edit = edited.map_or(String::new(), |time| format!("edit_date: {time} [INT],\n"));
    let update = if edited.is_some() {
        "updateEditChannelMessage"
    } else {
        "updateNewChannelMessage"
    };
    frame(&format!("{{ updateShort\nupdate: {{ {update}\nmessage: {{ message\nid: {id} [INT],\npeer_id: {{ peerChannel\nchannel_id: {CHAT} [LONG],\n}},\nfrom_id: {{ peerUser\nuser_id: {SENDER} [LONG],\n}},\ndate: {SENT} [INT],\n{edit}message: \"{text}\" [STRING],\n}},\n}},\n}}"))
}

fn delete(id: u32) -> String {
    frame(&format!("{{ updateDeleteChannelMessages\nchannel_id: {CHAT} [LONG],\nmessages: [ vector<0xa8509bda> (1) {id} [INT], ],\n}}"))
}

#[test]
fn projection_preserves_bootstrap_and_exact_identity_and_does_not_revert_late_edits() {
    let f = Fixture::new();
    let original = f.snapshot(&f.project);
    f.append(&message(1, "Edited Ж 😀", Some(SENT + 20)));
    f.append(&message(3, "New 😀", None));
    f.append(&message(1, "Late stale body", None));
    f.append(&delete(2));
    f.append(&delete(4));
    f.append(&message(4, "Late body after deletion", None));
    let mut capture = f.capture();
    capture.poll().unwrap();
    let p = f.apply(&mut capture);
    let s = f.snapshot(&p);
    assert_eq!(s.coverage.level, CoverageLevel::Partial);
    let first = &s.messages[0];
    assert_eq!(first.text, "Edited Ж 😀");
    assert_eq!(
        first.sender_id.as_deref(),
        Some(format!("user{SENDER}").as_str())
    );
    assert_eq!(first.sender_name.as_deref(), Some("Alice"));
    assert_eq!(first.key.source.account_local_id, "Основной: fixture 😀");
    assert_eq!(first.timestamp, original.messages[0].timestamp);
    assert_eq!(first.timestamp_unix, original.messages[0].timestamp_unix);
    assert_eq!(first.reply_to, original.messages[0].reply_to);
    assert_eq!(first.thread_id, original.messages[0].thread_id);
    assert_eq!(first.attachments, original.messages[0].attachments);
    assert_eq!(first.edited_unix, Some((SENT + 20).to_string()));
    assert_eq!(s.messages[1].text, "Only in bootstrap");
    assert_eq!(
        s.messages[1].metadata.as_ref().unwrap().deletion_state,
        DeletionState::Deleted
    );
    assert_eq!(
        s.messages
            .iter()
            .find(|m| m.key.message_id == "4")
            .unwrap()
            .metadata
            .as_ref()
            .unwrap()
            .deletion_state,
        DeletionState::Deleted
    );
    let new = s.messages.iter().find(|m| m.key.message_id == "3").unwrap();
    assert_eq!(new.timestamp_unix, Some(SENT.to_string()));
    assert_eq!(new.sender_name.as_deref(), Some("Alice"));
    let checkpoint = p.telegram_continuous["selected"]
        .checkpoint
        .clone()
        .unwrap();
    assert_eq!(capture.status().unwrap().applied, Some(ack(&checkpoint)));
    assert_eq!(f.apply(&mut capture).revision, p.revision);
    drop(capture);
    let mut capture = f.capture();
    capture.poll().unwrap();
    assert_eq!(f.apply(&mut capture), p);
}

#[test]
fn crash_after_snapshot_or_after_project_cas_replays_without_duplicate_revisions() {
    for after_cas in [false, true] {
        let f = Fixture::new();
        f.append(&message(3, "Crash boundary", None));
        let mut capture = f.capture();
        capture.poll().unwrap();
        let result = f.store.apply_telegram_with(
            &f.project.project_id,
            "selected",
            &mut capture,
            |_| {
                if after_cas {
                    Ok(())
                } else {
                    Err(io::Error::other("synthetic crash before CAS"))
                }
            },
            |_| Err(io::Error::other("synthetic crash after CAS")),
        );
        assert!(result.is_err());
        assert!(capture.status().unwrap().applied.is_none());
        let on_disk = f.store.open(&f.project.project_id).unwrap();
        assert_eq!(on_disk.revision, f.project.revision + u64::from(after_cas));
        drop(capture);
        let mut capture = f.capture();
        let recovered = f.apply(&mut capture);
        assert_eq!(recovered.revision, f.project.revision + 1);
        assert_eq!(
            f.snapshot(&recovered)
                .messages
                .iter()
                .filter(|m| m.key.message_id == "3")
                .count(),
            1
        );
        assert_eq!(
            fs::read_dir(
                f.store
                    .snapshots(&f.project.project_id)
                    .unwrap()
                    .directory()
            )
            .unwrap()
            .count(),
            2
        );
        assert_eq!(f.apply(&mut capture).revision, recovered.revision);
    }
}

#[test]
fn stop_or_concurrent_edit_cannot_publish_stale_events_or_acknowledge_them() {
    for stop in [false, true] {
        let f = Fixture::new();
        f.append(&message(3, "Pending", None));
        let mut capture = f.capture();
        capture.poll().unwrap();
        let result = f.store.apply_telegram_with(
            &f.project.project_id,
            "selected",
            &mut capture,
            |p| {
                if stop {
                    f.stop();
                } else {
                    f.store.update(
                        &p.project_id,
                        p.revision,
                        ProjectChange::Rename("Concurrent".into()),
                    )?;
                }
                Ok(())
            },
            |_| Ok(()),
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::WouldBlock);
        assert!(capture.status().unwrap().applied.is_none());
        let current = f.store.open(&f.project.project_id).unwrap();
        assert_eq!(
            current.sources[0].latest_snapshot_id.as_deref(),
            Some("bootstrap")
        );
        if stop {
            assert!(f
                .store
                .apply_telegram_observations(&current.project_id, "selected", &mut capture)
                .is_err());
        } else {
            assert_eq!(f.snapshot(&f.apply(&mut capture)).messages.len(), 4);
        }
    }
}

#[test]
fn pages_do_not_use_future_latest_index_and_gap_only_revision_is_recoverable() {
    let f = Fixture::new();
    f.append(&message(3, "First version", None));
    for id in 10..521 {
        f.append(&message(id, "Filler", None));
    }
    f.append(&message(3, "Future edit", Some(SENT + 20)));
    let mut capture = f.capture();
    capture.poll().unwrap();
    assert_eq!(capture.status().unwrap().events, 513);
    let first = f.apply(&mut capture);
    assert_eq!(
        first.telegram_continuous["selected"]
            .checkpoint
            .as_ref()
            .unwrap()
            .sequence,
        512
    );
    assert_eq!(
        first.telegram_continuous["selected"]
            .checkpoint
            .as_ref()
            .unwrap()
            .observation_revision,
        0
    );
    assert_eq!(
        f.snapshot(&first)
            .messages
            .iter()
            .find(|m| m.key.message_id == "3")
            .unwrap()
            .text,
        "First version"
    );
    let final_page = f.apply(&mut capture);
    assert_eq!(
        f.snapshot(&final_page)
            .messages
            .iter()
            .find(|m| m.key.message_id == "3")
            .unwrap()
            .text,
        "Future edit"
    );
    f.append(&frame("{ updatesTooLong }"));
    capture.poll().unwrap();
    let gap = f.apply(&mut capture);
    assert_eq!(
        gap.telegram_continuous["selected"]
            .checkpoint
            .as_ref()
            .unwrap()
            .sequence,
        513
    );
    assert!(
        gap.telegram_continuous["selected"]
            .checkpoint
            .as_ref()
            .unwrap()
            .observation_revision
            > final_page.telegram_continuous["selected"]
                .checkpoint
                .as_ref()
                .unwrap()
                .observation_revision
    );
    assert!(
        f.snapshot(&gap).coverage.known_gaps.len()
            > f.snapshot(&final_page).coverage.known_gaps.len()
    );
    assert_eq!(f.apply(&mut capture), gap);
}

#[test]
fn wrong_journal_and_foreign_replay_snapshot_are_rejected_without_moving_checkpoint() {
    let f = Fixture::new();
    f.append(&message(3, "Pending", None));
    let mut capture = f.capture();
    capture.poll().unwrap();
    let foreign = f.root.join("foreign-journal");
    let mut config = CaptureConfig::new(
        f.logs.clone(),
        foreign,
        f.project.sources[0].scope.account_local_id.clone(),
        f.project.telegram_continuous["selected"]
            .settings
            .peer
            .clone(),
    );
    config.confirmed_single_account = true;
    let mut other = ContinuousCapture::open(config).unwrap();
    other.poll().unwrap();
    assert!(f
        .store
        .apply_telegram_observations(&f.project.project_id, "selected", &mut other)
        .is_err());
    assert!(other.status().unwrap().applied.is_none());
    f.store
        .apply_telegram_with(
            &f.project.project_id,
            "selected",
            &mut capture,
            |_| Err(io::Error::other("before CAS")),
            |_| Ok(()),
        )
        .unwrap_err();
    let path = fs::read_dir(
        f.store
            .snapshots(&f.project.project_id)
            .unwrap()
            .directory(),
    )
    .unwrap()
    .map(|e| e.unwrap().path())
    .find(|p| {
        p.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("debug-")
    })
    .unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["coverage"]["reason"] = "Foreign observation".into();
    let foreign_bytes = serde_json::to_vec(&value).unwrap();
    fs::write(&path, &foreign_bytes).unwrap();
    assert!(f
        .store
        .apply_telegram_observations(&f.project.project_id, "selected", &mut capture)
        .is_err());
    assert_eq!(fs::read(path).unwrap(), foreign_bytes);
    assert!(capture.status().unwrap().applied.is_none());
    assert_eq!(f.store.open(&f.project.project_id).unwrap(), f.project);
}

#[test]
fn local_package_uses_current_snapshot_and_retains_it_after_stop_without_json() {
    let f = Fixture::new();
    f.configure_package(vec!["selected".into()]);
    f.append(&message(3, "Continuous result 😀", None));
    let mut capture = f.capture();
    capture.poll().unwrap();
    let applied = f.apply(&mut capture);
    fs::remove_file(f.input.join("result.json")).unwrap(); // Own synthetic fixture.
    let state = f
        .store
        .refresh_local_package(&f.project.project_id, 123, || false)
        .unwrap();
    assert_eq!(state.phase, "ready", "{}", state.message);
    let receipt = state.ready.unwrap();
    assert!(receipt.local_only);
    let readme = fs::read_to_string(receipt.directory.join("README.md")).unwrap();
    assert!(readme.contains("локальные наблюдения из диагностических логов"));
    assert!(readme.contains("автоматическая публикация отключена"));
    assert!(!f
        .store
        .local_package_allows_publication(&f.project.project_id)
        .unwrap());
    assert_eq!(receipt.files, 5);
    assert_eq!(receipt.messages, 3); // Topic service is excluded by saved filter.
    assert_eq!(
        f.store.open(&f.project.project_id).unwrap().sources[0].latest_snapshot_id,
        applied.sources[0].latest_snapshot_id
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(receipt.directory.join("package.json")).unwrap()).unwrap();
    let texts: String = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["name"].as_str())
        .filter(|n| n.ends_with(".md"))
        .map(|n| fs::read_to_string(receipt.directory.join(n)).unwrap())
        .collect();
    assert!(texts.contains("Continuous result 😀"));
    assert!(texts.contains("Initial body"));
    f.stop();
    let repeated = f
        .store
        .refresh_local_package(&f.project.project_id, 124, || false)
        .unwrap();
    assert_eq!(repeated.phase, "ready", "{}", repeated.message);
    assert_eq!(repeated.ready.unwrap().generation, receipt.generation);
    assert_eq!(
        f.store.open(&f.project.project_id).unwrap().sources[0].latest_snapshot_id,
        applied.sources[0].latest_snapshot_id
    );
}

#[test]
fn mixed_package_refreshes_archive_chat_without_replacing_the_live_chat() {
    let f = Fixture::new();
    let other = SourceScope::telegram(f.project.sources[0].scope.account_local_id.clone(), "17");
    f.store
        .update(
            &f.project.project_id,
            f.project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "archive-chat".into(),
                connector_id: "telegram_json".into(),
                scope: other,
                archive_path: Some(f.input.join("result.json")),
                latest_snapshot_id: None,
                selection: Default::default(),
            }),
        )
        .unwrap();
    let initial: serde_json::Value =
        serde_json::from_slice(&fs::read(f.input.join("result.json")).unwrap()).unwrap();
    let combined = json!({"chats":{"list":[initial, {"id":17,"name":"Archive chat","type":"personal_chat","messages":[
        {"id":7,"text":"Other archive one"},{"id":8,"text":"Other archive two"}
    ]}]}});
    fs::write(
        f.input.join("result.json"),
        serde_json::to_vec(&combined).unwrap(),
    )
    .unwrap();
    f.configure_package(vec!["selected".into(), "archive-chat".into()]);
    f.append(&message(3, "Continuous text absent from JSON", None));
    let mut capture = f.capture();
    capture.poll().unwrap();
    let applied = f.apply(&mut capture);
    let state = f
        .store
        .refresh_local_package(&f.project.project_id, 123, || false)
        .unwrap();
    assert_eq!(state.phase, "ready", "{}", state.message);
    let receipt = state.ready.unwrap();
    assert!(receipt.local_only);
    assert_eq!(receipt.messages, 5);
    assert_eq!(receipt.conversations, 2);
    let current = f.store.open(&f.project.project_id).unwrap();
    assert_eq!(
        current.sources[0].latest_snapshot_id,
        applied.sources[0].latest_snapshot_id
    );
    let other = f
        .store
        .snapshots(&current.project_id)
        .unwrap()
        .load(current.sources[1].latest_snapshot_id.as_ref().unwrap())
        .unwrap();
    assert_eq!(other.messages.len(), 2);
    assert_eq!(
        f.snapshot(&current)
            .messages
            .iter()
            .find(|m| m.key.message_id == "3")
            .unwrap()
            .text,
        "Continuous text absent from JSON"
    );
    let repeated = f
        .store
        .refresh_local_package(&f.project.project_id, 124, || false)
        .unwrap();
    assert_eq!(repeated.ready.unwrap().generation, receipt.generation);
}

#[test]
fn original_time_and_forum_reply_fields_survive_catchup_of_an_already_edited_message() {
    let f = Fixture::new();
    let packet = message(3, "Already edited", Some(SENT + 20));
    let packet = packet.replace("message: \"Already edited\"", "reply_to: { messageReplyHeader\nflags: 26 [INT],\nreply_to_msg_id: 1 [INT],\nreply_to_top_id: 9 [INT],\n},\nmessage: \"Already edited\"");
    f.append(&packet);
    let mut capture = f.capture();
    capture.poll().unwrap();
    let project = f.apply(&mut capture);
    let snapshot = f.snapshot(&project);
    let message = snapshot
        .messages
        .iter()
        .find(|m| m.key.message_id == "3")
        .unwrap();
    assert_eq!(message.timestamp_unix, Some(SENT.to_string()));
    assert_eq!(message.timestamp.as_deref(), Some("2023-11-14T22:13:20Z"));
    assert_eq!(message.edited_unix, Some((SENT + 20).to_string()));
    assert_eq!(message.reply_to.as_deref(), Some("1"));
    assert_eq!(message.thread_id.as_deref(), Some("9"));
}

#[test]
fn failed_recovery_barrier_does_not_acknowledge_a_visible_project_revision() {
    let f = Fixture::new();
    f.append(&message(3, "Pending recovery", None));
    let mut capture = f.capture();
    capture.poll().unwrap();
    f.store
        .apply_telegram_with(
            &f.project.project_id,
            "selected",
            &mut capture,
            |_| Ok(()),
            |_| Err(io::Error::other("crash before acknowledgement")),
        )
        .unwrap_err();
    let project = f.store.open(&f.project.project_id).unwrap();
    let status = capture.status().unwrap();
    let result = recover_ack(
        &project.telegram_continuous["selected"],
        &status,
        &mut capture,
        || Err(io::Error::other("injected recovery sync failure")),
    );
    assert!(result.is_err());
    assert!(capture.status().unwrap().applied.is_none());
    assert_eq!(f.store.open(&f.project.project_id).unwrap(), project);
    assert_eq!(f.apply(&mut capture), project);
    assert_eq!(
        capture.status().unwrap().applied,
        Some(ack(project.telegram_continuous["selected"]
            .checkpoint
            .as_ref()
            .unwrap()))
    );
}

#[test]
fn general_topic_filter_includes_new_default_forum_messages_without_inventing_topics_in_nonforums()
{
    let f = Fixture::new();
    let mut selection = f.project.sources[0].selection.clone();
    selection.filter.topic_ids = Some(vec!["1".into()]);
    f.store
        .update(
            &f.project.project_id,
            f.project.revision,
            ProjectChange::Selection {
                source_id: "selected".into(),
                selection,
            },
        )
        .unwrap();
    f.append(&message(3, "General arrival", None));
    let mut capture = f.capture();
    capture.poll().unwrap();
    let project = f.apply(&mut capture);
    let snapshot = f.snapshot(&project);
    assert!(
        crate::scope::select_messages(&snapshot, &project.sources[0].selection, None)
            .unwrap()
            .messages
            .iter()
            .any(|m| m.key.message_id == "3")
    );
    assert_eq!(
        snapshot
            .messages
            .iter()
            .find(|m| m.key.message_id == "3")
            .unwrap()
            .thread_id
            .as_deref(),
        Some("1")
    );
    let mut nonforum = f.snapshot(&f.project);
    for m in &mut nonforum.messages {
        m.thread_id = None;
        m.service_action = None;
    }
    let status = capture.status().unwrap();
    let page = capture.events_after(0, 512).unwrap();
    let projected = project_page(
        nonforum,
        &page,
        &f.project.telegram_continuous["selected"].settings.peer,
        &status,
        true,
    )
    .unwrap();
    assert!(projected
        .messages
        .iter()
        .find(|m| m.message.key.message_id == "3")
        .unwrap()
        .message
        .thread_id
        .is_none());
}

#[test]
fn empty_forum_bootstrap_with_only_a_topic_marker_keeps_general_arrivals() {
    let f = Fixture::new();
    f.append(&message(3, "First General arrival", None));
    let mut capture = f.capture();
    capture.poll().unwrap();
    let mut empty_forum = f.snapshot(&f.project);
    empty_forum
        .messages
        .retain(|m| m.service_action.as_deref() == Some("topic_created"));
    assert_eq!(empty_forum.messages.len(), 1);
    assert!(empty_forum.messages[0].thread_id.is_none());
    let projected = project_page(
        empty_forum,
        &capture.events_after(0, 512).unwrap(),
        &f.project.telegram_continuous["selected"].settings.peer,
        &capture.status().unwrap(),
        true,
    )
    .unwrap();
    assert_eq!(
        projected
            .messages
            .iter()
            .find(|m| m.message.key.message_id == "3")
            .unwrap()
            .message
            .thread_id
            .as_deref(),
        Some("1")
    );
}

#[test]
fn explicit_moved_media_root_is_used_and_retained_after_generated_choices_are_empty() {
    let f = Fixture::new();
    let moved = f.root.join("moved-media");
    fs::create_dir(&moved).unwrap();
    fs::rename(f.input.join("photos"), moved.join("photos")).unwrap(); // Own fixture.
    let mut source = f.project.sources[0].clone();
    source.selection.attachments = Some(crate::attachments::AttachmentSelection {
        root: moved.clone(),
        files: vec![crate::attachments::AttachmentChoice {
            message_id: "1".into(),
            position: 0,
            expected: f.snapshot(&f.project).messages[0].attachments[0].clone(),
        }],
    });
    f.store
        .update(
            &f.project.project_id,
            f.project.revision,
            ProjectChange::Source(source),
        )
        .unwrap();
    f.configure_package(vec!["selected".into()]);
    let first = f
        .store
        .refresh_local_package(&f.project.project_id, 123, || false)
        .unwrap();
    assert_eq!(first.phase, "ready", "{}", first.message);
    let current = f.store.open(&f.project.project_id).unwrap();
    assert!(current.sources[0].selection.attachments.is_none());
    assert_eq!(
        current.telegram_continuous["selected"].media_directory,
        Some(moved)
    );
    let repeated = f
        .store
        .refresh_local_package(&f.project.project_id, 124, || false)
        .unwrap();
    assert_eq!(repeated.phase, "ready", "{}", repeated.message);
    assert_eq!(
        repeated.ready.unwrap().generation,
        first.ready.unwrap().generation
    );
}

#[test]
fn text_only_live_package_does_not_open_missing_bootstrap_media_directory() {
    let f = Fixture::new();
    f.configure_package(vec!["selected".into()]);
    let current = f.store.open(&f.project.project_id).unwrap();
    let mut settings = f
        .store
        .local_package(&f.project.project_id)
        .unwrap()
        .unwrap()
        .settings;
    settings.include_images = false;
    settings.include_office = false;
    f.store
        .configure_local_package(&current.project_id, current.revision, settings)
        .unwrap();
    fs::remove_dir_all(&f.input).unwrap(); // Only this test's disposable archive.
    let state = f
        .store
        .refresh_local_package(&f.project.project_id, 123, || false)
        .unwrap();
    assert_eq!(state.phase, "ready", "{}", state.message);
    assert_eq!(state.ready.unwrap().skipped_attachments, 1);
}

#[test]
fn legacy_migration_and_stop_preserve_private_state_and_prevent_archive_overwrite() {
    let f = Fixture::new();
    let revision = f.project.revision;
    let mut changed = f.project.sources[0].clone();
    changed.latest_snapshot_id = None;
    assert!(f
        .store
        .update(
            &f.project.project_id,
            revision,
            ProjectChange::Source(changed)
        )
        .is_err());
    let mut wrong = f.project.telegram_continuous["selected"].settings.clone();
    wrong.peer.kind = PeerKind::User;
    assert!(f
        .store
        .update(
            &f.project.project_id,
            revision,
            ProjectChange::TelegramContinuous {
                source_id: "selected".into(),
                settings: wrong,
                manage_client: false,
            }
        )
        .is_err());
    let stopped = f.stop();
    assert!(f
        .store
        .open_telegram_capture(&f.project.project_id, "selected")
        .is_err());
    assert_eq!(stopped.telegram_continuous["selected"].checkpoint, None);
    assert_eq!(stopped.sources, f.project.sources);
    let path = f
        .store
        .directory(&f.project.project_id)
        .unwrap()
        .join("revisions")
        .join(format!("{:020}.json", stopped.revision));
    let mut forged = serde_json::to_value(&stopped).unwrap();
    forged["schema_version"] = 9.into();
    let bytes = serde_json::to_vec(&forged).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(f.store.open(&f.project.project_id).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    forged
        .as_object_mut()
        .unwrap()
        .remove("telegram_continuous");
    let bytes = serde_json::to_vec(&forged).unwrap();
    fs::write(&path, &bytes).unwrap();
    let upgraded = f.store.open(&f.project.project_id).unwrap();
    assert_eq!(upgraded.schema_version, 11);
    assert!(upgraded.telegram_continuous.is_empty());
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
