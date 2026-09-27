use super::{busy, invalid, Automation};
use crate::project::ProjectStore;
use fs4::{FileExt, TryLockError};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;

const LIMIT: u64 = 128 * 1024;

fn directory(store: &ProjectStore, id: &str) -> io::Result<PathBuf> {
    let parent = store.directory(id)?;
    if !fs::symlink_metadata(&parent)?.is_dir() {
        return Err(invalid("automation Project directory is not regular"));
    }
    Ok(parent.join("automation"))
}
fn create_root(store: &ProjectStore, id: &str) -> io::Result<PathBuf> {
    let root = directory(store, id)?;
    match fs::create_dir(&root) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
            }
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    if !fs::symlink_metadata(&root)?.is_dir() {
        return Err(invalid("automation directory is not regular"));
    }
    Ok(root)
}

pub(super) fn read(store: &ProjectStore, id: &str) -> io::Result<Option<Automation>> {
    let root = directory(store, id)?;
    match fs::symlink_metadata(&root) {
        Ok(meta) if meta.is_dir() => (),
        Ok(_) => return Err(invalid("automation directory is not regular")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    }
    let mut latest = None;
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        let revision = stem
            .parse::<u64>()
            .map_err(|_| invalid("invalid automation revision file"))?;
        if stem != format!("{revision:020}") {
            return Err(invalid("invalid automation revision name"));
        }
        if latest.as_ref().is_none_or(|(old, _)| revision > *old) {
            latest = Some((revision, entry.path()));
        }
    }
    let Some((revision, path)) = latest else {
        return Ok(None);
    };
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return Err(invalid("invalid automation record file"));
    }
    let mut bytes = Vec::new();
    File::open(&path)?.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(invalid("automation record too large"));
    }
    let value: Automation =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid automation record JSON"))?;
    value.validate()?;
    if value.project_id != id || value.revision != revision {
        return Err(invalid("automation identity mismatch"));
    }
    Ok(Some(value))
}
pub(super) fn write(store: &ProjectStore, value: &Automation) -> io::Result<()> {
    value.validate()?;
    let root = create_root(store, &value.project_id)?;
    let bytes = serde_json::to_vec(value).map_err(invalid)?;
    if bytes.len() as u64 > LIMIT {
        return Err(invalid("automation record too large"));
    }
    let mut staged = tempfile::NamedTempFile::new_in(&root)?;
    staged.write_all(&bytes)?;
    staged.as_file().sync_all()?;
    staged
        .persist_noclobber(root.join(format!("{:020}.json", value.revision)))
        .map_err(|e| {
            if e.error.kind() == io::ErrorKind::AlreadyExists {
                busy()
            } else {
                e.error
            }
        })?;
    Ok(())
}

pub(super) struct Lease(File);
impl Lease {
    pub(super) fn acquire(store: &ProjectStore, id: &str) -> io::Result<Option<Self>> {
        let root = create_root(store, id)?;
        let path = root.join("lease");
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => (),
            Ok(_) => return Err(invalid("automation lease must be a regular file")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        match FileExt::try_lock(&file) {
            Ok(()) => Ok(Some(Self(file))),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}
