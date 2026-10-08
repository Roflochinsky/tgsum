use super::*;
use std::fs::{self, OpenOptions as StdOpenOptions};
use std::os::unix::fs::{symlink, FileExt as PositionalFileExt, PermissionsExt};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use super::super::parser::{PeerKind, TypedPeer};

struct Fixture {
    _root: tempfile::TempDir,
    root: PathBuf,
    input: PathBuf,
    output: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temporary.path()).unwrap();
        let input = root.join("DebugLogs");
        fs::create_dir(&input).unwrap();
        let output = root.join("journal");
        Self {
            _root: temporary,
            root,
            input,
            output,
        }
    }
    fn config(&self) -> CaptureConfig {
        config(self.input.clone(), self.output.clone())
    }
    fn append(&self, bytes: &[u8]) {
        StdOpenOptions::new()
            .append(true)
            .create(true)
            .open(self.input.join("mtp_12_00.txt"))
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
}
fn config(input: PathBuf, output: PathBuf) -> CaptureConfig {
    let mut config = CaptureConfig::new(
        input,
        output,
        "Основной: fixture 😀".into(),
        TypedPeer {
            kind: PeerKind::Channel,
            id: "9007199254740995".into(),
        },
    );
    config.confirmed_single_account = true;
    config
}
fn object(name: &str, fields: &[(&str, String)]) -> String {
    format!(
        "{{ {name}\n{}}}",
        fields
            .iter()
            .map(|(field, value)| format!("  {field}: {value},\n"))
            .collect::<String>()
    )
}
fn frame(body: String) -> String {
    let core = object(
        "core_message",
        &[
            ("msg_id", "7352359257580183524 [LONG]".into()),
            ("seq_no", "1 [INT]".into()),
            ("bytes", "400 [INT]".into()),
            ("body", body),
        ],
    );
    format!("[12:00:00.123 00-0000001] (dc:2_main) Recv: {core} (dc:2,key:123456,session:987654)\n")
}
fn message(id: u32, value: &str, edit: Option<i64>, foreign: bool) -> String {
    let mut fields = vec![
        ("flags", "256 [LONG]".into()),
        ("id", format!("{id} [INT]")),
        (
            "peer_id",
            object(
                if foreign { "peerUser" } else { "peerChannel" },
                &[(
                    if foreign { "user_id" } else { "channel_id" },
                    "9007199254740995 [LONG]".into(),
                )],
            ),
        ),
        (
            "from_id",
            object("peerUser", &[("user_id", "9007199254740993 [LONG]".into())]),
        ),
        ("date", "1700000000 [INT]".into()),
        (
            "message",
            format!(
                "\"{}\" [STRING]",
                value
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
            ),
        ),
    ];
    if let Some(date) = edit {
        fields.push(("edit_date", format!("{date} [INT]")));
    }
    frame(object(
        "updateShort",
        &[
            (
                "update",
                object(
                    if edit.is_some() {
                        "updateEditChannelMessage"
                    } else {
                        "updateNewChannelMessage"
                    },
                    &[
                        ("message", object("message", &fields)),
                        ("pts", "1 [INT]".into()),
                        ("pts_count", "1 [INT]".into()),
                    ],
                ),
            ),
            ("date", "1700000000 [INT]".into()),
        ],
    ))
}
fn delete(id: u32) -> String {
    frame(object(
        "updateDeleteChannelMessages",
        &[
            ("channel_id", "9007199254740995 [LONG]".into()),
            ("messages", format!("[ vector<0x-1> (1) {id} [INT], ]")),
            ("pts", "1 [INT]".into()),
            ("pts_count", "1 [INT]".into()),
        ],
    ))
}
fn latest_text(capture: &ContinuousCapture, id: &str) -> String {
    match capture.latest_message(id).unwrap().unwrap().event.unwrap() {
        ParsedEvent::Message { text, .. } => text,
        _ => panic!("expected message"),
    }
}

