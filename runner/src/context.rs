use std::io;
use std::path::PathBuf;

use tempfile::TempDir;
use tgsum_core::project::ProjectStore;

use crate::{Cancellation, RunnerError};

/// Only a reviewed core bundle can construct this capability. No public method
/// accepts an arbitrary directory, export archive or private Project root.
pub struct PreparedContext {
    _staging: TempDir,
    #[cfg(target_os = "linux")]
    directory: PathBuf,
    store_root: PathBuf,
    project_id: String,
    revision: u64,
}

impl PreparedContext {
    pub fn from_bundle(
        store_root: PathBuf,
        project_id: &str,
        bundle_id: &str,
        expected_revision: u64,
        cancellation: &Cancellation,
    ) -> Result<Self, RunnerError> {
        if cancellation.is_cancelled() {
            return Err(RunnerError::Cancelled);
        }
        let store_root = std::fs::canonicalize(store_root)?;
        let store = ProjectStore::new(&store_root);
        let staging = tempfile::Builder::new().prefix("tgsum-run-").tempdir()?;
        let exported = store.export_bundle(
            project_id,
            bundle_id,
            expected_revision,
            staging.path(),
            || cancellation.is_cancelled(),
        )?;
        // The root TempDir owns the exported subdirectory too.
        #[cfg(not(target_os = "linux"))]
        let _ = exported;
        Ok(Self {
            _staging: staging,
            #[cfg(target_os = "linux")]
            directory: exported.directory,
            store_root,
            project_id: project_id.into(),
            revision: expected_revision,
        })
    }

    pub(crate) fn check_revision(&self) -> Result<(), RunnerError> {
        if ProjectStore::new(&self.store_root)
            .open(&self.project_id)?
            .revision
            != self.revision
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Project changed after context preparation; prepare and review again",
            )
            .into());
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn directory(&self) -> &std::path::Path {
        &self.directory
    }
}
