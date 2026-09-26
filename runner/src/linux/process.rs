use std::io::{self, Read, Write};
use std::os::fd::AsFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rustix::fs::{fcntl_getfl, fcntl_setfl, OFlags};

use crate::{Cancellation, RunLimits, RunOutput, RunnerError, Termination};

const TICK: Duration = Duration::from_millis(5);
const CLEANUP: Duration = Duration::from_secs(2);

struct ChildGuard {
    child: Child,
    reaped: bool,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

// SAFETY: This narrowly scoped pre_exec hook performs only a Linux syscall and
// errno conversion after fork, without allocation, environment access or locks.
// CLOEXEC keeps Rust's internal exec-error pipe usable until exec. Failure is
// propagated by spawn; there is no less restrictive fallback. See the dated
// runner-isolation research for the inherited-FD hole in bwrap itself.
#[allow(unsafe_code)]
fn close_inherited_descriptors(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            if libc::syscall(
                libc::SYS_close_range,
                3u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            ) == -1
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

fn nonblocking(fd: &impl AsFd) -> io::Result<()> {
    let flags = fcntl_getfl(fd)?;
    fcntl_setfl(fd, flags | OFlags::NONBLOCK)?;
    Ok(())
}

/// Read at most 64 KiB each tick so a flooding stream cannot starve cancellation,
/// the other stream, or stdin. Return (EOF, limit exceeded).
fn drain(input: &mut impl Read, output: &mut Vec<u8>, limit: usize) -> io::Result<(bool, bool)> {
    let mut buffer = [0; 8192];
    for _ in 0..8 {
        match input.read(&mut buffer) {
            Ok(0) => return Ok((true, false)),
            Ok(count) => {
                let available = limit - output.len();
                output.extend_from_slice(&buffer[..count.min(available)]);
                if count > available {
                    return Ok((false, true));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok((false, false))
}

pub(super) fn execute(
    mut command: Command,
    input: &[u8],
    limits: RunLimits,
    cancellation: &Cancellation,
) -> Result<RunOutput, RunnerError> {
    if cancellation.is_cancelled() {
        return Err(RunnerError::Cancelled);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    close_inherited_descriptors(&mut command);
    let started = Instant::now();
    let mut owned = ChildGuard {
        child: command.spawn()?,
        reaped: false,
    };
    let mut stdin = owned.child.stdin.take();
    let mut stdout = owned.child.stdout.take().expect("piped stdout");
    let mut stderr = owned.child.stderr.take().expect("piped stderr");
    nonblocking(stdin.as_ref().expect("piped stdin"))?;
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let mut output = RunOutput {
        termination: Termination::Exited,
        exit_code: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    let mut sent = 0;
    let mut exited_at = None;
    let mut killed = false;
    loop {
        let (out_eof, out_overflow) = drain(&mut stdout, &mut output.stdout, limits.stdout_bytes)?;
        let (err_eof, err_overflow) = drain(&mut stderr, &mut output.stderr, limits.stderr_bytes)?;
        let reason = if out_overflow {
            Some(Termination::StdoutLimit)
        } else if err_overflow {
            Some(Termination::StderrLimit)
        } else if cancellation.is_cancelled() {
            Some(Termination::Cancelled)
        } else if started.elapsed() >= limits.timeout {
            Some(Termination::TimedOut)
        } else {
            None
        };
        if let Some(reason) = reason {
            if !killed {
                output.termination = reason;
                if !owned.reaped {
                    owned.child.kill()?;
                }
                killed = true;
                stdin = None;
            }
        }
        if let Some(pipe) = &mut stdin {
            match pipe.write(&input[sent..]) {
                Ok(count) => sent += count,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                // A process may deliberately close stdin and return a diagnostic.
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => stdin = None,
                Err(e) => return Err(e.into()),
            }
            if sent == input.len() {
                stdin = None;
            }
        }
        if !owned.reaped {
            if let Some(status) = owned.child.try_wait()? {
                owned.reaped = true;
                output.exit_code = status.code();
                exited_at = Some(Instant::now());
                stdin = None;
            }
        }
        if owned.reaped && out_eof && err_eof {
            break;
        }
        if exited_at.is_some_and(|exit: Instant| exit.elapsed() >= CLEANUP) {
            output.termination = Termination::CleanupTimedOut;
            break;
        }
        std::thread::sleep(TICK);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_error_survives_close_range_hook() {
        let result = execute(
            Command::new("/tgsum-no-such-executable"),
            &[],
            RunLimits::default(),
            &Cancellation::default(),
        );
        assert!(matches!(result, Err(RunnerError::Io(e)) if e.kind() == io::ErrorKind::NotFound));
    }
}
