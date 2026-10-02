use super::{hash, invalid, PackageManifest, PackageReceipt, PackageState, MAX_PACKAGE_BYTES};
use crate::{attachments::ArchiveFiles, project::ProjectStore};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub(super) fn root(store: &ProjectStore, state: &PackageState) -> io::Result<PathBuf> {
    // Validate the Project ID before using it in a filesystem name.
    store.open(&state.project_id)?;
    ArchiveFiles::open(&state.settings.output_directory)?;
    let root = state
        .settings
        .output_directory
        .join(format!("tgsum-{}", state.project_id));
    match fs::create_dir(&root) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
            }
            super::write_file(&root.join(".tgsum-owner"), state.project_id.as_bytes())?;
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    let files = ArchiveFiles::open(&root)?;
    let mut owner = String::new();
    files
        .open_regular(Path::new(".tgsum-owner"))?
        .take(1024)
        .read_to_string(&mut owner)?;
    if owner != state.project_id {
        return Err(invalid("Папка результата принадлежит другому проекту."));
    }
    Ok(root)
}

fn generation_name(name: &str) -> bool {
    name.starts_with("tgsum-context-")
        && name.len() < 100
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub(super) fn verify(receipt: &PackageReceipt) -> io::Result<()> {
    if !generation_name(&receipt.generation) {
        return Err(invalid("invalid package generation"));
    }
    let parent = receipt
        .directory
        .parent()
        .ok_or_else(|| invalid("invalid package path"))?;
    let target = fs::read_link(&receipt.directory)?;
    if target != Path::new(&receipt.generation) {
        return Err(invalid("current package changed"));
    }
    let manifest = verify_generation(&parent.join(&receipt.generation))?;
    if hash(&serde_json::to_vec_pretty(&manifest).map_err(invalid)?) != receipt.content_sha256 {
        return Err(invalid("package manifest changed"));
    }
    Ok(())
}

pub(super) fn copy(
    receipt: &PackageReceipt,
    destination: &Path,
    cancelled: &impl Fn() -> bool,
) -> io::Result<()> {
    verify(receipt)?;
    ArchiveFiles::open(destination)?;
    if fs::read_dir(destination)?.next().is_some() {
        return Err(invalid("package copy destination must be empty"));
    }
    let generation = receipt
        .directory
        .parent()
        .ok_or_else(|| invalid("invalid package root"))?
        .join(&receipt.generation);
    let manifest = verify_generation(&generation)?;
    let files = ArchiveFiles::open(&generation)?;
    for file in &manifest.files {
        super::check(cancelled)?;
        let mut input = files.open_regular(Path::new(&file.name))?;
        let mut bytes = Vec::new();
        (&mut input).take(file.bytes + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != file.bytes || hash(&bytes) != file.sha256 {
            return Err(invalid("package changed during copying"));
        }
        super::write_file(&destination.join(&file.name), &bytes)?;
    }
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(invalid)?;
    super::write_file(&destination.join("package.json"), &bytes)?;
    Ok(())
}

fn verify_generation(path: &Path) -> io::Result<PackageManifest> {
    let files = ArchiveFiles::open(path)?;
    let mut bytes = Vec::new();
    files
        .open_regular(Path::new("package.json"))?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(invalid("package manifest too large"));
    }
    let manifest: PackageManifest = serde_json::from_slice(&bytes).map_err(invalid)?;
    if manifest.schema_version != 1 || manifest.files.len() > 4096 {
        return Err(invalid("unsupported package manifest"));
    }
    let mut allowed: BTreeSet<String> = ["package.json".into(), ".tgsum-generation".into()].into();
    let mut total = 0u64;
    for file in &manifest.files {
        if file.name.starts_with('.')
            || file.name.contains(['/', '\\', ':'])
            || !allowed.insert(file.name.clone())
        {
            return Err(invalid("invalid package file name"));
        }
        total = total
            .checked_add(file.bytes)
            .ok_or_else(|| invalid("package size overflow"))?;
        if total > MAX_PACKAGE_BYTES {
            return Err(invalid("package exceeds size limit"));
        }
        let mut f = files.open_regular(Path::new(&file.name))?;
        if f.metadata()?.len() != file.bytes
            || super::digest_reader(&mut f, &|| false)? != file.sha256
        {
            return Err(invalid("package file changed"));
        }
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if !allowed.contains(&entry.file_name().to_string_lossy().into_owned())
            || !entry.file_type()?.is_file()
        {
            return Err(invalid("package contains an unexpected file"));
        }
    }
    Ok(manifest)
}

pub(super) fn publish(
    store: &ProjectStore,
    state: &PackageState,
    staging: Staging,
    manifest: &PackageManifest,
    input_hash: &str,
    now: u64,
    cancelled: &impl Fn() -> bool,
) -> io::Result<PackageReceipt> {
    let root = root(store, state)?;
    if staging.path().parent() != Some(&root) {
        return Err(invalid("package staging is outside its owned root"));
    }
    let generation = staging
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| invalid("invalid generation"))?
        .to_owned();
    if !generation_name(&generation) {
        return Err(invalid("invalid generation"));
    }
    super::write_file(
        &staging.path().join(".tgsum-generation"),
        state.project_id.as_bytes(),
    )?;
    verify_generation(staging.path())?;
    File::open(staging.path())?.sync_all()?;
    super::check(cancelled)?;
    let current = root.join("current");
    if let Ok(meta) = fs::symlink_metadata(&current) {
        if !meta.file_type().is_symlink()
            || !fs::read_link(&current)?
                .to_str()
                .is_some_and(generation_name)
        {
            return Err(invalid("current is not a TGSUM-owned package pointer"));
        }
    }
    #[cfg(unix)]
    {
        let next = tempfile::NamedTempFile::new_in(&root)?.into_temp_path();
        fs::remove_file(&next)?;
        std::os::unix::fs::symlink(&generation, &next)?;
        super::check(cancelled)?;
        fs::rename(&next, &current)?;
    }
    #[cfg(not(unix))]
    return Err(invalid(
        "replaceable local packages are currently qualified only on Linux",
    ));
    let _retained = staging.keep();
    File::open(&root)?.sync_all()?;
    // Cleanup failure cannot turn a successfully published current generation
    // back into the previous one. A subsequent refresh retries cleanup.
    let _ = cleanup(state, &generation);
    Ok(PackageReceipt {
        conversations: state.settings.source_ids.len(),
        directory: current,
        generation,
        input_sha256: input_hash.into(),
        content_sha256: hash(&serde_json::to_vec_pretty(manifest).map_err(invalid)?),
        prepared_at: now,
        messages: manifest.messages,
        files: manifest.files.len() + 1,
        bytes: manifest.files.iter().map(|f| f.bytes).sum(),
        skipped_attachments: manifest.skipped_attachments,
        initials_replacements: manifest.initials_replacements,
    })
}

pub(super) fn cleanup(state: &PackageState, current: &str) -> io::Result<()> {
    let root = state
        .settings
        .output_directory
        .join(format!("tgsum-{}", state.project_id));
    ArchiveFiles::open(&root)?;
    if fs::read_link(root.join("current"))? != Path::new(current) {
        return Err(invalid("current changed before cleanup"));
    }
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !generation_name(&name) || name == current || !entry.file_type()?.is_dir() {
            continue;
        }
        let directory = ArchiveFiles::open(&entry.path())?;
        let mut owner = String::new();
        let Ok(file) = directory.open_regular(Path::new(".tgsum-generation")) else {
            continue;
        };
        file.take(1024).read_to_string(&mut owner)?;
        if owner == state.project_id && verify_generation(&entry.path()).is_ok() {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

/// Owns only the fresh directory returned by export_bundle.
pub(super) struct Staging {
    path: PathBuf,
    retained: bool,
}
impl Staging {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            retained: false,
        }
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    fn keep(mut self) -> PathBuf {
        self.retained = true;
        self.path.clone()
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        if !self.retained {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
