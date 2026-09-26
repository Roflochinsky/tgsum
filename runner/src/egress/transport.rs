use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::{hello, Destination, Failure, Limits, Stop};

const TICK: Duration = Duration::from_millis(2);

fn transient(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

fn check(stop: &Stop, deadline: Instant) -> Result<(), Failure> {
    stop.check()?;
    if Instant::now() >= deadline {
        return Err(Failure::Timeout);
    }
    Ok(())
}

fn read_exact(
    stream: &mut impl Read,
    mut bytes: &mut [u8],
    stop: &Stop,
    deadline: Instant,
) -> Result<(), Failure> {
    while !bytes.is_empty() {
        check(stop, deadline)?;
        match stream.read(bytes) {
            Ok(0) => return Err(Failure::Io),
            Ok(n) => bytes = &mut bytes[n..],
            Err(e) if transient(&e) => std::thread::sleep(TICK),
            Err(_) => return Err(Failure::Io),
        }
    }
    Ok(())
}

fn write_all(
    stream: &mut impl Write,
    mut bytes: &[u8],
    stop: &Stop,
    deadline: Instant,
) -> Result<(), Failure> {
    while !bytes.is_empty() {
        check(stop, deadline)?;
        match stream.write(bytes) {
            Ok(0) => return Err(Failure::Io),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if transient(&e) => std::thread::sleep(TICK),
            Err(_) => return Err(Failure::Io),
        }
    }
    Ok(())
}

pub(super) fn handshake(
    stream: &mut UnixStream,
    destination: Destination,
    stop: &Stop,
    limits: Limits,
) -> Result<Vec<u8>, Failure> {
    stream.set_nonblocking(true).map_err(|_| Failure::Io)?;
    let deadline = Instant::now() + limits.handshake;
    let mut request = Vec::new();
    loop {
        if request.len() >= 8192 {
            return Err(Failure::ConnectRequest);
        }
        let mut byte = [0];
        // No buffered reader may eat a pipelined ClientHello and lose it.
        read_exact(stream, &mut byte, stop, deadline)?;
        request.push(byte[0]);
        if request.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    super::policy::connect_request(&request, destination)?;
    // No content-length/transfer-encoding on CONNECT success (RFC 9110).
    // Upstream is still unopened; TLS name is checked before any external byte.
    write_all(
        stream,
        b"HTTP/1.1 200 Connection Established\r\n\r\n",
        stop,
        deadline,
    )?;
    let mut wire = Vec::new();
    let mut handshake = Vec::new();
    for _ in 0..16 {
        let mut header = [0; 5];
        read_exact(stream, &mut header, stop, deadline)?;
        let size = u16::from_be_bytes([header[3], header[4]]) as usize;
        if header[0] != 22
            || header[1] != 3
            || ![1, 3].contains(&header[2])
            || size == 0
            || size > 16384
            || handshake.len() + size > 65536
        {
            return Err(Failure::ClientHello);
        }
        let start = handshake.len();
        handshake.resize(start + size, 0);
        read_exact(stream, &mut handshake[start..], stop, deadline)?;
        wire.extend_from_slice(&header);
        wire.extend_from_slice(&handshake[start..]);
        if handshake.len() >= 4 {
            let length = ((handshake[1] as usize) << 16)
                | ((handshake[2] as usize) << 8)
                | handshake[3] as usize;
            if handshake[0] != 1 || length > 65532 || handshake.len() > length + 4 {
                return Err(Failure::ClientHello);
            }
            if handshake.len() == length + 4 {
                hello::validate(&handshake[4..], destination.host())?;
                return Ok(wire);
            }
        }
    }
    Err(Failure::ClientHello)
}

fn charge(
    n: usize,
    connection: &mut u64,
    total: &AtomicU64,
    limits: Limits,
) -> Result<(), Failure> {
    let n = n as u64;
    if *connection + n > limits.connection_bytes {
        return Err(Failure::Budget);
    }
    total
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
            used.checked_add(n).filter(|sum| *sum <= limits.total_bytes)
        })
        .map_err(|_| Failure::Budget)?;
    *connection += n;
    Ok(())
}

struct Pending {
    bytes: [u8; 16384],
    start: usize,
    end: usize,
    eof: bool,
    shutdown: bool,
}
impl Default for Pending {
    fn default() -> Self {
        Self {
            bytes: [0; 16384],
            start: 0,
            end: 0,
            eof: false,
            shutdown: false,
        }
    }
}
impl Pending {
    fn read(
        &mut self,
        source: &mut impl Read,
        connection: &mut u64,
        total: &AtomicU64,
        limits: Limits,
    ) -> Result<bool, Failure> {
        if self.eof || self.start != self.end {
            return Ok(false);
        }
        match source.read(&mut self.bytes) {
            Ok(0) => {
                self.eof = true;
                Ok(true)
            }
            Ok(n) => {
                charge(n, connection, total, limits)?;
                self.start = 0;
                self.end = n;
                Ok(true)
            }
            Err(e) if transient(&e) => Ok(false),
            Err(_) => Err(Failure::Io),
        }
    }
    fn write(&mut self, destination: &mut impl Write) -> Result<bool, Failure> {
        if self.start == self.end {
            return Ok(false);
        }
        match destination.write(&self.bytes[self.start..self.end]) {
            Ok(0) => Err(Failure::Io),
            Ok(n) => {
                self.start += n;
                Ok(true)
            }
            Err(e) if transient(&e) => Ok(false),
            Err(_) => Err(Failure::Io),
        }
    }
    fn ready_to_shutdown(&mut self) -> bool {
        if self.eof && self.start == self.end && !self.shutdown {
            self.shutdown = true;
            true
        } else {
            false
        }
    }
}

pub(super) fn forward(
    mut client: UnixStream,
    mut upstream: TcpStream,
    hello: &[u8],
    stop: &Stop,
    limits: Limits,
    total: &AtomicU64,
) -> Result<(), Failure> {
    upstream.set_nonblocking(true).map_err(|_| Failure::Io)?;
    let mut connection = 0;
    charge(hello.len(), &mut connection, total, limits)?;
    write_all(
        &mut upstream,
        hello,
        stop,
        Instant::now() + limits.handshake,
    )?;
    let mut upload = Pending::default();
    let mut download = Pending::default();
    let mut active = Instant::now();
    loop {
        check(stop, active + limits.idle)?;
        let mut progress = upload.read(&mut client, &mut connection, total, limits)?;
        progress |= download.read(&mut upstream, &mut connection, total, limits)?;
        progress |= upload.write(&mut upstream)?;
        progress |= download.write(&mut client)?;
        if upload.ready_to_shutdown() {
            let _ = upstream.shutdown(Shutdown::Write);
        }
        if download.ready_to_shutdown() {
            let _ = client.shutdown(Shutdown::Write);
        }
        if upload.shutdown && download.shutdown {
            return Ok(());
        }
        if progress {
            active = Instant::now();
        } else {
            std::thread::sleep(TICK);
        }
    }
}
