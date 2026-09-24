//! tgsum desktop app: a Tauri shell around `tgsum-core`.
//!
//! The UI (`ui/`) calls these commands through `window.__TAURI__.core.invoke`.
//! Both passes run on a blocking thread, stream the export from disk, emit
//! throttled `progress` events and stop early when the user cancels.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Builder, Emitter, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tgsum_core::{
    cancelled, extract_reader, index_reader, is_cancelled, resolve_export_path, write_units,
    ChatIndex, ProgressReader, Selection, DEFAULT_MAX_TOKENS,
};

/// Default output folder, created next to the export.
const OUT_DIR_NAME: &str = "tgsum-output";

/// Minimum gap between two `progress` events.
const PROGRESS_EVERY: Duration = Duration::from_millis(80);

/// Error returned to the UI: `{ kind: "cancelled" }` or
/// `{ kind: "failed", message }`.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
enum CmdError {
    Cancelled,
    Failed(String),
}

impl From<io::Error> for CmdError {
    fn from(e: io::Error) -> Self {
        if is_cancelled(&e) {
            CmdError::Cancelled
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
        .invoke_handler(tauri::generate_handler![
            initial_path,
            pick_export,
            pick_out_dir,
            index_export,
            export_selection,
            cancel_job,
            open_folder,
            reveal_file,
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

    app(tauri::Builder::default())
        .run(tauri::generate_context!())
        .expect("failed to start tgsum");
}
