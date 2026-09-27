//! Human-assisted export settings. Acquisition stays in the selected client;
//! these private references never belong in an agent's context bundle.

use std::fs::{self, File, Metadata};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::project::{Project, ProjectChange, ProjectStore};
use crate::snapshot::{Snapshot, SnapshotDiff};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportDelta {
    pub created: usize,
    pub edited: usize,
    pub deleted: usize,
    pub missing: usize,
    pub unchanged: usize,
}

#[derive(Debug, Serialize)]
pub struct CompletedImport {
    pub project: Project,
    pub delta: ImportDelta,
}

impl ImportDelta {
    fn from_snapshots(current: &Snapshot, previous: Option<&Snapshot>) -> io::Result<Self> {
        let diff = previous.map(|old| current.diff(old)).transpose()?;
        Ok(match diff {
            Some(SnapshotDiff {
                created,
                edited,
                deleted,
                missing,
                unchanged,
            }) => Self {
                created: created.len(),
                edited: edited.len(),
                deleted: deleted.len(),
                missing: missing.len(),
                unchanged,
            },
            None => Self {
                created: current.messages.len(),
                edited: 0,
                deleted: 0,
                missing: 0,
                unchanged: 0,
            },
        })
    }
}

impl ProjectStore {
    /// Stage a human-confirmed source in private temporary storage and compare
    /// its hash with a fresh read of the selected path before publication.
    /// Quiet filesystem observations alone never authorize this call.
    pub fn import_stable_assisted_export(
        &self,
        request: &AssistedImportRequest,
        mut on_progress: impl FnMut(u64, u64) -> io::Result<()>,
        is_cancelled: impl Fn() -> bool,
    ) -> io::Result<CompletedImport> {
        self.checked_assisted_project(request)?;
        let directory = self.directory(&request.project_id)?;
        let staged = stage_and_verify(
            &request.archive_path,
            &directory,
            &mut on_progress,
            &is_cancelled,
        )?;
        self.import_assisted_receipt(request, || Ok(staged), is_cancelled)
    }

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
        Ok(self
            .import_assisted_receipt(request, open, is_cancelled)?
            .project)
    }

    fn import_assisted_receipt<R: Read>(
        &self,
        request: &AssistedImportRequest,
        open: impl FnOnce() -> io::Result<R>,
        is_cancelled: impl Fn() -> bool,
    ) -> io::Result<CompletedImport> {
        let project = self.checked_assisted_project(request)?;
        let snapshots = self.snapshots(&project.project_id)?;
        let mut source = project
            .sources
            .iter()
            .find(|s| s.source_id == request.source_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "source disconnected"))?
            .clone();
        let previous = source
            .latest_snapshot_id
            .as_deref()
            .map(|id| snapshots.load(id))
            .transpose()?;
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
        let snapshot = snapshots.import_telegram(&id, &source.scope, reader)?;
        let delta = ImportDelta::from_snapshots(&snapshot, previous.as_ref())?;
        if is_cancelled() {
            return Err(crate::cancelled());
        }
        source.archive_path = Some(request.archive_path.clone());
        source.latest_snapshot_id = Some(id);
        let project = self.update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(source),
        )?;
        Ok(CompletedImport { project, delta })
    }

    fn checked_assisted_project(&self, request: &AssistedImportRequest) -> io::Result<Project> {
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
        Ok(project)
    }
}

fn stage_and_verify(
    path: &PathBuf,
    directory: &PathBuf,
    on_progress: &mut impl FnMut(u64, u64) -> io::Result<()>,
    is_cancelled: &impl Fn() -> bool,
) -> io::Result<File> {
    let before = fs::symlink_metadata(path)?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "export must be a regular file",
        ));
    }
    let mut source = File::open(path)?;
    if !same_metadata(&before, &source.metadata()?) {
        return Err(changed());
    }
    let mut staged = tempfile::tempfile_in(directory)?;
    let mut digest = Sha256::new();
    let mut copied = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    let total = before.len().saturating_mul(2);
    loop {
        if is_cancelled() {
            return Err(crate::cancelled());
        }
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        staged.write_all(&buffer[..read])?;
        digest.update(&buffer[..read]);
        copied = copied.saturating_add(read as u64);
        on_progress(copied, total)?;
    }
    if !same_metadata(&before, &source.metadata()?)
        || !same_metadata(&before, &fs::symlink_metadata(path)?)
        || copied != before.len()
    {
        return Err(changed());
    }
    staged.flush()?;
    staged.sync_all()?;
    let mut reread = File::open(path)?;
    if !same_metadata(&before, &reread.metadata()?) {
        return Err(changed());
    }
    let mut confirm = Sha256::new();
    let mut verified = 0u64;
    loop {
        if is_cancelled() {
            return Err(crate::cancelled());
        }
        let read = reread.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        confirm.update(&buffer[..read]);
        verified = verified.saturating_add(read as u64);
        on_progress(copied.saturating_add(verified), total)?;
    }
    if verified != copied
        || !same_metadata(&before, &reread.metadata()?)
        || !same_metadata(&before, &fs::symlink_metadata(path)?)
        || digest.finalize() != confirm.finalize()
    {
        return Err(changed());
    }
    if is_cancelled() {
        return Err(crate::cancelled());
    }
    staged.seek(SeekFrom::Start(0))?;
    Ok(staged)
}

fn same_metadata(a: &Metadata, b: &Metadata) -> bool {
    a.is_file() && b.is_file() && a.len() == b.len() && a.modified().ok() == b.modified().ok()
}

fn changed() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "export changed while being copied or verified",
    )
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
