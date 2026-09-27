use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tgsum_core::assisted::AssistedExportSettings;
use tgsum_core::bridge_refresh::{
    DriverEvent, DriverRequest, DriverStatus, RefreshCoordinator, RefreshDriver,
    RefreshObservation, RefreshState, UnavailableDriver,
};
use tgsum_core::bridge_schedule::{
    ClientReview, RefreshCadence, RefreshContext, RefreshDecision, RefreshTrigger,
    TelegramExportLease,
};
use tgsum_core::project::{AnalysisOutcome, Project, ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;

#[derive(Default)]
struct Events {
    starts: Vec<DriverRequest>,
    queue: VecDeque<DriverEvent>,
    cancels: usize,
    fail_start: bool,
    before_event: Option<Box<dyn FnOnce() + Send>>,
}
struct FakeDriver {
    events: Arc<Mutex<Events>>,
    store: ProjectStore,
}
impl RefreshDriver for FakeDriver {
    fn available(&self) -> bool {
        true
    }
    fn start(&mut self, request: &DriverRequest) -> io::Result<()> {
        let project = self.store.open(&request.project_id)?;
        assert!(
            project.telegram_refresh[&request.source_id]
                .checkpoint
                .unresolved_attempt
        );
        assert!(TelegramExportLease::try_acquire(&self.store)?.is_none());
        let mut events = self.events.lock().unwrap();
        events.starts.push(request.clone());
        if events.fail_start {
            return Err(io::Error::other("ambiguous start"));
        }
        Ok(())
    }
    fn poll(&mut self, _: &DriverRequest) -> io::Result<Option<DriverEvent>> {
        let mut events = self.events.lock().unwrap();
        if let Some(hook) = events.before_event.take() {
            hook();
        }
        Ok(events.queue.pop_front())
    }
    fn cancel(&mut self, _: &DriverRequest) -> io::Result<()> {
        self.events.lock().unwrap().cancels += 1;
        Ok(())
    }
}
struct Fixture {
    _root: tempfile::TempDir,
    directory: PathBuf,
    store: ProjectStore,
    project: Project,
    events: Arc<Mutex<Events>>,
    at: Instant,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().canonicalize().unwrap().join("exports");
        std::fs::create_dir(&directory).unwrap();
        let store = ProjectStore::new(root.path().join("projects"));
        let project = configured(&store, &directory);
        Self {
            _root: root,
            directory,
            store,
            project,
            events: Default::default(),
            at: Instant::now(),
        }
    }
    fn coordinator(&self) -> RefreshCoordinator<FakeDriver> {
        RefreshCoordinator::new(
            self.store.clone(),
            FakeDriver {
                events: self.events.clone(),
                store: self.store.clone(),
            },
            Duration::from_secs(600),
        )
        .unwrap()
    }
    fn project(&self) -> Project {
        self.store.open(&self.project.project_id).unwrap()
    }
    fn observation(&self) -> RefreshObservation {
        RefreshObservation {
            unix_seconds: 100,
            active_unlocked: true,
            at: self.at,
        }
    }
    fn archive(&self, folder: &str, json: &str) -> PathBuf {
        let folder = self.directory.join(folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("result.json");
        std::fs::write(&path, json).unwrap();
        path
    }
    fn event(&self, run: &RefreshCoordinator<FakeDriver>, status: DriverStatus) {
        self.events.lock().unwrap().queue.push_back(DriverEvent {
            request: run.active_request().unwrap().clone(),
            status,
        });
    }
    fn start(&self, run: &mut RefreshCoordinator<FakeDriver>) {
        assert!(matches!(
            run.start(
                &self.project(),
                "pilot",
                RefreshTrigger::Manual,
                context(100),
                self.at
            )
            .unwrap(),
            RefreshState::WaitingForClient
        ));
    }
    fn exporting(&self, run: &mut RefreshCoordinator<FakeDriver>) {
        self.event(run, DriverStatus::Exporting);
        assert!(matches!(
            run.poll(self.observation()).unwrap(),
            RefreshState::Exporting
        ));
    }
}
fn configured(store: &ProjectStore, directory: &std::path::Path) -> Project {
    let mut p = store.create("Synthetic Telegram").unwrap();
    for (source_id, id) in [("pilot", "42"), ("other", "43")] {
        p = store
            .update(
                &p.project_id,
                p.revision,
                ProjectChange::Source(ProjectSource {
                    source_id: source_id.into(),
                    connector_id: "telegram_json".into(),
                    scope: SourceScope::telegram("synthetic-account", id),
                    archive_path: None,
                    latest_snapshot_id: None,
                    selection: Default::default(),
                }),
            )
            .unwrap();
        p = store
            .update(
                &p.project_id,
                p.revision,
                ProjectChange::AssistedExport {
                    source_id: source_id.into(),
                    settings: Some(AssistedExportSettings {
                        directory: directory.to_owned(),
                        client: None,
                    }),
                },
            )
            .unwrap();
    }
    p
}
fn context(now: u64) -> RefreshContext<'static> {
    RefreshContext {
        now,
        launch_id: "synthetic-launch",
        session_active_unlocked: true,
        client_not_before: None,
        any_export_in_flight: false,
    }
}
const FIRST: &str = r#"{"id":42,"name":"Synthetic","type":"private_group","messages":[{"id":1,"type":"message","text":"first"},{"id":2,"type":"message","text":"second"}]}"#;
const SECOND: &str = r#"{"id":42,"name":"Synthetic","type":"private_group","messages":[{"id":2,"type":"message","text":"edited"},{"id":3,"type":"message","text":"new"}]}"#;

