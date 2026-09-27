//! Human-assisted export settings. Acquisition stays in the selected client;
//! these private references never belong in an agent's context bundle.

use std::io::{self, Read};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::project::{Project, ProjectChange, ProjectStore};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistedExportSettings {
    /// Destination to choose in Desktop. A nonempty folder may receive a new
    /// ChatExport_* subdirectory, so this is not a completion/file locator.
    pub directory: PathBuf,
    /// Explicitly chosen executable (or macOS app bundle); no discovery.
    pub client: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssistedImportRequest {
    pub project_id: String,
    pub expected_revision: u64,
    pub source_id: String,
    /// File explicitly selected after Desktop reports completion. It may be
    /// outside the planned directory (manual fallback / moved archive).
    pub archive_path: PathBuf,
    pub scope_and_completion_confirmed: bool,
}

impl ProjectStore {
    /// Parses a human-confirmed completed export into the existing selected
    /// namespace, then publishes its file reference and snapshot together.
    /// A conflict/cancel can leave an unreferenced immutable snapshot; it never
    /// advances the Project or the analysis baseline. The reader is opened only
    /// after confirmation, source/configuration and revision checks.
    pub fn import_assisted_export<R: Read>(
        &self,
        request: &AssistedImportRequest,
        open: impl FnOnce() -> io::Result<R>,
        is_cancelled: impl Fn() -> bool,
    ) -> io::Result<Project> {
        if !request.scope_and_completion_confirmed || !request.archive_path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "confirm the selected conversation and completed export, and select its JSON file",
            ));
        }
        let project = self.open(&request.project_id)?;
        if project.revision != request.expected_revision {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Project changed; reload before importing",
            ));
        }
        if !project.assisted_exports.contains_key(&request.source_id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "configure assisted export for this source first",
            ));
        }
        let mut source = project
            .sources
            .iter()
            .find(|s| s.source_id == request.source_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "source disconnected"))?
            .clone();
        if is_cancelled() {
            return Err(crate::cancelled());
        }
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
        let id = format!("assisted-{:032x}", u128::from_be_bytes(bytes));
        let reader = crate::ProgressReader::new(open()?, |_| {
            if is_cancelled() {
                Err(crate::cancelled())
            } else {
                Ok(())
            }
        });
        self.snapshots(&project.project_id)?
            .import_telegram(&id, &source.scope, reader)?;
        if is_cancelled() {
            return Err(crate::cancelled());
        }
        source.archive_path = Some(request.archive_path.clone());
        source.latest_snapshot_id = Some(id);
        self.update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(source),
        )
    }
}

impl AssistedExportSettings {
    pub(crate) fn validate(&self) -> io::Result<()> {
        if !self.directory.is_absolute() || self.client.as_ref().is_some_and(|p| !p.is_absolute()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "assisted export paths must be absolute",
            ));
        }
        Ok(())
    }
}
