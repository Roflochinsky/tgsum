//! Fixed launcher inside the runner's network namespace.
use std::ffi::OsString;
use std::io;
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{transport, Failure, Limits, Stop, SOCKET_PATH};
use crate::Cancellation;

pub const RELAY_VERSION: &str = "tgsum-codex-relay 1";

/// Launcher entry point, not a host-side adapter interface. Fixed child and
/// socket paths; no inherited auth/endpoint/proxy/config environment.
pub fn codex_relay(args: Vec<OsString>) -> Result<i32, &'static str> {
    if args == ["--version"] {
        println!("{RELAY_VERSION}");
        let status = Command::new("/runtime/codex")
            .env_clear()
            .arg("--version")
            // The version runner supplies an empty pipe; /dev is not mounted.
            .status()
            .map_err(|_| "Codex version probe failed")?;
        return Ok(status.code().unwrap_or(125));
    }
    if !Path::new(SOCKET_PATH).exists() {
        return Err("gateway socket is absent");
    }
    std::fs::create_dir_all("/home/agent/.codex").map_err(|_| "private Codex home setup failed")?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| "relay bind failed")?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "relay setup failed")?;
    let proxy = format!(
        "http://{}",
        listener.local_addr().map_err(|_| "relay address failed")?
    );
    let stop = Stop {
        local: Cancellation::default(),
        parent: Cancellation::default(),
        deadline: Instant::now() + Duration::from_secs(1800),
    };
    let limits = Limits {
        duration: Duration::from_secs(1800),
        idle: Duration::from_secs(60),
        total_bytes: 256 * 1024 * 1024,
        connection_bytes: 64 * 1024 * 1024,
        connections: 32,
        ..Limits::default()
    };
    let total = Arc::new(AtomicU64::new(0));
    let mut workers: Vec<JoinHandle<Result<(), Failure>>> = Vec::new();
    let mut command = Command::new("/runtime/codex");
    command
        .env_clear()
        .current_dir("/context")
        .args(args)
        .env("HOME", "/home/agent")
        .env("CODEX_HOME", "/home/agent/.codex")
        .env("TMPDIR", "/tmp")
        .env("PATH", "/runtime")
        .env("LANG", "C")
        .env("PWD", "/context")
        .env("NO_PROXY", "")
        .env("no_proxy", "");
    for name in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
        command.env(name, &proxy);
    }
    // A separately staged public trust bundle. No host CA/env discovery here.
    if Path::new("/runtime/provider-ca.pem").is_file() {
        command.env("CODEX_CA_CERTIFICATE", "/runtime/provider-ca.pem");
    }
    let mut child = command.spawn().map_err(|_| "Codex launch failed")?;
    let mut accepted = 0;
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status.code().unwrap_or(125)),
            Err(_) => break Err("Codex wait failed"),
            _ => {}
        }
        if stop.check().is_err() {
            break Err("relay deadline exceeded");
        }
        if accepted < limits.connections {
            match listener.accept() {
                Ok((socket, peer)) => {
                    accepted += 1;
                    if !peer.ip().is_loopback()
                        || workers.iter().filter(|w| !w.is_finished()).count() >= limits.concurrent
                    {
                        continue;
                    }
                    let stop = stop.clone();
                    let total = total.clone();
                    match std::thread::Builder::new()
                        .name("tgsum-proxy-relay".into())
                        .spawn(move || {
                            let host = connect_host(Path::new(SOCKET_PATH))?;
                            // Opaque copy between the only host socket and the
                            // local TCP client. Host gateway validates CONNECT/TLS.
                            transport::forward(host, socket, &[], &stop, limits, &total)
                        }) {
                        Ok(worker) => workers.push(worker),
                        Err(_) => break Err("relay worker failed"),
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(_) => break Err("relay accept failed"),
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    stop.local.cancel();
    drop(listener);
    for worker in workers {
        let _ = worker.join();
    }
    result
}

fn connect_host(path: &Path) -> Result<UnixStream, Failure> {
    use rustix::net::{
        connect, socket_with, AddressFamily, SocketAddrUnix, SocketFlags, SocketType,
    };
    let socket = socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        None,
    )
    .map_err(|_| Failure::Io)?;
    let address = SocketAddrUnix::new(path).map_err(|_| Failure::Io)?;
    // Linux AF_UNIX completes locally or returns EAGAIN when the accept queue
    // is full. Reject that connection; never block cleanup joining a worker.
    connect(&socket, &address).map_err(|_| Failure::Io)?;
    Ok(socket.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_host_socket_queue_never_blocks_relay_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("proxy.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        rustix::net::listen(&listener, 0).unwrap();
        let _first = connect_host(&path).unwrap();
        let started = Instant::now();
        assert_eq!(connect_host(&path).unwrap_err(), Failure::Io);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
