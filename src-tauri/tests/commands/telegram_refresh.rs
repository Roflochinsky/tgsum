use super::*;
use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tgsum_app::telegram_refresh::TelegramRefreshState;
use tgsum_core::assisted::AssistedExportSettings;
use tgsum_core::bridge_refresh::{
    DriverEvent, DriverRequest, DriverStatus, RefreshDriver, RefreshObservation,
};
use tgsum_core::project::{ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;

#[derive(Default)]
struct Events {
    queue: VecDeque<DriverEvent>,
    starts: usize,
    cancels: usize,
}
struct Driver(Arc<Mutex<Events>>, PathBuf);
impl RefreshDriver for Driver {
    fn available(&self) -> bool {
        true
    }
    fn start(&mut self, request: &DriverRequest) -> io::Result<()> {
        let mut events = self.0.lock().unwrap();
        events.starts += 1;
        events.queue.push_back(DriverEvent {
            request: request.clone(),
            status: DriverStatus::Exporting,
        });
        events.queue.push_back(DriverEvent {
            request: request.clone(),
            status: DriverStatus::Completed {
                archive_path: self.1.clone(),
            },
        });
        Ok(())
    }
    fn poll(&mut self, _: &DriverRequest) -> io::Result<Option<DriverEvent>> {
        Ok(self.0.lock().unwrap().queue.pop_front())
    }
    fn cancel(&mut self, _: &DriverRequest) -> io::Result<()> {
        self.0.lock().unwrap().cancels += 1;
        Ok(())
    }
}

#[test]
fn refresh_ipc_and_timer_share_scope_cancellation_cadence_and_recovery() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().canonicalize().unwrap();
    let path = directory.join("result.json");
    std::fs::write(&path, r#"{"id":42,"name":"Synthetic","type":"private_group","messages":[{"id":1,"type":"message","text":"synthetic"}]}"#).unwrap();
    let store = ProjectStore::new(directory.join("projects"));
    let mut p = store.create("Refresh IPC").unwrap();
    p = store
        .update(
            &p.project_id,
            p.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "selected".into(),
                connector_id: "telegram_json".into(),
                scope: SourceScope::telegram("synthetic", "42"),
                archive_path: None,
                latest_snapshot_id: None,
                selection: Default::default(),
            }),
        )
        .unwrap();
    p = store
        .update(
            &p.project_id,
            p.revision,
            ProjectChange::AssistedExport {
                source_id: "selected".into(),
                settings: Some(AssistedExportSettings {
                    directory,
                    client: None,
                }),
            },
        )
        .unwrap();
    let events = Arc::new(Mutex::new(Events::default()));
    let time = Arc::new(Mutex::new(RefreshObservation {
        unix_seconds: 100,
        active_unlocked: false,
        at: Instant::now(),
    }));
    let clock = Arc::clone(&time);
    let runtime = TelegramRefreshState::with_driver(
        store.clone(),
        Box::new(Driver(Arc::clone(&events), path)),
        Arc::new(move || *clock.lock().unwrap()),
        "test-launch".into(),
    )
    .unwrap();
    let app = tgsum_app::app(
        command_builder()
            .manage(store.clone())
            .manage(runtime.clone()),
    )
    .build(mock_context(noop_assets()))
    .unwrap();
    let w = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let target = json!({"projectId":p.project_id,"sourceId":"selected"});
    let mut change = target.clone();
    change["expectedRevision"] = p.revision.into();
    change["cadence"] = "on_start".into();
    let scheduled = invoke(&w, "set_telegram_refresh_cadence", change.clone()).unwrap();
    assert_eq!(
        scheduled["telegram_refresh"]["selected"]["cadence"],
        "on_start"
    );
    assert_eq!(
        invoke(&w, "set_telegram_refresh_cadence", change).unwrap_err()["kind"],
        "conflict"
    );
    runtime.tick().unwrap();
    assert_eq!(events.lock().unwrap().starts, 0);
    time.lock().unwrap().active_unlocked = true;
    runtime.tick().unwrap();
    let running = invoke(&w, "telegram_refresh_status", target.clone()).unwrap();
    assert_eq!(running["state"]["state"], "waiting_for_client");
    assert!(running["run_id"].is_string());
    assert_eq!(
        invoke(
            &w,
            "cancel_telegram_refresh",
            json!({"projectId":p.project_id,"sourceId":"selected","runId":"old-run"})
        )
        .unwrap_err()["kind"],
        "conflict"
    );
    runtime.tick().unwrap();
    runtime.tick().unwrap();
    let done = invoke(&w, "telegram_refresh_status", target.clone()).unwrap();
    assert_eq!(done["state"]["state"], "ready");
    assert_eq!(done["state"]["delta"]["created"], 1);
    assert!(done["project"]["sources"][0]["latest_snapshot_id"].is_string());
    runtime.tick().unwrap();
    assert_eq!(events.lock().unwrap().starts, 1);

    let start = |revision: Value| {
        let mut args = target.clone();
        args["expectedRevision"] = revision;
        invoke(&w, "start_telegram_refresh", args)
    };
    assert_eq!(
        start(done["project"]["revision"].clone()).unwrap()["state"],
        "waiting_for_client"
    );
    let running = invoke(&w, "telegram_refresh_status", target.clone()).unwrap();
    let mut cancel = target.clone();
    cancel["runId"] = running["run_id"].clone();
    invoke(&w, "cancel_telegram_refresh", cancel).unwrap();
    runtime.tick().unwrap();
    runtime.tick().unwrap();
    let stopped = invoke(&w, "telegram_refresh_status", target.clone()).unwrap();
    assert_eq!(stopped["state"]["state"], "cancelled");
    assert_eq!(events.lock().unwrap().cancels, 1);
    assert_eq!(stopped["project"]["sources"], done["project"]["sources"]);
    assert_eq!(
        start(stopped["project"]["revision"].clone()).unwrap()["decision"]["wait_until"],
        1000
    );

    time.lock().unwrap().unix_seconds = 1001;
    time.lock().unwrap().at += Duration::from_secs(901);
    start(stopped["project"]["revision"].clone()).unwrap();
    time.lock().unwrap().active_unlocked = false;
    runtime.tick().unwrap();
    let unresolved = invoke(&w, "telegram_refresh_status", target.clone()).unwrap();
    assert_eq!(unresolved["state"]["state"], "needs_user_action");
    let mut review = target.clone();
    review["expectedRevision"] = unresolved["project"]["revision"].clone();
    review["clientStoppedConfirmed"] = false.into();
    assert!(invoke(&w, "resolve_telegram_refresh", review.clone()).is_err());
    review["clientStoppedConfirmed"] = true.into();
    let recovered = invoke(&w, "resolve_telegram_refresh", review.clone()).unwrap();
    assert_eq!(
        recovered["telegram_refresh"]["selected"]["checkpoint"]["unresolved_attempt"],
        false
    );
    assert_eq!(
        invoke(&w, "resolve_telegram_refresh", review).unwrap_err()["kind"],
        "conflict"
    );
}

#[test]
fn unavailable_binding_refuses_automatic_cadence_and_never_claims_a_source() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let p = store.create("Unavailable runtime").unwrap();
    let runtime = TelegramRefreshState::with_driver(
        store.clone(),
        Box::new(tgsum_core::bridge_refresh::UnavailableDriver),
        Arc::new(|| RefreshObservation {
            unix_seconds: 100,
            active_unlocked: true,
            at: Instant::now(),
        }),
        "unavailable-test".into(),
    )
    .unwrap();
    let app = tgsum_app::app(
        command_builder()
            .manage(store.clone())
            .manage(runtime.clone()),
    )
    .build(mock_context(noop_assets()))
    .unwrap();
    let w = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let args =
        json!({"projectId":p.project_id,"sourceId":"selected","expectedRevision":p.revision});
    assert_eq!(
        invoke(&w, "start_telegram_refresh", args.clone()).unwrap()["state"],
        "unavailable"
    );
    let mut cadence = args;
    cadence["cadence"] = "daily".into();
    assert_eq!(
        invoke(&w, "set_telegram_refresh_cadence", cadence).unwrap_err()["kind"],
        "failed"
    );
    runtime.tick().unwrap();
    assert_eq!(store.open(&p.project_id).unwrap(), p);
}
