//! tgsum desktop app: a Tauri shell around `tgsum-core`.
//!
//! The UI (`ui/`) calls these commands through `window.__TAURI__.core.invoke`.
//! Both passes run on a blocking thread, stream the export from disk, emit
//! throttled `progress` events and stop early when the user cancels.

mod desktop;
#[cfg(target_os = "linux")]
mod launcher;

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Builder, Emitter, Manager, Runtime, State, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tgsum_core::project::{Project, ProjectChange, ProjectEntry, ProjectStore, SourceAvailability};
use tgsum_core::{
    cancelled, extract_reader, index_reader, is_cancelled, resolve_export_path, write_units,
    ChatIndex, ProgressReader, Selection, DEFAULT_MAX_TOKENS,
};

/// Default output folder, created next to the export.
const OUT_DIR_NAME: &str = "tgsum-output";

/// Minimum gap between two `progress` events.
const PROGRESS_EVERY: Duration = Duration::from_millis(80);

/// Error returned to the UI: cancelled, failed, or a Project revision conflict.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
enum CmdError {
    Cancelled,
    Conflict(String),
    Failed(String),
}

impl From<io::Error> for CmdError {
    fn from(e: io::Error) -> Self {
        if is_cancelled(&e) {
            CmdError::Cancelled
        } else if e.kind() == io::ErrorKind::WouldBlock {
            CmdError::Conflict(e.to_string())
        } else {
            CmdError::Failed(e.to_string())
        }
    }
}

/// The running pass's cancel flag. Each pass gets a fresh flag, so a late
/// cancel can never hit the next pass.
#[derive(Default)]
struct Jobs {
    current: Mutex<Arc<AtomicBool>>,
}

impl Jobs {
    fn start(&self) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        *self.current.lock().unwrap() = Arc::clone(&flag);
        flag
    }

    fn cancel(&self) {
        self.current.lock().unwrap().store(true, Ordering::Relaxed);
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    phase: &'static str,
    read: u64,
    total: u64,
}

/// Opens the export, reporting read progress as `progress` events and
/// aborting once `cancel` is set.
fn open_tracked<R: Runtime>(
    app: &AppHandle<R>,
    path: &Path,
    phase: &'static str,
    cancel: Arc<AtomicBool>,
) -> io::Result<impl Read> {
    let file = File::open(path)?;
    let total = file.metadata()?.len();
    let app = app.clone();
    let mut last: Option<Instant> = None;
    Ok(ProgressReader::new(file, move |read| {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        if read == total || last.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY) {
            last = Some(Instant::now());
            let _ = app.emit("progress", Progress { phase, read, total });
        }
        Ok(())
    }))
}

async fn run_blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, CmdError> + Send + 'static,
) -> Result<T, CmdError> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| CmdError::Failed(e.to_string()))?
}

fn path_string(p: &Path) -> String {
    p.display().to_string()
}

#[tauri::command]
async fn create_project(store: State<'_, ProjectStore>, name: String) -> Result<Project, CmdError> {
    let store = store.inner().clone();
    run_blocking(move || Ok(store.create(&name)?)).await
}

#[tauri::command]
async fn list_projects(store: State<'_, ProjectStore>) -> Result<Vec<ProjectEntry>, CmdError> {
    let store = store.inner().clone();
    run_blocking(move || Ok(store.list()?)).await
}

#[tauri::command]
async fn open_project(
    store: State<'_, ProjectStore>,
    project_id: String,
) -> Result<Project, CmdError> {
    let store = store.inner().clone();
    run_blocking(move || Ok(store.open(&project_id)?)).await
}

#[tauri::command]
async fn update_project(
    store: State<'_, ProjectStore>,
    project_id: String,
    expected_revision: u64,
    change: ProjectChange,
) -> Result<Project, CmdError> {
    let store = store.inner().clone();
    run_blocking(move || Ok(store.update(&project_id, expected_revision, change)?)).await
}

#[tauri::command]
async fn project_source_status(
    store: State<'_, ProjectStore>,
    project_id: String,
    source_id: String,
) -> Result<SourceAvailability, CmdError> {
    let store = store.inner().clone();
    run_blocking(move || {
        let project = store.open(&project_id)?;
        let source = project
            .sources
            .iter()
            .find(|s| s.source_id == source_id)
            .ok_or_else(|| CmdError::Failed("source not connected to this project".into()))?;
        Ok(source.availability())
    })
    .await
}

/// A path the app was launched with (`tgsum result.json`, "Open with").
#[tauri::command]
fn initial_path() -> Option<String> {
    std::env::args_os()
        .skip(1)
        .find_map(|arg| resolve_export_path(Path::new(&arg)))
        .map(|p| path_string(&p))
}

/// Native "open file" dialog for `result.json`.
#[tauri::command]
async fn pick_export<R: Runtime>(app: AppHandle<R>) -> Option<String> {
    app.dialog()
        .file()
        .set_title("Выберите result.json")
        .add_filter("Выгрузка Telegram (JSON)", &["json"])
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
        .map(|p| path_string(&p))
}

