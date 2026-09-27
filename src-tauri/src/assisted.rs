//! Explicit local client launch and human-confirmed archive handoff. No client
//! discovery, profile access, export automation or completion inference.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use tgsum_core::assisted::{AssistedImportRequest, CompletedImport};
use tgsum_core::export_inbox::{ExportCandidate, ExportInbox};
use tgsum_core::project::{ProjectEntry, ProjectStore};

use crate::{analysis, run_blocking, CmdError, Jobs, Progress, PROGRESS_EVERY};

#[derive(Default)]
pub(crate) struct ExportInboxState(pub(crate) Arc<Mutex<ExportInbox>>);

#[derive(Clone, Serialize)]
pub(crate) struct CandidateNotice {
    project_id: String,
    project_name: String,
    source_id: String,
    count: usize,
}

/// Keep scanning configured Project folders while the desktop application is
/// running, including when the source card is closed. Events are hints only;
/// the import command still requires explicit completion confirmation.
pub(crate) fn start_background_watcher<R: Runtime>(
    app: AppHandle<R>,
    store: ProjectStore,
    inbox: Arc<Mutex<ExportInbox>>,
    stop: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let mut announced = HashMap::<(String, String, PathBuf), (u64, Option<String>)>::new();
        while !stop.load(Ordering::Relaxed) {
            let mut active = HashSet::new();
            if let Ok(projects) = store.list() {
                for entry in projects {
                    let ProjectEntry::Ready { project } = entry else {
                        continue;
                    };
                    for source_id in project.assisted_exports.keys() {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        let candidates = match inbox.lock() {
                            Ok(mut watcher) => watcher.poll(&project, source_id, Instant::now()),
                            Err(_) => return,
                        };
                        let Ok(candidates) = candidates else { continue };
                        let mut fresh = 0;
                        for candidate in candidates {
                            let key = (
                                project.project_id.clone(),
                                source_id.clone(),
                                candidate.path,
                            );
                            active.insert(key.clone());
                            let fingerprint = (candidate.bytes, candidate.modified_unix_ns);
                            if announced.get(&key) != Some(&fingerprint) {
                                announced.insert(key, fingerprint);
                                fresh += 1;
                            }
                        }
                        if fresh > 0 {
                            let _ = app.emit(
                                "assisted-export-candidate",
                                CandidateNotice {
                                    project_id: project.project_id.clone(),
                                    project_name: project.name.clone(),
                                    source_id: source_id.clone(),
                                    count: fresh,
                                },
                            );
                        }
                    }
                }
            }
            announced.retain(|key, _| active.contains(key));
            for _ in 0..30 {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
}

#[derive(Serialize)]
pub(crate) struct LaunchReceipt {
    state: &'static str,
}

#[tauri::command]
pub(crate) async fn pick_assisted_path<R: Runtime>(
    app: AppHandle<R>,
    kind: String,
) -> Result<Option<PathBuf>, CmdError> {
    let picked = match kind.as_str() {
        "directory" => app
            .dialog()
            .file()
            .set_title("Папка для экспорта Telegram Desktop")
            .blocking_pick_folder(),
        "client" => app
            .dialog()
            .file()
            .set_title("Исполняемый файл Telegram Desktop")
            .blocking_pick_file(),
        "archive" => app
            .dialog()
            .file()
            .set_title("JSON завершённого экспорта")
            .add_filter("Telegram JSON", &["json"])
            .blocking_pick_file(),
        _ => return Err(CmdError::Failed("Неизвестный тип файла".into())),
    };
    picked
        .map(|p| {
            let path = p.into_path().map_err(|e| CmdError::Failed(e.to_string()))?;
            Ok(path.canonicalize()?)
        })
        .transpose()
}

#[tauri::command]
pub(crate) async fn launch_assisted_client(
    store: State<'_, ProjectStore>,
    project_id: String,
    source_id: String,
    expected_revision: u64,
) -> Result<LaunchReceipt, CmdError> {
    let store = store.inner().clone();
    run_blocking(move || {
        let project = store.open(&project_id)?;
        if project.revision != expected_revision {
            return Err(CmdError::Conflict(
                "Проект изменён. Откройте его заново.".into(),
            ));
        }
        let client = project
            .assisted_exports
            .get(&source_id)
            .and_then(|s| s.client.as_ref())
            .ok_or_else(|| {
                CmdError::Failed(
                    "Выберите исполняемый файл или откройте Telegram Desktop самостоятельно."
                        .into(),
                )
            })?;
        launch_native(client)?;
        // spawn proves neither successful initialization nor export. Do not
        // wait for or kill the user's GUI process when TGSUM cancels/closes.
        Ok(LaunchReceipt {
            state: "needs_user_action",
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn poll_assisted_exports(
    store: State<'_, ProjectStore>,
    inbox: State<'_, ExportInboxState>,
    project_id: String,
    source_id: String,
    expected_revision: u64,
) -> Result<Vec<ExportCandidate>, CmdError> {
    let store = store.inner().clone();
    let inbox = Arc::clone(&inbox.inner().0);
    run_blocking(move || {
        let project = store.open(&project_id)?;
        if project.revision != expected_revision {
            return Err(CmdError::Conflict(
                "Проект изменён. Откройте его заново.".into(),
            ));
        }
        let mut watcher = inbox.lock().map_err(|e| CmdError::Failed(e.to_string()))?;
        Ok(watcher.poll(&project, &source_id, Instant::now())?)
    })
    .await
}

fn launch_native(path: &Path) -> io::Result<()> {
    let unsupported = || {
        io::Error::new(io::ErrorKind::InvalidInput,
        "Нужен исполняемый файл Desktop. Пакет приложения или ярлык откройте самостоятельно; затем выберите готовый JSON.")
    };
    if !path.is_absolute() || !path.is_file() {
        return Err(unsupported());
    }
    let mut magic = [0u8; 4];
    File::open(path)?.read_exact(&mut magic)?;
    let native = if cfg!(target_os = "linux") {
        magic == *b"\x7fELF"
    } else if cfg!(target_os = "windows") {
        magic.starts_with(b"MZ")
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    } else if cfg!(target_os = "macos") {
        matches!(
            magic,
            [0xfe, 0xed, 0xfa, 0xce | 0xcf]
                | [0xce | 0xcf, 0xfa, 0xed, 0xfe]
                | [0xca, 0xfe, 0xba, 0xbe | 0xbf]
                | [0xbe | 0xbf, 0xba, 0xfe, 0xca]
        )
    } else {
        false
    };
    if !native {
        return Err(unsupported());
    }
    // Header/type checks reject shell/batch/desktop shortcuts; they do not
    // authenticate a publisher. No shell, PATH search or Telegram arguments.
    let mut child = Command::new(path)
        .current_dir(path.parent().ok_or_else(unsupported)?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[tauri::command]
pub(crate) async fn import_assisted_export<R: Runtime>(
    analysis: State<'_, analysis::AnalysisState>,
    app: AppHandle<R>,
    jobs: State<'_, Jobs>,
    store: State<'_, ProjectStore>,
    request: AssistedImportRequest,
) -> Result<CompletedImport, CmdError> {
    analysis.before_edit(&store, &request.project_id)?;
    let store = store.inner().clone();
    let cancel = jobs.start();
    run_blocking(move || {
        let mut last: Option<Instant> = None;
        let progress_cancel = Arc::clone(&cancel);
        Ok(store.import_stable_assisted_export(
            &request,
            |read, total| {
                if progress_cancel.load(Ordering::Relaxed) {
                    return Err(tgsum_core::cancelled());
                }
                if read == total || last.is_none_or(|at| at.elapsed() >= PROGRESS_EVERY) {
                    last = Some(Instant::now());
                    let _ = app.emit(
                        "progress",
                        Progress {
                            phase: "import",
                            read,
                            total,
                        },
                    );
                }
                Ok(())
            },
            || cancel.load(Ordering::Relaxed),
        )?)
    })
    .await
}