#[test]
fn actual_completed_path_and_checkpoint_publish_together_with_deterministic_diff() {
    let f = Fixture::new();
    let mut run = f.coordinator();
    let first = f.archive("ChatExport_2026-09-28", FIRST);
    f.start(&mut run);
    let claimed = f.project();
    f.exporting(&mut run);
    f.event(
        &run,
        DriverStatus::Completed {
            archive_path: first.clone(),
        },
    );
    let RefreshState::Ready { project, delta } = run.poll(f.observation()).unwrap() else {
        panic!("expected ready")
    };
    assert_eq!(delta.created, 2);
    assert_eq!(project.revision, claimed.revision + 1);
    assert_eq!(project.sources[0].archive_path, Some(first));
    assert!(
        !project.telegram_refresh["pilot"]
            .checkpoint
            .unresolved_attempt
    );
    assert_eq!(project.sources[1], f.project.sources[1]);
    // Set a real baseline for this source; refresh must not advance it.
    let mut selection = project.sources[1].selection.clone();
    selection.enabled = false;
    let project = f
        .store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Selection {
                source_id: "other".into(),
                selection,
            },
        )
        .unwrap();
    let project = f
        .store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::BeginAnalysis {
                run_id: "synthetic-analysis".into(),
            },
        )
        .unwrap();
    let project = f
        .store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::FinishAnalysis {
                run_id: "synthetic-analysis".into(),
                outcome: AnalysisOutcome::Succeeded,
            },
        )
        .unwrap();
    let second = f.archive("ChatExport_2026-09-28 (1)", SECOND);
    f.start(&mut run);
    f.exporting(&mut run);
    f.event(
        &run,
        DriverStatus::Completed {
            archive_path: second.clone(),
        },
    );
    let RefreshState::Ready {
        project: refreshed,
        delta,
    } = run.poll(f.observation()).unwrap()
    else {
        panic!("expected ready")
    };
    assert_eq!(
        (delta.created, delta.edited, delta.missing, delta.deleted),
        (1, 1, 1, 0)
    );
    assert_eq!(refreshed.baselines, project.baselines);
    assert_eq!(refreshed.analysis_run, project.analysis_run);
    assert_eq!(refreshed.sources[0].archive_path, Some(second));
    let starts = &f.events.lock().unwrap().starts;
    assert_ne!(starts[0].run_id, starts[1].run_id);
    assert!(TelegramExportLease::try_acquire(&f.store)
        .unwrap()
        .is_some());
}

