mod process;

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use tempfile::TempDir;

use crate::{
    AdapterContract, Cancellation, Invocation, RunLimits, RunOutput, RunnerError, RuntimeSpec,
    Termination,
};

const BWRAP: &str = "/usr/bin/bwrap";
const BWRAP_VERSION: &[u8] = b"bubblewrap 0.12.0\n";
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_RUNTIME_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) struct Backend {
    _staging: TempDir,
    mounts: Vec<(PathBuf, PathBuf)>,
}

impl Backend {
    pub(crate) fn qualify(
        contract: &AdapterContract,
        runtime: RuntimeSpec,
        cancellation: &Cancellation,
    ) -> Result<Self, RunnerError> {
        check_helper(cancellation)?;
        let backend = Self::stage(runtime, cancellation)?;
        let empty = tempfile::tempdir()?;
        let output = backend.run(
            empty.path(),
            &contract.version_probe,
            probe_limits(),
            cancellation,
        )?;
        if output.termination == Termination::Cancelled {
            return Err(RunnerError::Cancelled);
        }
        if !output.process_succeeded() || output.stdout != contract.expected_version_output {
            return Err(RunnerError::ExportOnly(
                "adapter version or isolation probe failed",
            ));
        }
        Ok(backend)
    }

    fn stage(runtime: RuntimeSpec, cancellation: &Cancellation) -> Result<Self, RunnerError> {
        if runtime.files.len() > 128 {
            return Err(RunnerError::InvalidRequest("too many runtime files"));
        }
        let mut sources = vec![(runtime.executable, PathBuf::from("/runtime/agent"))];
        let mut destinations = BTreeSet::new();
        destinations.insert(PathBuf::from("/runtime/agent"));
        for file in runtime.files {
            validate_guest(&file.guest)?;
            if destinations
                .iter()
                .any(|d| d.starts_with(&file.guest) || file.guest.starts_with(d))
            {
                return Err(RunnerError::InvalidRequest(
                    "overlapping runtime destinations",
                ));
            }
            destinations.insert(file.guest.clone());
            sources.push((file.source, file.guest));
        }
        let staging = tempfile::Builder::new()
            .prefix("tgsum-runtime-")
            .tempdir()?;
        let mut mounts = Vec::new();
        let mut total = 0;
        for (index, (source, guest)) in sources.into_iter().enumerate() {
            if cancellation.is_cancelled() {
                return Err(RunnerError::Cancelled);
            }
            if !source.is_absolute() {
                return Err(RunnerError::InvalidRequest(
                    "runtime source must be absolute",
                ));
            }
            // Allow installation symlinks, then reject a swapped final link or
            // FIFO. A manifest is trusted host configuration, not chat metadata.
            let source = source.canonicalize()?;
            let mut input = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(source)?;
            let metadata = input.metadata()?;
            if !metadata.is_file()
                || metadata.len() > MAX_FILE_BYTES
                || (index == 0 && metadata.mode() & 0o111 == 0)
            {
                return Err(RunnerError::InvalidRequest(
                    "runtime needs bounded regular files",
                ));
            }
            let destination = staging.path().join(index.to_string());
            let mut output = File::create(&destination)?;
            let mut bytes = 0;
            let mut buffer = [0; 64 * 1024];
            loop {
                if cancellation.is_cancelled() {
                    return Err(RunnerError::Cancelled);
                }
                let count = input.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                bytes += count as u64;
                total += count as u64;
                if bytes > MAX_FILE_BYTES || total > MAX_RUNTIME_BYTES {
                    return Err(RunnerError::InvalidRequest("runtime size limit exceeded"));
                }
                output.write_all(&buffer[..count])?;
            }
            // Copy bytes into a private snapshot before version qualification.
            // Later replacement of the installed executable cannot change it.
            output.set_permissions(fs::Permissions::from_mode(0o500))?;
            mounts.push((destination, guest));
        }
        Ok(Self {
            _staging: staging,
            mounts,
        })
    }

