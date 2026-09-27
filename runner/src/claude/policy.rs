//! Snapshot the endpoint-managed files, preserving their native relative names.
//! This is policy input, not a claim that the policy is compatible with a run.
use crate::{Cancellation, RunnerError};
use rustix::fs::{openat, Dir, Mode, OFlags, CWD};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fmt,
    fs::File,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

const ROOT: &str = "/etc/claude-code";
const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 4 * MAX_FILE_BYTES;
const MAX_FILES: usize = 128;
const MAX_DIRECTORY_ENTRIES: usize = 1024;

/// A bounded snapshot of the system-managed files. Does not read a user profile,
/// discover credentials, run helpers, or certify enterprise policy compatibility.
pub struct EndpointPolicy {
    root: PathBuf,
    owner: u32,
    snapshot: Snapshot,
}

#[derive(Default, PartialEq, Eq)]
struct Snapshot {
    root_present: bool,
    fragments_present: bool,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl EndpointPolicy {
    /// The application captures the fixed Linux system location before Review.
    /// Absence is distinct from unreadable/unsafe policy. No arbitrary policy
    /// root can be selected by an imported conversation or public API.
    pub fn capture(cancel: &Cancellation) -> Result<Self, RunnerError> {
        Self::capture_at(Path::new(ROOT), 0, cancel)
    }

    fn capture_at(root: &Path, owner: u32, cancel: &Cancellation) -> Result<Self, RunnerError> {
        let policy = Self {
            root: root.to_owned(),
            owner,
            snapshot: read(root, owner, cancel)?,
        };
        policy.check_unchanged(cancel)?;
        Ok(policy)
    }

    /// Recheck at launch; removing/adding/editing an admin file invalidates the
    /// prepared snapshot. It never silently falls back to a policy-free run.
    pub fn check_unchanged(&self, cancel: &Cancellation) -> Result<(), RunnerError> {
        if read(&self.root, self.owner, cancel)? != self.snapshot {
            return Err(RunnerError::ExportOnly(
                "Claude managed settings changed; prepare and review again",
            ));
        }
        Ok(())
    }

    pub(crate) fn stage(
        &self,
        directory: &Path,
        cancel: &Cancellation,
    ) -> Result<Vec<(PathBuf, PathBuf)>, RunnerError> {
        use std::os::unix::fs::PermissionsExt;
        self.check_unchanged(cancel)?;
        // Refuse executable/receiver-changing endpoint settings before launch;
        // safe-mode alone is not a promise that managed helpers never run.
        for bytes in self.snapshot.files.values() {
            let text = std::str::from_utf8(bytes).map_err(io_error)?;
            let text = text.strip_prefix('\u{feff}').unwrap_or(text).trim();
            let super::wire::UniqueValue(value) =
                serde_json::from_str(if text.is_empty() { "{}" } else { text })
                    .map_err(io_error)?;
            if !super::preflight::compatible(&value) {
                return Err(RunnerError::ExportOnly(
                    "Claude managed settings require an unsupported execution profile",
                ));
            }
        }
        let mut files = self.snapshot.files.clone();
        // Native 2.1.280 orders fragments using JS Array.sort (UTF-16). Extend
        // the greatest existing name, so a scalar force=false in any original
        // fragment cannot weaken our added startup barrier. Originals survive.
        let last = files
            .keys()
            .filter(|p| p.parent() == Some(Path::new("managed-settings.d")))
            .filter_map(|p| p.file_name()?.to_str())
            .max_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        let name = match last {
            Some(last) => format!("{last}.tgsum.json"),
            None => "tgsum-required-policy.json".into(),
        };
        if name.len() > 255 {
            return Err(unsupported());
        }
        files.insert(
            Path::new("managed-settings.d").join(name),
            b"{\"forceRemoteSettingsRefresh\":true}".to_vec(),
        );
        let mut mounts = Vec::new();
        for (index, (name, bytes)) in files.into_iter().enumerate() {
            cancelled(cancel)?;
            let path = directory.join(format!("claude-policy-{index}"));
            std::fs::write(&path, bytes).map_err(io_error)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400))
                .map_err(io_error)?;
            mounts.push((path, Path::new(ROOT).join(name)));
        }
        self.check_unchanged(cancel)?;
        Ok(mounts)
    }

    #[cfg(test)]
    pub(crate) fn fixture(root: &Path, cancel: &Cancellation) -> Result<Self, RunnerError> {
        Self::capture_at(root, rustix::process::geteuid().as_raw(), cancel)
    }
}

