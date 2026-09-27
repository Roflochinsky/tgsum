//! Scheduling policy for a future qualified official-client export driver.
//!
//! This module decides when an export attempt may be offered. It never starts
//! Telegram or interprets a file as a completed export. The host must persist
//! the checkpoint atomically before calling a driver, and hold one global
//! in-flight lease while any source is exporting.

use serde::{Deserialize, Serialize};

const SIX_HOURS: u64 = 6 * 60 * 60;
const DAY: u64 = 24 * 60 * 60;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshCadence {
    #[default]
    Manual,
    OnStart,
    EverySixHours,
    Daily,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshCheckpoint {
    /// Recorded before invoking the client, so a restart cannot replay the job.
    pub last_started_at: Option<u64>,
    /// A stable identifier for one TGSUM process lifetime, not a client session.
    pub last_launch_id: Option<String>,
    /// A crash/timeout may leave a client export alive; require human review.
    pub unresolved_attempt: bool,
    /// Includes cancellation and ambiguous client outcomes; never auto-retry early.
    pub retry_not_before: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshContext<'a> {
    pub now: u64,
    pub launch_id: &'a str,
    pub session_active_unlocked: bool,
    /// Additional platform/client delay measured by the future driver.
    pub client_not_before: Option<u64>,
    /// The host's global lease across every connected source.
    pub any_export_in_flight: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshDecision {
    ManualOnly,
    AlreadyAttempted,
    NeedsUserAction,
    WaitUntil(u64),
    NeedsActiveSession,
    Busy,
    Ready,
}

impl RefreshCheckpoint {
    pub fn decide(&self, cadence: RefreshCadence, context: RefreshContext<'_>) -> RefreshDecision {
        if self.unresolved_attempt {
            return RefreshDecision::NeedsUserAction;
        }
        if cadence == RefreshCadence::Manual {
            return RefreshDecision::ManualOnly;
        }
        if context.any_export_in_flight {
            return RefreshDecision::Busy;
        }
        let cadence_ready_at = match cadence {
            RefreshCadence::Manual => unreachable!(),
            RefreshCadence::OnStart
                if self.last_launch_id.as_deref() == Some(context.launch_id) =>
            {
                return RefreshDecision::AlreadyAttempted;
            }
            RefreshCadence::OnStart => 0,
            RefreshCadence::EverySixHours => self
                .last_started_at
                .map_or(0, |started| started.saturating_add(SIX_HOURS)),
            RefreshCadence::Daily => self
                .last_started_at
                .map_or(0, |started| started.saturating_add(DAY)),
        };
        let ready_at = cadence_ready_at
            .max(self.retry_not_before.unwrap_or(0))
            .max(context.client_not_before.unwrap_or(0));
        if context.now < ready_at {
            return RefreshDecision::WaitUntil(ready_at);
        }
        if !context.session_active_unlocked {
            return RefreshDecision::NeedsActiveSession;
        }
        RefreshDecision::Ready
    }

    /// The host must persist the new checkpoint under its global lease before
    /// asking the client to export. A stale Project revision must retry here.
    pub fn try_claim(
        &self,
        cadence: RefreshCadence,
        context: RefreshContext<'_>,
    ) -> Result<Self, RefreshDecision> {
        let decision = self.decide(cadence, context);
        if decision != RefreshDecision::Ready {
            return Err(decision);
        }
        Ok(Self {
            last_started_at: Some(context.now),
            last_launch_id: Some(context.launch_id.to_owned()),
            unresolved_attempt: true,
            retry_not_before: None,
        })
    }

    /// Only a known terminal client outcome can clear the unresolved attempt.
    pub fn resolved_with_backoff(&self, now: u64, seconds: u64) -> Self {
        Self {
            unresolved_attempt: false,
            retry_not_before: Some(now.saturating_add(seconds)),
            ..self.clone()
        }
    }

    pub fn resolved_successfully(&self) -> Self {
        Self {
            unresolved_attempt: false,
            retry_not_before: None,
            ..self.clone()
        }
    }
}
