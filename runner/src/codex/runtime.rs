//! Fixed Linux native-runtime manifest. Never runs ldd, shells, npm wrappers or
//! user profile discovery. Namespace version qualification is still mandatory.
use super::{CodexNetworkRunner, CodexRequest};
use crate::{Cancellation, RunnerError, RuntimeFile};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

impl CodexNetworkRunner {
    pub fn qualify_installed(
        request: &CodexRequest<'_>,
        relay: PathBuf,
        codex: PathBuf,
        cancel: &Cancellation,
    ) -> Result<Self, RunnerError> {
        let mut files = system_files()?;
        files.push(request.schema_runtime_file());
        Self::qualify(relay, codex, files, cancel)
    }
}

fn system_files() -> Result<Vec<RuntimeFile>, RunnerError> {
    let mut files = Vec::new();
    for (name, guest) in [
        ("libc.so.6", "/usr/lib/libc.so.6"),
        ("libgcc_s.so.1", "/usr/lib/libgcc_s.so.1"),
        ("ld-linux-x86-64.so.2", "/lib64/ld-linux-x86-64.so.2"),
    ] {
        let source = ["/usr/lib", "/lib/x86_64-linux-gnu", "/usr/lib64"]
            .into_iter()
            .map(|dir| Path::new(dir).join(name))
            .find_map(|p| trusted_file(&p).ok())
            .ok_or(RunnerError::ExportOnly(
                "qualified Linux runtime library is missing",
            ))?;
        files.push(RuntimeFile {
            source,
            guest: guest.into(),
        });
    }
    let source = [
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
    ]
    .into_iter()
    .find_map(|p| trusted_file(Path::new(p)).ok())
    .ok_or(RunnerError::ExportOnly(
        "system public CA bundle is missing",
    ))?;
    files.push(RuntimeFile {
        source,
        guest: "/runtime/provider-ca.pem".into(),
    });
    Ok(files)
}

fn trusted_file(path: &Path) -> Result<PathBuf, RunnerError> {
    let path = path.canonicalize()?;
    let m = std::fs::metadata(&path)?;
    if !m.is_file()
        || m.uid() != 0
        || m.mode() & 0o022 != 0
        || m.len() == 0
        || m.len() > 32 * 1024 * 1024
    {
        return Err(RunnerError::ExportOnly("untrusted system runtime file"));
    }
    Ok(path)
}
