//! Explicit local client launch and human-confirmed archive handoff. No client
//! discovery, profile access, export automation or completion inference.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use tgsum_core::assisted::AssistedImportRequest;
use tgsum_core::project::{Project, ProjectStore};

use crate::{analysis, open_tracked, run_blocking, CmdError, Jobs};

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
) -> Result<Project, CmdError> {
    analysis.before_edit(&store, &request.project_id)?;
    let store = store.inner().clone();
    let cancel = jobs.start();
    run_blocking(move || {
        Ok(store.import_assisted_export(
            &request,
            || open_tracked(&app, &request.archive_path, "import", cancel.clone()),
            || cancel.load(Ordering::Relaxed),
        )?)
    })
    .await
}