    pub(crate) fn run(
        &self,
        context: &Path,
        invocation: &Invocation,
        limits: RunLimits,
        cancellation: &Cancellation,
    ) -> Result<RunOutput, RunnerError> {
        limits.validate(invocation)?;
        // Recheck the backend on every launch; never silently accept a helper
        // update while the application is running.
        check_helper(cancellation)?;
        let mut command = Command::new(BWRAP);
        command.env_clear().current_dir("/");
        command.args([
            "--unshare-user",
            "--unshare-ipc",
            "--unshare-pid",
            "--unshare-net",
            "--unshare-uts",
            "--unshare-cgroup",
            "--disable-userns",
            "--new-session",
            "--die-with-parent",
            "--clearenv",
            "--cap-drop",
            "ALL",
            "--hostname",
            "tgsum",
            "--setenv",
            "HOME",
            "/home/agent",
            "--setenv",
            "TMPDIR",
            "/tmp",
            "--setenv",
            "PATH",
            "/runtime",
            "--setenv",
            "LANG",
            "C",
            "--size",
            "67108864",
            "--tmpfs",
            "/tmp",
            "--size",
            "67108864",
            "--tmpfs",
            "/home/agent",
        ]);
        for (source, guest) in &self.mounts {
            command.arg("--ro-bind").arg(source).arg(guest);
        }
        command.arg("--ro-bind").arg(context).arg("/context");
        command.args([
            "--chdir",
            "/context",
            "--remount-ro",
            "/",
            "--",
            "/runtime/agent",
        ]);
        command.args(&invocation.args);
        process::execute(command, &invocation.stdin, limits, cancellation)
    }
}

fn validate_guest(path: &Path) -> Result<(), RunnerError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        || !["/runtime", "/lib", "/lib64", "/usr/lib", "/usr/lib64"]
            .iter()
            .any(|root| path.starts_with(root) && path != Path::new(root))
    {
        return Err(RunnerError::InvalidRequest("invalid runtime destination"));
    }
    Ok(())
}

fn probe_limits() -> RunLimits {
    RunLimits {
        timeout: Duration::from_secs(5),
        stdin_bytes: 4096,
        stdout_bytes: 4096,
        stderr_bytes: 4096,
    }
}

fn check_helper(cancellation: &Cancellation) -> Result<(), RunnerError> {
    let metadata =
        fs::metadata(BWRAP).map_err(|_| RunnerError::ExportOnly("bubblewrap is unavailable"))?;
    if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o6022 != 0 {
        return Err(RunnerError::ExportOnly(
            "bubblewrap ownership or mode is unsupported",
        ));
    }
    let mut command = Command::new(BWRAP);
    command.env_clear().current_dir("/").arg("--version");
    let output = process::execute(command, &[], probe_limits(), cancellation)?;
    if output.termination == Termination::Cancelled {
        return Err(RunnerError::Cancelled);
    }
    if !output.process_succeeded() || output.stdout != BWRAP_VERSION {
        return Err(RunnerError::ExportOnly("unreviewed bubblewrap version"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_rejects_non_files_and_overlapping_mounts_before_execution() {
        let directory = tempfile::tempdir().unwrap();
        let fifo = directory.path().join("fifo");
        rustix::fs::mkfifoat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .unwrap();
        for source in [directory.path(), fifo.as_path()] {
            assert!(Backend::stage(
                RuntimeSpec {
                    executable: source.into(),
                    files: vec![]
                },
                &Cancellation::default()
            )
            .is_err());
        }
        let executable = std::env::current_exe().unwrap();
        for guest in [
            "/runtime/agent",
            "/runtime/agent/child",
            "/context/raw.json",
        ] {
            assert!(Backend::stage(
                RuntimeSpec {
                    executable: executable.clone(),
                    files: vec![crate::RuntimeFile {
                        source: executable.clone(),
                        guest: guest.into()
                    }]
                },
                &Cancellation::default()
            )
            .is_err());
        }
    }

    #[test]
    fn runtime_paths_cannot_replace_context_or_expose_whole_directories() {
        for invalid in [
            "/",
            "/context/manifest.json",
            "/home/a",
            "/runtime",
            "/usr/lib",
            "/runtime/../../raw",
            "relative/lib",
        ] {
            assert!(validate_guest(Path::new(invalid)).is_err(), "{invalid}");
        }
        for valid in [
            "/runtime/assets/model",
            "/lib64/ld-linux-x86-64.so.2",
            "/usr/lib/libc.so.6",
        ] {
            validate_guest(Path::new(valid)).unwrap();
        }
    }
}