/// Native folder picker for the output folder.
#[tauri::command]
async fn pick_out_dir<R: Runtime>(app: AppHandle<R>, current: Option<String>) -> Option<String> {
    let mut dialog = app.dialog().file().set_title("Куда сохранить файлы");
    if let Some(dir) = current.map(PathBuf::from) {
        let existing = dir.ancestors().find(|d| d.is_dir()).map(Path::to_path_buf);
        if let Some(existing) = existing {
            dialog = dialog.set_directory(existing);
        }
    }
    dialog
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
        .map(|p| path_string(&p))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Indexed {
    /// The resolved `result.json` (a dropped folder resolves to the file inside).
    path: String,
    file_name: String,
    size: u64,
    /// Suggested output folder next to the export.
    out_dir: String,
    chats: Vec<ChatIndex>,
}

/// Pass 1: index every chat and forum topic of the export.
#[tauri::command]
async fn index_export<R: Runtime>(
    app: AppHandle<R>,
    jobs: State<'_, Jobs>,
    path: String,
) -> Result<Indexed, CmdError> {
    let path = resolve_export_path(Path::new(&path))
        .ok_or_else(|| CmdError::Failed(format!("Файл не найден: {path}")))?;
    let cancel = jobs.start();
    run_blocking(move || {
        let size = fs::metadata(&path)?.len();
        let chats = index_reader(open_tracked(&app, &path, "index", cancel)?)?;
        let dir = path.parent().unwrap_or(Path::new("."));
        Ok(Indexed {
            file_name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            out_dir: path_string(&dir.join(OUT_DIR_NAME)),
            path: path_string(&path),
            size,
            chats,
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WrittenFile {
    name: String,
    path: String,
    bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Exported {
    out_dir: String,
    files: Vec<WrittenFile>,
}

/// Pass 2: extract the selection, format it and write the `.md` files.
#[tauri::command]
async fn export_selection<R: Runtime>(
    app: AppHandle<R>,
    jobs: State<'_, Jobs>,
    path: String,
    selection: Vec<Selection>,
    out_dir: String,
    max_tokens: Option<usize>,
) -> Result<Exported, CmdError> {
    let cancel = jobs.start();
    run_blocking(move || {
        let units = extract_reader(
            open_tracked(&app, Path::new(&path), "extract", cancel)?,
            &selection,
        )?;
        let out_dir = PathBuf::from(out_dir);
        let written = write_units(&units, &out_dir, max_tokens.unwrap_or(DEFAULT_MAX_TOKENS))?;
        let files = written
            .iter()
            .map(|p| WrittenFile {
                name: p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: path_string(p),
                bytes: fs::metadata(p).map(|m| m.len()).unwrap_or(0),
            })
            .collect();
        Ok(Exported {
            out_dir: path_string(&out_dir),
            files,
        })
    })
    .await
}

/// Cancels the running pass.
#[tauri::command]
fn cancel_job(jobs: State<'_, Jobs>) {
    jobs.cancel();
}

/// The desktop theme to follow (the active Omarchy theme), if any.
#[tauri::command]
fn desktop_theme() -> Option<desktop::DesktopTheme> {
    current_theme()
}

fn current_theme() -> Option<desktop::DesktopTheme> {
    if cfg!(target_os = "linux") {
        desktop::omarchy_theme(|key| std::env::var(key).ok())
    } else {
        None
    }
}

/// Creates the main window from its config (`create: false` there), without
/// the system title bar on tiling compositors, and with the desktop theme
/// available to `ui/theme.js` before the first paint.
fn create_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let Some(config) = app.config().app.windows.iter().find(|w| w.label == "main") else {
        return Ok(());
    };
    let decorations = config.decorations && desktop::native_decorations(|k| std::env::var(k).ok());
    let theme = serde_json::to_string(&current_theme())?;
    WebviewWindowBuilder::from_config(app, config)?
        .decorations(decorations)
        .initialization_script(format!("window.__TGSUM_THEME__ = {theme};"))
        .build()?;
    Ok(())
}

/// Opens a folder in the system file manager.
#[tauri::command]
fn open_folder<R: Runtime>(app: AppHandle<R>, path: String) -> Result<(), CmdError> {
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| CmdError::Failed(e.to_string()))
}

/// Shows a file selected in the system file manager.
#[tauri::command]
fn reveal_file<R: Runtime>(app: AppHandle<R>, path: String) -> Result<(), CmdError> {
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| CmdError::Failed(e.to_string()))
}

/// Registers plugins, state and commands on `builder` (any runtime, so tests
/// can drive the commands on Tauri's mock runtime).
pub fn app<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Jobs::default())
        .setup(|app| {
            if app.try_state::<ProjectStore>().is_none() {
                app.manage(ProjectStore::new(
                    app.path().app_local_data_dir()?.join("projects"),
                ));
            }
            Ok(create_main_window(app.handle())?)
        })
        .invoke_handler(tauri::generate_handler![
            initial_path,
            desktop_theme,
            pick_export,
            pick_out_dir,
            index_export,
            export_selection,
            cancel_job,
            open_folder,
            reveal_file,
            create_project,
            list_projects,
            open_project,
            update_project,
            project_source_status,
        ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // WebKitGTK's DMA-BUF renderer shows a blank window on some Linux GPU
    // drivers (notably NVIDIA); the fallback renderer works everywhere.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    // `cargo install` builds add themselves to the app launcher. The
    // environment is read up front: GTK modifies it while starting up.
    #[cfg(target_os = "linux")]
    {
        let env: std::collections::HashMap<String, String> = std::env::vars_os()
            .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
            .collect();
        std::thread::spawn(move || {
            let synced = std::env::current_exe()
                .and_then(|exe| launcher::sync(|key| env.get(key).cloned(), &exe));
            if let Err(err) = synced {
                eprintln!("tgsum: could not update the launcher entry: {err}");
            }
        });
    }

    app(tauri::Builder::default())
        .run(tauri::generate_context!())
        .expect("failed to start tgsum");
}
