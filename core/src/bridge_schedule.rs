//! Scheduling policy for a future qualified official-client export driver.
//!
//! This module decides when an export attempt may be offered. It never starts
//! Telegram or interprets a file as a completed export. The host must persist
//! the checkpoint atomically before calling a driver, and hold one global
//! in-flight lease while any source is exporting.

use std::fs::{File, OpenOptions};
use std::io;

use fs4::{FileExt, TryLockError};
use serde::{Deserialize, Serialize};

use crate::project::{Project, ProjectEntry, ProjectStore};

const SIX_HOURS: u64 = 6 * 60 * 60;
const DAY: u64 = 24 * 60 * 60;
const MANUAL_RECOVERY_BACKOFF: u64 = 15 * 60;

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

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramRefreshPlan {
    #[serde(default)]
    pub cadence: RefreshCadence,
    #[serde(default)]
    pub checkpoint: RefreshCheckpoint,
}

/// Held for the entire client export, across all projects and TGSUM processes.
/// Dropping the owner explicitly unlocks the file; an inherited handle in a
/// concurrently spawned child must not prolong its lease. A persisted checkpoint
/// still prevents replay after a crash.
pub struct TelegramExportLease {
    _file: File,
}

impl Drop for TelegramExportLease {
    fn drop(&mut self) {
        // Closing just this handle can leave a Unix flock held by a child
        // between fork and exec. The lease owner controls explicit release.
        let _ = FileExt::unlock(&self._file);
    }
}

#[derive(Debug, Clone)]
pub struct RefreshAttempt {
    project_id: String,
    source_id: String,
    checkpoint: RefreshCheckpoint,
    pub(crate) project: Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshTrigger {
    Scheduled,
    Manual,
}

/// The user has checked the official client after an interrupted TGSUM run.
/// This is a host assertion, not a state inferred from an archive or a timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientReview {
    NoExportInProgress,
}

