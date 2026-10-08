//! Linux window lifetime and an explicitly managed user-session service.
//! Service configuration is separate from source plans; disabling login startup
//! does not stop a running collector or remove its retained observations.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tgsum_core::project::{ProjectEntry, ProjectStore};

use crate::{run_blocking, CmdError};

#[derive(Default)]
pub(crate) struct BackgroundState {
    pub(crate) launch_hidden: bool,
    pub(crate) quit_on_ready: bool,
    #[cfg(target_os = "linux")]
    pub(crate) handle_signals: bool,
    pub(crate) foreground_requested: AtomicBool,
    launch_exports: std::sync::Mutex<std::collections::VecDeque<std::path::PathBuf>>,
    launch_overflow: AtomicBool,
    #[cfg(all(debug_assertions, feature = "desktop-e2e"))]
    fixture_enabled: std::sync::Arc<AtomicBool>,
    keep_alive: AtomicBool,
    quitting: AtomicBool,
    #[cfg(target_os = "linux")]
    configuration: std::sync::Mutex<()>,
}

impl BackgroundState {
    pub(crate) fn for_launch(hidden: bool, quit_on_ready: bool) -> Self {
        Self {
            launch_hidden: hidden || quit_on_ready,
            quit_on_ready,
            #[cfg(target_os = "linux")]
            handle_signals: true,
            keep_alive: AtomicBool::new(hidden),
            ..Self::default()
        }
    }
}

pub(crate) fn keep_alive<R: Runtime>(app: &AppHandle<R>) -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    if let Some(state) = app.try_state::<BackgroundState>() {
        if state.quitting.load(Ordering::Relaxed) {
            return false;
        }
        if state.keep_alive.load(Ordering::Relaxed) {
            return true;
        }
    }
    app.try_state::<ProjectStore>().is_some_and(|store| {
        store.list().map_or(true, |entries| {
            entries.into_iter().any(|entry| match entry {
                ProjectEntry::Ready { project } => project
                    .telegram_continuous
                    .values()
                    .any(|plan| plan.settings.enabled),
                // An unreadable Project is not evidence that its saved plan
                // was disabled. Keep the process available for recovery.
                ProjectEntry::Unavailable { .. } => true,
            })
        })
    })
}

pub(crate) fn reopen<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<BackgroundState>() {
        state.foreground_requested.store(true, Ordering::Release);
    }
    if let Some(window) = app.get_webview_window("main") {
        if window.show().is_ok() {
            if let Some(state) = app.try_state::<BackgroundState>() {
                state.foreground_requested.store(false, Ordering::Release);
            }
        }
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub(crate) fn forward_launch<R: Runtime>(app: &AppHandle<R>, arguments: &[String], cwd: &str) {
    if arguments.iter().any(|arg| arg == "--quit") {
        quit_application(app.clone());
        return;
    }
    if arguments.iter().any(|arg| arg == "--background") {
        return;
    }
    let export = arguments.iter().skip(1).find_map(|arg| {
        let path = std::path::Path::new(arg);
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            std::path::Path::new(cwd).join(path)
        };
        tgsum_core::resolve_export_path(&absolute)
    });
    if let Some(export) = export {
        if let Some(state) = app.try_state::<BackgroundState>() {
            if let Ok(mut queue) = state.launch_exports.lock() {
                if queue.len() < 32 {
                    queue.push_back(export);
                } else {
                    state.launch_overflow.store(true, Ordering::Release);
                }
            }
        }
        let _ = app.emit("tgsum:open-export", ());
    }
    reopen(app);
}

#[tauri::command]
pub(crate) fn next_launch_export(
    state: State<'_, BackgroundState>,
) -> Result<Option<String>, CmdError> {
    if state.launch_overflow.swap(false, Ordering::AcqRel) {
        return Err(CmdError::Failed("Одновременно открыто слишком много файлов. Повторите последний запуск после обработки очереди.".into()));
    }
    let mut queue = state.launch_exports.lock().map_err(|_| unavailable())?;
    Ok(queue.pop_front().map(|path| path.display().to_string()))
}

#[derive(Serialize)]
pub(crate) struct BackgroundView {
    available: bool,
    keeps_running: bool,
    window_visible: bool,
    autostart_enabled: bool,
    service_path: Option<String>,
}