#[test]
fn unavailable_manual_default_and_locked_session_never_dispatch_a_driver() {
    let f = Fixture::new();
    let mut unavailable =
        RefreshCoordinator::new(f.store.clone(), UnavailableDriver, Duration::from_secs(1))
            .unwrap();
    assert!(matches!(
        unavailable
            .start(
                &f.project,
                "pilot",
                RefreshTrigger::Manual,
                context(100),
                f.at
            )
            .unwrap(),
        RefreshState::Unavailable
    ));
    assert_eq!(f.project(), f.project);
    let mut run = f.coordinator();
    assert!(matches!(
        run.start(
            &f.project(),
            "pilot",
            RefreshTrigger::Scheduled,
            context(100),
            f.at
        )
        .unwrap(),
        RefreshState::Deferred {
            decision: RefreshDecision::ManualOnly
        }
    ));
    let mut locked = context(100);
    locked.session_active_unlocked = false;
    assert!(matches!(
        run.start(&f.project(), "pilot", RefreshTrigger::Manual, locked, f.at)
            .unwrap(),
        RefreshState::Deferred {
            decision: RefreshDecision::NeedsActiveSession
        }
    ));
    assert!(f.events.lock().unwrap().starts.is_empty());
    f.start(&mut run);
    assert!(matches!(
        run.start(
            &f.project(),
            "other",
            RefreshTrigger::Manual,
            context(100),
            f.at
        )
        .unwrap(),
        RefreshState::Deferred {
            decision: RefreshDecision::Busy
        }
    ));
    assert_eq!(f.events.lock().unwrap().starts.len(), 1);
}

#[test]
fn stale_event_cannot_publish_and_completion_without_exporting_needs_review() {
    let f = Fixture::new();
    let mut run = f.coordinator();
    let file = f.archive("done", FIRST);
    f.start(&mut run);
    let mut request = run.active_request().unwrap().clone();
    request.source.conversation_id = "43".into();
    f.events.lock().unwrap().queue.push_back(DriverEvent {
        request,
        status: DriverStatus::Completed {
            archive_path: file.clone(),
        },
    });
    assert!(run.poll(f.observation()).is_err());
    assert!(run.active_request().is_some());
    assert!(f.project().sources[0].latest_snapshot_id.is_none());
    f.event(&run, DriverStatus::Completed { archive_path: file });
    assert!(run.poll(f.observation()).is_err());
    assert!(matches!(run.state(), RefreshState::NeedsUserAction));
    assert!(
        f.project().telegram_refresh["pilot"]
            .checkpoint
            .unresolved_attempt
    );
}

#[test]
fn cancel_is_sent_once_and_late_completion_never_imports() {
    let f = Fixture::new();
    let mut run = f.coordinator();
    let file = f.archive("cancelled", FIRST);
    f.start(&mut run);
    f.exporting(&mut run);
    let cancel = run.cancellation().unwrap();
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(matches!(
        run.poll(f.observation()).unwrap(),
        RefreshState::Cancelling
    ));
    run.cancel(f.observation());
    assert_eq!(f.events.lock().unwrap().cancels, 1);
    assert!(
        f.project().telegram_refresh["pilot"]
            .checkpoint
            .unresolved_attempt
    );
    f.event(&run, DriverStatus::Completed { archive_path: file });
    assert!(matches!(
        run.poll(f.observation()).unwrap(),
        RefreshState::Cancelled
    ));
    assert!(f.project().sources[0].latest_snapshot_id.is_none());
    assert!(
        !f.project().telegram_refresh["pilot"]
            .checkpoint
            .unresolved_attempt
    );
    assert!(matches!(
        run.start(
            &f.project(),
            "pilot",
            RefreshTrigger::Manual,
            context(999),
            f.at
        )
        .unwrap(),
        RefreshState::Deferred {
            decision: RefreshDecision::WaitUntil(1000)
        }
    ));
}

