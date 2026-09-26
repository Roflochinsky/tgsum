//! Process isolation lives outside the offline core. An adapter supplies a
//! reviewed runtime file list, exact version probe and argv/stdin contract.
//! This crate never discovers credentials or falls back to an unsandboxed run.

pub mod codex;
mod context;
mod discovery;
#[cfg(target_os = "linux")]
mod linux;

pub use context::PreparedContext;
pub use discovery::{discover, ExecutableCandidate};

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const LINUX_OFFLINE_PROFILE: &str = "linux-x86_64-bwrap-offline-v1";

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// These are host/adapter configuration, never fields read from chat content.
pub struct RuntimeFile {
    pub source: PathBuf,
    pub guest: PathBuf,
}

pub struct RuntimeSpec {
    pub executable: PathBuf,
    pub files: Vec<RuntimeFile>,
}

/// No shell command string or inherited environment is accepted.
#[derive(Default)]
pub struct Invocation {
    pub args: Vec<OsString>,
    pub stdin: Vec<u8>,
}

pub struct AdapterContract {
    pub id: String,
    pub isolation_profile: String,
    pub version_probe: Invocation,
    /// Exact stdout bytes, including newline. A CLI update requires re-review.
    pub expected_version_output: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthAvailability {
    Unknown,
    NotRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedAdapter {
    pub id: String,
    pub version_output: String,
    pub isolation_profile: &'static str,
    pub authentication: AuthAvailability,
}

#[derive(Debug, Clone, Copy)]
pub struct RunLimits {
    pub timeout: Duration,
    pub stdin_bytes: usize,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(300),
            stdin_bytes: 64 * 1024,
            stdout_bytes: 4 * 1024 * 1024,
            stderr_bytes: 256 * 1024,
        }
    }
}

impl RunLimits {
    fn validate(&self, invocation: &Invocation) -> Result<(), RunnerError> {
        if self.timeout.is_zero()
            || self.timeout > Duration::from_secs(1800)
            || self.stdin_bytes > 1024 * 1024
            || self.stdout_bytes == 0
            || self.stdout_bytes > 16 * 1024 * 1024
            || self.stderr_bytes == 0
            || self.stderr_bytes > 1024 * 1024
            || invocation.stdin.len() > self.stdin_bytes
            || invocation.args.len() > 128
            || invocation
                .args
                .iter()
                .map(|s| s.as_encoded_bytes().len())
                .try_fold(0usize, |sum, n| sum.checked_add(n))
                .is_none_or(|n| n > 64 * 1024)
        {
            return Err(RunnerError::InvalidRequest(
                "invalid process limits or input",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    Exited,
    Cancelled,
    TimedOut,
    StdoutLimit,
    StderrLimit,
    /// The supervisor exited but a pipe failed to close within the cleanup window.
    CleanupTimedOut,
}

/// Raw process output is untrusted. It must pass adapter schema/evidence checks
/// before a caller saves an analysis or advances its baseline.
pub struct RunOutput {
    pub termination: Termination,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl RunOutput {
    pub fn process_succeeded(&self) -> bool {
        self.termination == Termination::Exited && self.exit_code == Some(0)
    }
}

impl fmt::Debug for RunOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunOutput")
            .field("termination", &self.termination)
            .field("exit_code", &self.exit_code)
            .field("stdout_bytes", &self.stdout.len())
            .field("stderr_bytes", &self.stderr.len())
            .finish()
    }
}

#[derive(Debug)]
pub enum RunnerError {
    /// The UI must offer Export only, without retrying outside isolation.
    ExportOnly(&'static str),
    InvalidRequest(&'static str),
    Cancelled,
    Io(io::Error),
}

impl fmt::Display for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExportOnly(reason) => write!(f, "Export only: {reason}"),
            Self::InvalidRequest(reason) => f.write_str(reason),
            Self::Cancelled => f.write_str("run cancelled"),
            Self::Io(error) => write!(f, "runner I/O error: {error}"),
        }
    }
}

impl std::error::Error for RunnerError {}

impl From<io::Error> for RunnerError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// A staged runtime which passed its exact version probe in the offline sandbox.
/// This is a filesystem/network/process boundary, not an AI provider integration.
pub struct OfflineRunner {
    info: QualifiedAdapter,
    #[cfg(target_os = "linux")]
    backend: linux::Backend,
}

impl OfflineRunner {
    pub fn qualify(
        contract: AdapterContract,
        runtime: RuntimeSpec,
        cancellation: &Cancellation,
    ) -> Result<Self, RunnerError> {
        if cancellation.is_cancelled() {
            return Err(RunnerError::Cancelled);
        }
        if contract.isolation_profile != LINUX_OFFLINE_PROFILE {
            return Err(RunnerError::ExportOnly("unknown isolation profile"));
        }
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Err(RunnerError::ExportOnly(
                "no verified backend for this OS/architecture",
            ));
        }
        if contract.id.is_empty()
            || contract.id.len() > 128
            || contract.expected_version_output.is_empty()
            || contract.expected_version_output.len() > 4096
            || std::str::from_utf8(&contract.expected_version_output).is_err()
        {
            return Err(RunnerError::InvalidRequest("invalid adapter identity"));
        }
        #[cfg(target_os = "linux")]
        {
            let backend = linux::Backend::qualify(&contract, runtime, cancellation)?;
            Ok(Self {
                info: QualifiedAdapter {
                    id: contract.id,
                    version_output: String::from_utf8(contract.expected_version_output)
                        .expect("version validated above"),
                    isolation_profile: LINUX_OFFLINE_PROFILE,
                    authentication: AuthAvailability::NotRequired,
                },
                backend,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = runtime;
            Err(RunnerError::ExportOnly("no verified backend for this OS"))
        }
    }

    pub fn info(&self) -> &QualifiedAdapter {
        &self.info
    }

    /// Synchronous: call on a dedicated host worker, keeping that thread alive
    /// until this method returns (Linux parent-death signals are thread-bound).
    pub fn run(
        &self,
        context: &PreparedContext,
        invocation: &Invocation,
        limits: RunLimits,
        cancellation: &Cancellation,
    ) -> Result<RunOutput, RunnerError> {
        limits.validate(invocation)?;
        context.check_revision()?;
        #[cfg(target_os = "linux")]
        {
            self.backend
                .run(context.directory(), invocation, limits, cancellation)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = cancellation;
            Err(RunnerError::ExportOnly("no verified backend for this OS"))
        }
    }
}
