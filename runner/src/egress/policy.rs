use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::os::unix::fs::MetadataExt;
use std::process::Command;
use std::time::Duration;

use super::{Destination, Failure, Stop};
use crate::{linux::process, RunLimits};

pub(super) fn connect_request(bytes: &[u8], destination: Destination) -> Result<(), Failure> {
    let text = std::str::from_utf8(bytes).map_err(|_| Failure::ConnectRequest)?;
    if !text.ends_with("\r\n\r\n") {
        return Err(Failure::ConnectRequest);
    }
    let mut lines = text[..text.len() - 4].split("\r\n");
    let target = format!("{}:443", destination.host());
    if lines.next() != Some(format!("CONNECT {target} HTTP/1.1").as_str()) {
        return Err(Failure::ConnectRequest);
    }
    let mut seen = BTreeSet::new();
    let mut host = false;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(Failure::ConnectRequest)?;
        if !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || !value.bytes().all(|c| (32..=126).contains(&c) || c == b'\t')
        {
            return Err(Failure::ConnectRequest);
        }
        let name = name.to_ascii_lowercase();
        if !seen.insert(name.clone()) {
            return Err(Failure::ConnectRequest);
        }
        match name.as_str() {
            "host" if value.trim() == target => host = true,
            "user-agent" | "proxy-connection" | "connection" => {}
            // No body framing, proxy credentials, or forwarded destination.
            _ => return Err(Failure::ConnectRequest),
        }
    }
    host.then_some(()).ok_or(Failure::ConnectRequest)
}

/// Conservative public-unicast subset of the IANA special-purpose registries
/// reviewed 2026-09-27; special-purpose exceptions are intentionally not used.
pub(super) fn public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let n = u32::from(ip);
            ![
                (0x00000000, 8),
                (0x0a000000, 8),
                (0x64400000, 10),
                (0x7f000000, 8),
                (0xa9fe0000, 16),
                (0xac100000, 12),
                (0xc0000000, 24),
                (0xc0000200, 24),
                (0xc0586300, 24),
                (0xc0a80000, 16),
                (0xc6120000, 15),
                (0xc6336400, 24),
                (0xcb007100, 24),
                (0xe0000000, 3),
            ]
            .into_iter()
            .any(|(base, bits)| n >> (32 - bits) == base >> (32 - bits))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            // Only 2000::/3, excluding special 2001::/23, documentation,
            // deprecated 6to4 and 3fff::/20. Mapped/local/NAT64 cannot pass.
            s[0] & 0xe000 == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x0200 || s[1] == 0x0db8))
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] & 0xf000 == 0)
        }
    }
}

pub(super) fn resolved_addresses(bytes: &[u8]) -> Result<Vec<SocketAddr>, Failure> {
    let text = std::str::from_utf8(bytes).map_err(|_| Failure::Resolution)?;
    let mut addresses = BTreeSet::new();
    for line in text.lines() {
        let ip: IpAddr = line
            .split_whitespace()
            .next()
            .ok_or(Failure::Resolution)?
            .parse()
            .map_err(|_| Failure::Resolution)?;
        if !public_address(ip) {
            return Err(Failure::PrivateAddress);
        }
        addresses.insert(SocketAddr::new(ip, 443));
        if addresses.len() > 16 {
            return Err(Failure::Resolution);
        }
    }
    if addresses.is_empty() {
        return Err(Failure::Resolution);
    }
    Ok(addresses.into_iter().collect())
}

pub(super) fn dial(destination: Destination, stop: &Stop) -> Result<TcpStream, Failure> {
    stop.check()?;
    // libc's in-process resolver can block beyond a cancelled thread. A bounded
    // subprocess owns NSS/DNS here, so cancellation kills/reaps the resolver.
    // The name is from Destination, never from the CONNECT request or corpus.
    let metadata = std::fs::metadata("/usr/bin/getent").map_err(|_| Failure::Resolution)?;
    if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o6022 != 0 {
        return Err(Failure::Resolution);
    }
    let mut command = Command::new("/usr/bin/getent");
    command
        .env_clear()
        .current_dir("/")
        .args(["ahosts", destination.host()]);
    let output = process::execute(
        command,
        &[],
        RunLimits {
            timeout: stop.remaining()?.min(Duration::from_secs(5)),
            stdin_bytes: 0,
            stdout_bytes: 8192,
            stderr_bytes: 1024,
        },
        &stop.local,
    )
    .map_err(|_| Failure::Resolution)?;
    stop.check()?;
    if !output.process_succeeded() {
        return Err(Failure::Resolution);
    }
    for address in resolved_addresses(&output.stdout)? {
        stop.check()?;
        // Connect to the validated literal; never resolve a second time.
        if let Ok(stream) =
            TcpStream::connect_timeout(&address, stop.remaining()?.min(Duration::from_secs(1)))
        {
            stop.check()?;
            if stream.peer_addr().map_err(|_| Failure::Io)? != address {
                return Err(Failure::PrivateAddress);
            }
            return Ok(stream);
        }
    }
    Err(Failure::Unreachable)
}
