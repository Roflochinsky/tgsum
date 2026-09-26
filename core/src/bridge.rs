//! Offline export lifecycle shared by assisted export and future client drivers.
//!
//! This module does not launch or automate a messenger. A driver must observe
//! the official client's completion signal before sending `Completed`; file
//! existence, a quiet watcher, and valid JSON alone do not prove completion.
//! The driver owns UI interaction; this module owns request correlation, state
//! transitions, validation, and snapshot publication. Tests use a simulated
//! client. No native OS automation has been qualified by this implementation.

use std::fs::File;
use std::io;
use std::path::PathBuf;

use crate::connector::{ArchiveImporter, ConnectorDescriptor, TelegramJson};
use crate::snapshot::{validate_snapshot_id, Snapshot, SnapshotStore, SourceScope};

/// Every attempt needs a fresh run ID and an explicitly chosen archive path.
/// The account namespace is configured locally, not authenticated by an export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    pub run_id: String,
    pub source: SourceScope,
    pub archive_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportState {
    WaitingForClient,
    NeedsUserAction(String),
    Exporting,
    Ready,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientStatus {
    NeedsUserAction(String),
    Exporting,
    Completed,
    Failed(String),
    Cancelled,
}

/// A driver echoes the exact request it is completing. A delayed callback from
/// an old attempt, another source, or another output path cannot publish here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientUpdate {
    pub request: ExportRequest,
    pub status: ClientStatus,
}

pub struct ExportJob {
    request: ExportRequest,
    state: ExportState,
    importer: ConnectorDescriptor,
}

impl ExportJob {
    pub fn new(request: ExportRequest) -> io::Result<Self> {
        Self::for_importer(request, &TelegramJson)
    }

    pub fn for_importer(
        request: ExportRequest,
        importer: &dyn ArchiveImporter,
    ) -> io::Result<Self> {
        request.source.validate()?;
        validate_snapshot_id(&request.run_id)?;
        let importer = importer.descriptor();
        if request.source.platform != importer.platform || !request.archive_path.is_absolute() {
            return Err(invalid(
                "export job requires a matching importer and an explicit absolute archive path",
            ));
        }
        Ok(Self {
            request,
            state: ExportState::WaitingForClient,
            importer,
        })
    }

    pub fn request(&self) -> &ExportRequest {
        &self.request
    }
    pub fn state(&self) -> &ExportState {
        &self.state
    }

    /// Apply an observed client event. `Completed` is synchronous and reads
    /// the configured file exactly once through the snapshot importer. The
    /// desktop host should run this on its blocking worker, not the UI thread.
    /// Import errors mark the attempt failed; retry creates a new job/run ID.
    /// Incorrect or late events return an error without changing this job.
    pub fn apply(
        &mut self,
        update: ClientUpdate,
        store: &SnapshotStore,
    ) -> io::Result<Option<Snapshot>> {
        self.apply_with_importer(update, store, &TelegramJson)
    }

    pub fn apply_with_importer(
        &mut self,
        update: ClientUpdate,
        store: &SnapshotStore,
        importer: &dyn ArchiveImporter,
    ) -> io::Result<Option<Snapshot>> {
        if importer.descriptor() != self.importer {
            return Err(invalid("export callback importer does not match this job"));
        }
        if update.request != self.request {
            return Err(invalid("export callback does not match this request"));
        }
        if matches!(
            self.state,
            ExportState::Ready | ExportState::Failed(_) | ExportState::Cancelled
        ) {
            return Err(invalid(
                "export attempt is already terminal; create a new attempt to retry",
            ));
        }
        match update.status {
            ClientStatus::NeedsUserAction(reason) => {
                self.state = ExportState::NeedsUserAction(reason)
            }
            ClientStatus::Exporting => self.state = ExportState::Exporting,
            ClientStatus::Failed(reason) => self.state = ExportState::Failed(reason),
            ClientStatus::Cancelled => self.state = ExportState::Cancelled,
            ClientStatus::Completed => {
                if self.state != ExportState::Exporting {
                    return Err(invalid("completion requires an active export"));
                }
                let result = File::open(&self.request.archive_path).and_then(|mut file| {
                    store.import(
                        importer,
                        &self.request.run_id,
                        &self.request.source,
                        &mut file,
                    )
                });
                match result {
                    Ok(snapshot) => {
                        self.state = ExportState::Ready;
                        return Ok(Some(snapshot));
                    }
                    Err(error) => {
                        self.state = ExportState::Failed(error.to_string());
                        return Err(error);
                    }
                }
            }
        }
        Ok(None)
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
