use super::*;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Write};

use tgsum_core::project::ProjectSource;
use tgsum_core::snapshot::SourceScope;

const CHAT: &str = "9007199254740995";
const SENDER: &str = "9007199254740993";

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
                }
            )
            .is_err());
        assert_eq!(fixture.project(), project);
    }
    fixture.enable(&runtime, true);
    runtime.shutdown();
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
    runtime.shutdown();
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
    restarted.shutdown();
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
    runtime.shutdown();
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
    restarted.shutdown();
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
    runtime.shutdown();
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
    runtime.shutdown();
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
    restarted.shutdown();
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
    runtime.shutdown();
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
        .disconnected(&fixture.project_id, "selected")
        .unwrap();
    assert!(runtime.0.lock().unwrap().captures.is_empty());
    assert!(fixture
        .store
        .telegram_journal_path(&fixture.project_id, "selected")
        .is_err());
    assert!(fixture.project().sources.is_empty());
}
