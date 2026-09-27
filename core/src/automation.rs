//! Durable recurring analysis of already imported Project snapshots.
//!
//! No messenger acquisition, timer thread, credential access or native agent
//! registration lives here. A host must explicitly supply a qualified executor
//! and the same execution exclusion/cancellation boundary as manual analysis.

mod storage;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io;

use crate::analysis::{AnalysisSpec, FailureCode};
use crate::bundle::{AttachmentStatus, BundleManifest};
use crate::project::{Project, ProjectStore};
use crate::recipe::{Recipe, RecipeEvidence, RecipeOutput};
use crate::scope::select_messages;
use storage::{read, write, Lease};

const MAX_CONTEXT_BYTES: u64 = 1024 * 1024;
const MAX_GRANT_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cadence {
    EverySixHours,
    Daily,
}
impl Cadence {
    fn seconds(self) -> u64 {
        match self {
            Self::EverySixHours => 6 * 3600,
            Self::Daily => 24 * 3600,
        }
    }
}

/// A specification is execution metadata, not credentials or a discovered
/// executable. Saving this object alone never registers an executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSettings {
    pub cadence: Cadence,
    pub spec: AnalysisSpec,
    pub expires_at: u64,
    pub max_context_bytes: u64,
}
impl AutomationSettings {
    fn validate(&self) -> io::Result<()> {
        self.spec.validate()?;
        Recipe::from_version(&self.spec.recipe, self.spec.recipe_version)?;
        if self.max_context_bytes == 0 || self.max_context_bytes > MAX_CONTEXT_BYTES {
            return Err(invalid(
                "automation context budget must be 1..=1048576 bytes",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    NeverRun,
    Running { run_id: Option<String> },
    Succeeded { run_id: String },
    NoChanges,
    NoMessages,
    NeedsReview,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Automation {
    pub schema_version: u32,
    pub project_id: String,
    pub revision: u64,
    pub settings: AutomationSettings,
    pub paused: bool,
    /// Digest of selected identities, filters and privacy policy, not snapshots.
    pub scope_sha256: String,
    pub approved_at: u64,
    pub last_attempt_at: Option<u64>,
    pub last_success_sha256: Option<String>,
    pub outcome: Outcome,
}

impl Automation {
    pub(crate) fn validate(&self) -> io::Result<()> {
        self.settings.validate()?;
        crate::snapshot::validate_snapshot_id(&self.project_id)?;
        if self.schema_version != 1
            || !is_digest(&self.scope_sha256)
            || self
                .last_success_sha256
                .as_ref()
                .is_some_and(|v| !is_digest(v))
            || self.settings.expires_at <= self.approved_at
            || self.settings.expires_at - self.approved_at > MAX_GRANT_SECONDS
        {
            return Err(invalid("invalid automation record"));
        }
        let run_id = match &self.outcome {
            Outcome::Running { run_id } => run_id.as_deref(),
            Outcome::Succeeded { run_id } => Some(run_id.as_str()),
            _ => None,
        };
        if let Some(id) = run_id {
            crate::snapshot::validate_snapshot_id(id)?;
            if !id.starts_with("run-") {
                return Err(invalid("invalid automation run ID"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deferred {
    NotConfigured,
    Paused,
    Expired,
    WaitUntil(u64),
    Busy,
    NeedsReview,
    ExecutorUnavailable,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tick {
    Deferred(Deferred),
    Finished(Box<Automation>),
}

/// Only sanitized content and public source coverage reach the callback.
/// Chat text is data; the compiled recipe remains the control instruction.
pub struct AutomationInput {
    pub spec: AnalysisSpec,
    pub manifest: BundleManifest,
    pub documents: Vec<String>,
}
pub trait AutomationExecutor {
    fn available(&self, spec: &AnalysisSpec) -> bool;
    /// Must obey its own bounded deadline and cancellation, and return an
    /// output only after accepting the transport/process outcome.
    fn analyze(
        &mut self,
        input: &AutomationInput,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<RecipeOutput>;
}

impl ProjectStore {
    pub fn automation(&self, project_id: &str) -> io::Result<Option<Automation>> {
        read(self, project_id)
    }

    /// Explicit configuration of one routine per Project. Binds the exact
    /// displayed Project and automation revisions. A new plan starts paused.
    pub fn configure_automation(
        &self,
        reviewed: &Project,
        expected: Option<u64>,
        settings: AutomationSettings,
        now: u64,
    ) -> io::Result<Automation> {
        let _lease = Lease::acquire(self, &reviewed.project_id)?.ok_or_else(busy)?;
        if self.open(&reviewed.project_id)? != *reviewed {
            return Err(busy());
        }
        settings.validate()?;
        if !reviewed.sources.iter().any(|s| s.selection.enabled) {
            return Err(invalid("automation needs at least one selected source"));
        }
        let old = read(self, &reviewed.project_id)?;
        check_expected(old.as_ref(), expected)?;
        if old
            .as_ref()
            .is_some_and(|p| matches!(p.outcome, Outcome::Running { .. }))
        {
            return Err(invalid(
                "review the interrupted attempt before replacing its plan",
            ));
        }
        let plan = Automation {
            schema_version: 1,
            project_id: reviewed.project_id.clone(),
            revision: next_revision(expected)?,
            settings,
            paused: true,
            scope_sha256: scope_digest(reviewed)?,
            approved_at: now,
            last_attempt_at: None,
            last_success_sha256: None,
            outcome: Outcome::NeverRun,
        };
        plan.validate()?;
        write(self, &plan)?;
        Ok(plan)
    }

    /// Resume is an explicit opt-in, still subject to expiry and scope review.
    /// It preserves the last attempt and success, so toggling cannot duplicate a run.
    pub fn pause_automation(
        &self,
        project_id: &str,
        expected: u64,
        paused: bool,
        now: u64,
    ) -> io::Result<Automation> {
        let _lease = Lease::acquire(self, project_id)?.ok_or_else(busy)?;
        let mut plan = required(self, project_id, expected)?;
        if matches!(plan.outcome, Outcome::Running { .. }) {
            return Err(invalid("review the interrupted attempt before resuming"));
        }
        if !paused
            && (now < plan.approved_at
                || now >= plan.settings.expires_at
                || scope_digest(&self.open(project_id)?)? != plan.scope_sha256)
        {
            return Err(invalid("automation needs a new configuration review"));
        }
        plan.paused = paused;
        advance(self, &mut plan)?;
        Ok(plan)
    }

    /// After the host has established that no old agent process is still active,
    /// acknowledge interruption without replaying or pretending a result succeeded.
    /// Any saved analysis result remains in the normal analysis journal for review.
    pub fn acknowledge_automation_interruption(
        &self,
        project_id: &str,
        expected: u64,
    ) -> io::Result<Automation> {
        let _lease = Lease::acquire(self, project_id)?.ok_or_else(busy)?;
        let mut plan = required(self, project_id, expected)?;
        let Outcome::Running { ref run_id } = plan.outcome else {
            return Err(invalid("automation has no interrupted attempt"));
        };
        // The Project may have changed since the crash; do not require the
        // old ticket to be executable just to pause its automation. Retain its
        // journal entry (including any uncommitted result) for explicit review.
        if let Some(id) = run_id {
            self.read_analysis(project_id, id)?;
        }
        plan.outcome = Outcome::Interrupted;
        plan.paused = true;
        advance(self, &mut plan)?;
        Ok(plan)
    }

    /// One due check, not a replay queue. The injected clock is consulted again
    /// before dispatch and before committing the result. A panic/process crash
    /// leaves Running on disk and prevents any automatic second invocation.
    pub fn tick_automation(
        &self,
        project_id: &str,
        clock: impl Fn() -> u64,
        executor: &mut impl AutomationExecutor,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<Tick> {
        let Some(_lease) = Lease::acquire(self, project_id)? else {
            return Ok(Tick::Deferred(Deferred::Busy));
        };
        let Some(mut plan) = read(self, project_id)? else {
            return Ok(Tick::Deferred(Deferred::NotConfigured));
        };
        let now = clock();
        let project = self.open(project_id)?;
        if let Some(reason) = decision(&plan, &project, now)? {
            return Ok(Tick::Deferred(reason));
        }
        if !executor.available(&plan.settings.spec) {
            return Ok(Tick::Deferred(Deferred::ExecutorUnavailable));
        }
        if cancelled() {
            return Err(crate::cancelled());
        }
        plan.last_attempt_at = Some(now);
        plan.outcome = Outcome::Running { run_id: None };
        advance(self, &mut plan)?;

        let attempt = (|| -> io::Result<(Outcome, Option<String>)> {
            if selected_count(self, &project, &cancelled)? == 0 {
                return Ok((Outcome::NoMessages, None));
            }
            let review = self.prepare_saved_bundle(project_id, project.revision, &cancelled)?;
            if review.manifest.privacy.needs_review != 0
                || review
                    .manifest
                    .attachments
                    .iter()
                    .any(|a| a.status == AttachmentStatus::Missing)
                || review.manifest.attachment_choices_outside_scope != 0
            {
                return Ok((Outcome::NeedsReview, None));
            }
            let documents = self.automation_documents(
                project_id,
                &review.bundle_id,
                plan.settings.max_context_bytes,
                &cancelled,
            )?;
            let content = content_digest(&review.manifest)?;
            if plan.last_success_sha256.as_deref() == Some(&content) {
                return Ok((Outcome::NoChanges, None));
            }
            check_scope(self, &plan, clock())?;
            let recipe = Recipe::from_version(
                &plan.settings.spec.recipe,
                plan.settings.spec.recipe_version,
            )?;
            let evidence = RecipeEvidence::from_markdown(documents.iter().map(String::as_str))?;
            let ticket = self.begin_analysis(
                project_id,
                &review.bundle_id,
                review.project_revision,
                plan.settings.spec.clone(),
                &cancelled,
            )?;
            plan.outcome = Outcome::Running {
                run_id: Some(ticket.request().run_id.clone()),
            };
            advance(self, &mut plan)?;
            let result = (|| {
                if cancelled() {
                    return Err(crate::cancelled());
                }
                check_scope(self, &plan, clock())?;
                self.check_pending_analysis(&ticket, &cancelled)?;
                let output = executor.analyze(
                    &AutomationInput {
                        spec: plan.settings.spec.clone(),
                        manifest: review.manifest,
                        documents,
                    },
                    &cancelled,
                )?;
                if cancelled() {
                    return Err(crate::cancelled());
                }
                check_scope(self, &plan, clock())?;
                self.save_analysis_result(
                    &ticket,
                    &output,
                    |v| recipe.validate(v, &evidence),
                    &cancelled,
                )?;
                self.commit_analysis(&ticket, &cancelled)?;
                Ok((
                    Outcome::Succeeded {
                        run_id: ticket.request().run_id.clone(),
                    },
                    Some(content),
                ))
            })();
            if let Err(error) = &result {
                let _ = if crate::is_cancelled(error) {
                    self.cancel_analysis(&ticket)
                } else {
                    self.fail_analysis(&ticket, FailureCode::Agent)
                };
            }
            result
        })();
        match attempt {
            Ok((outcome, digest)) => {
                if let Some(digest) = digest {
                    plan.last_success_sha256 = Some(digest);
                }
                plan.paused |= matches!(outcome, Outcome::NeedsReview);
                plan.outcome = outcome;
            }
            Err(error) => {
                plan.paused = true;
                plan.outcome = if crate::is_cancelled(&error) {
                    Outcome::Cancelled
                } else {
                    Outcome::Failed
                };
                advance(self, &mut plan)?;
                return Err(error);
            }
        }
        advance(self, &mut plan)?;
        Ok(Tick::Finished(Box::new(plan)))
    }
}

fn decision(plan: &Automation, project: &Project, now: u64) -> io::Result<Option<Deferred>> {
    if matches!(plan.outcome, Outcome::Running { .. }) {
        return Ok(Some(Deferred::NeedsReview));
    }
    if plan.paused {
        return Ok(Some(Deferred::Paused));
    }
    if now < plan.approved_at || now >= plan.settings.expires_at {
        return Ok(Some(Deferred::Expired));
    }
    if scope_digest(project)? != plan.scope_sha256 {
        return Ok(Some(Deferred::NeedsReview));
    }
    if project
        .analysis_run
        .as_ref()
        .is_some_and(|r| r.outcome.is_none())
        || project
            .telegram_refresh
            .values()
            .any(|p| p.checkpoint.unresolved_attempt)
    {
        return Ok(Some(Deferred::Busy));
    }
    if let Some(last) = plan.last_attempt_at {
        let next = last.saturating_add(plan.settings.cadence.seconds());
        if now < next {
            return Ok(Some(Deferred::WaitUntil(next)));
        }
    }
    Ok(None)
}

fn check_scope(store: &ProjectStore, plan: &Automation, now: u64) -> io::Result<()> {
    if now < plan.approved_at
        || now >= plan.settings.expires_at
        || scope_digest(&store.open(&plan.project_id)?)? != plan.scope_sha256
    {
        return Err(invalid(
            "automation review expired or selected scope changed",
        ));
    }
    if read(store, &plan.project_id)?.as_ref() != Some(plan) {
        return Err(busy());
    }
    Ok(())
}

fn scope_digest(project: &Project) -> io::Result<String> {
    let sources: Vec<_> = project
        .sources
        .iter()
        .filter(|s| s.selection.enabled)
        .map(|s| (&s.source_id, &s.connector_id, &s.scope, &s.selection))
        .collect();
    hash(&(
        sources,
        &project.name,
        &project.privacy_options,
        &project.custom_terms,
        project.settings.max_tokens,
        crate::sanitize::RULES_VERSION,
    ))
}
fn content_digest(manifest: &BundleManifest) -> io::Result<String> {
    // Exclude snapshot/bundle IDs, attempt time and source diff counters.
    // Evidence revisions and sanitized attachment bytes remain in file hashes.
    hash(&(
        manifest
            .files
            .iter()
            .map(|f| (&f.name, &f.sha256))
            .collect::<Vec<_>>(),
        manifest
            .sources
            .iter()
            .map(|s| (&s.id, &s.coverage, s.known_gaps))
            .collect::<Vec<_>>(),
    ))
}
fn selected_count(
    store: &ProjectStore,
    project: &Project,
    cancel: &impl Fn() -> bool,
) -> io::Result<usize> {
    let snapshots = store.snapshots(&project.project_id)?;
    let mut count = 0usize;
    for source in project.sources.iter().filter(|s| s.selection.enabled) {
        if cancel() {
            return Err(crate::cancelled());
        }
        let snapshot =
            snapshots.load(source.latest_snapshot_id.as_deref().ok_or_else(|| {
                invalid("automation requires every selected source to be imported")
            })?)?;
        if snapshot.source != source.scope {
            return Err(invalid("snapshot source mismatch"));
        }
        let baseline = project
            .baselines
            .iter()
            .find(|b| b.source_id == source.source_id);
        let previous = baseline
            .map(|b| snapshots.load(&b.snapshot_id))
            .transpose()?;
        let selected = select_messages(
            &snapshot,
            &source.selection,
            previous.as_ref().zip(baseline.map(|b| &b.filter)),
        )?;
        count = count.saturating_add(selected.stats.selected);
    }
    Ok(count)
}
fn hash(value: &impl Serialize) -> io::Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(invalid)?)
    ))
}
fn is_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn required(store: &ProjectStore, id: &str, revision: u64) -> io::Result<Automation> {
    let value = read(store, id)?.ok_or_else(|| invalid("automation is not configured"))?;
    check_expected(Some(&value), Some(revision))?;
    Ok(value)
}
fn check_expected(old: Option<&Automation>, revision: Option<u64>) -> io::Result<()> {
    if old.map(|v| v.revision) != revision {
        return Err(busy());
    }
    Ok(())
}
fn next_revision(old: Option<u64>) -> io::Result<u64> {
    old.map_or(Ok(0), |v| {
        v.checked_add(1)
            .ok_or_else(|| invalid("automation revision exhausted"))
    })
}
fn advance(store: &ProjectStore, value: &mut Automation) -> io::Result<()> {
    check_expected(
        read(store, &value.project_id)?.as_ref(),
        Some(value.revision),
    )?;
    value.revision = next_revision(Some(value.revision))?;
    write(store, value)
}
fn invalid(value: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, value.to_string())
}
fn busy() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "automation changed or is running",
    )
}