#[test]
fn more_than_ten_thousand_events_survive_restart_without_loading_history() {
    let fixture = Fixture::new();
    let mut config = fixture.config();
    config.limits.max_events = 1; // The spike's lifetime cap does not govern this journal.
    let mut capture = ContinuousCapture::open(config).unwrap();
    let mut bytes = String::from("20261008\n");
    for id in 1..=10_005 {
        bytes.push_str(&message(id, "одинаковый текст 😀\nnext", None, false));
    }
    bytes.push_str(&message(10_006, "foreign", None, true));
    fixture.append(bytes.as_bytes());
    assert_eq!(capture.poll().unwrap().added_events, 10_005);
    assert_eq!(capture.status().unwrap().events, 10_005);
    let page = capture.events_after(10_000, 20).unwrap();
    assert_eq!(page.len(), 5);
    assert_eq!(page[0].sequence, 10_001);
    assert!(capture.latest_message("10006").unwrap().is_none());
    assert_eq!(capture.checkpoint().unwrap().follower.events.len(), 0);
    assert_eq!(
        fs::metadata(capture.database_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&fixture.output).unwrap().permissions().mode() & 0o777,
        0o700
    );
    drop(capture);
    let mut resumed = ContinuousCapture::open(fixture.config()).unwrap();
    assert_eq!(resumed.poll().unwrap().added_events, 0);
    fixture.append(message(10_005, "одинаковый текст 😀\nnext", None, false).as_bytes());
    assert_eq!(resumed.poll().unwrap().duplicates, 1);
    assert_eq!(latest_text(&resumed, "10005"), "одинаковый текст 😀\nnext");
    assert_eq!(resumed.status().unwrap().events, 10_005);
}

#[test]
fn exact_ids_utf8_partial_tail_and_applied_checkpoint_are_durable() {
    let fixture = Fixture::new();
    let mut capture = ContinuousCapture::open(fixture.config()).unwrap();
    let packet = message(
        2_147_483_647,
        "Привет 😀 {quoted} \\\"\nnext\r\t",
        None,
        false,
    );
    fixture.append(&packet.as_bytes()[..80]);
    assert!(capture.poll().unwrap().pending_tail);
    assert_eq!(capture.status().unwrap().events, 0);
    fixture.append(&packet.as_bytes()[80..]);
    assert_eq!(capture.poll().unwrap().added_events, 1);
    let page = capture.events_after(0, 1).unwrap();
    match &page[0].event {
        ParsedEvent::Message {
            peer,
            message_id,
            sender: Some(sender),
            text,
            ..
        } => {
            assert_eq!(peer.id, "9007199254740995");
            assert_eq!(sender.id, "9007199254740993");
            assert_eq!(message_id, "2147483647");
            assert_eq!(text, "Привет 😀 {quoted} \\\"\nnext\r\t");
        }
        _ => panic!("expected native message"),
    }
    let ack = AppliedCheckpoint {
        sequence: 1,
        observation_revision: capture.status().unwrap().observation_revision,
        snapshot_id: "snapshot-fixture".into(),
    };
    capture.acknowledge(ack.clone()).unwrap();
    capture.acknowledge(ack.clone()).unwrap();
    assert_eq!(
        capture.acknowledge(AppliedCheckpoint {
            sequence: 0,
            observation_revision: 0,
            snapshot_id: "older".into()
        }),
        Err(CaptureError::InvalidConfiguration)
    );
    drop(capture);
    let resumed = ContinuousCapture::open(fixture.config()).unwrap();
    assert_eq!(resumed.status().unwrap().applied, Some(ack));
    assert!(resumed.events_after(2, 1).is_err());
    assert!(resumed.events_after(0, 0).is_err());
    assert!(resumed.events_after(0, 513).is_err());
}

#[test]
fn late_history_cannot_undo_edits_or_delete_before_message() {
    let fixture = Fixture::new();
    let mut capture = ContinuousCapture::open(fixture.config()).unwrap();
    fixture.append(delete(1).as_bytes());
    fixture.append(message(1, "latest", Some(1_700_000_040), false).as_bytes());
    fixture.append(message(1, "old", None, false).as_bytes());
    fixture.append(message(1, "equal-second edit", Some(1_700_000_040), false).as_bytes());
    assert_eq!(capture.poll().unwrap().added_events, 4);
    assert_eq!(latest_text(&capture, "1"), "equal-second edit");
    let latest = capture.latest_message("1").unwrap().unwrap();
    assert!(latest.deleted);
    assert_eq!(latest.versions, 3);
    drop(capture);
    let mut resumed = ContinuousCapture::open(fixture.config()).unwrap();
    fixture.append(message(1, "old", None, false).as_bytes());
    fixture.append(delete(1).as_bytes());
    assert_eq!(resumed.poll().unwrap().duplicates, 2);
    assert!(resumed.latest_message("1").unwrap().unwrap().deleted);
    assert_eq!(latest_text(&resumed, "1"), "equal-second edit");
}

