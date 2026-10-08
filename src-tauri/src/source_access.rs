//! Metadata for the source UI, derived from the actual desktop import binding.
//! A selected conversation never establishes the grant of an API credential.
use serde::Serialize;
use tauri::State;
use tgsum_core::connector::{
    ArchiveImporter, AttachmentAccess, ConnectorKind, CredentialKind, RefreshMethod, TelegramJson,
};
use tgsum_core::project::{ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;

use crate::{run_blocking, CmdError};

/// Keep refresh execution and its advertised capabilities on the same binding.
pub(crate) fn importer(source: &ProjectSource) -> Option<TelegramJson> {
    let descriptor = TelegramJson.descriptor();
    (source.connector_id == descriptor.id && source.scope.platform == descriptor.platform)
        .then_some(TelegramJson)
}

#[derive(Serialize)]
pub(crate) struct ImportMethod {
    kind: ConnectorKind,
    credentials: CredentialKind,
    refresh: RefreshMethod,
    attachments: AttachmentAccess,
    /// Assisted means user-operated export and explicit completed-file import.
    assisted_export: bool,
}

#[derive(Serialize)]
pub(crate) struct SourceAccess {
    source_id: String,
    selected_scope: SourceScope,
    /// None means unverified/unimplemented; never infer rights from selection.
    method: Option<ImportMethod>,
    attachment_choices: usize,
    continuous: Option<ContinuousAccess>,
}

#[derive(Serialize)]
struct ContinuousAccess {
    enabled: bool,
    available: bool,
}

#[derive(Serialize)]
pub(crate) struct SourceAccesses {
    project_revision: u64,
    sources: Vec<SourceAccess>,
}

#[tauri::command]
pub(crate) async fn project_source_accesses(
    store: State<'_, ProjectStore>,
    project_id: String,
    expected_revision: u64,
) -> Result<SourceAccesses, CmdError> {
    let store = store.inner().clone();
    run_blocking(move || {
        let project = store.open(&project_id)?;
        if project.revision != expected_revision {
            return Err(CmdError::Conflict(
                "Проект изменён. Откройте его заново.".into(),
            ));
        }
        Ok(SourceAccesses {
            project_revision: project.revision,
            sources: project
                .sources
                .iter()
                .map(|source| {
                    let method = importer(source).map(|importer| {
                        let descriptor = importer.descriptor();
                        ImportMethod {
                            kind: descriptor.kind,
                            credentials: descriptor.capabilities.credentials,
                            refresh: descriptor.capabilities.refresh,
                            attachments: descriptor.capabilities.attachments,
                            assisted_export: true,
                        }
                    });
                    SourceAccess {
                        source_id: source.source_id.clone(),
                        selected_scope: source.scope.clone(),
                        method,
                        attachment_choices: source
                            .selection
                            .attachments
                            .as_ref()
                            .map_or(0, |s| s.files.len()),
                        continuous: project.telegram_continuous.get(&source.source_id).map(
                            |plan| ContinuousAccess {
                                enabled: plan.settings.enabled,
                                available: cfg!(target_os = "linux"),
                            },
                        ),
                    }
                })
                .collect(),
        })
    })
    .await
}
