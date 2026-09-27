//! Application timer and IPC for the injectable Telegram refresh coordinator.
//! Production binds UnavailableDriver until an OS/client profile is qualified.

#[cfg(all(debug_assertions, feature = "desktop-e2e"))]
pub(crate) mod fixture;

use std::collections::HashMap;
use std::io;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::State;
use tgsum_core::bridge_refresh::{
    RefreshCoordinator, RefreshDriver, RefreshObservation, RefreshState, UnavailableDriver,
};
use tgsum_core::bridge_schedule::{
    ClientReview, RefreshCadence, RefreshContext, RefreshDecision, RefreshTrigger,
    TelegramExportLease,
};
use tgsum_core::project::{Project, ProjectChange, ProjectStore};

use crate::{analysis, run_blocking, CmdError};

type SourceKey = (String, String);
type Cancellation = Option<(SourceKey, String, Arc<AtomicBool>)>;
type Coordinator = RefreshCoordinator<Box<dyn RefreshDriver + Send>>;

struct Host {
    coordinator: Coordinator,
    last: HashMap<SourceKey, RefreshState>,
}

#[derive(Clone)]
pub struct TelegramRefreshState {
    host: Arc<Mutex<Host>>,
    cancellation: Arc<Mutex<Cancellation>>,
    clock: Arc<dyn Fn() -> RefreshObservation + Send + Sync>,
    launch_id: String,
}

impl TelegramRefreshState {
    /// Dependency injection for native adapters and offline command tests.
    pub fn with_driver(
        store: ProjectStore,
        driver: Box<dyn RefreshDriver + Send>,
        clock: Arc<dyn Fn() -> RefreshObservation + Send + Sync>,
        launch_id: String,
    ) -> io::Result<Self> {
        if launch_id.is_empty() {
            return Err(io::Error::other("refresh launch ID is empty"));
        }
        Ok(Self {
            host: Arc::new(Mutex::new(Host {
                coordinator: RefreshCoordinator::new(
                    store,
                    driver,
                    Duration::from_secs(6 * 60 * 60),
                )?,
                last: HashMap::new(),
            })),
            cancellation: Default::default(),
            clock,
            launch_id,
        })
    }

    pub(crate) fn for_app(store: ProjectStore) -> io::Result<Self> {
        Self::with_driver(
            store,
            Box::new(UnavailableDriver),
            Arc::new(|| RefreshObservation {
                unix_seconds: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                // A visible/focused Tauri window does not establish unlocked OS state.
                active_unlocked: false,
                at: Instant::now(),
            }),
            format!("{}-{:?}", std::process::id(), SystemTime::now()),
        )
    }

    fn context(&self, observation: RefreshObservation) -> RefreshContext<'_> {
        RefreshContext {
            now: observation.unix_seconds,
            launch_id: &self.launch_id,
            session_active_unlocked: observation.active_unlocked,
            client_not_before: None,
            any_export_in_flight: false,
        }
    }

    fn record(&self, host: &mut Host, key: Option<SourceKey>) -> io::Result<()> {
        if let Some(key) = key {
            host.last.insert(key, host.coordinator.state().clone());
        }
        let active = host.coordinator.active_request().and_then(|request| {
            host.coordinator.cancellation().map(|flag| {
                (
                    (request.project_id.clone(), request.source_id.clone()),
                    request.run_id.clone(),
                    flag,
                )
            })
        });
        *self.cancellation.lock().map_err(poisoned)? = active;
        Ok(())
    }

    pub fn tick(&self) -> io::Result<()> {
        let observation = (self.clock)();
        let mut host = self.host.lock().map_err(poisoned)?;
        let before = host
            .coordinator
            .active_request()
            .map(|r| (r.project_id.clone(), r.source_id.clone()));
        let result = host
            .coordinator
            .tick(self.context(observation), observation.at);
        let key = before.or_else(|| {
            host.coordinator
                .active_request()
                .map(|r| (r.project_id.clone(), r.source_id.clone()))
        });
        self.record(&mut host, key)?;
        result.map(|_| ())
    }
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other("refresh state unavailable")
}

#[derive(Serialize)]
pub(crate) struct RefreshView {
    available: bool,
    project: Project,
    state: RefreshState,
    decision: RefreshDecision,
    run_id: Option<String>,
}

