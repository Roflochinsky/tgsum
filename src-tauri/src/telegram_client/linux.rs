//! Pinned Arch stock-client adapter. Reads only launch metadata from /proc,
//! package version and the existence (never contents) of the persistent flag.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Component, Path};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;

use super::{error, Desktop, Identity, Launch, Running};

const EXECUTABLE: &str = "/usr/bin/Telegram";
const VERSION: &str = "telegram-desktop 7.2.5-1";
const MAX_METADATA: u64 = 32 * 1024;
const WAIT: Duration = Duration::from_secs(8);

#[derive(Default)]
pub struct StockDesktop {
    children: Vec<Child>,
}

impl StockDesktop {
    fn package(&mut self) -> io::Result<()> {
        self.children
            .retain_mut(|child| matches!(child.try_wait(), Ok(None)));
        let metadata = fs::metadata(EXECUTABLE)?;
        if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(error("Не найден штатный пакет Telegram Desktop."));
        }
        let mut command = Command::new("/usr/bin/pacman");
        command.args(["-Q", "telegram-desktop"]);
        let output = bounded_command(command)?;
        if output.trim() != VERSION {
            return Err(error("Управление клиентом проверено только для Arch Telegram Desktop 7.2.5-1. Для другой версии используйте готовые логи."));
        }
        Ok(())
    }

    fn validate(&mut self, launch: &Launch) -> io::Result<()> {
        self.package()?;
        if launch.executable != Path::new(EXECUTABLE) {
            return Err(error("Запуск не принадлежит штатному Telegram Desktop."));
        }
        validate_arguments(&launch.arguments, &launch.working_directory)?;
        if pinned_arguments(&launch.arguments, &launch.working_directory)? != launch.arguments {
            return Err(error(
                "Запуск Telegram должен явно сохранять прежнюю папку профиля.",
            ));
        }
        tgsum_core::telegram_debug::settings::validate_log_directory(&launch.working_directory)?;
        let metadata = fs::metadata(&launch.working_directory)?;
        if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o022 != 0 {
            return Err(error(
                "Папка Telegram недоступна для безопасного перезапуска.",
            ));
        }
        if self.persistent_debug_exists(launch)? {
            return Err(error(
                "Настройка отладки Telegram изменилась. TGSUM не читает и не меняет её.",
            ));
        }
        Ok(())
    }
}

impl Desktop for StockDesktop {
    fn current(&mut self) -> io::Result<Option<Running>> {
        self.package()?;
        let boot_id = metadata_text(Path::new("/proc/sys/kernel/random/boot_id"))?
            .trim()
            .to_owned();
        let mut result = None;
        for entry in fs::read_dir("/proc")? {
            let entry = entry?;
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let path = entry.path();
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if metadata.uid() != rustix::process::geteuid().as_raw() {
                continue;
            }
            let executable = match fs::read_link(path.join("exe")) {
                Ok(path) => path,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
                    ) =>
                {
                    continue
                }
                Err(error) => return Err(error),
            };
            let name = executable
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if !matches!(name, "Telegram" | "telegram-desktop" | "Telegram (deleted)") {
                continue;
            }
            if executable != Path::new(EXECUTABLE) || result.is_some() {
                return Err(error("Обнаружено несколько или неизвестный запуск Telegram. Управление клиентом остановлено."));
            }
            let running = match inspect(&path, pid, &boot_id) {
                Ok(running) => running,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            result = Some(running);
        }
        Ok(result)
    }

