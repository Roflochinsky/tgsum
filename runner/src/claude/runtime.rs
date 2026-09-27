//! Native Linux executable selection; no shell, ldd, profile or network probe.
use super::{ClaudeNetworkRunner, EndpointPolicy};
use crate::{Cancellation, RunnerError};
use std::path::PathBuf;

impl ClaudeNetworkRunner {
    pub fn qualify_installed(
        relay: PathBuf,
        claude: PathBuf,
        policy: EndpointPolicy,
        cancel: &Cancellation,
    ) -> Result<Self, RunnerError> {
        let files = crate::native_runtime::system_files(&[
            "librt.so.1",
            "libc.so.6",
            "libpthread.so.0",
            "libdl.so.2",
            "libm.so.6",
            "libgcc_s.so.1",
            "ld-linux-x86-64.so.2",
        ])?;
        Self::qualify(relay, claude, files, policy, cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "explicit native Claude/relay; version only with synthetic endpoint policy, no auth/network"]
    fn installed_claude_manifest_qualifies_without_accounts() {
        let root = tempfile::tempdir().unwrap();
        let cancel = Cancellation::default();
        let relay: PathBuf = std::env::var_os("TGSUM_CLAUDE_RELAY_TEST_BINARY")
            .expect("explicit relay")
            .into();
        let binary: PathBuf = std::env::var_os("TGSUM_CLAUDE_TEST_BINARY")
            .expect("explicit native binary")
            .into();
        let policy = || EndpointPolicy::fixture(root.path(), &cancel).unwrap();
        let runner =
            ClaudeNetworkRunner::qualify_installed(relay.clone(), binary, policy(), &cancel)
                .unwrap();
        assert_eq!(runner.info().id, "claude");
        assert_eq!(
            runner.info().authentication,
            crate::AuthAvailability::Unknown
        );
        assert!(ClaudeNetworkRunner::qualify_installed(
            relay,
            "/usr/bin/true".into(),
            policy(),
            &cancel
        )
        .is_err());
    }
}
