//! Fixed system library/public CA inputs, never credential/profile discovery.
use crate::{RunnerError, RuntimeFile};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub(crate) fn system_files(libraries: &[&str]) -> Result<Vec<RuntimeFile>, RunnerError> {
    let mut files = Vec::new();
    for name in libraries {
        let guest = if *name == "ld-linux-x86-64.so.2" {
            PathBuf::from("/lib64/ld-linux-x86-64.so.2")
        } else {
            Path::new("/usr/lib").join(name)
        };
        let source = ["/usr/lib", "/lib/x86_64-linux-gnu", "/usr/lib64"]
            .into_iter()
            .map(|dir| Path::new(dir).join(name))
            .find_map(|p| trusted_file(&p).ok())
            .ok_or(RunnerError::ExportOnly(
                "qualified Linux runtime library is missing",
            ))?;
        files.push(RuntimeFile { source, guest });
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
