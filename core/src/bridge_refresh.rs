//! Host-side Telegram refresh orchestration with an injected client driver.
//!
//! A driver owns bounded OS interactions and must qualify native source and
//! completion observations before production registration. This module does not
//! discover clients, inspect sessions or turn registry metadata into a driver.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::assisted::{stage_with_opener, AssistedExportSettings, CompletedImport, ImportDelta};
use crate::attachments::ArchiveFiles;
use crate::bridge_schedule::{
    RefreshAttempt, RefreshCadence, RefreshContext, RefreshDecision, RefreshTrigger,
    TelegramExportLease,
};
use crate::project::{Project, ProjectChange, ProjectEntry, ProjectStore};
use crate::snapshot::SourceScope;

const RETRY_BACKOFF: u64 = 15 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverRequest {
    pub run_id: String,
    pub project_id: String,
    pub source_id: String,
    pub source: SourceScope,
    pub settings: AssistedExportSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverStatus {
    Exporting,
    /// Observed terminal completion for this request, including the actual file.
    /// File existence/size or a previous Done panel cannot establish this event.
    Completed {
        archive_path: PathBuf,
    },
    /// Observed terminal failure/cancellation or a confirmed refusal to start.
    Stopped {
        cancelled: bool,
        retry_after: u64,
    },
    /// Outcome is ambiguous, or the client requires interaction from its user.
    NeedsUserAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverEvent {
    pub request: DriverRequest,
    pub status: DriverStatus,
}

/// All methods must return within the driver's documented call deadline.
/// `poll` performs bounded observation; it must never replay `start` or `cancel`.
/// An error from any method is ambiguous and leaves the persisted claim intact.
pub trait RefreshDriver {
    fn available(&self) -> bool;
    fn start(&mut self, request: &DriverRequest) -> io::Result<()>;
    fn poll(&mut self, request: &DriverRequest) -> io::Result<Option<DriverEvent>>;
    fn cancel(&mut self, request: &DriverRequest) -> io::Result<()>;
}

impl<D: RefreshDriver + ?Sized> RefreshDriver for Box<D> {
    fn available(&self) -> bool {
        (**self).available()
    }
    fn start(&mut self, request: &DriverRequest) -> io::Result<()> {
        (**self).start(request)
    }
    fn poll(&mut self, request: &DriverRequest) -> io::Result<Option<DriverEvent>> {
        (**self).poll(request)
    }
    fn cancel(&mut self, request: &DriverRequest) -> io::Result<()> {
        (**self).cancel(request)
    }
}

/// Default host binding until a real OS/client profile has been qualified.
pub struct UnavailableDriver;
impl RefreshDriver for UnavailableDriver {
    fn available(&self) -> bool {
        false
    }
    fn start(&mut self, _: &DriverRequest) -> io::Result<()> {
        Err(unavailable())
    }
    fn poll(&mut self, _: &DriverRequest) -> io::Result<Option<DriverEvent>> {
        Err(unavailable())
    }
    fn cancel(&mut self, _: &DriverRequest) -> io::Result<()> {
        Err(unavailable())
    }
}

fn unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "no qualified Telegram export driver",
    )
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RefreshState {
    Idle,
    Unavailable,
    Deferred {
        decision: RefreshDecision,
    },
    WaitingForClient,
    Exporting,
    Cancelling,
    NeedsUserAction,
    Cancelled,
    Failed,
    Ready {
        project: Box<Project>,
        delta: ImportDelta,
    },
}

/// Wall time controls saved cadence; monotonic time bounds a running attempt.
#[derive(Clone, Copy)]
pub struct RefreshObservation {
    pub unix_seconds: u64,
    pub active_unlocked: bool,
    pub at: Instant,
}

struct ActiveRun {
    lease: TelegramExportLease,
    attempt: RefreshAttempt,
    request: DriverRequest,
    started_at: Instant,
    saw_exporting: bool,
    cancel_requested: bool,
    cancel: Arc<AtomicBool>,
}

pub struct RefreshCoordinator<D> {
    store: ProjectStore,
    driver: D,
    timeout: Duration,
    active: Option<ActiveRun>,
    state: RefreshState,
}

impl<D: RefreshDriver> RefreshCoordinator<D> {
    pub fn new(store: ProjectStore, driver: D, timeout: Duration) -> io::Result<Self> {
        if timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "export timeout must be positive",
            ));
        }
        Ok(Self {
            store,
            driver,
            timeout,
            active: None,
            state: RefreshState::Idle,
        })
    }

    pub fn state(&self) -> &RefreshState {
        &self.state
    }
    pub fn available(&self) -> bool {
        self.driver.available()
    }
    pub fn active_request(&self) -> Option<&DriverRequest> {
        self.active.as_ref().map(|run| &run.request)
    }

    /// May be signalled while a blocking worker is staging/importing a file.
    /// A new run gets its own flag; a late UI cancellation cannot hit it.
    pub fn cancellation(&self) -> Option<Arc<AtomicBool>> {
        self.active.as_ref().map(|run| Arc::clone(&run.cancel))
    }

    /// One host timer tick: observe the active run or dispatch at most one due
    /// source. No historical timer events are replayed after sleep/restart.
    pub fn tick(&mut self, context: RefreshContext<'_>, at: Instant) -> io::Result<RefreshState> {
        if self.active.is_some() {
            return self.poll(RefreshObservation {
                unix_seconds: context.now,
                active_unlocked: context.session_active_unlocked,
                at,
            });
        }
        if !self.available() {
            self.state = RefreshState::Unavailable;
            return Ok(self.state.clone());
        }
        for entry in self.store.list()? {
            let ProjectEntry::Ready { project } = entry else {
                continue;
            };
            for (source_id, plan) in &project.telegram_refresh {
                if plan.checkpoint.decide(plan.cadence, context) == RefreshDecision::Ready {
                    let result =
                        self.start(&project, source_id, RefreshTrigger::Scheduled, context, at)?;
                    if matches!(
                        result,
                        RefreshState::Deferred {
                            decision: RefreshDecision::Busy
                        }
                    ) {
                        continue;
                    }
                    return Ok(result);
                }
            }
        }
        Ok(self.state.clone())
    }

    /// The Project is the exact revision displayed/selected by the host. All
    /// persisted claims precede the first possibly mutating driver call.
    pub fn start(
        &mut self,
        reviewed: &Project,
        source_id: &str,
        trigger: RefreshTrigger,
        context: RefreshContext<'_>,
        at: Instant,
    ) -> io::Result<RefreshState> {
        if self.active.is_some() {
            return Ok(RefreshState::Deferred {
                decision: RefreshDecision::Busy,
            });
        }
        if !self.available() {
            self.state = RefreshState::Unavailable;
            return Ok(self.state.clone());
        }
        let mut project = self.store.open(&reviewed.project_id)?;
        if project != *reviewed {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Project changed before refresh",
            ));
        }
        if project
            .analysis_run
            .as_ref()
            .is_some_and(|run| run.outcome.is_none())
        {
            self.state = RefreshState::Deferred {
                decision: RefreshDecision::Busy,
            };
            return Ok(self.state.clone());
        }
        let source = project
            .sources
            .iter()
            .find(|source| {
                source.source_id == source_id
                    && source.connector_id == "telegram_json"
                    && source.scope.platform == "telegram"
            })
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "select a Telegram source")
            })?;
        let settings = project
            .assisted_exports
            .get(source_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "configure export first"))?
            .clone();
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
        let request = DriverRequest {
            run_id: format!("refresh-{:032x}", u128::from_be_bytes(bytes)),
            project_id: project.project_id.clone(),
            source_id: source_id.to_owned(),
            source: source.scope.clone(),
            settings,
        };
        let Some(lease) = TelegramExportLease::try_acquire(&self.store)? else {
            self.state = RefreshState::Deferred {
                decision: RefreshDecision::Busy,
            };
            return Ok(self.state.clone());
        };
        if !project.telegram_refresh.contains_key(source_id) {
            if trigger == RefreshTrigger::Scheduled {
                self.state = RefreshState::Deferred {
                    decision: RefreshDecision::ManualOnly,
                };
                return Ok(self.state.clone());
            }
            project = self.store.update(
                &project.project_id,
                project.revision,
                ProjectChange::TelegramRefreshCadence {
                    source_id: source_id.into(),
                    cadence: RefreshCadence::Manual,
                },
            )?;
        }
        let attempt =
            match lease.claim_reviewed(&self.store, &project, source_id, trigger, context)? {
                Ok(attempt) => attempt,
                Err(decision) => {
                    self.state = RefreshState::Deferred { decision };
                    return Ok(self.state.clone());
                }
            };
        self.active = Some(ActiveRun {
            lease,
            attempt,
            request: request.clone(),
            started_at: at,
            saw_exporting: false,
            cancel_requested: false,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        self.state = RefreshState::WaitingForClient;
        if self.driver.start(&request).is_err() {
            self.require_review();
        }
        Ok(self.state.clone())
    }

    pub fn poll(&mut self, observation: RefreshObservation) -> io::Result<RefreshState> {
        let Some(run) = &self.active else {
            return Ok(self.state.clone());
        };
        if !observation.active_unlocked
            || observation.at.saturating_duration_since(run.started_at) >= self.timeout
        {
            self.require_review();
            return Ok(self.state.clone());
        }
        if run.cancel.load(Ordering::Relaxed) && !run.cancel_requested {
            self.cancel(observation);
        }
        let Some(run) = &self.active else {
            return Ok(self.state.clone());
        };
        // A source/destination edit cannot redirect an already running export.
        let current = match self.store.open(&run.request.project_id) {
            Ok(current) => current,
            Err(error) => {
                self.require_review();
                return Err(error);
            }
        };
        if current != run.attempt.project {
            self.require_review();
            return Ok(self.state.clone());
        }
        let request = run.request.clone();
        let event = match self.driver.poll(&request) {
            Ok(Some(event)) => event,
            Ok(None) => return Ok(self.state.clone()),
            Err(_) => {
                self.require_review();
                return Ok(self.state.clone());
            }
        };
        if event.request != request {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "driver event does not match active export",
            ));
        }
        match event.status {
            DriverStatus::Exporting => {
                let run = self.active.as_mut().unwrap();
                run.saw_exporting = true;
                self.state = if run.cancel_requested {
                    RefreshState::Cancelling
                } else {
                    RefreshState::Exporting
                };
            }
            DriverStatus::NeedsUserAction => self.require_review(),
            DriverStatus::Stopped {
                cancelled,
                retry_after,
            } => {
                self.stop_known(observation.unix_seconds, retry_after, cancelled)?;
            }
            DriverStatus::Completed { archive_path } => {
                let run = self.active.as_ref().unwrap();
                if !run.saw_exporting {
                    self.require_review();
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "completion requires observed export start",
                    ));
                }
                if run.cancel_requested || run.cancel.load(Ordering::Relaxed) {
                    self.stop_known(observation.unix_seconds, RETRY_BACKOFF, true)?;
                } else {
                    let result = import_completed(&self.store, run, &archive_path);
                    match result {
                        Ok(completed) => {
                            self.active = None;
                            self.state = RefreshState::Ready {
                                project: Box::new(completed.project),
                                delta: completed.delta,
                            };
                        }
                        Err(error) => {
                            // Even an invalid archive cannot erase the previous
                            // snapshot. Conflicts leave the claim for review.
                            if error.kind() == io::ErrorKind::WouldBlock {
                                self.require_review();
                            } else {
                                self.stop_known(
                                    observation.unix_seconds,
                                    RETRY_BACKOFF,
                                    crate::is_cancelled(&error),
                                )?;
                            }
                            return Err(error);
                        }
                    }
                }
            }
        }
        Ok(self.state.clone())
    }

    /// Request cancellation once; a delivered request is not terminal evidence.
    pub fn cancel(&mut self, observation: RefreshObservation) -> RefreshState {
        let Some(run) = self.active.as_mut() else {
            return self.state.clone();
        };
        if !observation.active_unlocked {
            self.require_review();
            return self.state.clone();
        }
        if !run.cancel_requested {
            run.cancel.store(true, Ordering::Relaxed);
            run.cancel_requested = true;
            let request = run.request.clone();
            self.state = RefreshState::Cancelling;
            if self.driver.cancel(&request).is_err() {
                self.require_review();
            }
        }
        self.state.clone()
    }

    fn require_review(&mut self) {
        self.active = None; // Persisted unresolved claim blocks every source.
        self.state = RefreshState::NeedsUserAction;
    }

    fn stop_known(&mut self, now: u64, retry_after: u64, cancelled: bool) -> io::Result<()> {
        let run = self.active.take().unwrap();
        self.state = RefreshState::NeedsUserAction;
        run.lease.resolve_with_backoff(
            &self.store,
            &run.attempt,
            now,
            retry_after.max(RETRY_BACKOFF),
        )?;
        self.state = if cancelled || run.cancel_requested {
            RefreshState::Cancelled
        } else {
            RefreshState::Failed
        };
        Ok(())
    }
}