#[tauri::command]
pub(crate) async fn telegram_refresh_status(
    runtime: State<'_, TelegramRefreshState>,
    store: State<'_, ProjectStore>,
    project_id: String,
    source_id: String,
) -> Result<RefreshView, CmdError> {
    let runtime = runtime.inner().clone();
    let store = store.inner().clone();
    run_blocking(move || {
        let project = store.open(&project_id)?;
        if !project.sources.iter().any(|s| s.source_id == source_id) {
            return Err(CmdError::Failed("Источник отключён.".into()));
        }
        let host = runtime.host.lock().map_err(poisoned)?;
        let request = host.coordinator.active_request();
        let this_run = request.filter(|r| r.project_id == project_id && r.source_id == source_id);
        let plan = project
            .telegram_refresh
            .get(&source_id)
            .cloned()
            .unwrap_or_default();
        let mut context = runtime.context((runtime.clock)());
        context.any_export_in_flight = request.is_some();
        let state = if this_run.is_some() {
            host.coordinator.state().clone()
        } else if plan.checkpoint.unresolved_attempt {
            RefreshState::NeedsUserAction
        } else {
            host.last
                .get(&(project_id, source_id))
                .cloned()
                .unwrap_or(RefreshState::Idle)
        };
        Ok(RefreshView {
            available: host.coordinator.available(),
            project,
            state,
            decision: plan.checkpoint.decide(plan.cadence, context),
            run_id: this_run.map(|r| r.run_id.clone()),
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn start_telegram_refresh(
    runtime: State<'_, TelegramRefreshState>,
    store: State<'_, ProjectStore>,
    analysis: State<'_, analysis::AnalysisState>,
    project_id: String,
    source_id: String,
    expected_revision: u64,
) -> Result<RefreshState, CmdError> {
    analysis.before_edit(&store, &project_id)?;
    let runtime = runtime.inner().clone();
    let store = store.inner().clone();
    run_blocking(move || {
        let project = reviewed(&store, &project_id, expected_revision)?;
        let observation = (runtime.clock)();
        let mut host = runtime.host.lock().map_err(poisoned)?;
        let result = host.coordinator.start(
            &project,
            &source_id,
            RefreshTrigger::Manual,
            runtime.context(observation),
            observation.at,
        );
        // A refused second start must not replace the first run's status.
        if !matches!(
            result,
            Ok(RefreshState::Deferred {
                decision: RefreshDecision::Busy
            })
        ) {
            runtime.record(&mut host, Some((project_id, source_id)))?;
        }
        Ok(result?)
    })
    .await
}

#[tauri::command]
pub(crate) fn cancel_telegram_refresh(
    runtime: State<'_, TelegramRefreshState>,
    project_id: String,
    source_id: String,
    run_id: String,
) -> Result<(), CmdError> {
    // Independent mutex: cancellation can interrupt a worker importing a large file.
    let cancellation = runtime.cancellation.lock().map_err(poisoned)?;
    let (key, active_id, flag) = cancellation
        .as_ref()
        .ok_or_else(|| CmdError::Failed("Нет активного обновления.".into()))?;
    if key != &(project_id, source_id) || active_id != &run_id {
        return Err(CmdError::Conflict("Обновляется другой источник.".into()));
    }
    flag.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub(crate) async fn set_telegram_refresh_cadence(
    runtime: State<'_, TelegramRefreshState>,
    store: State<'_, ProjectStore>,
    analysis: State<'_, analysis::AnalysisState>,
    project_id: String,
    source_id: String,
    expected_revision: u64,
    cadence: RefreshCadence,
) -> Result<Project, CmdError> {
    analysis.before_edit(&store, &project_id)?;
    let runtime = runtime.inner().clone();
    let store = store.inner().clone();
    run_blocking(move || {
        let host = runtime.host.lock().map_err(poisoned)?;
        if host
            .coordinator
            .active_request()
            .is_some_and(|r| r.project_id == project_id)
        {
            return Err(CmdError::Conflict(
                "Дождитесь завершения или остановите текущее обновление.".into(),
            ));
        }
        if cadence != RefreshCadence::Manual && !host.coordinator.available() {
            return Err(CmdError::Failed(
                "Автоматический экспорт пока недоступен для этого клиента.".into(),
            ));
        }
        let project = reviewed(&store, &project_id, expected_revision)?;
        Ok(store.update(
            &project_id,
            project.revision,
            ProjectChange::TelegramRefreshCadence { source_id, cadence },
        )?)
    })
    .await
}

#[tauri::command]
pub(crate) async fn resolve_telegram_refresh(
    runtime: State<'_, TelegramRefreshState>,
    store: State<'_, ProjectStore>,
    project_id: String,
    source_id: String,
    expected_revision: u64,
    client_stopped_confirmed: bool,
) -> Result<Project, CmdError> {
    if !client_stopped_confirmed {
        return Err(CmdError::Failed(
            "Проверьте, что экспорт в Telegram завершён или остановлен.".into(),
        ));
    }
    let runtime = runtime.inner().clone();
    let store = store.inner().clone();
    run_blocking(move || {
        let mut host = runtime.host.lock().map_err(poisoned)?;
        let project = reviewed(&store, &project_id, expected_revision)?;
        let lease = TelegramExportLease::try_acquire(&store)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "export is still active"))?;
        let resolved = lease.resolve_after_client_review(
            &store,
            &project,
            &source_id,
            ClientReview::NoExportInProgress,
            (runtime.clock)().unix_seconds,
        )?;
        host.last
            .insert((project_id, source_id), RefreshState::Idle);
        Ok(resolved)
    })
    .await
}

fn reviewed(store: &ProjectStore, id: &str, revision: u64) -> io::Result<Project> {
    let project = store.open(id)?;
    if project.revision != revision {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "Project changed; reload before refresh",
        ));
    }
    Ok(project)
}

pub(crate) fn start_timer(runtime: TelegramRefreshState, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            // Errors are represented by the coordinator's status/checkpoint;
            // archive contents and paths are not written to background logs.
            let _ = runtime.tick();
            for _ in 0..10 {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
}
