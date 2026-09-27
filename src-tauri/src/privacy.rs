//! Local privacy controls and explicit original-data preview; no agent launch.
use crate::{run_blocking, CmdError, Jobs};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use tgsum_core::{
    attachments::AttachmentCatalog,
    bundle::{EvidencePreview, EvidenceRef, ReviewItems},
    project::ProjectStore,
};

#[tauri::command]
pub(crate) fn privacy_presets() -> Vec<serde_json::Value> {
    tgsum_core::privacy::PrivacyPreset::ALL
        .into_iter()
        .map(|preset| {
            serde_json::json!({
                "id":preset.id(),"options":preset.options()
            })
        })
        .collect()
}

#[tauri::command]
pub(crate) async fn attachment_catalog(
    jobs: State<'_, Jobs>,
    store: State<'_, ProjectStore>,
    project_id: String,
    expected_revision: u64,
    source_id: String,
    offset: usize,
) -> Result<AttachmentCatalog, CmdError> {
    let store = store.inner().clone();
    let cancel = jobs.start();
    run_blocking(move || {
        Ok(
            store.attachment_catalog(&project_id, expected_revision, &source_id, offset, || {
                cancel.load(Ordering::Relaxed)
            })?,
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn review_items(
    jobs: State<'_, Jobs>,
    store: State<'_, ProjectStore>,
    project_id: String,
    bundle_id: String,
    expected_revision: u64,
    offset: usize,
) -> Result<ReviewItems, CmdError> {
    let store = store.inner().clone();
    let cancel = jobs.start();
    run_blocking(move || {
        Ok(
            store.review_items(&project_id, &bundle_id, expected_revision, offset, || {
                cancel.load(Ordering::Relaxed)
            })?,
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn preview_evidence(
    jobs: State<'_, Jobs>,
    store: State<'_, ProjectStore>,
    project_id: String,
    bundle_id: String,
    expected_revision: u64,
    reference: EvidenceRef,
) -> Result<EvidencePreview, CmdError> {
    let store = store.inner().clone();
    let cancel = jobs.start();
    run_blocking(move || {
        Ok(store.preview_evidence(
            &project_id,
            &bundle_id,
            expected_revision,
            &reference,
            || cancel.load(Ordering::Relaxed),
        )?)
    })
    .await
}

#[tauri::command]
pub(crate) async fn pick_attachment_root<R: Runtime>(
    app: AppHandle<R>,
    current: Option<String>,
) -> Result<Option<PathBuf>, CmdError> {
    let mut dialog = app.dialog().file().set_title("Папка с файлами экспорта");
    if let Some(current) = current.map(PathBuf::from) {
        if let Some(existing) = current
            .ancestors()
            .find(|p| p.is_dir())
            .map(Path::to_path_buf)
        {
            dialog = dialog.set_directory(existing);
        }
    }
    // Resolve the user's explicit choice and display/store that target. Later
    // attachment reads still walk all components with no-follow capabilities.
    dialog
        .blocking_pick_folder()
        .map(|file| {
            let path = file
                .into_path()
                .map_err(|_| CmdError::Failed("Выберите локальную папку экспорта".into()))?;
            std::fs::canonicalize(path)
                .map_err(|_| CmdError::Failed("Выбранная папка недоступна".into()))
        })
        .transpose()
}
