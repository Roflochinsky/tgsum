use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, DirBuilder, DirBuilderExt, MetadataExt, OpenOptions, OpenOptionsExt};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

pub(super) const UNIT: &str = "tgsum-background.service";
const MARKER: &str = "# Managed by TGSUM background v1";
const MAX_UNIT_BYTES: u64 = 64 * 1024;

pub(super) struct Configuration {
    root: PathBuf,
    fixture_enabled: Option<Arc<AtomicBool>>,
}

impl Configuration {
    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            root,
            fixture_enabled: None,
        }
    }

    #[cfg(any(test, all(debug_assertions, feature = "desktop-e2e")))]
    pub(super) fn fixture(root: PathBuf, enabled: Arc<AtomicBool>) -> Self {
        Self {
            root,
            fixture_enabled: Some(enabled),
        }
    }

    pub(super) fn path(&self) -> PathBuf {
        self.root.join("systemd/user").join(UNIT)
    }

    pub(super) fn directory(&self, create: bool) -> io::Result<Option<Dir>> {
        let Some(root) = open_absolute(&self.root, create)? else {
            return Ok(None);
        };
        check_directory(&root)?;
        let Some(systemd) = child_directory(&root, "systemd", create)? else {
            return Ok(None);
        };
        check_directory(&systemd)?;
        let Some(user) = child_directory(&systemd, "user", create)? else {
            return Ok(None);
        };
        check_directory(&user)?;
        Ok(Some(user))
    }

    pub(super) fn enabled(&self) -> io::Result<bool> {
        let Some(directory) = self.directory(false)? else {
            return Ok(false);
        };
        let Some(_) = read_owned(&directory)? else {
            return Ok(false);
        };
        if let Some(enabled) = &self.fixture_enabled {
            return Ok(enabled.load(Ordering::Relaxed));
        }
        let state = systemctl(&["is-enabled", UNIT], true)?;
        match state.trim() {
            "enabled" | "enabled-runtime" => Ok(true),
            "disabled" | "masked" | "masked-runtime" | "static" | "indirect" => Ok(false),
            _ => Err(io::Error::other(
                "Не удалось определить состояние пользовательского сервиса TGSUM.",
            )),
        }
    }

    pub(super) fn set(&self, enabled: bool, exe: &Path) -> io::Result<()> {
        // Unit files are immutable. Updating the installed binary at the same
        // path requires no config replacement. Never replace an existing leaf,
        // even if a marker says it once belonged to this application.
        let text = service_text(exe)?;
        let Some(directory) = self.directory(enabled)? else {
            return Ok(());
        };
        let old = read_owned(&directory)?;
        if enabled {
            match old.as_deref() {
                Some(bytes) if bytes == text.as_bytes() => {}
                Some(_) => return Err(io::Error::other(
                    "Сервис уже настроен для другого запуска. Его файл сохранён без изменений. Используйте установленный TGSUM по прежнему пути или настройте сервис отдельно."
                )),
                None => publish_new(&directory, text.as_bytes())?,
            }
        }
        // Do not let a replaced pathname redirect the service manager after
        // writes confined to the original retained directory handle.
        let current = self.directory(false)?.ok_or_else(changed)?;
        let a = directory.dir_metadata()?;
        let b = current.dir_metadata()?;
        if (a.dev(), a.ino()) != (b.dev(), b.ino()) {
            return Err(changed());
        }
        if let Some(state) = &self.fixture_enabled {
            state.store(enabled, Ordering::Relaxed);
        } else if enabled {
            systemctl(&["daemon-reload"], false)?;
            // Enable the next login; the current process keeps its writer.
            systemctl(&["enable", UNIT], false)?;
        } else if old.is_some() {
            systemctl(&["disable", UNIT], false)?;
        }
        Ok(())
    }
}

fn changed() -> io::Error {
    io::Error::other("Папка настройки сервиса изменилась. Повторите действие.")
}

fn open_absolute(path: &Path, create: bool) -> io::Result<Option<Dir>> {
    if !path.is_absolute() {
        return Err(changed());
    }
    let mut directory = Dir::open_ambient_dir("/", cap_std::ambient_authority())?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let Some(next) = child_directory(&directory, name, create)? else {
                    return Ok(None);
                };
                directory = next;
            }
            _ => return Err(changed()),
        }
    }
    Ok(Some(directory))
}