    fn persistent_debug_exists(&mut self, launch: &Launch) -> io::Result<bool> {
        // Only no-follow metadata. Do not open or read the flag or session DB.
        let directory =
            Dir::open_ambient_dir(&launch.working_directory, cap_std::ambient_authority())?;
        let tdata = match directory.open_dir_nofollow("tdata") {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        match tdata.symlink_metadata("withdebug") {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn quit(&mut self, running: &Running) -> io::Result<()> {
        self.validate(&running.launch)?;
        let deadline = Instant::now() + WAIT;
        while !ipc_ready(running)? {
            if self.current()?.as_ref() != Some(running) || Instant::now() >= deadline {
                return Err(error(
                    "Штатный процесс Telegram ещё не готов к команде остановки.",
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
        if self.current()?.as_ref() != Some(running) {
            return Err(error(
                "Telegram сменил процесс. Неизвестный запуск сохранён.",
            ));
        }
        let endpoint =
            ipc_endpoint(running)?.ok_or_else(|| error("Штатный IPC Telegram недоступен."))?;
        // The stock -quit helper addresses a profile name, not a PID. Bind the
        // same fixed stock command to an authenticated retained connection.
        // Never reconnect after an error or redirect it to a replacement.
        send_owned_quit(
            &endpoint,
            running.identity.pid,
            rustix::process::geteuid().as_raw(),
            || {
                if self.current()?.as_ref() != Some(running) {
                    return Err(error("Telegram сменил процесс до команды остановки."));
                }
                Ok(())
            },
        )?;
        let deadline = Instant::now() + WAIT;
        loop {
            match self.current()? {
                None => return Ok(()),
                Some(current) if current.identity == running.identity => {}
                Some(_) => {
                    return Err(error(
                        "Во время остановки появился другой процесс Telegram. Он сохранён.",
                    ))
                }
            }
            if Instant::now() >= deadline {
                return Err(error(
                    "Telegram не завершился штатно. TGSUM не завершает его принудительно.",
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn start(&mut self, launch: &Launch) -> io::Result<Running> {
        self.validate(launch)?;
        if self.current()?.is_some() {
            return Err(error("Другой Telegram уже работает. Его запуск сохранён."));
        }
        // A Desktop spawned directly inside the TGSUM service would inherit
        // its cgroup and could be killed when that service stops. A transient
        // scope inherits this graphical environment but has its own cgroup.
        let unit = format!(
            "tgsum-telegram-{}-{}.scope",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let child = scope_command(launch, &unit).spawn()?;
        let pid = child.id();
        // These are user-facing Desktop processes. Dropping the adapter does
        // not kill them; retain handles only for reaping exited children.
        self.children.push(child);
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(running) = self.current()? {
                if running.identity.pid != pid || running.launch != *launch {
                    return Err(error(
                        "Telegram запустился иначе, чем ожидалось. Неизвестный процесс сохранён.",
                    ));
                }
                if ipc_ready(&running)? {
                    return Ok(running);
                }
            }
            if Instant::now() >= deadline {
                return Err(error("Запуск Telegram не подтверждён. Исходный запуск сохранён; неизвестный процесс не будет завершён автоматически."));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

pub(super) fn scope_command(launch: &Launch, unit: &str) -> Command {
    let mut command = Command::new("/usr/bin/systemd-run");
    command
        .args([
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            "--no-ask-password",
            "--expand-environment=no",
        ])
        .arg(format!("--unit={unit}"))
        .arg("--")
        .arg(&launch.executable)
        .args(&launch.arguments)
        .current_dir(&launch.working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn inspect(path: &Path, pid: u32, boot_id: &str) -> io::Result<Running> {
    let before = start_ticks(&metadata_text(&path.join("stat"))?)?;
    let executable = fs::read_link(path.join("exe"))?;
    let metadata = fs::metadata(path.join("exe"))?;
    let working_directory = fs::read_link(path.join("cwd"))?;
    let bytes = metadata_bytes(&path.join("cmdline"))?;
    let all: Vec<_> = bytes
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .collect();
    if all.is_empty() || all.len() > 17 {
        return Err(error("Неизвестные параметры запуска Telegram."));
    }
    let arguments: Vec<String> = all[1..]
        .iter()
        .map(|arg| {
            std::str::from_utf8(arg)
                .map(str::to_owned)
                .map_err(|_| error("Неизвестные параметры запуска Telegram."))
        })
        .collect::<io::Result<_>>()?;
    let arguments = pinned_arguments(&arguments, &working_directory)?;
    let after = start_ticks(&metadata_text(&path.join("stat"))?)?;
    if before != after || executable != Path::new(EXECUTABLE) {
        return Err(error("Telegram изменился во время проверки запуска."));
    }
    Ok(Running {
        identity: Identity {
            pid,
            start_ticks: before,
            boot_id: boot_id.into(),
            executable_device: metadata.dev(),
            executable_inode: metadata.ino(),
        },
        launch: Launch {
            executable,
            arguments,
            working_directory,
        },
    })
}

fn metadata_bytes(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_METADATA + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_METADATA {
        return Err(error("Слишком большой объём параметров запуска Telegram."));
    }
    Ok(bytes)
}

fn metadata_text(path: &Path) -> io::Result<String> {
    String::from_utf8(metadata_bytes(path)?)
        .map_err(|_| error("Неизвестный формат параметров процесса."))
}

pub(super) fn start_ticks(stat: &str) -> io::Result<u64> {
    // comm may contain spaces and parentheses; starttime is field22, i.e.
    // the twentieth field after the closing parenthesis of field2.
    stat.rsplit_once(") ")
        .and_then(|(_, rest)| rest.split_whitespace().nth(19))
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| error("Не удалось проверить время запуска Telegram."))
}

pub(super) fn validate_arguments(arguments: &[String], working_directory: &Path) -> io::Result<()> {
    fn clean(path: &Path) -> bool {
        path.is_absolute()
            && !path
                .components()
                .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
            && path
                .to_str()
                .is_some_and(|s| !s.chars().any(char::is_control))
    }
    if !clean(working_directory) || arguments.len() > 16 {
        return Err(error("Неизвестная папка запуска Telegram."));
    }
    let mut seen = BTreeSet::new();
    let mut arguments = arguments.iter();
    while let Some(arg) = arguments.next() {
        if !seen.insert(arg) {
            return Err(error("Повторяющиеся параметры запуска Telegram."));
        }
        match arg.as_str() {
            "-debug" | "-autostart" | "-startintray" => {},
            "-workdir" => {
                if arguments.next().is_none_or(|value| Path::new(value) != working_directory) {
                    return Err(error("Папка Telegram не совпадает с параметрами запуска."));
                }
            }
            _ => return Err(error("Этот запуск Telegram содержит неподдерживаемые параметры. Используйте сбор уже готовых логов.")),
        }
    }
    Ok(())
}

pub(super) fn pinned_arguments(arguments: &[String], directory: &Path) -> io::Result<Vec<String>> {
    validate_arguments(arguments, directory)?;
    let mut pinned = arguments.to_vec();
    if !arguments.iter().any(|arg| arg == "-workdir") {
        pinned.push("-workdir".into());
        pinned.push(
            directory
                .to_str()
                .ok_or_else(|| error("Папка Telegram не является UTF-8."))?
                .into(),
        );
    }
    Ok(pinned)
}

fn ipc_ready(running: &Running) -> io::Result<bool> {
    Ok(ipc_endpoint(running)?.is_some())
}

fn ipc_endpoint(running: &Running) -> io::Result<Option<String>> {
    let path = Path::new("/proc").join(running.identity.pid.to_string());
    if start_ticks(&metadata_text(&path.join("stat"))?)? != running.identity.start_ticks {
        return Err(error(
            "Процесс Telegram сменился до готовности штатного IPC.",
        ));
    }
    let mut sockets = BTreeSet::new();
    for (count, entry) in fs::read_dir(path.join("fd"))?.enumerate() {
        if count >= 4096 {
            return Err(error(
                "Слишком много дескрипторов Telegram для проверки IPC.",
            ));
        }
        let entry = entry?;
        let target = match fs::read_link(entry.path()) {
            Ok(target) => target,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if let Some(inode) = target
            .to_str()
            .and_then(|name| name.strip_prefix("socket:["))
            .and_then(|name| name.strip_suffix(']'))
            .and_then(|name| name.parse::<u64>().ok())
        {
            sockets.insert(inode);
        }
    }
    // Kernel endpoint metadata only; never connect, send IPC or read a socket.
    let mut bytes = Vec::new();
    fs::File::open(path.join("net/unix"))?
        .take((1 << 20) + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1 << 20 {
        return Err(error("Слишком большой список IPC для проверки Telegram."));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| error("Неизвестный формат списка IPC."))?;
    let endpoint = owned_ipc_endpoint(text, &sockets);
    if start_ticks(&metadata_text(&path.join("stat"))?)? != running.identity.start_ticks {
        return Err(error("Процесс Telegram изменился во время проверки IPC."));
    }
    Ok(endpoint)
}

pub(super) fn owned_ipc_endpoint(text: &str, owned: &BTreeSet<u64>) -> Option<String> {
    text.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let flags = fields.nth(3);
        let kind = fields.next();
        let _state = fields.next();
        let inode = fields.next().and_then(|value| value.parse::<u64>().ok());
        let name = fields.next().and_then(|name| name.strip_prefix('@'));
        let hash = name.and_then(|name| name.strip_suffix("-TelegramDesktop"));
        (flags == Some("00010000")
            && kind == Some("0001")
            && inode.is_some_and(|inode| owned.contains(&inode))
            && hash.is_some_and(|hash| {
                hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit())
            }))
        .then(|| name.unwrap().to_owned())
    })
}

pub(super) fn send_owned_quit(
    endpoint: &str,
    pid: u32,
    uid: u32,
    validate_identity: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let address = rustix::net::SocketAddrUnix::new_abstract_name(endpoint.as_bytes())?;
    let fd = rustix::net::socket_with(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::STREAM,
        rustix::net::SocketFlags::CLOEXEC | rustix::net::SocketFlags::NONBLOCK,
        None,
    )?;
    // Full AF_UNIX listen queues must not block the TGSUM worker indefinitely.
    rustix::net::connect(&fd, &address)?;
    let mut stream = UnixStream::from(fd);
    let peer = rustix::net::sockopt::socket_peercred(&stream)?;
    if peer.pid.as_raw_nonzero().get() as u32 != pid || peer.uid.as_raw() != uid {
        return Err(error(
            "Сокет Telegram принадлежит другому процессу. Команда не отправлена.",
        ));
    }
    validate_identity()?;
    stream.set_nonblocking(false)?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(b"CMD:quit;")
}

fn bounded_command(mut command: Command) -> io::Result<String> {
    let mut child = command
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
            // Only the command helper which this call spawned, never the
            // pre-existing user process. Reap it even when termination fails.
            let _ = child.kill();
            let _ = child.wait();
            return Err(error("Штатная команда Telegram не завершилась вовремя."));
        }
        thread::sleep(Duration::from_millis(50));
    };
    if !status.success() {
        return Err(error("Не удалось выполнить штатную команду Telegram."));
    }
    let mut bytes = Vec::new();
    if let Some(mut output) = child.stdout.take() {
        // The version helper has exited. Drain only bytes already available;
        // an inherited pipe in a descendant must not hold shutdown open.
        let flags = rustix::fs::fcntl_getfl(&output)?;
        rustix::fs::fcntl_setfl(&output, flags | rustix::fs::OFlags::NONBLOCK)?;
        match (&mut output).take(2049).read_to_end(&mut bytes) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error),
        }
    }
    if bytes.len() > 2048 {
        return Err(error("Неожиданный ответ штатной команды."));
    }
    String::from_utf8(bytes).map_err(|_| error("Неожиданный формат ответа штатной команды."))
}
