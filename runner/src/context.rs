use std::io::{self, Read};
use std::path::PathBuf;

use serde::Serialize;
use tempfile::TempDir;
use tgsum_core::bundle::BundleFile;
use tgsum_core::project::ProjectStore;

use crate::{Cancellation, RunnerError};

/// Only a reviewed core bundle can construct this capability. No public method
/// accepts an arbitrary directory, export archive or private Project root.
pub struct PreparedContext {
    _staging: TempDir,
    directory: PathBuf,
    files: Vec<BundleFile>,
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
        Ok(Self {
            _staging: staging,
            directory: exported.directory,
            files: exported.files,
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

    /// Inline only the immutable public export, never traverse the source tree.
    /// This is an adapter payload limit, independent of the streaming importer.
    pub(crate) fn documents(
        &self,
        max_bytes: usize,
        cancellation: &Cancellation,
    ) -> Result<Vec<PublicDocument>, RunnerError> {
        self.check_revision()?;
        let mut remaining = max_bytes;
        let mut documents = Vec::new();
        for name in
            std::iter::once("manifest.json").chain(self.files.iter().map(|file| file.name.as_str()))
        {
            if cancellation.is_cancelled() {
                return Err(RunnerError::Cancelled);
            }
            let path = self.directory.join(name);
            let metadata = std::fs::symlink_metadata(&path)?;
            if !metadata.is_file() || metadata.len() > remaining as u64 {
                return Err(RunnerError::InvalidRequest(
                    "selected context exceeds adapter input limit or is not a regular file",
                ));
            }
            let mut bytes = Vec::new();
            std::fs::File::open(path)?
                .take(remaining as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > remaining {
                return Err(RunnerError::InvalidRequest("adapter input limit exceeded"));
            }
            remaining -= bytes.len();
            let text = String::from_utf8(bytes)
                .map_err(|_| RunnerError::InvalidRequest("context is not UTF-8"))?;
            documents.push(PublicDocument {
                name: name.into(),
                text,
            });
        }
        if cancellation.is_cancelled() {
            return Err(RunnerError::Cancelled);
        }
        self.check_revision()?;
        Ok(documents)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn directory(&self) -> &std::path::Path {
        &self.directory
    }
}

#[derive(Serialize)]
pub(crate) struct PublicDocument {
    pub name: String,
    pub text: String,
}