#[test]
fn gap_only_observation_revision_can_be_acknowledged_without_advancing_events() {
    let fixture = Fixture::new();
    let mut capture = ContinuousCapture::open(fixture.config()).unwrap();
    fixture.append(message(1, "one", None, false).as_bytes());
    capture.poll().unwrap();
    let revision = capture.status().unwrap().observation_revision;
    capture
        .acknowledge(AppliedCheckpoint {
            sequence: 1,
            observation_revision: revision,
            snapshot_id: "one".into(),
        })
        .unwrap();
    fixture.append(b"NEW LOGGING INSTANCE STARTED!!!\n");
    assert_eq!(capture.poll().unwrap().added_events, 0);
    let status = capture.status().unwrap();
    assert!(status.observation_revision > revision);
    let updated = AppliedCheckpoint {
        sequence: 1,
        observation_revision: status.observation_revision,
        snapshot_id: "coverage-updated".into(),
    };
    capture.acknowledge(updated.clone()).unwrap();
    capture.acknowledge(updated.clone()).unwrap();
    assert_eq!(
        capture.acknowledge(AppliedCheckpoint {
            sequence: 1,
            observation_revision: revision,
            snapshot_id: "stale".into()
        }),
        Err(CaptureError::InvalidConfiguration)
    );
    drop(capture);
    assert_eq!(
        ContinuousCapture::open(fixture.config())
            .unwrap()
            .status()
            .unwrap()
            .applied,
        Some(updated)
    );
}

#[test]
fn failed_poll_rolls_back_all_indexes_and_file_positions() {
    let fixture = Fixture::new();
    let mut capture = ContinuousCapture::open(fixture.config()).unwrap();
    fixture.append(message(1, "one", None, false).as_bytes());
    let before = capture.checkpoint().unwrap();
    assert_eq!(
        capture.poll_transaction(|| Ok(()), || Err(CaptureError::Capacity)),
        Err(CaptureError::Capacity)
    );
    assert!(capture.checkpoint().unwrap() == before);
    assert!(capture.events_after(0, 1).unwrap().is_empty());
    assert!(capture.latest_message("1").unwrap().is_none());
    assert_eq!(capture.poll().unwrap().added_events, 1);
}

