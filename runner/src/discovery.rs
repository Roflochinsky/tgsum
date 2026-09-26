use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use crate::{AuthAvailability, RunnerError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableCandidate {
    pub path: PathBuf,
    pub authentication: AuthAvailability,
    pub version: Option<String>,
}

/// Search explicit directories only; do not execute candidates or inspect
/// agent settings/auth files. Symlinks are resolved to the actual executable.
pub fn discover(
    name: &OsStr,
    directories: &[PathBuf],
) -> Result<Vec<ExecutableCandidate>, RunnerError> {
    let path = Path::new(name);
    if !matches!(path.components().next(), Some(Component::Normal(_)))
        || path.components().count() != 1
    {
        return Err(RunnerError::InvalidRequest(
            "executable name must be a basename",
        ));
    }
    let mut found = BTreeSet::new();
    for directory in directories {
        if !directory.is_absolute() {
            return Err(RunnerError::InvalidRequest(
                "discovery directory must be absolute",
            ));
        }
        let Ok(path) = directory.join(name).canonicalize() else {
            continue;
        };
        let metadata = std::fs::metadata(&path)?;
        if !metadata.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        found.insert(path);
    }
    Ok(found
        .into_iter()
        .map(|path| ExecutableCandidate {
            path,
            authentication: AuthAvailability::Unknown,
            version: None,
        })
        .collect())
}