#[test]
fn interrupted_export_blocks_other_projects_and_cannot_be_erased_by_disconnect() {
    let f = Fixture::new();
    let mut run = f.coordinator();
    f.start(&mut run);
    drop(run);
    let pending = f.project();
    assert!(f
        .store
        .update(
            &pending.project_id,
            pending.revision,
            ProjectChange::RemoveSource("pilot".into())
        )
        .is_err());
    let mut replacement = pending.sources[0].clone();
    replacement.scope.conversation_id = "99".into();
    assert!(f
        .store
        .update(
            &pending.project_id,
            pending.revision,
            ProjectChange::Source(replacement)
        )
        .is_err());
    let other_project = configured(&f.store, &f.directory);
    let mut second = f.coordinator();
    assert!(matches!(
        second
            .start(
                &other_project,
                "pilot",
                RefreshTrigger::Manual,
                context(100),
                f.at
            )
            .unwrap(),
        RefreshState::Deferred {
            decision: RefreshDecision::NeedsUserAction
        }
    ));
    assert_eq!(f.events.lock().unwrap().starts.len(), 1);
    let lease = TelegramExportLease::try_acquire(&f.store).unwrap().unwrap();
    lease
        .resolve_after_client_review(
            &f.store,
            &pending,
            "pilot",
            ClientReview::NoExportInProgress,
            100,
        )
        .unwrap();
    drop(lease);
    let current_other = f.store.open(&other_project.project_id).unwrap();
    assert!(matches!(
        second
            .start(
                &current_other,
                "pilot",
                RefreshTrigger::Manual,
                context(101),
                f.at
            )
            .unwrap(),
        RefreshState::WaitingForClient
    ));
}

#[test]
fn timeout_lock_and_ambiguous_start_preserve_claim_without_replaying_actions() {
    for mode in ["timeout", "lock", "start_error"] {
        let f = Fixture::new();
        let mut run = f.coordinator();
        f.events.lock().unwrap().fail_start = mode == "start_error";
        run.start(
            &f.project(),
            "pilot",
            RefreshTrigger::Manual,
            context(100),
            f.at,
        )
        .unwrap();
        let mut observation = f.observation();
        if mode == "timeout" {
            observation.at += Duration::from_secs(601);
        }
        if mode == "lock" {
            observation.active_unlocked = false;
        }
        assert!(matches!(
            run.poll(observation).unwrap(),
            RefreshState::NeedsUserAction
        ));
        assert!(
            f.project().telegram_refresh["pilot"]
                .checkpoint
                .unresolved_attempt
        );
        assert!(f.project().sources[0].latest_snapshot_id.is_none());
        assert_eq!(f.events.lock().unwrap().starts.len(), 1);
        assert_eq!(f.events.lock().unwrap().cancels, 0);
    }
}