#[tauri::command]
pub(crate) async fn background_status<R: Runtime>(
    app: AppHandle<R>,
) -> Result<BackgroundView, CmdError> {
    let keeps_running = keep_alive(&app);
    let window_visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    run_blocking(move || {
        #[cfg(target_os = "linux")]
        {
            let state = app.state::<BackgroundState>();
            let _guard = state.configuration.lock().map_err(|_| unavailable())?;
            let config = configuration(&app)?;
            let autostart_enabled = config.enabled()?;
            Ok(BackgroundView {
                available: true,
                keeps_running,
                window_visible,
                autostart_enabled,
                service_path: Some(config.path().display().to_string()),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = app;
            Ok(BackgroundView {
                available: false,
                keeps_running: false,
                window_visible,
                autostart_enabled: false,
                service_path: None,
            })
        }
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_background_autostart<R: Runtime>(
    app: AppHandle<R>,
    enabled: bool,
) -> Result<BackgroundView, CmdError> {
    let keeps_running = keep_alive(&app);
    let window_visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    run_blocking(move || {
        #[cfg(target_os = "linux")]
        {
            let state = app.state::<BackgroundState>();
            let _guard = state.configuration.lock().map_err(|_| unavailable())?;
            let config = configuration(&app)?;
            config.set(enabled, &std::env::current_exe()?)?;
            Ok(BackgroundView {
                available: true,
                keeps_running,
                window_visible,
                autostart_enabled: config.enabled()?,
                service_path: Some(config.path().display().to_string()),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (app, enabled);
            Err(std::io::Error::other("Автозапуск сборщика пока доступен только на Linux.").into())
        }
    })
    .await
}

#[tauri::command]
pub(crate) fn hide_application<R: Runtime>(app: AppHandle<R>) -> Result<(), CmdError> {
    if !cfg!(target_os = "linux") {
        return Err(CmdError::Failed(
            "Фоновый режим пока доступен только на Linux.".into(),
        ));
    }
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| CmdError::Failed("Окно приложения недоступно.".into()))?;
    window.hide().map_err(|_| unavailable())?;
    app.state::<BackgroundState>()
        .keep_alive
        .store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub(crate) fn quit_application<R: Runtime>(app: AppHandle<R>) {
    app.state::<BackgroundState>()
        .quitting
        .store(true, Ordering::Relaxed);
    app.exit(0);
}

#[cfg(target_os = "linux")]
fn unavailable() -> std::io::Error {
    std::io::Error::other("Фоновый режим недоступен. Повторите действие после перезапуска TGSUM.")
}

#[cfg(not(target_os = "linux"))]
fn unavailable() -> std::io::Error {
    std::io::Error::other("Фоновый режим недоступен.")
}

#[cfg(target_os = "linux")]
pub(crate) fn start_signal_handler<R: Runtime>(app: AppHandle<R>) -> std::io::Result<()> {
    use signal_hook::consts::{SIGINT, SIGTERM};
    let mut signals = signal_hook::iterator::Signals::new([SIGINT, SIGTERM])?;
    std::thread::spawn(move || {
        if signals.forever().next().is_some() {
            // Let Tauri finish the journal writer on ExitRequested. A user
            // service stop must not bypass normal application shutdown.
            app.state::<BackgroundState>()
                .quitting
                .store(true, Ordering::Relaxed);
            app.exit(0);
        }
    });
    Ok(())
}

#[cfg(target_os = "linux")]
fn configuration<R: Runtime>(app: &AppHandle<R>) -> std::io::Result<linux::Configuration> {
    #[cfg(all(debug_assertions, feature = "desktop-e2e"))]
    if let Some(harness) = app.try_state::<crate::desktop_e2e::Harness>() {
        return Ok(linux::Configuration::fixture(
            harness.root.join("config"),
            std::sync::Arc::clone(&app.state::<BackgroundState>().fixture_enabled),
        ));
    }
    Ok(linux::Configuration::new(app.path().config_dir().map_err(
        |_| std::io::Error::other("Папка пользовательских настроек недоступна."),
    )?))
}

#[cfg(target_os = "linux")]
mod linux;

#[cfg(all(test, target_os = "linux"))]
mod tests;