fn import_completed(
    store: &ProjectStore,
    run: &ActiveRun,
    path: &Path,
) -> io::Result<CompletedImport> {
    if store.open(&run.request.project_id)? != run.attempt.project {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "Project changed before import",
        ));
    }
    let root = &run.request.settings.directory;
    let relative = export_relative_path(path, root)?;
    let files = ArchiveFiles::open(root)?;
    let project = &run.attempt.project;
    let source = project
        .sources
        .iter()
        .find(|s| s.source_id == run.request.source_id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "refresh source missing"))?;
    let snapshots = store.snapshots(&project.project_id)?;
    let previous = source
        .latest_snapshot_id
        .as_deref()
        .map(|id| snapshots.load(id))
        .transpose()?;
    let cancelled = || run.cancel.load(Ordering::Relaxed);
    let staged = stage_with_opener(
        || files.open_regular(relative),
        &store.directory(&project.project_id)?,
        &mut |_, _| Ok(()),
        &cancelled,
    )?;
    let reader = crate::ProgressReader::new(staged, |_| {
        if cancelled() {
            Err(crate::cancelled())
        } else {
            Ok(())
        }
    });
    let snapshot = snapshots.import_telegram(&run.request.run_id, &source.scope, reader)?;
    let delta = ImportDelta::from_snapshots(&snapshot, previous.as_ref())?;
    if cancelled() {
        return Err(crate::cancelled());
    }
    let project = store.publish_telegram_refresh_result(
        project,
        &run.request.source_id,
        &run.request.run_id,
        path.to_path_buf(),
    )?;
    Ok(CompletedImport { project, delta })
}

fn export_relative_path<'a>(path: &'a Path, root: &Path) -> io::Result<&'a Path> {
    let outside = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "export output must be inside its configured directory",
        )
    };
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        // canonicalize adds the verbatim drive prefix on Windows. Match that
        // representation to the same local drive without resolving symlinks or
        // reparse points. The retained handle opener still validates the tail.
        let drive = |component| match component {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    Some(letter.to_ascii_uppercase())
                }
                _ => None,
            },
            _ => None,
        };
        let mut path_parts = path.components();
        let mut root_parts = root.components();
        let source_drive = drive(path_parts.next()).ok_or_else(outside)?;
        if !path.is_absolute()
            || !root.is_absolute()
            || Some(source_drive) != drive(root_parts.next())
        {
            return Err(outside());
        }
        path_parts
            .as_path()
            .strip_prefix(root_parts.as_path())
            .map_err(|_| outside())
    }
    #[cfg(not(windows))]
    path.strip_prefix(root).map_err(|_| outside())
}
