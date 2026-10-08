//! Private append-only control receipts. Retained directory/file descriptors,
//! no-follow reads, a single-writer lock and atomic no-clobber publication.

use std::cell::Cell;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, DirBuilder, DirBuilderExt, MetadataExt, OpenOptions, OpenOptionsExt};
use serde::{Deserialize, Serialize};

use super::{error, Lease, Phase};

const MARKER: &str = "owner.json";
const LIMIT: u64 = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    format: String,
    schema_version: u32,
    directory_device: u64,
    directory_inode: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    sequence: u64,
    lease: Lease,
}

pub struct LeaseJournal {
    path: PathBuf,
    directory: Dir,
    owner: File,
    owner_bytes: Vec<u8>,
    poisoned: Cell<bool>,
    #[cfg(test)]
    pub(super) fail_after_link: Cell<bool>,
}

impl LeaseJournal {
    /// The parent must already exist; an existing unbranded leaf is preserved.
    pub fn open(path: &Path) -> io::Result<Self> {
        let parent = absolute(path.parent().ok_or_else(unsafe_path)?)?;
        let name = path.file_name().ok_or_else(unsafe_path)?;
        let directory = match parent.open_dir_nofollow(name) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                create_branded(&parent, name.as_ref(), || Ok(()))?;
                parent.open_dir_nofollow(name)?
            }
            Err(error) => return Err(error),
        };
        private_directory(&directory)?;
        let metadata = directory.dir_metadata()?;
        let (owner, owner_bytes) = read(&directory, MARKER, true)?;
        let marker: Owner = serde_json::from_slice(&owner_bytes)?;
        if marker.format != "tgsum-telegram-client-control"
            || marker.schema_version != 1
            || (marker.directory_device, marker.directory_inode) != (metadata.dev(), metadata.ino())
        {
            return Err(unsafe_path());
        }
        rustix::fs::flock(&owner, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| error("Другой TGSUM уже управляет режимом Telegram."))?;
        let journal = Self {
            path: path.into(),
            directory,
            owner,
            owner_bytes,
            poisoned: Cell::new(false),
            #[cfg(test)]
            fail_after_link: Cell::new(false),
        };
        journal.check()?;
        let last = journal.last()?;
        // Reopen is the recovery boundary for an uncertain publication. Sync
        // its latest receipt, marker, directory and parent before any client
        // action can rely on a merely visible name surviving a power loss.
        if let Some(last) = last {
            read(
                &journal.directory,
                &format!("{:020}.json", last.sequence),
                false,
            )?
            .0
            .sync_all()?;
        }
        journal.owner.sync_all()?;
        sync(&journal.directory)?;
        sync(&parent)?;
        journal.check()?;
        Ok(journal)
    }

    pub(super) fn check(&self) -> io::Result<()> {
        if self.poisoned.get() {
            return Err(error("Запись управления Telegram могла не закрепиться на диске. До повторного открытия журнала управление клиентом остановлено."));
        }
        let current = absolute(&self.path)?;
        private_directory(&current)?;
        let original = self.directory.dir_metadata()?;
        let current_metadata = current.dir_metadata()?;
        if (original.dev(), original.ino()) != (current_metadata.dev(), current_metadata.ino()) {
            return Err(unsafe_path());
        }
        let (file, bytes) = read(&current, MARKER, false)?;
        use std::os::unix::fs::MetadataExt as _;
        let locked = self.owner.metadata()?;
        let reopened = file.metadata()?;
        if (locked.dev(), locked.ino()) != (reopened.dev(), reopened.ino())
            || bytes != self.owner_bytes
        {
            return Err(unsafe_path());
        }
        Ok(())
    }

    fn last(&self) -> io::Result<Option<Receipt>> {
        self.check()?;
        let mut last = None;
        for entry in self.directory.entries()? {
            let entry = entry?;
            let name = entry.file_name();
            if name == MARKER {
                continue;
            }
            let name = name.to_str().ok_or_else(unsafe_path)?;
            let digits = name.strip_suffix(".json").ok_or_else(unsafe_path)?;
            if digits.len() != 20 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(unsafe_path());
            }
            let sequence: u64 = digits.parse().map_err(|_| unsafe_path())?;
            if last.is_none_or(|old| sequence > old) {
                last = Some(sequence);
            }
        }
        let Some(sequence) = last else {
            return Ok(None);
        };
        let (_, bytes) = read(&self.directory, &format!("{sequence:020}.json"), false)?;
        let receipt: Receipt = serde_json::from_slice(&bytes)?;
        let lease = &receipt.lease;
        if receipt.sequence != sequence
            || lease.schema_version != 1
            || lease
                .original
                .launch
                .arguments
                .iter()
                .any(|a| a == "-debug")
            || (matches!(lease.phase, Phase::Active | Phase::Restoring) && lease.managed.is_none())
            || (!matches!(lease.phase, Phase::Active | Phase::Restoring) && lease.managed.is_some())
        {
            return Err(unsafe_path());
        }
        Ok(Some(receipt))
    }

    pub(super) fn latest(&self) -> io::Result<Option<Lease>> {
        Ok(self.last()?.map(|receipt| receipt.lease))
    }

    pub(super) fn append(&self, lease: &Lease) -> io::Result<()> {
        let sequence = match self.last()? {
            Some(receipt) => receipt.sequence.checked_add(1).ok_or_else(unsafe_path)?,
            None => 0,
        };
        let bytes = serde_json::to_vec(&Receipt {
            sequence,
            lease: lease.clone(),
        })?;
        if bytes.len() as u64 > LIMIT {
            return Err(unsafe_path());
        }
        self.check()?;
        #[cfg(test)]
        let fail_after_link = self.fail_after_link.replace(false);
        #[cfg(not(test))]
        let fail_after_link = false;
        let result = publish(
            &self.directory,
            &format!("{sequence:020}.json"),
            &bytes,
            fail_after_link,
        )
        .and_then(|_| self.check());
        if result.is_err() {
            self.poisoned.set(true);
        }
        result
    }
}