impl fmt::Debug for EndpointPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EndpointPolicy")
            .field("files", &self.snapshot.files.len())
            .finish_non_exhaustive()
    }
}

fn read(root: &Path, owner: u32, cancel: &Cancellation) -> Result<Snapshot, RunnerError> {
    cancelled(cancel)?;
    let mut snapshot = Snapshot::default();
    let Some(directory) = open_optional(CWD, root, true)? else {
        return Ok(snapshot);
    };
    validate(&directory, owner, true)?;
    snapshot.root_present = true;
    for name in ["managed-settings.json", "managed-mcp.json"] {
        if let Some(file) = open_optional(&directory, Path::new(name), false)? {
            snapshot.insert(PathBuf::from(name), file, owner, cancel)?;
        }
    }
    if let Some(fragments) = open_optional(&directory, Path::new("managed-settings.d"), true)? {
        validate(&fragments, owner, true)?;
        snapshot.fragments_present = true;
        let entries = Dir::read_from(&fragments).map_err(io_error)?;
        for (index, entry) in entries.enumerate() {
            cancelled(cancel)?;
            if index >= MAX_DIRECTORY_ENTRIES {
                return Err(unsupported());
            }
            let entry = entry.map_err(io_error)?;
            use std::os::unix::ffi::OsStrExt;
            let name = Path::new(OsStr::from_bytes(entry.file_name().to_bytes()));
            if name.extension() != Some(OsStr::new("json"))
                || name.as_os_str().as_bytes().starts_with(b".")
            {
                continue;
            }
            // Each enumerated leaf is opened relative to the pinned directory,
            // without following a replaced directory or symlink out of it.
            let file = open_optional(&fragments, name, false)?.ok_or_else(unsupported)?;
            snapshot.insert(
                Path::new("managed-settings.d").join(name),
                file,
                owner,
                cancel,
            )?;
        }
    }
    cancelled(cancel)?;
    Ok(snapshot)
}

impl Snapshot {
    fn insert(
        &mut self,
        name: PathBuf,
        mut file: File,
        owner: u32,
        cancel: &Cancellation,
    ) -> Result<(), RunnerError> {
        if self.files.len() >= MAX_FILES || name.to_str().is_none() {
            return Err(unsupported());
        }
        let before = validate(&file, owner, false)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        cancelled(cancel)?;
        let after = validate(&file, owner, false)?;
        if before != after
            || bytes.len() > MAX_FILE_BYTES
            || bytes.len() != before.len as usize
            || self.files.values().map(Vec::len).sum::<usize>() + bytes.len() > MAX_TOTAL_BYTES
        {
            return Err(unsupported());
        }
        // Keep exact bytes. Parsing/compatibility belongs to the native cascade
        // and preflight, not a partial reimplementation of managed settings.
        self.files.insert(name, bytes);
        Ok(())
    }
}

fn open_optional(
    fd: impl std::os::fd::AsFd,
    path: &Path,
    directory: bool,
) -> Result<Option<File>, RunnerError> {
    let mut flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
    if directory {
        flags |= OFlags::DIRECTORY;
    }
    match openat(fd, path, flags, Mode::empty()) {
        Ok(file) => Ok(Some(file.into())),
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(_) => Err(unsupported()),
    }
}

#[derive(PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    len: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

fn validate(file: &File, owner: u32, directory: bool) -> Result<Stamp, RunnerError> {
    let m = file.metadata().map_err(io_error)?;
    if m.uid() != owner
        || m.mode() & 0o7022 != 0
        || (directory && !m.is_dir())
        || (!directory && (!m.is_file() || m.nlink() != 1 || m.len() > MAX_FILE_BYTES as u64))
    {
        return Err(unsupported());
    }
    Ok(Stamp {
        device: m.dev(),
        inode: m.ino(),
        len: m.len(),
        modified: (m.mtime(), m.mtime_nsec()),
        changed: (m.ctime(), m.ctime_nsec()),
    })
}

fn cancelled(cancel: &Cancellation) -> Result<(), RunnerError> {
    if cancel.is_cancelled() {
        Err(RunnerError::Cancelled)
    } else {
        Ok(())
    }
}
fn unsupported() -> RunnerError {
    RunnerError::ExportOnly(
        "Claude managed settings are unreadable, unsafe, changing or exceed limits",
    )
}
fn io_error(_: impl std::fmt::Debug) -> RunnerError {
    unsupported()
}

#[cfg(test)]
mod tests;
