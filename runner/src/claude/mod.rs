//! Reviewed Claude Code print protocol. Network execution is an explicit
//! capability, with selected auth, original policy and durable Review binding.
//! Desktop wiring and real account qualification are separate work.

#[cfg(target_os = "linux")]
pub(crate) mod auth;
#[cfg(target_os = "linux")]
mod policy;
#[cfg(target_os = "linux")]
pub(crate) mod preflight;
#[cfg(target_os = "linux")]
pub use policy::EndpointPolicy;
#[cfg(target_os = "linux")]
mod network;
#[cfg(target_os = "linux")]
pub use network::{ClaudeNetworkRunner, NetworkDecodeError, NetworkOutput};
#[cfg(target_os = "linux")]
mod lifecycle;
#[cfg(target_os = "linux")]
pub use lifecycle::{AnalysisError, AnalysisJob, CompletedAnalysis};
#[cfg(all(test, target_os = "linux"))]
mod network_tests;
mod recipe;
mod request;
mod response;
mod wire;
#[cfg(target_os = "linux")]
pub use auth::SelectedAuthFile;

pub use recipe::RecipeRequest;
pub use request::ClaudeRequest;
pub use response::decode;

use crate::{Invocation, RunLimits, Termination};
use std::fmt;

pub const VERSION: &str = "2.1.280";
pub const VERSION_STDOUT: &[u8] = b"2.1.280 (Claude Code)\n";
/// Private read-only procfs, only null/urandom devices, empty HOME and fixed
/// traffic-disable variables. Optional selected auth; no host network/shell/settings.
pub const LINUX_OFFLINE_PROFILE: &str = "linux-x86_64-bwrap-claude-offline-v1";
/// Explicit first-party egress with original policy and preflight.
#[cfg(target_os = "linux")]
pub const LINUX_EGRESS_PROFILE: &str = "linux-x86_64-bwrap-claude-egress-v1";
#[cfg(target_os = "linux")]
pub(crate) const RUNTIME_ENV: [(&str, &str); 4] = [
    ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
    ("CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL", "1"),
    ("ENABLE_CLAUDEAI_MCP_SERVERS", "false"),
    ("CLAUDE_CODE_MAX_RETRIES", "0"),
];
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;
pub const MAX_EVENTS: usize = 4096;

pub fn version_probe() -> Invocation {
    Invocation {
        args: vec!["--version".into()],
        stdin: Vec::new(),
    }
}

pub fn run_limits() -> RunLimits {
    RunLimits {
        stdin_bytes: MAX_INPUT_BYTES,
        stdout_bytes: MAX_OUTPUT_BYTES,
        ..RunLimits::default()
    }
}

/// Main-loop token counts, not a billing statement or a claim about total
/// provider usage. No subagent/tool/secondary-model work is accepted here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
}

pub struct Decoded<T> {
    pub value: T,
    pub usage: Usage,
}
impl<T> fmt::Debug for Decoded<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decoded")
            .field("usage", &self.usage)
            .finish_non_exhaustive()
    }
}

/// Static diagnostics only: no source text, arbitrary CLI output or tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    Process {
        termination: Termination,
        exit_code: Option<i32>,
    },
    Stream {
        line: usize,
        reason: &'static str,
    },
    InvalidResult,
    RejectedResult,
}
impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Process {
                termination,
                exit_code,
            } => write!(
                f,
                "Claude process failed: {termination:?}, exit {exit_code:?}"
            ),
            Self::Stream { line, reason } => {
                write!(f, "Claude event stream rejected at line {line}: {reason}")
            }
            Self::InvalidResult => {
                f.write_str("Claude structured result does not match the expected JSON type")
            }
            Self::RejectedResult => f.write_str("Claude result failed recipe/evidence validation"),
        }
    }
}
impl std::error::Error for DecodeError {}
