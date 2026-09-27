use serde_json::json;
use std::cell::Cell;
use std::io::{self, Cursor};
use std::panic::{catch_unwind, AssertUnwindSafe};
use tgsum_core::analysis::AnalysisSpec;
use tgsum_core::automation::*;
use tgsum_core::project::{Project, ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::recipe::{Recipe, RecipeOutput};
use tgsum_core::snapshot::SourceScope;

struct Fixture {
    root: tempfile::TempDir,
    store: ProjectStore,
    id: String,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(root.path());
        let p = store.create("Work chats").unwrap();
        let scope = SourceScope::telegram("synthetic", "42");
        store.snapshots(&p.project_id).unwrap().import_telegram("first", &scope,
            Cursor::new(r#"{"id":42,"type":"private_group","messages":[{"id":1,"text":"Alice owns deployment."}]}"#)).unwrap();
        store
            .update(
                &p.project_id,
                p.revision,
                ProjectChange::Source(ProjectSource {
                    source_id: "work".into(),
                    connector_id: "telegram_json".into(),
                    scope,
                    archive_path: None,
                    latest_snapshot_id: Some("first".into()),
                    selection: Default::default(),
                }),
            )
            .unwrap();
        Self {
            root,
            store,
            id: p.project_id,
        }
    }
    fn project(&self) -> Project {
        self.store.open(&self.id).unwrap()
    }
    fn config(&self) -> AutomationSettings {
        AutomationSettings {
            cadence: Cadence::EverySixHours,
            expires_at: 100 + 30 * 86400,
            max_context_bytes: 1024 * 1024,
            spec: AnalysisSpec {
                agent: "synthetic".into(),
                agent_version: "1".into(),
                isolation_profile: "synthetic-only".into(),
                destination: "local_fixture".into(),
                model: "fixture".into(),
                recipe: "actions".into(),
                recipe_version: 1,
            },
        }
    }
    fn configure(&self) -> Automation {
        self.store
            .configure_automation(
                &self.project(),
                self.store.automation(&self.id).unwrap().map(|a| a.revision),
                self.config(),
                100,
            )
            .unwrap()
    }
    fn enable(&self) -> Automation {
        let p = self.configure();
        self.store
            .pause_automation(&self.id, p.revision, false, 100)
            .unwrap()
    }
    fn tick(&self, time: u64, engine: &mut Engine) -> io::Result<Tick> {
        self.store
            .tick_automation(&self.id, || time, engine, || false)
    }
    fn replace(&self, name: &str, text: &str) {
        let p = self.project();
        self.store
            .snapshots(&self.id)
            .unwrap()
            .import_telegram(
                name,
                &p.sources[0].scope,
                Cursor::new(
                    json!({"id":42,"type":"private_group","messages":[{"id":1,"text":text}]})
                        .to_string(),
                ),
            )
            .unwrap();
        self.store
            .update(
                &self.id,
                p.revision,
                ProjectChange::RecordSnapshot {
                    source_id: "work".into(),
                    snapshot_id: name.into(),
                },
            )
            .unwrap();
    }
}

#[derive(Default)]
struct Engine {
    calls: usize,
    expected_sources: Option<usize>,
    unavailable: bool,
    bad_evidence: bool,
    fail: bool,
    crash: bool,
    hook: Option<Box<dyn FnMut()>>,
}
impl AutomationExecutor for Engine {
    fn available(&self, _: &AnalysisSpec) -> bool {
        !self.unavailable
    }
    fn analyze(
        &mut self,
        input: &AutomationInput,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<RecipeOutput> {
        self.calls += 1;
        if let Some(hook) = &mut self.hook {
            hook();
        }
        assert!(!input.documents.is_empty());
        assert_eq!(
            input.manifest.sources.len(),
            self.expected_sources.unwrap_or(1)
        );
        assert!(!input
            .documents
            .join("\n")
            .contains("PRIVATE_UNSELECTED_CHAT"));
        assert_eq!(input.manifest.privacy.needs_review, 0);
        if self.crash {
            panic!("synthetic worker crash");
        }
        if self.fail {
            return Err(io::Error::other("synthetic transport failure"));
        }
        if cancelled() {
            return Err(tgsum_core::cancelled());
        }
        let (id, revision) = input
            .documents
            .iter()
            .flat_map(|s| s.lines())
            .find_map(|s| s.strip_prefix("## Evidence "))
            .unwrap()
            .split_once('@')
            .unwrap();
        let recipe = Recipe::from_version(&input.spec.recipe, input.spec.recipe_version)?;
        let reference =
            json!({"id": if self.bad_evidence {"forged"} else {id},"revision":revision});
        Ok(serde_json::from_value(json!({
            "recipe":recipe.id(),"version":1,
            "sections":recipe.sections().iter().map(|id| json!({"id":id,"claims":[]})).collect::<Vec<_>>(),
            "actions":[{"task":{"text":"Follow up on deployment","evidence":[reference]},"owner":null,"deadline":null}]
        })).unwrap())
    }
}

#[test]
fn saved_plan_requires_opt_in_and_obeys_clock_expiry_and_pause() {
    let f = Fixture::new();
    let mut engine = Engine::default();
    assert_eq!(
        f.tick(100, &mut engine).unwrap(),
        Tick::Deferred(Deferred::NotConfigured)
    );
    let p = f.configure();
    assert_eq!(
        f.tick(100, &mut engine).unwrap(),
        Tick::Deferred(Deferred::Paused)
    );
    let enabled = f
        .store
        .pause_automation(&f.id, p.revision, false, 100)
        .unwrap();
    assert_eq!(
        f.tick(99, &mut engine).unwrap(),
        Tick::Deferred(Deferred::Expired)
    );
    engine.unavailable = true;
    assert_eq!(
        f.tick(100, &mut engine).unwrap(),
        Tick::Deferred(Deferred::ExecutorUnavailable)
    );
    assert_eq!(f.store.automation(&f.id).unwrap().unwrap(), enabled);
    engine.unavailable = false;
    assert!(matches!(
        f.tick(100, &mut engine).unwrap(),
        Tick::Finished(plan) if matches!(plan.outcome, Outcome::Succeeded { .. })
    ));
    assert_eq!(engine.calls, 1);
    assert_eq!(
        f.tick(101, &mut engine).unwrap(),
        Tick::Deferred(Deferred::WaitUntil(21700))
    );
    let a = f.store.automation(&f.id).unwrap().unwrap();
    let paused = f
        .store
        .pause_automation(&f.id, a.revision, true, 101)
        .unwrap();
    assert_eq!(
        f.tick(21700, &mut engine).unwrap(),
        Tick::Deferred(Deferred::Paused)
    );
    assert!(f
        .store
        .pause_automation(&f.id, paused.revision, false, f.config().expires_at)
        .is_err());
}

#[test]
fn identical_reimports_skip_agent_and_changed_context_saves_evidence_and_baseline() {
    let f = Fixture::new();
    f.enable();
    let mut engine = Engine::default();
    let Tick::Finished(first) = f.tick(100, &mut engine).unwrap() else {
        panic!()
    };
    let Outcome::Succeeded { run_id } = first.outcome else {
        panic!()
    };
    let result = f.store.read_analysis(&f.id, &run_id).unwrap();
    assert!(result.committed_revision.is_some());
    let baseline = f.project().baselines;
    assert_eq!(baseline[0].snapshot_id, "first");
    f.replace("second", "Alice owns deployment.");
    assert!(matches!(
        f.tick(21700, &mut engine).unwrap(),
        Tick::Finished(plan) if matches!(plan.outcome, Outcome::NoChanges)
    ));
    assert_eq!(engine.calls, 1);
    assert_eq!(f.project().baselines, baseline);
    f.replace("third", "Bob owns deployment.");
    assert!(matches!(
        f.tick(43300, &mut engine).unwrap(),
        Tick::Finished(plan) if matches!(plan.outcome, Outcome::Succeeded { .. })
    ));
    assert_eq!(engine.calls, 2);
    assert_eq!(f.project().baselines[0].snapshot_id, "third");
}

#[test]
fn delta_with_no_new_messages_does_not_call_agent_or_publish_success() {
    let f = Fixture::new();
    let p = f.project();
    let mut selection = p.sources[0].selection.clone();
    selection.only_changes = true;
    f.store
        .update(
            &f.id,
            p.revision,
            ProjectChange::Selection {
                source_id: "work".into(),
                selection,
            },
        )
        .unwrap();
    f.enable();
    let mut engine = Engine::default();
    f.tick(100, &mut engine).unwrap();
    let baseline = f.project().baselines;
    assert!(matches!(
        f.tick(21700, &mut engine).unwrap(),
        Tick::Finished(plan) if matches!(plan.outcome, Outcome::NoMessages)
    ));
    assert_eq!(engine.calls, 1);
    assert_eq!(f.project().baselines, baseline);
}

#[test]
fn edits_to_selected_scope_require_new_review_and_stale_configuration_is_rejected() {
    let f = Fixture::new();
    let enabled = f.enable();
    let old = f.project();
    f.store
        .update(
            &f.id,
            old.revision,
            ProjectChange::Rename("Changed project".into()),
        )
        .unwrap();
    let mut engine = Engine::default();
    assert_eq!(
        f.tick(100, &mut engine).unwrap(),
        Tick::Deferred(Deferred::NeedsReview)
    );
    assert!(f
        .store
        .configure_automation(&old, Some(enabled.revision), f.config(), 100)
        .is_err());
    assert!(f
        .store
        .configure_automation(&f.project(), Some(0), f.config(), 100)
        .is_err());
    assert_eq!(engine.calls, 0);
    let p = f.project();
    let mut selection = p.sources[0].selection.clone();
    selection.enabled = false;
    f.store
        .update(
            &f.id,
            p.revision,
            ProjectChange::Selection {
                source_id: "work".into(),
                selection,
            },
        )
        .unwrap();
    assert_eq!(
        f.tick(100, &mut engine).unwrap(),
        Tick::Deferred(Deferred::NeedsReview)
    );
    assert!(f
        .store
        .configure_automation(&f.project(), Some(enabled.revision), f.config(), 100)
        .is_err());
}

#[test]
fn automation_process_child() {
    let Some(root) = std::env::var_os("TGSUM_TEST_AUTOMATION_ROOT") else {
        return;
    };
    let id = std::env::var("TGSUM_TEST_AUTOMATION_PROJECT").unwrap();
    let expected = std::env::var("TGSUM_TEST_AUTOMATION_DEFERRED").unwrap();
    let store = ProjectStore::new(root);
    let mut engine = Engine::default();
    let actual = store
        .tick_automation(&id, || 100, &mut engine, || false)
        .unwrap();
    assert_eq!(
        actual,
        Tick::Deferred(if expected == "busy" {
            Deferred::Busy
        } else {
            Deferred::WaitUntil(21700)
        })
    );
    assert_eq!(engine.calls, 0);
}

fn check_process(root: &std::path::Path, id: &str, expected: &str) {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "automation_process_child", "--nocapture"])
        .env("TGSUM_TEST_AUTOMATION_ROOT", root)
        .env("TGSUM_TEST_AUTOMATION_PROJECT", id)
        .env("TGSUM_TEST_AUTOMATION_DEFERRED", expected)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "synthetic automation worker failed");
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("synthetic automation worker timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn independent_process_cannot_dispatch_during_run_or_replay_after_release() {
    let f = Fixture::new();
    f.enable();
    let root = f.root.path().to_path_buf();
    let id = f.id.clone();
    let mut engine = Engine {
        hook: Some(Box::new(move || check_process(&root, &id, "busy"))),
        ..Engine::default()
    };
    f.tick(100, &mut engine).unwrap();
    assert_eq!(engine.calls, 1);
    check_process(f.root.path(), &f.id, "scheduled");
}

#[test]
fn missing_source_and_unreviewed_secret_or_budget_never_reach_executor() {
    for scenario in ["missing", "missing-second", "secret", "budget"] {
        let f = Fixture::new();
        if scenario == "missing" {
            let p = f.project();
            let mut source = p.sources[0].clone();
            source.latest_snapshot_id = None;
            f.store
                .update(&f.id, p.revision, ProjectChange::Source(source))
                .unwrap();
        } else if scenario == "missing-second" {
            let p = f.project();
            let mut source = p.sources[0].clone();
            source.source_id = "second".into();
            source.scope = SourceScope::telegram("synthetic", "43");
            source.latest_snapshot_id = None;
            f.store
                .update(&f.id, p.revision, ProjectChange::Source(source))
                .unwrap();
        } else if scenario == "secret" {
            f.replace("secret", "sk-SYNTHETIC_NotARealToken_12345");
        }
        let mut config = f.config();
        if scenario == "budget" {
            config.max_context_bytes = 1;
        }
        let plan = f
            .store
            .configure_automation(&f.project(), None, config, 100)
            .unwrap();
        f.store
            .pause_automation(&f.id, plan.revision, false, 100)
            .unwrap();
        let mut engine = Engine::default();
        let _ = f.tick(100, &mut engine);
        assert_eq!(engine.calls, 0, "{scenario}");
        assert!(f.project().baselines.is_empty());
        assert!(f.store.automation(&f.id).unwrap().unwrap().paused);
    }
}

#[test]
fn failed_invalid_cancelled_and_scope_changed_outputs_never_advance_baselines() {
    for scenario in ["failed", "invalid", "cancel", "edit", "expiry"] {
        let f = Fixture::new();
        f.enable();
        let mut engine = Engine {
            fail: scenario == "failed",
            bad_evidence: scenario == "invalid",
            ..Default::default()
        };
        let cancelled = std::rc::Rc::new(Cell::new(false));
        let time = std::rc::Rc::new(Cell::new(100));
        if scenario == "cancel" {
            let flag = cancelled.clone();
            engine.hook = Some(Box::new(move || flag.set(true)));
        } else if scenario == "edit" {
            let store = f.store.clone();
            let id = f.id.clone();
            engine.hook = Some(Box::new(move || {
                let p = store.open(&id).unwrap();
                store
                    .update(&id, p.revision, ProjectChange::Rename("During run".into()))
                    .unwrap();
            }));
        } else if scenario == "expiry" {
            let flag = time.clone();
            let expiry = f.config().expires_at;
            engine.hook = Some(Box::new(move || flag.set(expiry)));
        }
        assert!(
            f.store
                .tick_automation(&f.id, || time.get(), &mut engine, || cancelled.get())
                .is_err(),
            "{scenario}"
        );
        assert!(f.project().baselines.is_empty(), "{scenario}");
        assert!(f.store.automation(&f.id).unwrap().unwrap().paused);
        let calls = engine.calls;
        assert_eq!(
            f.tick(30000, &mut engine).unwrap(),
            Tick::Deferred(Deferred::Paused)
        );
        assert_eq!(engine.calls, calls);
    }
}

#[test]
fn crashed_executor_remains_unresolved_after_restart_without_replaying() {
    let f = Fixture::new();
    f.enable();
    let mut engine = Engine {
        crash: true,
        ..Default::default()
    };
    assert!(catch_unwind(AssertUnwindSafe(|| f.tick(100, &mut engine))).is_err());
    let restarted = ProjectStore::new(f.root.path());
    assert_eq!(
        restarted
            .tick_automation(&f.id, || 30000, &mut Engine::default(), || false)
            .unwrap(),
        Tick::Deferred(Deferred::NeedsReview)
    );
    let pending = restarted.automation(&f.id).unwrap().unwrap();
    assert!(matches!(
        pending.outcome,
        Outcome::Running { run_id: Some(_) }
    ));
    let p = restarted.open(&f.id).unwrap();
    restarted
        .update(
            &f.id,
            p.revision,
            ProjectChange::Rename("Edited after crash".into()),
        )
        .unwrap();
    assert!(restarted
        .configure_automation(&f.project(), Some(pending.revision), f.config(), 100)
        .is_err());
    let closed = restarted
        .acknowledge_automation_interruption(&f.id, pending.revision)
        .unwrap();
    assert!(closed.paused);
    assert_eq!(closed.outcome, Outcome::Interrupted);
    assert!(restarted
        .acknowledge_automation_interruption(&f.id, pending.revision)
        .is_err());
}

#[test]
fn overlapping_worker_cannot_dispatch_or_reconfigure_running_attempt() {
    let f = Fixture::new();
    f.enable();
    let store = f.store.clone();
    let id = f.id.clone();
    let config = f.config();
    let mut engine = Engine {
        hook: Some(Box::new(move || {
            assert_eq!(
                store
                    .tick_automation(&id, || 100, &mut Engine::default(), || false)
                    .unwrap(),
                Tick::Deferred(Deferred::Busy)
            );
            let plan = store.automation(&id).unwrap().unwrap();
            assert!(store
                .pause_automation(&id, plan.revision, true, 100)
                .is_err());
            assert!(store
                .configure_automation(
                    &store.open(&id).unwrap(),
                    Some(plan.revision),
                    config.clone(),
                    100
                )
                .is_err());
        })),
        ..Default::default()
    };
    f.tick(100, &mut engine).unwrap();
    assert_eq!(engine.calls, 1);
}

#[test]
fn corrupt_latest_record_never_falls_back_to_an_older_enabled_plan() {
    let f = Fixture::new();
    let plan = f.enable();
    let path = f
        .root
        .path()
        .join(&f.id)
        .join("automation")
        .join(format!("{:020}.json", plan.revision));
    std::fs::write(&path, b"{").unwrap();
    let mut engine = Engine::default();
    assert!(f.tick(100, &mut engine).is_err());
    assert_eq!(engine.calls, 0);
}

#[test]
fn several_work_chats_share_one_run_while_unselected_chat_stays_private() {
    let f = Fixture::new();
    for (source_id, id, body, enabled) in [
        ("second", "43", "Prepare the release.", true),
        ("private", "44", "PRIVATE_UNSELECTED_CHAT", false),
    ] {
        let p = f.project();
        let scope = SourceScope::telegram("synthetic", id);
        f.store
            .snapshots(&f.id)
            .unwrap()
            .import_telegram(
                source_id,
                &scope,
                Cursor::new(json!({"id":id,"messages":[{"id":1,"text":body}]}).to_string()),
            )
            .unwrap();
        let mut source = p.sources[0].clone();
        source.source_id = source_id.into();
        source.scope = scope;
        source.latest_snapshot_id = Some(source_id.into());
        source.selection.enabled = enabled;
        f.store
            .update(&f.id, p.revision, ProjectChange::Source(source))
            .unwrap();
    }
    f.enable();
    let mut engine = Engine {
        expected_sources: Some(2),
        ..Default::default()
    };
    f.tick(100, &mut engine).unwrap();
    assert_eq!(engine.calls, 1);
    assert_eq!(f.project().baselines.len(), 2);
    assert!(f
        .project()
        .baselines
        .iter()
        .all(|b| b.source_id != "private"));
    let p = f.project();
    let mut selection = p
        .sources
        .iter()
        .find(|s| s.source_id == "private")
        .unwrap()
        .selection
        .clone();
    selection.enabled = true;
    f.store
        .update(
            &f.id,
            p.revision,
            ProjectChange::Selection {
                source_id: "private".into(),
                selection,
            },
        )
        .unwrap();
    assert_eq!(
        f.tick(30000, &mut engine).unwrap(),
        Tick::Deferred(Deferred::NeedsReview)
    );
    assert_eq!(engine.calls, 1);
}

#[test]
fn invalid_expiry_budget_recipe_and_future_schema_are_rejected() {
    let f = Fixture::new();
    for value in ["expired", "long", "zero", "large", "recipe"] {
        let mut config = f.config();
        match value {
            "expired" => config.expires_at = 100,
            "long" => config.expires_at += 1,
            "zero" => config.max_context_bytes = 0,
            "large" => config.max_context_bytes += 1,
            _ => config.spec.recipe_version = 99,
        }
        assert!(f
            .store
            .configure_automation(&f.project(), None, config, 100)
            .is_err());
        assert!(f.store.automation(&f.id).unwrap().is_none());
    }
    let plan = f.enable();
    let path = f
        .root
        .path()
        .join(&f.id)
        .join("automation")
        .join(format!("{:020}.json", plan.revision));
    let mut value = serde_json::to_value(&plan).unwrap();
    value["schema_version"] = 2.into();
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(f.tick(100, &mut Engine::default()).is_err());
}

#[cfg(unix)]
#[test]
fn symlink_automation_directory_never_reads_or_writes_its_target() {
    let f = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), f.root.path().join(&f.id).join("automation"))
        .unwrap();
    assert!(f.store.automation(&f.id).is_err());
    assert!(f
        .store
        .configure_automation(&f.project(), None, f.config(), 100)
        .is_err());
    assert!(f.tick(100, &mut Engine::default()).is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}
