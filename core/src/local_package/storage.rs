use super::{invalid, PackageState};
use crate::{attachments::ArchiveFiles, project::ProjectStore};
use fs4::{FileExt, TryLockError};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::PathBuf;

fn root(store: &ProjectStore, id: &str) -> io::Result<PathBuf> {
    let path = store.directory(id)?.join("local-package");
    match fs::create_dir(&path) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
            }
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    ArchiveFiles::open(&path)?;
    Ok(path)
}

pub(super) fn read(store: &ProjectStore, id: &str) -> io::Result<Option<PackageState>> {
    let path = root(store, id)?;
    let mut file = match ArchiveFiles::open(&path)?.open_regular(std::path::Path::new("state.json"))
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    (&mut file).take(128 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 {
        return Err(invalid("package state exceeds limit"));
    }
    let mut value: PackageState = serde_json::from_slice(&bytes).map_err(invalid)?;
    if !matches!(value.schema_version, 1 | 2) || value.project_id != id {
        return Err(invalid("invalid package state identity"));
    }
    value.schema_version = 2;
    Ok(Some(value))
}

pub(super) fn write(store: &ProjectStore, value: &PackageState) -> io::Result<()> {
    let root = root(store, &value.project_id)?;
    let mut staged = tempfile::NamedTempFile::new_in(&root)?;
    let bytes = serde_json::to_vec(value).map_err(invalid)?;
    if bytes.len() > 128 * 1024 {
        return Err(invalid("package state exceeds limit"));
    }
    staged.write_all(&bytes)?;
    staged.as_file().sync_all()?;
    staged
        .persist(root.join("state.json"))
        .map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(root)?.sync_all()?;
    Ok(())
}

pub(super) struct Lease(File);
impl Lease {
    pub(super) fn acquire(store: &ProjectStore, id: &str) -> io::Result<Self> {
        let root = root(store, id)?;
        match File::create_new(root.join("lease")) {
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e),
        }
        let file = ArchiveFiles::open(&root)?.open_regular(std::path::Path::new("lease"))?;
        match FileExt::try_lock(&file) {
            Ok(()) => Ok(Self(file)),
            Err(TryLockError::WouldBlock) => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Пакет уже обновляется.",
            )),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}
