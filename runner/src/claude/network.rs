//! Explicit network capability with fixed receiver and same-process preflight.
use super::{
    ClaudeRequest, EndpointPolicy, SelectedAuthFile, LINUX_EGRESS_PROFILE, VERSION_STDOUT,
};
use crate::{
    egress, linux, AdapterContract, AuthAvailability, Cancellation, QualifiedAdapter, RunLimits,
    RunOutput, RunnerError, RuntimeFile, RuntimeSpec,
};
use std::path::PathBuf;

/// Separate opt-in capability; never acquired by OfflineRunner or discovery.
pub struct ClaudeNetworkRunner {
    pub(super) backend: linux::Backend,
    info: QualifiedAdapter,
}

impl ClaudeNetworkRunner {
    /// Trusted runtime selection and captured fixed-location endpoint policy.
    /// Version qualification is offline and receives no credentials or corpus.
    pub fn qualify(
        relay: PathBuf,
        claude: PathBuf,
        mut files: Vec<RuntimeFile>,
        policy: EndpointPolicy,
        cancel: &Cancellation,
    ) -> Result<Self, RunnerError> {
        if cancel.is_cancelled() {
            return Err(RunnerError::Cancelled);
        }
        if !cfg!(target_arch = "x86_64") {
            return Err(RunnerError::ExportOnly(
                "unqualified Claude egress architecture",
            ));
        }
        files.push(RuntimeFile {
            source: claude,
            guest: "/runtime/claude".into(),
        });
        let mut version = format!("{}\n", egress::CLAUDE_RELAY_VERSION).into_bytes();
        version.extend_from_slice(VERSION_STDOUT);
        let backend = linux::Backend::qualify_claude(
            &AdapterContract {
                id: "claude".into(),
                isolation_profile: LINUX_EGRESS_PROFILE.into(),
                version_probe: super::version_probe(),
                expected_version_output: version.clone(),
            },
            RuntimeSpec {
                executable: relay,
                files,
            },
            policy,
            cancel,
        )?;
        Ok(Self {
            backend,
            info: QualifiedAdapter {
                id: "claude".into(),
                version_output: String::from_utf8(version).expect("static version"),
                isolation_profile: LINUX_EGRESS_PROFILE,
                authentication: AuthAvailability::Unknown,
            },
        })
    }

    pub fn info(&self) -> &QualifiedAdapter {
        &self.info
    }

    /// Caller must display the selected scope/model and api.anthropic.com before
    /// explicit Run. Only the selected read-only credential is supplied; refresh
    /// hosts stay denied, and any gateway failure invalidates the result.
    pub fn run(
        &self,
        request: &ClaudeRequest<'_>,
        auth: &SelectedAuthFile,
        limits: RunLimits,
        cancel: &Cancellation,
    ) -> Result<NetworkOutput, RunnerError> {
        limits.validate(request.invocation()?)?;
        auth.validate()?;
        let gateway = egress::InferenceGateway::start(
            egress::Destination::Anthropic,
            egress::Limits {
                duration: limits.timeout,
                ..egress::Limits::default()
            },
            cancel,
        )?;
        self.run_with_gateway(request, auth, limits, cancel, gateway)
    }

    pub(super) fn run_with_gateway(
        &self,
        request: &ClaudeRequest<'_>,
        auth: &SelectedAuthFile,
        limits: RunLimits,
        cancel: &Cancellation,
        gateway: egress::InferenceGateway,
    ) -> Result<NetworkOutput, RunnerError> {
        let process = self.backend.run_with_access(
            request.context().directory(),
            request.invocation()?,
            limits,
            cancel,
            linux::Access {
                auth: Some(linux::Auth::Claude(auth)),
                gateway: Some(&gateway),
            },
        )?;
        Ok(NetworkOutput {
            process,
            gateway: gateway.finish(),
            destination: egress::Destination::Anthropic,
        })
    }
}

#[derive(Debug)]
pub struct NetworkOutput {
    pub process: RunOutput,
    pub gateway: egress::Report,
    pub destination: egress::Destination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkDecodeError {
    Response(super::DecodeError),
    Gateway(egress::Failure),
    MissingTransport,
}
impl std::fmt::Display for NetworkDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Response(error) => error.fmt(f),
            Self::Gateway(reason) => write!(f, "Claude transport rejected: {reason:?}"),
            Self::MissingTransport => f.write_str("Claude returned without a completed transport"),
        }
    }
}
impl std::error::Error for NetworkDecodeError {}

impl NetworkOutput {
    /// In particular, a denied proactive OAuth refresh cannot be hidden by a
    /// later successful Messages response and exit 0. Never advance a baseline
    /// based on the CLI JSON alone.
    pub fn decode<T: serde::de::DeserializeOwned>(
        &self,
        model: &str,
        validate: impl FnOnce(&T) -> bool,
    ) -> Result<super::Decoded<T>, NetworkDecodeError> {
        if !self.process.process_succeeded() {
            return Err(NetworkDecodeError::Response(super::DecodeError::Process {
                termination: self.process.termination,
                exit_code: self.process.exit_code,
            }));
        }
        if let Some(failure) = self.gateway.failures.first() {
            return Err(NetworkDecodeError::Gateway(*failure));
        }
        if self.gateway.completed == 0 || self.gateway.admitted_bytes == 0 {
            return Err(NetworkDecodeError::MissingTransport);
        }
        super::decode(&self.process, model, validate).map_err(NetworkDecodeError::Response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_denied_transport_is_rejected_before_decoding_or_validation() {
        let mut output = NetworkOutput {
            process: RunOutput {
                termination: crate::Termination::Exited,
                exit_code: Some(0),
                stdout: vec![],
                stderr: vec![],
            },
            gateway: egress::Report::default(),
            destination: egress::Destination::Anthropic,
        };
        assert_eq!(
            output
                .decode::<serde_json::Value>("fixture", |_| panic!("no transport"))
                .unwrap_err(),
            NetworkDecodeError::MissingTransport
        );
        output.gateway.completed = 1;
        output.gateway.admitted_bytes = 1;
        output
            .gateway
            .failures
            .push(egress::Failure::ConnectRequest);
        assert_eq!(
            output
                .decode::<serde_json::Value>("fixture", |_| panic!("denied refresh"))
                .unwrap_err(),
            NetworkDecodeError::Gateway(egress::Failure::ConnectRequest)
        );
        output.process.termination = crate::Termination::Cancelled;
        assert!(matches!(
            output
                .decode::<serde_json::Value>("fixture", |_| panic!("cancelled"))
                .unwrap_err(),
            NetworkDecodeError::Response(super::super::DecodeError::Process {
                termination: crate::Termination::Cancelled,
                ..
            })
        ));
    }
}
