//! Review binding and durable completion for the qualified network runner.
use super::{
    ClaudeNetworkRunner, ClaudeRequest, DecodeError, Decoded, NetworkDecodeError, NetworkOutput,
    SelectedAuthFile, LINUX_EGRESS_PROFILE, VERSION,
};
use crate::egress::Destination;
use crate::{Cancellation, QualifiedAdapter, RunLimits, RunnerError, Termination};
use serde::{de::DeserializeOwned, Serialize};
use std::fmt;
use std::io;
use tgsum_core::analysis::{FailureCode, RunTicket};
use tgsum_core::bundle::EvidenceRef;
use tgsum_core::project::ProjectStore;

pub struct AnalysisJob<'a> {
    request: &'a ClaudeRequest<'a>,
    ticket: &'a RunTicket,
    store: ProjectStore,
    destination: Destination,
}
impl<'a> AnalysisJob<'a> {
    /// The application creates the ticket from the values displayed in Review.
    /// No extra recipient/model/recipe metadata may be supplied after launch.
    pub fn new(
        request: &'a ClaudeRequest<'a>,
        ticket: &'a RunTicket,
    ) -> Result<Self, AnalysisError> {
        let spec = &ticket.request().spec;
        let destination = match spec.destination.as_str() {
            "api.anthropic.com" => Destination::Anthropic,
            _ => return Err(AnalysisError::Binding),
        };
        if spec.agent != "claude"
            || spec.agent_version != VERSION
            || spec.isolation_profile != LINUX_EGRESS_PROFILE
            || spec.model != request.model()
        {
            return Err(AnalysisError::Binding);
        }
        let store = request
            .context()
            .analysis_store(ticket.request())
            .map_err(AnalysisError::Launch)?;
        Ok(Self {
            request,
            ticket,
            store,
            destination,
        })
    }

    pub(super) fn check(
        &self,
        info: &QualifiedAdapter,
        cancel: &Cancellation,
    ) -> Result<(), AnalysisError> {
        if info.id != "claude"
            || info.isolation_profile != self.ticket.request().spec.isolation_profile
        {
            return Err(AnalysisError::Binding);
        }
        self.request.invocation().map_err(AnalysisError::Launch)?;
        self.store
            .check_pending_analysis(self.ticket, || cancel.is_cancelled())
            .map_err(AnalysisError::Storage)
    }

    fn fail(&self, code: FailureCode, cancelled: bool) -> Result<(), AnalysisError> {
        if cancelled {
            self.store.cancel_analysis(self.ticket)
        } else {
            self.store.fail_analysis(self.ticket, code)
        }
        .map_err(AnalysisError::Storage)
    }

    pub(super) fn finish<T: DeserializeOwned + Serialize>(
        self,
        output: NetworkOutput,
        cancel: &Cancellation,
        validate: impl FnOnce(&T) -> io::Result<Vec<EvidenceRef>>,
    ) -> Result<CompletedAnalysis<T>, AnalysisError> {
        if cancel.is_cancelled() {
            self.fail(FailureCode::Interrupted, true)?;
            return Err(AnalysisError::Launch(RunnerError::Cancelled));
        }
        if output.destination != self.destination {
            self.fail(FailureCode::Transport, false)?;
            return Err(AnalysisError::Binding);
        }
        let mut evidence = None;
        let result = match output.decode(self.request.model(), |answer: &T| {
            evidence = validate(answer).ok();
            evidence.is_some()
        }) {
            Ok(result) => result,
            Err(error) => {
                let code = match &error {
                    NetworkDecodeError::Gateway(_) | NetworkDecodeError::MissingTransport => {
                        FailureCode::Transport
                    }
                    NetworkDecodeError::Response(DecodeError::Process {
                        termination: Termination::TimedOut,
                        ..
                    }) => FailureCode::TimedOut,
                    NetworkDecodeError::Response(DecodeError::Process { .. }) => FailureCode::Agent,
                    _ => FailureCode::InvalidResult,
                };
                self.fail(code, output.process.termination == Termination::Cancelled)?;
                return Err(AnalysisError::Response(error));
            }
        };
        let saved = self.store.save_analysis_result(
            self.ticket,
            &result.value,
            |_| Ok(evidence.expect("successful decoder called validator")),
            || cancel.is_cancelled(),
        );
        if let Err(error) = saved {
            self.fail(FailureCode::InvalidResult, cancel.is_cancelled())?;
            return Err(AnalysisError::Storage(error));
        }
        // A stale Project keeps a validated artifact for review, never advances
        // a different scope's baseline. Recovery uses the same ticket explicitly.
        let committed_revision = self
            .store
            .commit_analysis(self.ticket, || cancel.is_cancelled())
            .map_err(AnalysisError::Storage)?;
        Ok(CompletedAnalysis {
            run_id: self.ticket.request().run_id.clone(),
            committed_revision,
            result,
        })
    }
}

impl ClaudeNetworkRunner {
    pub fn run_analysis<T: DeserializeOwned + Serialize>(
        &self,
        job: AnalysisJob<'_>,
        auth: &SelectedAuthFile,
        limits: RunLimits,
        cancel: &Cancellation,
        validate: impl FnOnce(&T) -> io::Result<Vec<EvidenceRef>>,
    ) -> Result<CompletedAnalysis<T>, AnalysisError> {
        if let Err(error) = job.check(self.info(), cancel) {
            job.fail(FailureCode::Interrupted, cancel.is_cancelled())?;
            return Err(error);
        }
        let output = match self.run(job.request, auth, limits, cancel) {
            Ok(output) => output,
            Err(error) => {
                job.fail(FailureCode::Agent, cancel.is_cancelled())?;
                return Err(AnalysisError::Launch(error));
            }
        };
        job.finish(output, cancel, validate)
    }
}

pub struct CompletedAnalysis<T> {
    pub run_id: String,
    pub committed_revision: u64,
    pub result: Decoded<T>,
}
impl<T> fmt::Debug for CompletedAnalysis<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompletedAnalysis")
            .field("run_id", &self.run_id)
            .field("committed_revision", &self.committed_revision)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum AnalysisError {
    Binding,
    Launch(RunnerError),
    Response(NetworkDecodeError),
    Storage(io::Error),
}
impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binding => {
                f.write_str("Claude run differs from reviewed context, receiver or model")
            }
            Self::Launch(error) => error.fmt(f),
            Self::Response(error) => error.fmt(f),
            Self::Storage(_) => {
                f.write_str("Analysis could not be saved or committed; inspect local Project state")
            }
        }
    }
}
impl std::error::Error for AnalysisError {}
