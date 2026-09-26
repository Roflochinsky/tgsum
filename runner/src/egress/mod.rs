//! Bounded TLS pass-through for an explicitly selected inference destination.
//! No token access, TLS termination, remote refresh or arbitrary proxy targets.
//! The sandbox relay/launch integration is separate; this module alone does not
//! enable cloud Run or qualify an installed provider account.
mod hello;
mod policy;
#[cfg(test)]
mod tests;
mod transport;

use std::io;
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{Cancellation, RunnerError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    OpenAiApi,
    ChatGpt,
}
impl Destination {
    pub fn host(self) -> &'static str {
        match self {
            Self::OpenAiApi => "api.openai.com",
            Self::ChatGpt => "chatgpt.com",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub duration: Duration,
    pub handshake: Duration,
    pub idle: Duration,
    pub connection_bytes: u64,
    pub total_bytes: u64,
    pub connections: usize,
    pub concurrent: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(300),
            handshake: Duration::from_secs(10),
            idle: Duration::from_secs(30),
            connection_bytes: 16 * 1024 * 1024,
            total_bytes: 64 * 1024 * 1024,
            connections: 16,
            concurrent: 4,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<(), RunnerError> {
        if self.duration.is_zero()
            || self.duration > Duration::from_secs(1800)
            || self.handshake.is_zero()
            || self.handshake > Duration::from_secs(30)
            || self.idle.is_zero()
            || self.idle > Duration::from_secs(60)
            || self.connection_bytes == 0
            || self.connection_bytes > 64 * 1024 * 1024
            || self.total_bytes < self.connection_bytes
            || self.total_bytes > 256 * 1024 * 1024
            || self.connections == 0
            || self.connections > 32
            || self.concurrent == 0
            || self.concurrent > 4
            || self.concurrent > self.connections
        {
            return Err(RunnerError::InvalidRequest(
                "invalid inference gateway limits",
            ));
        }
        Ok(())
    }
}

/// Static diagnostics only: no request header, TLS bytes, URL, or credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    ConnectRequest,
    ClientHello,
    ServerName,
    EncryptedHello,
    PrivateAddress,
    Resolution,
    Unreachable,
    Timeout,
    Cancelled,
    Budget,
    Connections,
    Io,
    Worker,
}

#[derive(Debug, Default)]
pub struct Report {
    pub accepted: usize,
    pub completed: usize,
    /// Bytes admitted into bounded forwarding buffers, not billing or delivery.
    pub admitted_bytes: u64,
    pub failures: Vec<Failure>,
}

#[derive(Clone)]
struct Stop {
    local: Cancellation,
    parent: Cancellation,
    deadline: Instant,
}
impl Stop {
    fn check(&self) -> Result<(), Failure> {
        if self.local.is_cancelled() || self.parent.is_cancelled() {
            return Err(Failure::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(Failure::Timeout);
        }
        Ok(())
    }
    fn remaining(&self) -> Result<Duration, Failure> {
        self.check()?;
        Ok(self.deadline.saturating_duration_since(Instant::now()))
    }
}

type Dial = dyn Fn(Destination, &Stop) -> Result<TcpStream, Failure> + Send + Sync;

/// Host-owned, private Unix socket and bounded workers. Drop stops acceptance,
/// cancels/reaps DNS subprocesses and joins workers. No detached resolver work.
pub struct InferenceGateway {
    _directory: tempfile::TempDir,
    socket: PathBuf,
    stop: Cancellation,
    worker: Option<JoinHandle<Report>>,
}
impl InferenceGateway {
    pub fn start(
        destination: Destination,
        limits: Limits,
        cancellation: &Cancellation,
    ) -> Result<Self, RunnerError> {
        Self::start_with(destination, limits, cancellation, Arc::new(policy::dial))
    }

    // Internal seam used only by tests to connect to synthetic loopback servers.
    // The public constructor has no DNS/IP overrides or private-address switch.
    fn start_with(
        destination: Destination,
        limits: Limits,
        cancellation: &Cancellation,
        dial: Arc<Dial>,
    ) -> Result<Self, RunnerError> {
        limits.validate()?;
        if cancellation.is_cancelled() {
            return Err(RunnerError::Cancelled);
        }
        let directory = tempfile::Builder::new().prefix("tgsum-egress-").tempdir()?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        let socket = directory.path().join("proxy.sock");
        let listener = UnixListener::bind(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let stop = Stop {
            local: Cancellation::default(),
            parent: cancellation.clone(),
            deadline: Instant::now() + limits.duration,
        };
        let signal = stop.local.clone();
        let worker = std::thread::Builder::new()
            .name("tgsum-egress".into())
            .spawn(move || serve(listener, destination, limits, stop, dial))?;
        Ok(Self {
            _directory: directory,
            socket,
            stop: signal,
            worker: Some(worker),
        })
    }

    /// Mount only this socket into a private network namespace; do not expose
    /// its parent directory, host network, or unrelated Unix sockets.
    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    pub fn finish(mut self) -> Report {
        self.join()
    }

    fn join(&mut self) -> Report {
        self.stop.cancel();
        self.worker
            .take()
            .map(|worker| {
                worker.join().unwrap_or_else(|_| Report {
                    failures: vec![Failure::Worker],
                    ..Report::default()
                })
            })
            .unwrap_or_default()
    }
}
impl Drop for InferenceGateway {
    fn drop(&mut self) {
        self.join();
    }
}

fn serve(
    listener: UnixListener,
    destination: Destination,
    limits: Limits,
    stop: Stop,
    dial: Arc<Dial>,
) -> Report {
    let total = Arc::new(AtomicU64::new(0));
    let mut workers: Vec<JoinHandle<Result<(), Failure>>> = Vec::new();
    let mut report = Report::default();
    while stop.check().is_ok() {
        // Count all attempts, including rejected handshakes, to bound work.
        if report.accepted >= limits.connections {
            break;
        }
        match listener.accept() {
            Ok((mut client, _)) => {
                report.accepted += 1;
                if workers
                    .iter()
                    .filter(|worker| !worker.is_finished())
                    .count()
                    >= limits.concurrent
                {
                    report.failures.push(Failure::Connections);
                    continue;
                }
                let stop = stop.clone();
                let dial = dial.clone();
                let total = total.clone();
                let result = std::thread::Builder::new()
                    .name("tgsum-tunnel".into())
                    .spawn(move || {
                        let hello = transport::handshake(&mut client, destination, &stop, limits)?;
                        let upstream = dial(destination, &stop)?;
                        transport::forward(client, upstream, &hello, &stop, limits, &total)
                    });
                match result {
                    Ok(worker) => workers.push(worker),
                    Err(_) => report.failures.push(Failure::Worker),
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(_) => {
                report.failures.push(Failure::Io);
                break;
            }
        }
    }
    drop(listener);
    // Continue supervising existing tunnels after the total admission cap.
    while workers.iter().any(|worker| !worker.is_finished()) && stop.check().is_ok() {
        std::thread::sleep(Duration::from_millis(2));
    }
    stop.local.cancel();
    for worker in workers {
        match worker.join().unwrap_or(Err(Failure::Worker)) {
            Ok(()) => report.completed += 1,
            Err(failure) => report.failures.push(failure),
        }
    }
    report.admitted_bytes = total.load(Ordering::Relaxed);
    report
}
