//! Explicit credential capability. The host pins an inode without opening its
//! contents; only the selected agent reads it through a read-only mount.
use std::fmt;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use crate::RunnerError;

const MAX_AUTH_BYTES: u64 = 64 * 1024;

/// An explicitly selected private regular file, not an authenticated account.
/// No discovery, content parsing, copying, refresh, or keyring access occurs.
/// O_PATH pins the inode, not its bytes against concurrent host writes. Select
/// again for each run. This file handle grants no network or launch permission.
pub(crate) struct PinnedAuthFile {
    file: File,
}

impl PinnedAuthFile {
    pub(crate) fn select(path: &Path, name: &str) -> Result<Self, RunnerError> {
        if !path.is_absolute() || path.file_name().is_none_or(|n| n != name) {
            return Err(RunnerError::InvalidRequest(
                "select an absolute auth path with the required filename",
            ));
        }
        // O_PATH ignores access flags and cannot read/write file content.
        // NOFOLLOW pins a final symlink itself, then metadata rejects it.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        let auth = Self { file };
        auth.validate()?;
        Ok(auth)
    }

    pub(crate) fn validate(&self) -> Result<(), RunnerError> {
        let metadata = self.file.metadata()?;
        if !metadata.is_file()
            || self.file.as_raw_fd() < 3
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.nlink() != 1
            || metadata.mode() & 0o7077 != 0
            || metadata.mode() & 0o400 == 0
            || metadata.len() == 0
            || metadata.len() > MAX_AUTH_BYTES
        {
            return Err(RunnerError::InvalidRequest(
                "Agent auth must be a bounded private owner-readable regular file",
            ));
        }
        Ok(())
    }

    pub(crate) fn mount_fd(&self) -> BorrowedFd<'_> {
        // Pass only to bwrap --ro-bind-fd. Reviewed 0.12.0 verifies mounted
        // dev/inode identity and closes this descriptor before payload exec.
        self.file.as_fd()
    }
}

impl fmt::Debug for PinnedAuthFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PinnedAuthFile").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Read;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::PathBuf;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        fs::write(&path, "synthetic auth bytes").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        (dir, path)
    }

    #[test]
    fn auth_selection_has_no_read_capability_and_pins_inode_without_copy() {
        let (dir, path) = fixture();
        let mut selected = PinnedAuthFile::select(&path, "auth.json").unwrap();
        let original = selected.file.metadata().unwrap();
        assert_eq!(
            selected.file.read(&mut [0]).unwrap_err().raw_os_error(),
            Some(libc::EBADF)
        );
        fs::rename(&path, dir.path().join("old.json")).unwrap();
        fs::write(&path, "replacement").unwrap();
        selected.validate().unwrap();
        let pinned = selected.file.metadata().unwrap();
        assert_eq!(
            (pinned.dev(), pinned.ino()),
            (original.dev(), original.ino())
        );
        assert_ne!(fs::metadata(&path).unwrap().ino(), original.ino());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        assert_eq!(format!("{selected:?}"), "PinnedAuthFile { .. }");
    }

    #[test]
    fn auth_selection_rejects_unsafe_paths_types_permissions_and_sizes() {
        let (dir, path) = fixture();
        for mode in [0o644, 0o660, 0o000, 0o4600] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            assert!(PinnedAuthFile::select(&path, "auth.json").is_err());
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let other = dir.path().join("other");
        fs::hard_link(&path, &other).unwrap();
        assert!(PinnedAuthFile::select(&path, "auth.json").is_err());
        fs::remove_file(&other).unwrap();
        fs::rename(&path, &other).unwrap();
        symlink(&other, &path).unwrap();
        assert!(PinnedAuthFile::select(&path, "auth.json").is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(PinnedAuthFile::select(&path, "auth.json").is_err());
        fs::remove_dir(&path).unwrap();
        File::create(&path).unwrap();
        assert!(PinnedAuthFile::select(&path, "auth.json").is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        File::create(&path)
            .unwrap()
            .set_len(MAX_AUTH_BYTES + 1)
            .unwrap();
        assert!(PinnedAuthFile::select(&path, "auth.json").is_err());
        fs::write(&path, "synthetic").unwrap();
        let selected = PinnedAuthFile::select(&path, "auth.json").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(selected.validate().is_err());
        fs::remove_file(&path).unwrap();
        let _socket = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert!(PinnedAuthFile::select(&path, "auth.json").is_err());
        assert!(PinnedAuthFile::select(Path::new("auth.json"), "auth.json").is_err());
        assert!(PinnedAuthFile::select(&other, "auth.json").is_err());
    }
}
