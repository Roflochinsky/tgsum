//! Fixed Linux native-runtime manifest. Never runs ldd, shells, npm wrappers or
//! user profile discovery. Namespace version qualification is still mandatory.
use super::{CodexNetworkRunner, CodexRequest};
use crate::{Cancellation, RunnerError};
use std::path::PathBuf;

impl CodexNetworkRunner {
    pub fn qualify_installed(
        request: &CodexRequest<'_>,
        relay: PathBuf,
        codex: PathBuf,
        cancel: &Cancellation,
    ) -> Result<Self, RunnerError> {
        let mut files = crate::native_runtime::system_files(&[
            "libc.so.6",
            "libgcc_s.so.1",
            "ld-linux-x86-64.so.2",
        ])?;
        files.push(request.schema_runtime_file());
        Self::qualify(relay, codex, files, cancel)
    }
}
