use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tgsum_core::local_package::{PackageSettings, PackageState};
use tgsum_core::project::{ProjectEntry, ProjectStore};

use crate::{analysis::AnalysisState, run_blocking, CmdError};

#[derive(Default)]
pub(crate) struct PackageRuntime(Mutex<HashMap<String, Arc<AtomicBool>>>);

fn execute<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    stop: Option<&AtomicBool>,
) -> Result<PackageState, CmdError> {
    let runtime = app.state::<PackageRuntime>();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut active = runtime
            .0
            .lock()
            .map_err(|_| CmdError::Failed("Состояние обновления недоступно.".into()))?;
        if active.contains_key(id) {
            return Err(CmdError::Conflict("Пакет уже обновляется.".into()));
        }
        active.insert(id.into(), Arc::clone(&cancel));
    }
    let result = (|| {
        let store = app.state::<ProjectStore>();
        if stop.is_some() {
            if let Some(state) = store.local_package(id)? {
                if !state.settings.automatic {
                    return Ok(state);
                }
            }
        }
        app.state::<AnalysisState>().before_edit(&store, id)?;
        let cancelled =
            || cancel.load(Ordering::Relaxed) || stop.is_some_and(|s| s.load(Ordering::Relaxed));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut state = store.refresh_local_package(id, now, cancelled)?;
        if state.phase == "ready"
            && state.settings.github_repository.is_some()
            && store.local_package_allows_publication(id)?
        {
            if let Some(receipt) = &state.ready {
                if state.github_content_sha256.as_ref() != Some(&receipt.content_sha256) {
                    let digest = receipt.content_sha256.clone();
                    let _ = app.emit("local-package-publishing", id);
                    let result = crate::package_github::publish(&store, &state, cancelled)
                        .map_err(|e| e.to_string());
                    state = store.record_package_github(id, &digest, result)?;
                }
            }
        }
        Ok(state)
    })();
    let result = if cancel.load(Ordering::Relaxed) {
        app.state::<ProjectStore>()
            .pause_local_package(id)
            .map_err(CmdError::from)
    } else {
        result
    };
    if let Ok(mut active) = runtime.0.lock() {
        active.remove(id);
    }
    let _ = app.emit("local-package-changed", id);
    result
}

#[tauri::command]
pub(crate) async fn local_package_status(
    store: State<'_, ProjectStore>,
    project_id: String,
) -> Result<Option<PackageState>, CmdError> {
    let store = store.inner().clone();
    run_blocking(move || Ok(store.local_package(&project_id)?)).await
}

#[tauri::command]
pub(crate) async fn configure_local_package(
    analysis: State<'_, AnalysisState>,
    store: State<'_, ProjectStore>,
    project_id: String,
    expected_revision: u64,
    settings: PackageSettings,
) -> Result<PackageState, CmdError> {
    analysis.before_edit(&store, &project_id)?;
    let store = store.inner().clone();
    run_blocking(move || {
        Ok(store.configure_local_package(&project_id, expected_revision, settings)?)
    })
    .await
}

#[tauri::command]
pub(crate) async fn refresh_local_package<R: Runtime>(
    app: AppHandle<R>,
    project_id: String,
) -> Result<PackageState, CmdError> {
    run_blocking(move || execute(&app, &project_id, None)).await
}

#[tauri::command]
pub(crate) fn cancel_local_package(
    runtime: State<'_, PackageRuntime>,
    store: State<'_, ProjectStore>,
    project_id: String,
) -> Result<(), CmdError> {
    if let Ok(active) = runtime.0.lock() {
        if let Some(cancel) = active.get(&project_id) {
            cancel.store(true, Ordering::Relaxed);
            return Ok(());
        }
    }
    store.pause_local_package(&project_id)?;
    Ok(())
}

pub(crate) fn start_timer<R: Runtime>(app: AppHandle<R>, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        // Startup registration tests intentionally use only minimal state.
        if app.try_state::<PackageRuntime>().is_none() {
            return;
        }
        while !stop.load(Ordering::Relaxed) {
            if let Ok(projects) = app.state::<ProjectStore>().list() {
                for entry in projects {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let ProjectEntry::Ready { project } = entry else {
                        continue;
                    };
                    let store = app.state::<ProjectStore>();
                    let Ok(Some(state)) = store.local_package(&project.project_id) else {
                        continue;
                    };
                    if state.settings.automatic && state.phase != "needs_setup" {
                        let _ = execute(&app, &project.project_id, Some(&stop));
                    }
                }
            }
            // Bounded checks, responsive shutdown, no busy polling/network flood.
            for _ in 0..300 {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
}