#[test]
fn project_change_during_completion_prevents_snapshot_and_checkpoint_publication() {
    let f = Fixture::new();
    let mut run = f.coordinator();
    f.start(&mut run);
    f.exporting(&mut run);
    let claimed = f.project();
    let store = f.store.clone();
    let id = claimed.project_id.clone();
    f.events.lock().unwrap().before_event = Some(Box::new(move || {
        store
            .update(
                &id,
                claimed.revision,
                ProjectChange::Rename("Concurrent edit".into()),
            )
            .unwrap();
    }));
    f.event(
        &run,
        DriverStatus::Completed {
            archive_path: f.archive("done", FIRST),
        },
    );
    assert_eq!(
        run.poll(f.observation()).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert!(matches!(run.state(), RefreshState::NeedsUserAction));
    assert!(f.project().sources[0].latest_snapshot_id.is_none());
    assert!(
        f.project().telegram_refresh["pilot"]
            .checkpoint
            .unresolved_attempt
    );
    assert_eq!(f.project().name, "Concurrent edit");
}

#[test]
fn invalid_outputs_never_advance_project_and_known_failure_respects_backoff() {
    for input in ["truncated", "wrong_chat", "outside", "parent", "missing"] {
        let f = Fixture::new();
        let mut run = f.coordinator();
        let file = match input {
            "truncated" => f.archive("bad", "{\"id\":42,"),
            "wrong_chat" => f.archive("bad", &FIRST.replace("42", "43")),
            "outside" => f.directory.parent().unwrap().join("outside.json"),
            "parent" => f.directory.join("../outside.json"),
            _ => f.directory.join("missing.json"),
        };
        f.start(&mut run);
        f.exporting(&mut run);
        f.event(&run, DriverStatus::Completed { archive_path: file });
        assert!(run.poll(f.observation()).is_err(), "{input}");
        assert!(matches!(run.state(), RefreshState::Failed));
        assert!(f.project().sources[0].latest_snapshot_id.is_none());
        assert!(
            !f.project().telegram_refresh["pilot"]
                .checkpoint
                .unresolved_attempt
        );
    }
    let f = Fixture::new();
    let p = f
        .store
        .update(
            &f.project.project_id,
            f.project.revision,
            ProjectChange::TelegramRefreshCadence {
                source_id: "pilot".into(),
                cadence: RefreshCadence::Daily,
            },
        )
        .unwrap();
    let mut run = f.coordinator();
    run.start(&p, "pilot", RefreshTrigger::Scheduled, context(100), f.at)
        .unwrap();
    f.event(
        &run,
        DriverStatus::Stopped {
            cancelled: false,
            retry_after: 90_000,
        },
    );
    run.poll(f.observation()).unwrap();
    assert!(matches!(
        run.start(
            &f.project(),
            "pilot",
            RefreshTrigger::Manual,
            context(1000),
            f.at
        )
        .unwrap(),
        RefreshState::Deferred {
            decision: RefreshDecision::WaitUntil(90_100)
        }
    ));
}

#[cfg(unix)]
#[test]
fn returned_symlink_cannot_escape_export_directory() {
    for parent_link in [false, true] {
        let f = Fixture::new();
        let outside = f.directory.parent().unwrap().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("result.json"), FIRST).unwrap();
        let path = if parent_link {
            std::os::unix::fs::symlink(&outside, f.directory.join("linked")).unwrap();
            f.directory.join("linked/result.json")
        } else {
            std::os::unix::fs::symlink(
                outside.join("result.json"),
                f.directory.join("result.json"),
            )
            .unwrap();
            f.directory.join("result.json")
        };
        let mut run = f.coordinator();
        f.start(&mut run);
        f.exporting(&mut run);
        f.event(&run, DriverStatus::Completed { archive_path: path });
        assert!(run.poll(f.observation()).is_err());
        assert!(f.project().sources[0].latest_snapshot_id.is_none());
    }
}

#[test]
fn timer_resumes_after_unlock_and_processes_due_sources_once_per_launch() {
    let f = Fixture::new();
    let mut project = f.project();
    for source in ["pilot", "other"] {
        project = f
            .store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::TelegramRefreshCadence {
                    source_id: source.into(),
                    cadence: RefreshCadence::OnStart,
                },
            )
            .unwrap();
    }
    let mut run = f.coordinator();
    let mut asleep = context(100);
    asleep.session_active_unlocked = false;
    run.tick(asleep, f.at).unwrap();
    assert!(f.events.lock().unwrap().starts.is_empty());
    for (chat, directory) in [("43", "first"), ("42", "second")] {
        run.tick(context(100), f.at).unwrap();
        assert_eq!(run.active_request().unwrap().source.conversation_id, chat);
        f.exporting(&mut run);
        f.event(
            &run,
            DriverStatus::Completed {
                archive_path: f.archive(directory, &FIRST.replace("42", chat)),
            },
        );
        assert!(matches!(
            run.tick(context(100), f.at).unwrap(),
            RefreshState::Ready { .. }
        ));
    }
    run.tick(context(101), f.at).unwrap();
    assert_eq!(f.events.lock().unwrap().starts.len(), 2);
    assert!(run.active_request().is_none());
}
