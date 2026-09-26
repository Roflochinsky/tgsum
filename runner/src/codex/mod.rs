//! Wire protocol for the reviewed Codex CLI version. This module prepares
//! requests and validates responses; it does not authorize or launch cloud runs.
//! A qualified auth/network boundary and a Review naming the actual receiver
//! are still required. CLI flags alone are not an isolation boundary.

mod request;
mod response;

pub use request::CodexRequest;
pub use response::decode;

use std::fmt;

use serde::Deserialize;

use crate::{Invocation, RunLimits, Termination};

pub const VERSION_STDOUT: &[u8] = b"codex-cli 0.155.1\n";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub cache_write_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
}

/// Only constructed after a successful process, complete supported transcript,
/// typed JSON decoding and the caller's mandatory recipe/evidence validation.
/// This does not prove factual correctness or save/advance any Project state.
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

/// Contains no provider diagnostics, prompt text or arbitrary JSON values.
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
                "Codex process failed: {termination:?}, exit {exit_code:?}"
            ),
            Self::Stream { line, reason } => {
                write!(f, "Codex event stream rejected at line {line}: {reason}")
            }
            Self::InvalidResult => {
                f.write_str("Codex result does not match the expected JSON type")
            }
            Self::RejectedResult => f.write_str("Codex result failed recipe/evidence validation"),
        }
    }
}

impl std::error::Error for DecodeError {}