fn child_directory(parent: &Dir, name: impl AsRef<Path>, create: bool) -> io::Result<Option<Dir>> {
    match parent.open_dir_nofollow(name.as_ref()) {
        Ok(directory) => Ok(Some(directory)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if !create {
                return Ok(None);
            }
            let mut builder = DirBuilder::new();
            builder.mode(0o700);
            match parent.create_dir_with(name.as_ref(), &builder) {
                Ok(()) => sync_directory(parent)?,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            parent.open_dir_nofollow(name).map(Some)
        }
        Err(error) => Err(error),
    }
}

fn check_directory(directory: &Dir) -> io::Result<()> {
    let metadata = directory.dir_metadata()?;
    if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o022 != 0 {
        return Err(io::Error::other("Небезопасная папка настроек TGSUM."));
    }
    Ok(())
}

fn read_owned(directory: &Dir) -> io::Result<Option<Vec<u8>>> {
    let mut options = OpenOptions::new();
    // A foreign FIFO must not block the status/configuration worker before
    // metadata can reject it. Regular files are unaffected by NONBLOCK.
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    let file = match directory.open_with(UNIT, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o022 != 0
        || metadata.len() > MAX_UNIT_BYTES
    {
        return Err(io::Error::other("Чужой или небезопасный сервис TGSUM."));
    }
    let mut bytes = Vec::new();
    file.take(MAX_UNIT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_UNIT_BYTES || !bytes.starts_with(format!("{MARKER}\n").as_bytes()) {
        return Err(io::Error::other(
            "Сервис с этим именем настроен отдельно. TGSUM сохранил его без изменений.",
        ));
    }
    Ok(Some(bytes))
}

pub(super) fn publish_new(directory: &Dir, bytes: &[u8]) -> io::Result<()> {
    // Anonymous temporary inode: no named temp slot can be substituted. linkat
    // creates UNIT only if absent, atomically exposing fully synced contents.
    let fd = rustix::fs::openat(
        directory,
        ".",
        rustix::fs::OFlags::TMPFILE | rustix::fs::OFlags::WRONLY,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )?;
    let mut file = File::from(fd);
    file.write_all(bytes)?;
    file.sync_all()?;
    // /proc/self/fd with SYMLINK_FOLLOW supports unprivileged O_TMPFILE
    // publication without requiring CAP_DAC_READ_SEARCH for AT_EMPTY_PATH.
    rustix::fs::linkat(
        rustix::fs::CWD,
        format!("/proc/self/fd/{}", file.as_raw_fd()),
        directory,
        UNIT,
        rustix::fs::AtFlags::SYMLINK_FOLLOW,
    )?;
    sync_directory(directory)
}

fn sync_directory(directory: &Dir) -> io::Result<()> {
    // cap-std retains an O_PATH handle; fsync needs a readable directory FD.
    directory.open(".")?.into_std().sync_all()
}

pub(super) fn service_text(exe: &Path) -> io::Result<String> {
    let path = exe
        .to_str()
        .filter(|p| exe.is_absolute() && !p.chars().any(char::is_control))
        .ok_or_else(|| io::Error::other("Неподходящий путь приложения для автозапуска."))?;
    let arg = path
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$");
    Ok(format!("{MARKER}\n[Unit]\nDescription=TGSUM local Telegram collector\nPartOf=graphical-session.target\nAfter=graphical-session.target\n\n[Service]\nType=simple\nExecStart=\"{arg}\" --background\nRestart=on-failure\nRestartSec=5\nTimeoutStopSec=30\n\n[Install]\nWantedBy=graphical-session.target\n"))
}

fn systemctl(arguments: &[&str], allow_disabled: bool) -> io::Result<String> {
    let mut child = Command::new("/usr/bin/systemctl")
        .args(["--user", "--no-pager", "--no-ask-password"])
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Пользовательский менеджер сервисов не ответил.",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !status.success() && !(allow_disabled && status.code() == Some(1)) {
        return Err(io::Error::other(
            "Не удалось обновить пользовательский сервис. Файл настройки сохранён. Проверьте менеджер сервисов и повторите действие.",
        ));
    }
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .take(4096)
        .read_to_end(&mut bytes)?;
    String::from_utf8(bytes).map_err(io::Error::other)
}