#[test]
fn ownership_is_verified_before_initialization_or_recovery() {
    let fixture = Fixture::new();
    fs::create_dir(&fixture.output).unwrap();
    fs::set_permissions(&fixture.output, fs::Permissions::from_mode(0o700)).unwrap();
    let victim = fixture.output.join(DATABASE_FILE);
    fs::write(&victim, []).unwrap();
    assert!(matches!(
        ContinuousCapture::open(fixture.config()),
        Err(CaptureError::UnownedOutput)
    ));
    assert_eq!(fs::read(&victim).unwrap(), b"");

    let fixture = Fixture::new();
    let capture = ContinuousCapture::open(fixture.config()).unwrap();
    assert!(matches!(
        ContinuousCapture::open(fixture.config()),
        Err(CaptureError::Busy)
    ));
    let mut other = fixture.config();
    other.account_namespace = "other account".into();
    assert!(matches!(
        ContinuousCapture::open(other),
        Err(CaptureError::BindingMismatch)
    ));
    drop(capture);
    fs::rename(
        fixture.output.join(DATABASE_FILE),
        fixture.root.join("own-old.redb"),
    )
    .unwrap();
    fs::write(
        fixture.output.join(DATABASE_FILE),
        b"foreign malformed database",
    )
    .unwrap();
    fs::set_permissions(
        fixture.output.join(DATABASE_FILE),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(matches!(
        ContinuousCapture::open(fixture.config()),
        Err(CaptureError::UnownedOutput)
    ));
    assert_eq!(
        fs::read(fixture.output.join(DATABASE_FILE)).unwrap(),
        b"foreign malformed database"
    );
}

#[test]
fn retained_database_handle_never_writes_replacement_file_or_directory() {
    for replace_directory in [false, true] {
        let fixture = Fixture::new();
        let mut capture = ContinuousCapture::open(fixture.config()).unwrap();
        fixture.append(message(1, "one", None, false).as_bytes());
        let victim = fixture.output.join(DATABASE_FILE);
        let bytes = b"FOREIGN MUST REMAIN UNCHANGED";
        let report = capture
            .poll_transaction(
                || Ok(()),
                || {
                    if replace_directory {
                        fs::rename(&fixture.output, fixture.root.join("retained-original"))
                            .unwrap();
                        fs::create_dir(&fixture.output).unwrap();
                        fs::set_permissions(&fixture.output, fs::Permissions::from_mode(0o700))
                            .unwrap();
                    } else {
                        fs::rename(&victim, fixture.root.join("retained-original.redb")).unwrap();
                    }
                    fs::write(&victim, bytes).unwrap();
                    fs::set_permissions(&victim, fs::Permissions::from_mode(0o600)).unwrap();
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(report.added_events, 1);
        assert!(capture.poll().is_err());
        drop(capture); // redb Drop may write; it must still use the retained FD.
        assert_eq!(fs::read(&victim).unwrap(), bytes);
    }
}

#[test]
fn symlinks_hardlinks_marker_replacement_and_source_directory_swap_are_refused() {
    let fixture = Fixture::new();
    let mut capture = ContinuousCapture::open(fixture.config()).unwrap();
    fs::hard_link(capture.database_path(), fixture.root.join("hardlink")).unwrap();
    assert_eq!(capture.poll(), Err(CaptureError::UnsafePath));

    let fixture = Fixture::new();
    let mut capture = ContinuousCapture::open(fixture.config()).unwrap();
    let marker = fixture.output.join(OWNER_FILE);
    let original = fs::read(&marker).unwrap();
    fs::rename(&marker, fixture.root.join("old-marker")).unwrap();
    fs::write(&marker, original).unwrap();
    fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(capture.poll(), Err(CaptureError::UnownedOutput));

    let fixture = Fixture::new();
    let mut capture = ContinuousCapture::open(fixture.config()).unwrap();
    fs::rename(&fixture.input, fixture.root.join("old-input")).unwrap();
    fs::create_dir(&fixture.input).unwrap();
    fixture.append(message(2, "replacement source", None, false).as_bytes());
    assert_eq!(capture.poll(), Err(CaptureError::UnsafePath));

    let fixture = Fixture::new();
    let mut capture = ContinuousCapture::open(fixture.config()).unwrap();
    let victim = fixture.root.join("foreign");
    fs::write(&victim, b"unchanged").unwrap();
    fs::rename(capture.database_path(), fixture.root.join("old-db")).unwrap();
    symlink(&victim, capture.database_path()).unwrap();
    assert!(capture.poll().is_err());
    drop(capture);
    assert_eq!(fs::read(victim).unwrap(), b"unchanged");
}

#[test]
fn rewrite_between_read_and_fingerprint_aborts_events_and_cursor() {
    let fixture = Fixture::new();
    let mut capture = ContinuousCapture::open(fixture.config()).unwrap();
    let old = message(1, "same text", None, false);
    let new = message(2, "same text", None, false);
    assert_eq!(&old.as_bytes()[..64], &new.as_bytes()[..64]);
    assert_eq!(
        &old.as_bytes()[old.len() - 256..],
        &new.as_bytes()[new.len() - 256..]
    );
    fixture.append(old.as_bytes());
    let original = capture.checkpoint().unwrap();
    assert_eq!(
        capture.poll_transaction(
            || {
                fs::write(fixture.input.join("mtp_12_00.txt"), &new).unwrap();
                Ok(())
            },
            || Ok(())
        ),
        Err(CaptureError::SourceChanged)
    );
    assert!(capture.checkpoint().unwrap() == original);
    assert!(capture.latest_message("1").unwrap().is_none());
    assert_eq!(capture.poll().unwrap().added_events, 1);
    assert!(capture.latest_message("2").unwrap().is_some());
}

#[test]
fn budget_is_fair_across_log_files_and_rotation_keeps_dedup_history() {
    let fixture = Fixture::new();
    let mut config = fixture.config();
    config.limits.max_packet_bytes = 1024;
    config.limits.max_poll_bytes = 2048;
    let mut capture = ContinuousCapture::open(config).unwrap();
    let mut backlog = String::new();
    for id in 1..=20 {
        backlog.push_str(&message(id, "backlog", None, false));
    }
    fs::write(fixture.input.join("mtp_00_00.txt"), backlog).unwrap();
    fixture.append(message(100, "current", None, false).as_bytes());
    assert!(capture.poll().unwrap().pending_tail);
    assert!(capture.latest_message("100").unwrap().is_none());
    capture.poll().unwrap();
    assert_eq!(latest_text(&capture, "100"), "current");
    while capture.poll().unwrap().pending_tail {}
    fs::rename(
        fixture.input.join("mtp_12_00.txt"),
        fixture.root.join("old-log"),
    )
    .unwrap();
    fixture.append(message(100, "current", None, false).as_bytes());
    let report = capture.poll().unwrap();
    assert_eq!(report.duplicates, 1);
    assert!(capture
        .status()
        .unwrap()
        .gaps
        .iter()
        .any(|gap| gap.gap == CaptureGap::FileRotated));
    fixture.append(b"NEW LOGGING INSTANCE STARTED!!!\n");
    capture.poll().unwrap();
    assert!(capture
        .status()
        .unwrap()
        .gaps
        .iter()
        .any(|gap| gap.gap == CaptureGap::LoggingRestarted));
}

struct KillOnDrop(Child);

#[test]
fn interrupted_initialization_seal_is_completed_only_for_exact_owned_database() {
    for length in [0, 1, 32, 63] {
        let fixture = Fixture::new();
        let mut capture = ContinuousCapture::open(fixture.config()).unwrap();
        fixture.append(message(1, "preserved", None, false).as_bytes());
        capture.poll().unwrap();
        let seal = fixture.output.join(READY_FILE);
        let content = fs::read(&seal).unwrap();
        drop(capture);
        StdOpenOptions::new()
            .write(true)
            .open(&seal)
            .unwrap()
            .set_len(length)
            .unwrap();
        let resumed = ContinuousCapture::open(fixture.config()).unwrap();
        assert_eq!(resumed.status().unwrap().events, 1);
        assert_eq!(fs::read(&seal).unwrap(), content);
        assert_eq!(latest_text(&resumed, "1"), "preserved");
    }
    let fixture = Fixture::new();
    drop(ContinuousCapture::open(fixture.config()).unwrap());
    let seal = fixture.output.join(READY_FILE);
    fs::write(&seal, b"foreign contents").unwrap();
    assert!(matches!(
        ContinuousCapture::open(fixture.config()),
        Err(CaptureError::InvalidState)
    ));
    assert_eq!(fs::read(&seal).unwrap(), b"foreign contents");
}

#[test]
fn pages_stop_at_byte_budget_and_resume_without_losing_large_records() {
    let fixture = Fixture::new();
    let mut capture = ContinuousCapture::open(fixture.config()).unwrap();
    // Raw UTF-8 controls are legal dump characters; JSON escapes make each stored
    // event much larger than the source packet, exercising the byte budget.
    let text = "\u{0001}".repeat(60_000);
    let mut packets = String::new();
    for id in 1..=100 {
        packets.push_str(&message(id, &text, None, false));
    }
    fixture.append(packets.as_bytes());
    assert_eq!(capture.poll().unwrap().added_events, 100);
    let first = capture.events_after(0, 512).unwrap();
    assert!(!first.is_empty() && first.len() < 100);
    let bytes: usize = first
        .iter()
        .map(|entry| serde_json::to_vec(&entry.event).unwrap().len())
        .sum();
    assert!(bytes <= MAX_EVENT_BYTES);
    let second = capture
        .events_after(first.last().unwrap().sequence, 512)
        .unwrap();
    assert_eq!(first.len() + second.len(), 100);
    assert_eq!(second.last().unwrap().sequence, 100);
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn process_crash_before_and_after_commit_recovers_atomic_checkpoint() {
    for stage in ["before", "after"] {
        let fixture = Fixture::new();
        drop(ContinuousCapture::open(fixture.config()).unwrap());
        fixture.append(message(1, "crash fixture", None, false).as_bytes());
        let ready = fixture.root.join("child-ready");
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "telegram_debug::continuous::tests::crash_child",
                "--ignored",
            ])
            .env("TGSUM_CRASH_INPUT", &fixture.input)
            .env("TGSUM_CRASH_OUTPUT", &fixture.output)
            .env("TGSUM_CRASH_READY", &ready)
            .env("TGSUM_CRASH_STAGE", stage)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut child = KillOnDrop(child);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "crash fixture exited before boundary"
            );
            assert!(Instant::now() < deadline, "crash fixture boundary timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let mut recovered = ContinuousCapture::open(fixture.config()).unwrap();
        assert_eq!(
            recovered.status().unwrap().events,
            if stage == "before" { 0 } else { 1 }
        );
        assert_eq!(
            recovered.poll().unwrap().added_events,
            if stage == "before" { 1 } else { 0 }
        );
        assert_eq!(recovered.events_after(0, 10).unwrap().len(), 1);
    }
}

#[test]
#[ignore = "subprocess helper; exercised by process_crash_before_and_after_commit"]
fn crash_child() {
    let Some(input) = std::env::var_os("TGSUM_CRASH_INPUT") else {
        return;
    };
    let output = std::env::var_os("TGSUM_CRASH_OUTPUT").unwrap();
    let ready = PathBuf::from(std::env::var_os("TGSUM_CRASH_READY").unwrap());
    let stage = std::env::var("TGSUM_CRASH_STAGE").unwrap();
    let mut capture = ContinuousCapture::open(config(input.into(), output.into())).unwrap();
    let park = || -> Result<(), CaptureError> {
        fs::write(&ready, b"ready").unwrap();
        loop {
            std::thread::park();
        }
    };
    if stage == "before" {
        capture.poll_transaction(|| Ok(()), park).unwrap();
    } else {
        capture.poll().unwrap();
        park().unwrap();
    }
}

#[derive(Debug)]
struct FailingFile {
    file: File,
    fault: Arc<AtomicU8>,
}
impl redb::StorageBackend for FailingFile {
    fn len(&self) -> std::io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }
    fn read(&self, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
        let mut bytes = vec![0; len];
        self.file.read_exact_at(&mut bytes, offset)?;
        Ok(bytes)
    }
    fn set_len(&self, len: u64) -> std::io::Result<()> {
        self.file.set_len(len)
    }
    fn sync_data(&self, _eventual: bool) -> std::io::Result<()> {
        if self.fault.load(Ordering::SeqCst) == 2 {
            return Err(std::io::Error::other("synthetic sync failure"));
        }
        self.file.sync_data()
    }
    fn write(&self, offset: u64, bytes: &[u8]) -> std::io::Result<()> {
        if self.fault.load(Ordering::SeqCst) == 1 {
            return Err(std::io::Error::other("synthetic write failure"));
        }
        self.file.write_all_at(bytes, offset)
    }
}

#[test]
fn io_failure_stops_session_and_reopen_uses_recovered_atomic_state() {
    for fault_value in [1, 2] {
        let fixture = Fixture::new();
        let fault = Arc::new(AtomicU8::new(0));
        let shared = fault.clone();
        let mut capture = ContinuousCapture::open_with(fixture.config(), move |file| {
            Database::builder()
                .set_cache_size(CACHE_BYTES)
                .create_with_backend(FailingFile {
                    file,
                    fault: shared,
                })
                .map_err(|_| CaptureError::Io)
        })
        .unwrap();
        fixture.append(message(1, "durability fixture", None, false).as_bytes());
        fault.store(fault_value, Ordering::SeqCst);
        assert_eq!(capture.poll(), Err(CaptureError::Io));
        assert_eq!(capture.poll(), Err(CaptureError::Io));
        assert!(matches!(capture.status(), Err(CaptureError::Io)));
        drop(capture);
        fault.store(0, Ordering::SeqCst);
        let mut recovered = ContinuousCapture::open(fixture.config()).unwrap();
        let committed = recovered.status().unwrap().events;
        assert!(committed <= 1);
        assert_eq!(
            recovered.poll().unwrap().added_events,
            if committed == 0 { 1 } else { 0 }
        );
        assert_eq!(recovered.events_after(0, 10).unwrap().len(), 1);
        assert!(recovered.latest_message("1").unwrap().is_some());
    }
}