impl TelegramExportLease {
    pub fn try_acquire(store: &ProjectStore) -> io::Result<Option<Self>> {
        let path = store.telegram_refresh_lock_path()?;
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Telegram export lock must be a regular file",
                ));
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Telegram export lock must be a regular file",
            ));
        }
        match FileExt::try_lock(&file) {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }

    /// Persist the claim before calling any client driver. On a concurrent
    /// Project edit, return the conflict; the caller may recompute the claim,
    /// but must never call the client for the failed claim.
    pub fn claim(
        &self,
        store: &ProjectStore,
        project_id: &str,
        source_id: &str,
        context: RefreshContext<'_>,
    ) -> io::Result<Result<RefreshAttempt, RefreshDecision>> {
        let project = store.open(project_id)?;
        self.claim_reviewed(
            store,
            &project,
            source_id,
            RefreshTrigger::Scheduled,
            context,
        )
    }

    /// Pin the configuration reviewed by the host before dispatching a driver.
    pub fn claim_reviewed(
        &self,
        store: &ProjectStore,
        project: &Project,
        source_id: &str,
        trigger: RefreshTrigger,
        context: RefreshContext<'_>,
    ) -> io::Result<Result<RefreshAttempt, RefreshDecision>> {
        // The OS lock disappearing after a crash does not stop Telegram.
        // A pending attempt anywhere in this store blocks every other source.
        if store.list()?.iter().any(|entry| match entry {
            ProjectEntry::Ready { project } => project
                .telegram_refresh
                .values()
                .any(|plan| plan.checkpoint.unresolved_attempt),
            ProjectEntry::Unavailable { .. } => true,
        }) {
            return Ok(Err(RefreshDecision::NeedsUserAction));
        }
        let Some(plan) = project.telegram_refresh.get(source_id) else {
            return Ok(Err(RefreshDecision::ManualOnly));
        };
        let next = match plan
            .checkpoint
            .try_claim_for(plan.cadence, trigger, context)
        {
            Ok(next) => next,
            Err(decision) => return Ok(Err(decision)),
        };
        let claimed = store.publish_telegram_refresh_checkpoint(
            &project.project_id,
            source_id,
            &plan.checkpoint,
            Some(project.revision),
            next.clone(),
        )?;
        Ok(Ok(RefreshAttempt {
            project_id: project.project_id.clone(),
            source_id: source_id.to_owned(),
            checkpoint: next,
            project: claimed,
        }))
    }

    /// A driver may call this only after a known terminal outcome. An ambiguous
    /// timeout or cancellation leaves the persisted claim unresolved.
    pub fn resolve_successfully(
        &self,
        store: &ProjectStore,
        attempt: &RefreshAttempt,
    ) -> io::Result<Project> {
        self.resolve(store, attempt, attempt.checkpoint.resolved_successfully())
    }

    pub fn resolve_with_backoff(
        &self,
        store: &ProjectStore,
        attempt: &RefreshAttempt,
        now: u64,
        seconds: u64,
    ) -> io::Result<Project> {
        self.resolve(
            store,
            attempt,
            attempt.checkpoint.resolved_with_backoff(now, seconds),
        )
    }

    /// Recover a claim whose in-memory `RefreshAttempt` was lost on restart.
    /// The host must obtain an explicit user review that the official client is
    /// no longer exporting. It must handle any resulting archive separately;
    /// this operation never marks an archive as imported or a refresh as
    /// successful. A stale Project revision or checkpoint requires a new review.
    pub fn resolve_after_client_review(
        &self,
        store: &ProjectStore,
        reviewed_project: &Project,
        source_id: &str,
        _review: ClientReview,
        now: u64,
    ) -> io::Result<Project> {
        let expected_checkpoint = &reviewed_project
            .telegram_refresh
            .get(source_id)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Telegram refresh is not configured for this source",
                )
            })?
            .checkpoint;
        if !expected_checkpoint.unresolved_attempt {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Telegram refresh has no unresolved attempt to review",
            ));
        }
        let backoff_from = now.max(expected_checkpoint.last_started_at.unwrap_or(now));
        store.publish_telegram_refresh_checkpoint(
            &reviewed_project.project_id,
            source_id,
            expected_checkpoint,
            Some(reviewed_project.revision),
            expected_checkpoint.resolved_with_backoff(backoff_from, MANUAL_RECOVERY_BACKOFF),
        )
    }

    fn resolve(
        &self,
        store: &ProjectStore,
        attempt: &RefreshAttempt,
        next: RefreshCheckpoint,
    ) -> io::Result<Project> {
        store.publish_telegram_refresh_checkpoint(
            &attempt.project_id,
            &attempt.source_id,
            &attempt.checkpoint,
            None,
            next,
        )
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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
        self.decide_for(cadence, RefreshTrigger::Scheduled, context)
    }

    pub fn decide_for(
        &self,
        cadence: RefreshCadence,
        trigger: RefreshTrigger,
        context: RefreshContext<'_>,
    ) -> RefreshDecision {
        if self.unresolved_attempt {
            return RefreshDecision::NeedsUserAction;
        }
        if trigger == RefreshTrigger::Scheduled && cadence == RefreshCadence::Manual {
            return RefreshDecision::ManualOnly;
        }
        if context.any_export_in_flight {
            return RefreshDecision::Busy;
        }
        let cadence_ready_at = if trigger == RefreshTrigger::Manual {
            0
        } else {
            match cadence {
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
            }
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
        self.try_claim_for(cadence, RefreshTrigger::Scheduled, context)
    }

    pub fn try_claim_for(
        &self,
        cadence: RefreshCadence,
        trigger: RefreshTrigger,
        context: RefreshContext<'_>,
    ) -> Result<Self, RefreshDecision> {
        let decision = self.decide_for(cadence, trigger, context);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assisted::AssistedExportSettings;
    use crate::project::{ProjectChange, ProjectSource};
    use crate::snapshot::SourceScope;

    #[test]
    fn dropping_owner_releases_lease_even_with_an_inherited_handle() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(root.path());
        let lease = TelegramExportLease::try_acquire(&store).unwrap().unwrap();
        // A concurrent process launch can temporarily inherit the open file
        // description before exec closes CLOEXEC descriptors. Dup models it
        // deterministically without racing the operating system's scheduler.
        let inherited = lease._file.try_clone().unwrap();
        drop(lease);
        let next = TelegramExportLease::try_acquire(&store).unwrap();
        assert!(
            next.is_some(),
            "the owner ended its lease before child exec"
        );
        drop(inherited);
        assert!(TelegramExportLease::try_acquire(&store).unwrap().is_none());
    }

    #[test]
    fn changing_the_export_destination_cannot_publish_a_stale_claim() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(root.path());
        let project = store.create("Synthetic").unwrap();
        let project = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::Source(ProjectSource {
                    source_id: "pilot".into(),
                    connector_id: "telegram_json".into(),
                    scope: SourceScope::telegram("synthetic-account", "42"),
                    archive_path: None,
                    latest_snapshot_id: None,
                    selection: Default::default(),
                }),
            )
            .unwrap();
        let project = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::AssistedExport {
                    source_id: "pilot".into(),
                    settings: Some(AssistedExportSettings {
                        directory: root.path().to_owned(),
                        client: None,
                    }),
                },
            )
            .unwrap();
        let project = store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::TelegramRefreshCadence {
                    source_id: "pilot".into(),
                    cadence: RefreshCadence::Daily,
                },
            )
            .unwrap();
        let old_checkpoint = project.telegram_refresh["pilot"].checkpoint.clone();
        let next = old_checkpoint
            .try_claim(
                RefreshCadence::Daily,
                RefreshContext {
                    now: 100,
                    launch_id: "launch-1",
                    session_active_unlocked: true,
                    client_not_before: None,
                    any_export_in_flight: false,
                },
            )
            .unwrap();
        store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::AssistedExport {
                    source_id: "pilot".into(),
                    settings: Some(AssistedExportSettings {
                        directory: root.path().join("new-export-directory"),
                        client: None,
                    }),
                },
            )
            .unwrap();
        assert_eq!(
            store
                .publish_telegram_refresh_checkpoint(
                    &project.project_id,
                    "pilot",
                    &old_checkpoint,
                    Some(project.revision),
                    next,
                )
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            store.open(&project.project_id).unwrap().telegram_refresh["pilot"].checkpoint,
            old_checkpoint
        );
    }
}
