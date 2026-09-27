//! Decode the transport result separately from the CLI protocol. There is no
//! public cloud launcher until managed policy and lifecycle are qualified.
use crate::{egress, RunOutput};

#[derive(Debug)]
pub struct NetworkOutput {
    pub process: RunOutput,
    pub gateway: egress::Report,
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
