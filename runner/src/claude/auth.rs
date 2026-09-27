//! Explicit agent credential selection; no discovery, parsing or copying.
use crate::{auth_file::PinnedAuthFile, RunnerError};
use std::{fmt, os::fd::BorrowedFd, path::Path};

pub(crate) const AUTH_HOME: &str = "/home/agent/.claude";
pub(crate) const AUTH_PATH: &str = "/home/agent/.claude/.credentials.json";

/// Pins a selected private file, not a verified account. Only the sandboxed
/// agent may read it. O_PATH pins the inode, not immutable bytes. Remote token
/// refresh must be controlled separately; a read-only file cannot prevent it.
pub struct SelectedAuthFile(PinnedAuthFile);
impl SelectedAuthFile {
    pub fn select(path: &Path) -> Result<Self, RunnerError> {
        PinnedAuthFile::select(path, ".credentials.json").map(Self)
    }
    pub(crate) fn validate(&self) -> Result<(), RunnerError> {
        self.0.validate()
    }
    pub(crate) fn mount_fd(&self) -> BorrowedFd<'_> {
        self.0.mount_fd()
    }
}
impl fmt::Debug for SelectedAuthFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectedAuthFile").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    #[test]
    fn provider_filename_is_bound_and_debug_has_no_path_or_content() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["auth.json", ".credentials.json"] {
            let path = dir.path().join(name);
            fs::write(&path, "SYNTHETIC_AUTH").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            let selected = SelectedAuthFile::select(&path);
            if name == ".credentials.json" {
                assert_eq!(
                    format!("{:?}", selected.unwrap()),
                    "SelectedAuthFile { .. }"
                );
            } else {
                assert!(selected.is_err());
            }
        }
    }
}
