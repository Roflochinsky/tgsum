use super::*;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Write};

use tgsum_core::project::ProjectSource;
use tgsum_core::snapshot::SourceScope;

const CHAT: &str = "9007199254740995";
const SENDER: &str = "9007199254740993";

fn managed_runtime(
    f: &Fixture,
) -> (
    TelegramContinuousState,
    std::sync::Arc<std::sync::Mutex<client::fixture::State>>,
) {
    let fake = client::fixture::Fake::new(f.logs.parent().unwrap()).unwrap();
    let shared = std::sync::Arc::clone(&fake.0);
    let runtime = TelegramContinuousState::default();
    runtime.0.lock().unwrap().client = client::Client::new(Box::new(fake));
    (runtime, shared)
}

fn start_managed(f: &Fixture, runtime: &TelegramContinuousState, project_id: &str) -> Project {
    runtime
        .set_enabled(
            &f.store,
            project_id,
            "selected",
            f.store.open(project_id).unwrap().revision,
            StartOptions {
                enabled: true,
                input_directory: None,
                confirmed_single_account: true,
                manage_client: Some(true),
            },
        )
        .unwrap()
}

fn tick(f: &Fixture, runtime: &TelegramContinuousState) {
    runtime
        .tick(&f.store, |id, source, capture| {
            f.store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
}

fn second_project(f: &Fixture, account: &str) -> String {
    let project = f.store.create("Second synthetic selected chat").unwrap();
    let mut source = f.project().sources[0].clone();
    source.latest_snapshot_id = Some("bootstrap".into());
    source.scope.account_local_id = account.into();
    f.store.snapshots(&project.project_id).unwrap().import_telegram("bootstrap", &source.scope,
        Cursor::new(serde_json::to_vec(&serde_json::json!({"id":CHAT,"name":"Second", "type":"private_supergroup", "messages":[]})).unwrap())).unwrap();
    f.store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(source),
        )
        .unwrap();
    project.project_id
}

#[test]
fn managed_start_detects_creates_logs_applies_native_text_and_stop_restores() {
    let f = Fixture::new();
    fs::remove_dir(&f.logs).unwrap(); // Our empty synthetic logs only.
    let (runtime, shared) = managed_runtime(&f);
    assert_eq!(runtime.detect_client().unwrap(), Some(f.logs.clone()));
    assert!(!f.store.telegram_client_control_path().exists());
    let plan = start_managed(&f, &runtime, &f.project_id);
    assert!(plan.telegram_continuous["selected"].manage_client);
    assert_eq!(plan.schema_version, 11);
    assert!(shared.lock().unwrap().calls.is_empty());
    tick(&f, &runtime);
    assert_eq!(shared.lock().unwrap().calls, ["quit", "start-debug"]);
    f.append(10, CHAT, "Managed native Ж 😀");
    tick(&f, &runtime);
    assert_eq!(f.messages(), 2);
    assert!(runtime.client_status().unwrap().phase == client::Phase::ManagedDebug);
    f.enable(&runtime, false);
    assert_eq!(
        shared.lock().unwrap().calls,
        ["quit", "start-debug", "quit", "start-original"]
    );
    assert!(runtime.0.lock().unwrap().captures.is_empty());
    assert!(!shared
        .lock()
        .unwrap()
        .running
        .as_ref()
        .unwrap()
        .launch
        .arguments
        .iter()
        .any(|a| a == "-debug"));
}

#[test]
fn shared_client_survives_first_stop_and_restores_after_last_across_projects() {
    let f = Fixture::new();
    let second = second_project(&f, "Основной: fixture 😀");
    let (runtime, shared) = managed_runtime(&f);
    start_managed(&f, &runtime, &f.project_id);
    tick(&f, &runtime);
    start_managed(&f, &runtime, &second);
    tick(&f, &runtime);
    assert_eq!(runtime.client_status().unwrap().enabled_sources, 2);
    f.enable(&runtime, false);
    assert_eq!(shared.lock().unwrap().calls, ["quit", "start-debug"]);
    f.append(11, CHAT, "Still collecting second");
    tick(&f, &runtime);
    assert_eq!(
        runtime
            .status(&f.store, &second, "selected")
            .unwrap()
            .observation
            .applied_events,
        1
    );
    runtime
        .set_enabled(
            &f.store,
            &second,
            "selected",
            f.store.open(&second).unwrap().revision,
            StartOptions {
                enabled: false,
                input_directory: None,
                confirmed_single_account: false,
                manage_client: None,
            },
        )
        .unwrap();
    assert_eq!(
        shared.lock().unwrap().calls,
        ["quit", "start-debug", "quit", "start-original"]
    );
}

#[test]
fn graceful_exit_releases_control_lock_and_saved_opt_in_resumes_without_running_client() {
    let f = Fixture::new();
    let (runtime, shared) = managed_runtime(&f);
    start_managed(&f, &runtime, &f.project_id);
    tick(&f, &runtime);
    runtime.shutdown(&f.store).unwrap();
    assert!(f.project().telegram_continuous["selected"].settings.enabled);
    // Old Tauri state is still alive when the replacement takes the lock.
    shared.lock().unwrap().running = None;
    let restarted = TelegramContinuousState::default();
    restarted.0.lock().unwrap().client =
        client::Client::new(Box::new(client::fixture::Fake(shared.clone())));
    tick(&f, &restarted);
    assert!(f.view(&restarted).observation.phase == Phase::Watching);
    assert_eq!(
        shared.lock().unwrap().calls,
        [
            "quit",
            "start-debug",
            "quit",
            "start-original",
            "start-debug"
        ]
    );
    restarted.shutdown(&f.store).unwrap();
}

#[test]
fn failed_restore_stops_writer_and_is_visible_until_explicit_recovery() {
    let f = Fixture::new();
    let (runtime, shared) = managed_runtime(&f);
    start_managed(&f, &runtime, &f.project_id);
    tick(&f, &runtime);
    shared.lock().unwrap().fail_restore = true;
    let stopped = f.enable(&runtime, false);
    assert!(!stopped.telegram_continuous["selected"].settings.enabled);
    assert!(runtime.0.lock().unwrap().captures.is_empty());
    let state = runtime.client_status().unwrap();
    assert!(state.phase == client::Phase::Failed && state.can_restore);
    let before = shared.lock().unwrap().calls.clone();
    tick(&f, &runtime);
    assert_eq!(shared.lock().unwrap().calls, before);
    shared.lock().unwrap().fail_restore = false;
    runtime.restore_client(&f.store).unwrap();
    assert!(runtime.client_status().unwrap().phase == client::Phase::Unmanaged);
    assert!(shared.lock().unwrap().running.is_some());
}

#[test]
fn conflicting_account_or_profile_is_rejected_before_plan_or_client_change() {
    let f = Fixture::new();
    let (runtime, shared) = managed_runtime(&f);
    start_managed(&f, &runtime, &f.project_id);
    tick(&f, &runtime);
    for (account, directory) in [
        ("Other", f.logs.clone()),
        ("Основной: fixture 😀", f.logs.join("other")),
    ] {
        let id = second_project(&f, account);
        let before = f.store.open(&id).unwrap();
        assert!(runtime
            .set_enabled(
                &f.store,
                &id,
                "selected",
                before.revision,
                StartOptions {
                    enabled: true,
                    input_directory: Some(directory),
                    confirmed_single_account: true,
                    manage_client: Some(true)
                }
            )
            .is_err());
        assert_eq!(f.store.open(&id).unwrap(), before);
        assert_eq!(shared.lock().unwrap().calls, ["quit", "start-debug"]);
    }
    runtime.shutdown(&f.store).unwrap();
}

#[test]
fn managed_disconnect_restores_client_and_preserves_journal() {
    let f = Fixture::new();
    let (runtime, shared) = managed_runtime(&f);
    start_managed(&f, &runtime, &f.project_id);
    tick(&f, &runtime);
    let journal = f
        .store
        .telegram_journal_path(&f.project_id, "selected")
        .unwrap();
    let project = f.project();
    f.store
        .update(
            &f.project_id,
            project.revision,
            ProjectChange::RemoveSource("selected".into()),
        )
        .unwrap();
    runtime
        .disconnected(&f.store, &f.project_id, "selected")
        .unwrap();
    assert!(journal.is_file());
    assert!(fs::metadata(journal).unwrap().len() > 0);
    assert_eq!(
        shared.lock().unwrap().calls,
        ["quit", "start-debug", "quit", "start-original"]
    );
}

#[test]
fn unreadable_project_never_counts_as_a_stopped_managed_source() {
    let f = Fixture::new();
    let second = second_project(&f, "Основной: fixture 😀");
    let (runtime, shared) = managed_runtime(&f);
    start_managed(&f, &runtime, &f.project_id);
    start_managed(&f, &runtime, &second);
    tick(&f, &runtime);
    let project = f.store.open(&second).unwrap();
    let path = f
        .store
        .telegram_client_control_path()
        .parent()
        .unwrap()
        .join(&second)
        .join("revisions")
        .join(format!("{:020}.json", project.revision));
    let original = fs::read(&path).unwrap();
    fs::write(&path, b"{broken synthetic manifest").unwrap();
    f.enable(&runtime, false);
    assert_eq!(shared.lock().unwrap().calls, ["quit", "start-debug"]);
    assert!(runtime.client_status().unwrap().phase == client::Phase::Failed);
    assert!(!runtime.client_status().unwrap().can_restore);
    assert!(runtime.restore_client(&f.store).is_err());
    fs::write(&path, original).unwrap();
    runtime
        .set_enabled(
            &f.store,
            &second,
            "selected",
            project.revision,
            StartOptions {
                enabled: false,
                input_directory: None,
                confirmed_single_account: false,
                manage_client: None,
            },
        )
        .unwrap();
    assert_eq!(
        shared.lock().unwrap().calls,
        ["quit", "start-debug", "quit", "start-original"]
    );
}

#[test]
fn borrowed_user_debug_and_legacy_passive_plans_never_switch_client() {
    let f = Fixture::new();
    let (runtime, shared) = managed_runtime(&f);
    shared
        .lock()
        .unwrap()
        .running
        .as_mut()
        .unwrap()
        .launch
        .arguments
        .push("-debug".into());
    start_managed(&f, &runtime, &f.project_id);
    tick(&f, &runtime);
    assert!(runtime.client_status().unwrap().phase == client::Phase::UserDebug);
    f.enable(&runtime, false);
    assert!(shared.lock().unwrap().calls.is_empty());
    assert!(shared
        .lock()
        .unwrap()
        .running
        .as_ref()
        .unwrap()
        .launch
        .arguments
        .iter()
        .any(|a| a == "-debug"));
    runtime.shutdown(&f.store).unwrap();
    let passive = Fixture::new();
    let (runtime, shared) = managed_runtime(&passive);
    passive.enable(&runtime, true);
    tick(&passive, &runtime);
    assert!(!passive.project().telegram_continuous["selected"].manage_client);
    assert!(!passive.store.telegram_client_control_path().exists());
    runtime.shutdown(&passive.store).unwrap();
    assert!(shared.lock().unwrap().calls.is_empty());
}

#[test]
fn running_capture_cannot_change_client_control_in_place() {
    let f = Fixture::new();
    let (runtime, shared) = managed_runtime(&f);
    f.enable(&runtime, true);
    let before = f.project();
    assert!(runtime
        .set_enabled(
            &f.store,
            &f.project_id,
            "selected",
            before.revision,
            StartOptions {
                enabled: true,
                input_directory: None,
                confirmed_single_account: true,
                manage_client: Some(true)
            }
        )
        .is_err());
    assert_eq!(f.project(), before);
    assert!(shared.lock().unwrap().calls.is_empty());
    runtime.shutdown(&f.store).unwrap();
}

#[test]
fn regression_managed_first_binding_rejects_other_directory_before_saving() {
    let f = Fixture::new();
    let (runtime, shared) = managed_runtime(&f);
    let before = f.project();
    let wrong = f.logs.parent().unwrap().join("other-logs");
    fs::create_dir(&wrong).unwrap();
    assert!(runtime
        .set_enabled(
            &f.store,
            &f.project_id,
            "selected",
            before.revision,
            StartOptions {
                enabled: true,
                input_directory: Some(wrong),
                confirmed_single_account: true,
                manage_client: Some(true)
            }
        )
        .is_err());
    assert_eq!(f.project(), before);
    assert!(shared.lock().unwrap().calls.is_empty());
    assert!(!f.store.telegram_client_control_path().exists());
}

#[test]
fn regression_managed_first_binding_rejects_symlink_before_saving() {
    let f = Fixture::new();
    let (runtime, shared) = managed_runtime(&f);
    let before = f.project();
    let target = f.logs.parent().unwrap().join("own-synthetic-log-target");
    fs::create_dir(&target).unwrap();
    fs::remove_dir(&f.logs).unwrap(); // Our own empty fixture only.
    std::os::unix::fs::symlink(&target, &f.logs).unwrap();
    assert!(runtime
        .set_enabled(
            &f.store,
            &f.project_id,
            "selected",
            before.revision,
            StartOptions {
                enabled: true,
                input_directory: None,
                confirmed_single_account: true,
                manage_client: Some(true)
            }
        )
        .is_err());
    assert_eq!(f.project(), before);
    assert!(shared.lock().unwrap().calls.is_empty());
    assert!(!f.store.telegram_client_control_path().exists());
}

#[test]
fn regression_shutdown_is_terminal_for_queued_commands_and_receipt_lock() {
    let f = Fixture::new();
    let second = second_project(&f, "Основной: fixture 😀");
    let (runtime, shared) = managed_runtime(&f);
    start_managed(&f, &runtime, &f.project_id);
    start_managed(&f, &runtime, &second);
    tick(&f, &runtime);
    runtime.shutdown(&f.store).unwrap();
    let calls = shared.lock().unwrap().calls.clone();
    let project = f.project();
    f.store
        .update(
            &f.project_id,
            project.revision,
            ProjectChange::RemoveSource("selected".into()),
        )
        .unwrap();
    assert!(runtime
        .disconnected(&f.store, &f.project_id, "selected")
        .is_err());
    assert_eq!(shared.lock().unwrap().calls, calls);
    let before = f.store.open(&second).unwrap();
    assert!(runtime
        .edit_project(
            &f.store,
            &second,
            before.revision,
            ProjectChange::RemoveSource("selected".into()),
            || panic!("closed command cancelled prepared analysis")
        )
        .is_err());
    assert!(runtime
        .set_enabled_with(
            &f.store,
            &second,
            "selected",
            before.revision,
            StartOptions {
                enabled: false,
                input_directory: None,
                confirmed_single_account: false,
                manage_client: None
            },
            || panic!("closed Start cancelled prepared analysis"),
            ProjectStore::update
        )
        .is_err());
    assert!(runtime
        .set_enabled(
            &f.store,
            &second,
            "selected",
            before.revision,
            StartOptions {
                enabled: false,
                input_directory: None,
                confirmed_single_account: false,
                manage_client: None
            }
        )
        .is_err());
    assert_eq!(f.store.open(&second).unwrap(), before);
    assert!(runtime.detect_client().is_err());
    assert!(runtime.restore_client(&f.store).is_err());
    assert!(runtime
        .tick(&f.store, |_, _, _| panic!("closed worker applied data"))
        .is_err());
    runtime.shutdown(&f.store).unwrap();
    assert_eq!(shared.lock().unwrap().calls, calls);
    let _available =
        crate::telegram_client::LeaseJournal::open(&f.store.telegram_client_control_path())
            .unwrap();
}

#[test]
fn validation_and_revision_conflicts_do_not_interrupt_managed_collection() {
    let f = Fixture::new();
    let (runtime, shared) = managed_runtime(&f);
    let old = start_managed(&f, &runtime, &f.project_id);
    tick(&f, &runtime);
    f.append(10, CHAT, "advance revision");
    tick(&f, &runtime);
    let current = f.project();
    let error = runtime
        .edit_project(
            &f.store,
            &f.project_id,
            old.revision,
            ProjectChange::Rename("stale edit".into()),
            || Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    let error = runtime
        .edit_project(
            &f.store,
            &f.project_id,
            current.revision,
            ProjectChange::Rename("".into()),
            || Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    let error = runtime
        .set_enabled(
            &f.store,
            &f.project_id,
            "selected",
            current.revision,
            StartOptions {
                enabled: true,
                input_directory: None,
                confirmed_single_account: true,
                manage_client: Some(false),
            },
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(f.project(), current);
    assert!(runtime.client_status().unwrap().phase == client::Phase::ManagedDebug);
    assert_eq!(runtime.0.lock().unwrap().captures.len(), 1);
    f.append(11, CHAT, "continues after rejected edit");
    tick(&f, &runtime);
    assert_eq!(f.view(&runtime).observation.applied_events, 2);
    assert_eq!(shared.lock().unwrap().calls, ["quit", "start-debug"]);
    runtime.shutdown(&f.store).unwrap();
}

#[test]
fn stop_cannot_grant_client_control_to_an_unverified_passive_binding() {
    let f = Fixture::new();
    let (runtime, shared) = managed_runtime(&f);
    let wrong = f.logs.parent().unwrap().join("passive-other-logs");
    fs::create_dir(&wrong).unwrap();
    runtime
        .set_enabled(
            &f.store,
            &f.project_id,
            "selected",
            f.project().revision,
            StartOptions {
                enabled: true,
                input_directory: Some(wrong),
                confirmed_single_account: true,
                manage_client: Some(false),
            },
        )
        .unwrap();
    runtime
        .set_enabled(
            &f.store,
            &f.project_id,
            "selected",
            f.project().revision,
            StartOptions {
                enabled: false,
                input_directory: None,
                confirmed_single_account: false,
                manage_client: None,
            },
        )
        .unwrap();
    let before = f.project();
    for enabled in [false, true] {
        assert!(runtime
            .set_enabled(
                &f.store,
                &f.project_id,
                "selected",
                before.revision,
                StartOptions {
                    enabled,
                    input_directory: None,
                    confirmed_single_account: true,
                    manage_client: Some(true)
                }
            )
            .is_err());
        assert_eq!(f.project(), before);
    }
    assert!(shared.lock().unwrap().calls.is_empty());
    assert!(!f.store.telegram_client_control_path().exists());
    runtime.shutdown(&f.store).unwrap();
}

#[test]
fn regression_failed_visible_start_does_not_activate_on_next_tick() {
    let f = Fixture::new();
    let (runtime, shared) = managed_runtime(&f);
    let before = f.project();
    let error = runtime
        .set_enabled_with(
            &f.store,
            &f.project_id,
            "selected",
            before.revision,
            StartOptions {
                enabled: true,
                input_directory: None,
                confirmed_single_account: true,
                manage_client: Some(true),
            },
            || Ok(()),
            |store, id, revision, change| {
                store.update(id, revision, change)?;
                // Same caller-visible state as an error after manifest publication:
                // Start fails, but the enabled revision is already readable.
                Err(io::Error::other("Synthetic post-publication sync failure"))
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("post-publication"));
    assert!(f.project().telegram_continuous["selected"].settings.enabled);
    assert!(shared.lock().unwrap().calls.is_empty());
    tick(&f, &runtime);
    assert!(
        shared.lock().unwrap().calls.is_empty(),
        "failed Start silently activated"
    );
    assert!(runtime.client_status().unwrap().phase == client::Phase::Failed);
    assert!(runtime.0.lock().unwrap().captures.is_empty());
    start_managed(&f, &runtime, &f.project_id); // Explicit retry after recovery.
    tick(&f, &runtime);
    assert_eq!(shared.lock().unwrap().calls, ["quit", "start-debug"]);
    runtime.shutdown(&f.store).unwrap();
}

struct Fixture {
    _root: tempfile::TempDir,
    logs: PathBuf,
    store: ProjectStore,
    project_id: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().canonicalize().unwrap();
        let logs = directory.join("DebugLogs");
        fs::create_dir(&logs).unwrap();
        let store = ProjectStore::new(directory.join("projects"));
        let project = store.create("Synthetic worker").unwrap();
        let scope = SourceScope::telegram("Основной: fixture 😀", CHAT);
        let bytes = serde_json::to_vec(&serde_json::json!({
            "id":CHAT, "name":"Synthetic", "type":"private_supergroup",
            "messages":[{"id":1,"text":"bootstrap","from":"Alice","from_id":format!("user{SENDER}")}]
        }))
        .unwrap();
        store
            .snapshots(&project.project_id)
            .unwrap()
            .import_telegram("bootstrap", &scope, Cursor::new(bytes))
            .unwrap();
        store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::Source(ProjectSource {
                    source_id: "selected".into(),
                    connector_id: "telegram_json".into(),
                    scope,
                    archive_path: None,
                    latest_snapshot_id: Some("bootstrap".into()),
                    selection: Default::default(),
                }),
            )
            .unwrap();
        Self {
            _root: root,
            logs,
            store,
            project_id: project.project_id,
        }
    }

    fn project(&self) -> Project {
        self.store.open(&self.project_id).unwrap()
    }

    fn enable(&self, runtime: &TelegramContinuousState, enabled: bool) -> Project {
        runtime
            .set_enabled(
                &self.store,
                &self.project_id,
                "selected",
                self.project().revision,
                StartOptions {
                    enabled,
                    input_directory: Some(self.logs.clone()),
                    confirmed_single_account: true,
                    manage_client: None,
                },
            )
            .unwrap()
    }

    fn append(&self, id: u32, peer: &str, text: &str) {
        let text = serde_json::to_string(text).unwrap();
        let frame = format!(
            "[12:00:00.123 00-0000001] (dc:2_main) Recv: {{ core_message\n  msg_id: 7352359257580183524 [LONG],\n  seq_no: 1 [INT],\n  bytes: 400 [INT],\n  body: {{ updateShort\n    update: {{ updateNewChannelMessage\n      message: {{ message\n        flags: 256 [LONG],\n        id: {id} [INT],\n        peer_id: {{ peerChannel\n          channel_id: {peer} [LONG],\n        }},\n        from_id: {{ peerUser\n          user_id: {SENDER} [LONG],\n        }},\n        date: 1700000000 [INT],\n        message: {text} [STRING],\n      }},\n      pts: 1 [INT],\n      pts_count: 1 [INT],\n    }},\n    date: 1700000000 [INT],\n  }},\n}} (dc:2,key:123456,session:987654)\n"
        );
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(self.logs.join("mtp_12_00.txt"))
            .unwrap()
            .write_all(frame.as_bytes())
            .unwrap();
    }

    fn view(&self, runtime: &TelegramContinuousState) -> ContinuousView {
        runtime
            .status(&self.store, &self.project_id, "selected")
            .unwrap()
    }

    fn messages(&self) -> usize {
        let project = self.project();
        self.store
            .snapshots(&self.project_id)
            .unwrap()
            .load(project.sources[0].latest_snapshot_id.as_ref().unwrap())
            .unwrap()
            .messages
            .len()
    }
}

#[test]
fn first_start_rejects_unconfirmed_account_or_invalid_directory_without_saving_plan() {
    let fixture = Fixture::new();
    let runtime = TelegramContinuousState::default();
    let project = fixture.project();
    for (directory, confirmed) in [
        (fixture.logs.clone(), false),
        (fixture.logs.join("missing"), true),
    ] {
        assert!(runtime
            .set_enabled(
                &fixture.store,
                &fixture.project_id,
                "selected",
                project.revision,
                StartOptions {
                    enabled: true,
                    input_directory: Some(directory),
                    confirmed_single_account: confirmed,
                    manage_client: None,
                }
            )
            .is_err());
        assert_eq!(fixture.project(), project);
    }
    fixture.enable(&runtime, true);
    runtime.shutdown(&fixture.store).unwrap();
}

#[test]
fn worker_collects_selected_source_stops_and_resumes_saved_position() {
    let fixture = Fixture::new();
    let runtime = TelegramContinuousState::default();
    let configured = fixture.enable(&runtime, true);
    fixture.append(10, CHAT, "Новый текст 😀");
    fixture.append(10, "42", "foreign text must stay outside project");
    runtime
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    let view = fixture.view(&runtime);
    assert!(view.observation.phase == Phase::Watching);
    assert_eq!(view.observation.events, 1);
    assert_eq!(view.observation.applied_events, 1);
    assert_eq!(fixture.messages(), 2);
    let revision = fixture.project().revision;
    let repeated = fixture.enable(&runtime, true);
    assert_eq!(
        repeated.revision, revision,
        "second Start must not restart a writer"
    );
    runtime
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    assert_eq!(fixture.project().revision, revision);
    let stopped = fixture.enable(&runtime, false);
    assert!(!stopped.telegram_continuous["selected"].settings.enabled);
    assert!(runtime.0.lock().unwrap().captures.is_empty());
    fixture.append(11, CHAT, "После остановки");
    runtime
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    assert_eq!(fixture.project().revision, stopped.revision);
    assert_eq!(fixture.messages(), 2);
    runtime.shutdown(&fixture.store).unwrap();
    let restarted = TelegramContinuousState::default();
    restarted
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    assert_eq!(
        fixture.messages(),
        2,
        "a saved Stop survives process restart"
    );
    let resumed = fixture.enable(&restarted, true);
    assert_eq!(
        configured.telegram_continuous["selected"]
            .settings
            .generation,
        resumed.telegram_continuous["selected"].settings.generation
    );
    restarted
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    let view = fixture.view(&restarted);
    assert_eq!(view.observation.events, 2);
    assert_eq!(view.observation.applied_events, 2);
    assert_eq!(fixture.messages(), 3);
    assert!(
        view.observation.gaps.iter().any(|gap| {
            gap.gap == tgsum_core::telegram_debug::capture::CaptureGap::CollectorRestarted
                && gap.count == 1
        }),
        "resume must report a collector gap distinct from StartedWithoutHistory"
    );
    restarted.shutdown(&fixture.store).unwrap();
}

#[test]
fn stopped_restart_does_not_claim_exact_counts_for_unapplied_journal() {
    let fixture = Fixture::new();
    let runtime = TelegramContinuousState::default();
    fixture.enable(&runtime, true);
    for id in 10..610 {
        fixture.append(id, CHAT, "queued");
    }
    runtime
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    assert_eq!(fixture.view(&runtime).observation.events, 600);
    assert_eq!(fixture.view(&runtime).observation.applied_events, 512);
    fixture.enable(&runtime, false);
    assert!(fixture.view(&runtime).observation.counts_known);
    runtime.shutdown(&fixture.store).unwrap();
    let restarted = TelegramContinuousState::default();
    let view = fixture.view(&restarted);
    assert_eq!(view.observation.applied_events, 512);
    assert!(
        !view.observation.counts_known,
        "checkpoint must not claim journal total"
    );
    fixture.enable(&restarted, true);
    restarted
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    let view = fixture.view(&restarted);
    assert!(view.observation.counts_known);
    assert_eq!(view.observation.events, 600);
    assert_eq!(view.observation.applied_events, 600);
    restarted.shutdown(&fixture.store).unwrap();
}

#[cfg(all(debug_assertions, feature = "analysis-fixtures"))]
#[test]
fn worker_never_cancels_prepared_analysis_on_idle_or_new_observation() {
    use serde_json::{json, Value};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tauri::WebviewWindowBuilder;

    let fixture = Fixture::new();
    let app = crate::app(
        mock_builder()
            .manage(fixture.store.clone())
            .manage(analysis::AnalysisState::synthetic()),
    )
    .build(mock_context(noop_assets()))
    .unwrap();
    let window = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let invoke = |command: &str, args: Value| {
        get_ipc_response(
            &window,
            InvokeRequest {
                cmd: command.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|body| body.deserialize::<Value>().unwrap())
        .unwrap()
    };
    let runtime = app.state::<TelegramContinuousState>().inner().clone();
    fixture.enable(&runtime, true);
    runtime
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    let project = fixture.project();
    let bundle = fixture
        .store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            tgsum_core::bundle::BundleOptions {
                redact_candidates: true,
                ..Default::default()
            },
            || false,
        )
        .unwrap();
    let prepared = invoke(
        "prepare_project_analysis",
        json!({
            "projectId": project.project_id, "expectedRevision": project.revision, "bundleId": bundle.bundle_id,
            "options": {"agent":"codex","executable":"/NEVER_EXECUTE","auth_file":"/NEVER_OPEN",
                        "model":"fixture-success","recipe":"summary","destination":"local_fixture"}
        }),
    );
    let revision = fixture.project().revision;
    let run_id = prepared["run_id"].clone();
    for _ in 0..3 {
        runtime
            .tick(&fixture.store, |id, source, capture| {
                app.state::<analysis::AnalysisState>()
                    .background_edit(id, || {
                        fixture
                            .store
                            .apply_telegram_observations(id, source, capture)
                            .map(|_| ())
                    })
            })
            .unwrap();
    }
    assert_eq!(
        fixture.project().revision,
        revision,
        "idle polling is inert"
    );
    fixture.append(10, CHAT, "queued behind reviewed run");
    runtime
        .tick(&fixture.store, |id, source, capture| {
            app.state::<analysis::AnalysisState>()
                .background_edit(id, || {
                    fixture
                        .store
                        .apply_telegram_observations(id, source, capture)
                        .map(|_| ())
                })
        })
        .unwrap();
    assert_eq!(
        fixture.project().revision,
        revision,
        "a reviewed analysis keeps its source revision"
    );
    assert_eq!(fixture.view(&runtime).observation.applied_events, 0);
    invoke(
        "run_project_analysis",
        json!({"projectId":project.project_id,"runId":run_id}),
    );
    runtime
        .tick(&fixture.store, |id, source, capture| {
            app.state::<analysis::AnalysisState>()
                .background_edit(id, || {
                    fixture
                        .store
                        .apply_telegram_observations(id, source, capture)
                        .map(|_| ())
                })
        })
        .unwrap();
    assert_eq!(fixture.view(&runtime).observation.applied_events, 1);
    runtime.shutdown(&fixture.store).unwrap();
}

#[test]
fn worker_journals_during_operation_and_recovers_after_process_restart() {
    let fixture = Fixture::new();
    let runtime = TelegramContinuousState::default();
    fixture.enable(&runtime, true);
    fixture.append(10, CHAT, "Сохранить до применения");
    runtime.tick(&fixture.store, |_, _, _| Ok(false)).unwrap();
    let view = fixture.view(&runtime);
    assert!(view.observation.phase == Phase::WaitingForOperation);
    assert_eq!(view.observation.events, 1);
    assert_eq!(view.observation.applied_events, 0);
    assert_eq!(fixture.messages(), 1);
    runtime.shutdown(&fixture.store).unwrap();
    let restarted = TelegramContinuousState::default();
    restarted
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    assert_eq!(fixture.messages(), 2);
    let view = fixture.view(&restarted);
    assert_eq!(view.observation.events, 1);
    assert_eq!(view.observation.applied_events, 1);
    restarted.shutdown(&fixture.store).unwrap();
}

#[test]
fn failed_start_is_visible_and_requires_explicit_retry() {
    let fixture = Fixture::new();
    let runtime = TelegramContinuousState::default();
    fixture.enable(&runtime, true);
    let moved = fixture.logs.with_file_name("moved-logs");
    fs::rename(&fixture.logs, &moved).unwrap();
    runtime
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    assert!(fixture.view(&runtime).observation.phase == Phase::Failed);
    fs::rename(moved, &fixture.logs).unwrap();
    fixture.append(10, CHAT, "retry");
    runtime
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    assert_eq!(fixture.messages(), 1, "failure must not silently retry");
    fixture.enable(&runtime, true);
    runtime
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    assert_eq!(fixture.messages(), 2);
    runtime.shutdown(&fixture.store).unwrap();
}

#[test]
fn stale_stop_cannot_close_writer_and_disconnect_closes_it() {
    let fixture = Fixture::new();
    let runtime = TelegramContinuousState::default();
    let before = fixture.enable(&runtime, true);
    fixture.append(10, CHAT, "advance revision");
    runtime
        .tick(&fixture.store, |id, source, capture| {
            fixture
                .store
                .apply_telegram_observations(id, source, capture)
                .map(|_| true)
        })
        .unwrap();
    let error = runtime
        .set_enabled(
            &fixture.store,
            &fixture.project_id,
            "selected",
            before.revision,
            StartOptions {
                enabled: false,
                input_directory: None,
                confirmed_single_account: false,
                manage_client: None,
            },
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(runtime.0.lock().unwrap().captures.len(), 1);
    let project = fixture.project();
    fixture
        .store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::RemoveSource("selected".into()),
        )
        .unwrap();
    runtime
        .disconnected(&fixture.store, &fixture.project_id, "selected")
        .unwrap();
    assert!(runtime.0.lock().unwrap().captures.is_empty());
    assert!(fixture
        .store
        .telegram_journal_path(&fixture.project_id, "selected")
        .is_err());
    assert!(fixture.project().sources.is_empty());
}