pub(super) fn create_branded(
    parent: &Dir,
    name: &Path,
    before_publish: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    // Build privately before exposing the fixed target name. A concurrent
    // foreign target never receives our marker or gets adopted/overwritten.
    let mut random = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    let stage = format!(
        ".tgsum-client-{}",
        random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    parent.create_dir_with(&stage, &builder)?;
    let directory = parent.open_dir_nofollow(&stage)?;
    private_directory(&directory)?;
    let metadata = directory.dir_metadata()?;
    let bytes = serde_json::to_vec(&Owner {
        format: "tgsum-telegram-client-control".into(),
        schema_version: 1,
        directory_device: metadata.dev(),
        directory_inode: metadata.ino(),
    })?;
    publish(&directory, MARKER, &bytes, false)?;
    before_publish()?;
    let current = parent.open_dir_nofollow(&stage)?.dir_metadata()?;
    if (metadata.dev(), metadata.ino()) != (current.dev(), current.ino()) {
        return Err(unsafe_path());
    }
    // On failure preserve the branded initialization record. Linux has no
    // conditional unlink-by-inode; pathname cleanup could remove a replacement.
    // The controller never deletes stage/control/history data automatically.
    rustix::fs::renameat_with(
        parent,
        &stage,
        parent,
        name,
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    sync(parent)
}

fn absolute(path: &Path) -> io::Result<Dir> {
    if !path.is_absolute() {
        return Err(unsafe_path());
    }
    let mut directory = Dir::open_ambient_dir("/", cap_std::ambient_authority())?;
    for part in path.components() {
        match part {
            Component::RootDir => {}
            Component::Normal(name) => directory = directory.open_dir_nofollow(name)?,
            _ => return Err(unsafe_path()),
        }
    }
    Ok(directory)
}

fn private_directory(directory: &Dir) -> io::Result<()> {
    let metadata = directory.dir_metadata()?;
    if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
        return Err(unsafe_path());
    }
    Ok(())
}

fn read(directory: &Dir, name: &str, writable: bool) -> io::Result<(File, Vec<u8>)> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(writable)
        .follow(FollowSymlinks::No)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    let mut file = directory.open_with(name, &options)?.into_std();
    use std::os::unix::fs::MetadataExt as _;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.mode() & 0o077 != 0
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.len() > LIMIT
    {
        return Err(unsafe_path());
    }
    let mut bytes = Vec::new();
    (&mut file).take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(unsafe_path());
    }
    Ok((file, bytes))
}

fn publish(directory: &Dir, name: &str, bytes: &[u8], fail_after_link: bool) -> io::Result<()> {
    let fd = rustix::fs::openat(
        directory,
        ".",
        rustix::fs::OFlags::TMPFILE | rustix::fs::OFlags::WRONLY,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )?;
    let mut file = File::from(fd);
    file.write_all(bytes)?;
    file.sync_all()?;
    rustix::fs::linkat(
        rustix::fs::CWD,
        format!("/proc/self/fd/{}", file.as_raw_fd()),
        directory,
        name,
        rustix::fs::AtFlags::SYMLINK_FOLLOW,
    )?;
    if fail_after_link {
        return Err(error("synthetic directory sync failure after link"));
    }
    sync(directory)
}

fn sync(directory: &Dir) -> io::Result<()> {
    directory.open(".")?.into_std().sync_all()
}

fn unsafe_path() -> io::Error {
    error("Журнал управления Telegram отсутствует, изменился или принадлежит другой программе. Клиент и существующие файлы сохранены без изменений.")
}
