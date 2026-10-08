//! Control only a verified stock Desktop launch, never account operations.
//! The immutable private lease survives TGSUM restarts. A launch which crashed
//! before its identity was recorded is deliberately not adopted by guesswork.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod journal;
mod linux;
pub use journal::LeaseJournal;
pub use linux::StockDesktop;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub pid: u32,
    pub start_ticks: u64,
    pub boot_id: String,
    pub executable_device: u64,
    pub executable_inode: u64,
}

/// Only launch metadata. No session, account, log or process-memory payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub executable: PathBuf,
    /// Validated effective options excluding argv[0]; explicitly pin the
    /// observed working directory even when the original argv omitted it.
    /// No URLs, keys or send actions, and no inherited profile assumptions.
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
}

impl Launch {
    pub fn log_directory(&self) -> PathBuf {
        self.working_directory.join("DebugLogs")
    }

    fn debug(&self) -> bool {
        self.arguments.iter().any(|arg| arg == "-debug")
    }

    fn with_debug(&self) -> Self {
        let mut launch = self.clone();
        launch.arguments.push("-debug".into());
        launch
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Running {
    pub identity: Identity,
    pub launch: Launch,
}

/// Mutations must revalidate identity, package, executable and profile. Quit is
/// the stock client's own IPC command, never a signal to the user's process.
pub trait Desktop {
    fn current(&mut self) -> io::Result<Option<Running>>;
    fn persistent_debug_exists(&mut self, launch: &Launch) -> io::Result<bool>;
    fn quit(&mut self, running: &Running) -> io::Result<()>;
    fn start(&mut self, launch: &Launch) -> io::Result<Running>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Prepared,
    DebugPending,
    Active,
    Restoring,
    RestorePending,
    Released,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Lease {
    schema_version: u32,
    original: RunningRecord,
    phase: Phase,
    managed: Option<Identity>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunningRecord {
    identity: Identity,
    launch: Launch,
}

impl From<Running> for RunningRecord {
    fn from(running: Running) -> Self {
        Self {
            identity: running.identity,
            launch: running.launch,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// Runtime -debug already belonged to the user; Stop leaves it alone.
    UserDebug,
    ManagedDebug,
}

/// One coordinator for the single existing client, shared by selected sources.
/// The caller must reconcile all enabled sources before calling restore().
pub struct Controller<D: Desktop> {
    desktop: D,
    journal: LeaseJournal,
}

impl<D: Desktop> Controller<D> {
    pub fn new(desktop: D, journal: LeaseJournal) -> Self {
        Self { desktop, journal }
    }

    /// Read-only detection; no lease/config creation and no client launch.
    pub fn detect(&mut self) -> io::Result<Option<Running>> {
        self.desktop.current()
    }

    pub fn ensure_debug(&mut self, log_directory: &Path) -> io::Result<Mode> {
        self.journal.check()?;
        let previous = self.journal.latest()?;
        let mut lease = match previous {
            Some(lease) if lease.phase != Phase::Released => lease,
            _ => {
                let running = self.desktop.current()?.ok_or_else(|| {
                    error("Откройте существующий Telegram Desktop перед первым запуском сбора.")
                })?;
                if running.launch.log_directory() != log_directory {
                    return Err(error("Папка логов не совпадает с работающим Telegram."));
                }
                if running.launch.debug() {
                    return Ok(Mode::UserDebug);
                }
                if self.desktop.persistent_debug_exists(&running.launch)? {
                    return Err(error("У Telegram есть отдельная настройка отладки. TGSUM не читает и не меняет её; используйте сбор уже готовых логов."));
                }
                let lease = Lease {
                    schema_version: 1,
                    original: running.into(),
                    phase: Phase::Prepared,
                    managed: None,
                };
                self.journal.append(&lease)?;
                lease
            }
        };
        if lease.original.launch.log_directory() != log_directory {
            return Err(error("Telegram уже связан с другой папкой логов."));
        }
        // Finish a previously requested restoration before acquiring anew.
        if matches!(lease.phase, Phase::Restoring | Phase::RestorePending) {
            self.restore()?;
            return self.ensure_debug(log_directory);
        }
        if lease.phase == Phase::Prepared {
            if let Some(current) = self.desktop.current()? {
                if current.identity != lease.original.identity
                    || current.launch != lease.original.launch
                {
                    return Err(changed());
                }
                self.journal.check()?;
                self.desktop.quit(&current)?;
            }
            lease.phase = Phase::DebugPending;
            self.journal.append(&lease)?;
        }
        if lease.phase == Phase::Active {
            if let Some(current) = self.desktop.current()? {
                if Some(&current.identity) == lease.managed.as_ref()
                    && current.launch == lease.original.launch.with_debug()
                {
                    return Ok(Mode::ManagedDebug);
                }
                return Err(changed());
            }
            // A previously claimed process disappeared. Preserve its original
            // launch and acquire a new, explicitly recorded process identity.
            lease.phase = Phase::DebugPending;
            lease.managed = None;
            self.journal.append(&lease)?;
        }
        if lease.phase != Phase::DebugPending || self.desktop.current()?.is_some() {
            return Err(changed());
        }
        self.journal.check()?;
        let managed = self.desktop.start(&lease.original.launch.with_debug())?;
        lease.managed = Some(managed.identity);
        lease.phase = Phase::Active;
        self.journal.append(&lease)?;
        Ok(Mode::ManagedDebug)
    }

    /// Stop restores only the exact client identity which this lease acquired.
    /// No source logs or persistent Telegram settings are deleted or changed.
    pub fn restore(&mut self) -> io::Result<()> {
        self.journal.check()?;
        let Some(mut lease) = self.journal.latest()? else {
            return Ok(());
        };
        if lease.phase == Phase::Released {
            return Ok(());
        }
        if let Some(current) = self.desktop.current()? {
            if current.launch == lease.original.launch {
                // Original normal launch is already present, possibly restored
                // by the user or after a crash before recording Released.
                lease.phase = Phase::Released;
                lease.managed = None;
                return self.journal.append(&lease);
            }
            if !matches!(lease.phase, Phase::Active | Phase::Restoring)
                || Some(&current.identity) != lease.managed.as_ref()
                || current.launch != lease.original.launch.with_debug()
            {
                return Err(changed());
            }
            lease.phase = Phase::Restoring;
            self.journal.append(&lease)?;
            self.journal.check()?;
            self.desktop.quit(&current)?;
        }
        lease.phase = Phase::RestorePending;
        lease.managed = None;
        self.journal.append(&lease)?;
        self.journal.check()?;
        self.desktop.start(&lease.original.launch)?;
        lease.phase = Phase::Released;
        self.journal.append(&lease)
    }
}

fn error(message: &str) -> io::Error {
    io::Error::other(message)
}
fn changed() -> io::Error {
    error("Запуск Telegram изменился или не успел получить подтверждение владения. TGSUM сохранил исходный запуск и не завершает неизвестный процесс. Верните Telegram к обычному запуску и повторите действие.")
}

#[cfg(test)]
mod tests;
