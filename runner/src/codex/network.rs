use super::{CodexRequest, SelectedAuthFile, VERSION_STDOUT};
use crate::egress::{Destination, InferenceGateway, Limits, Report, RELAY_VERSION};
use crate::{
    linux, AdapterContract, AuthAvailability, Cancellation, Invocation, QualifiedAdapter,
    RunLimits, RunOutput, RunnerError, RuntimeFile, RuntimeSpec,
};
use std::path::PathBuf;

pub const LINUX_EGRESS_PROFILE: &str = "linux-x86_64-bwrap-codex-egress-v1";

/// A separate opt-in profile. OfflineRunner cannot acquire a gateway.
pub struct CodexNetworkRunner {
    backend: linux::Backend,
    info: QualifiedAdapter,
}

#[derive(Debug)]
pub struct NetworkOutput {
    pub process: RunOutput,
    pub gateway: Report,
    pub destination: Destination,
}

impl CodexNetworkRunner {
    /// Trusted executable/runtime selection; never imported chat configuration.
    /// files includes request.schema_runtime_file(), platform libraries and an
    /// optional explicit public CA at /runtime/provider-ca.pem. No directory mounts.
    pub fn qualify(
        relay: PathBuf,
        codex: PathBuf,
        mut files: Vec<RuntimeFile>,
        cancel: &Cancellation,
    ) -> Result<Self, RunnerError> {
        if !cfg!(target_arch = "x86_64") {
            return Err(RunnerError::ExportOnly(
                "unqualified Codex egress architecture",
            ));
        }
        files.push(RuntimeFile {
            source: codex,
            guest: "/runtime/codex".into(),
        });
        let mut version = format!("{RELAY_VERSION}\n").into_bytes();
        version.extend_from_slice(VERSION_STDOUT);
        let contract = AdapterContract {
            id: "codex".into(),
            isolation_profile: LINUX_EGRESS_PROFILE.into(),
            version_probe: super::version_probe(),
            expected_version_output: version.clone(),
        };
        let backend = linux::Backend::qualify(
            &contract,
            RuntimeSpec {
                executable: relay,
                files,
            },
            cancel,
        )?;
        Ok(Self {
            backend,
            info: QualifiedAdapter {
                id: "codex".into(),
                version_output: String::from_utf8(version).expect("static version"),
                isolation_profile: LINUX_EGRESS_PROFILE,
                authentication: AuthAvailability::Unknown,
            },
        })
    }

    pub fn info(&self) -> &QualifiedAdapter {
        &self.info
    }

    /// The application must Review this destination before invoking. Auth is
    /// passed read-only, never copied; no auto-refresh host or network fallback.
    pub fn run(
        &self,
        request: &CodexRequest<'_>,
        auth: &SelectedAuthFile,
        destination: Destination,
        limits: RunLimits,
        cancel: &Cancellation,
    ) -> Result<NetworkOutput, RunnerError> {
        let invocation = network_invocation(request)?;
        limits.validate(&invocation)?;
        auth.validate()?;
        let gateway = InferenceGateway::start(
            destination,
            Limits {
                duration: limits.timeout,
                ..Limits::default()
            },
            cancel,
        )?;
        self.run_with_gateway(request, Some(auth), destination, limits, cancel, gateway)
    }

    fn run_with_gateway(
        &self,
        request: &CodexRequest<'_>,
        auth: Option<&SelectedAuthFile>,
        destination: Destination,
        limits: RunLimits,
        cancel: &Cancellation,
        gateway: InferenceGateway,
    ) -> Result<NetworkOutput, RunnerError> {
        let invocation = network_invocation(request)?;
        let process = self.backend.run_with_access(
            request.context().directory(),
            &invocation,
            limits,
            cancel,
            linux::Access {
                auth,
                gateway: Some(&gateway),
            },
        )?;
        let gateway = gateway.finish();
        Ok(NetworkOutput {
            process,
            gateway,
            destination,
        })
    }
}

fn network_invocation(request: &CodexRequest<'_>) -> Result<Invocation, RunnerError> {
    let original = request.invocation()?;
    let mut args = original.args.clone();
    if args.pop().as_deref() != Some(std::ffi::OsStr::new("-")) {
        return Err(RunnerError::InvalidRequest("invalid Codex request ending"));
    }
    // Auth mode determines the built-in API/ChatGPT endpoint. Gateway permits
    // only the declared destination; a mismatching auth mode fails closed.
    for setting in [
        "features.respect_system_proxy=false",
        "features.unbounded_connection_retries=false",
        // 0.155.1 reserves built-in IDs; use our own fixed SSE-only profile.
        "model_provider=\"tgsum_openai\"",
        "model_providers.tgsum_openai={name=\"TGSUM OpenAI\",wire_api=\"responses\",requires_openai_auth=true,supports_websockets=false,request_max_retries=0,stream_max_retries=0,stream_idle_timeout_ms=30000}",
    ] { args.extend(["--config".into(), setting.into()]); }
    args.push("-".into());
    Ok(Invocation {
        args,
        stdin: original.stdin.clone(),
    })
}

#[cfg(test)]
mod tests;
