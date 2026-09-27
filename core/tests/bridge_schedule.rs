use tgsum_core::assisted::AssistedExportSettings;
use tgsum_core::bridge_schedule::{
    RefreshCadence, RefreshCheckpoint, RefreshContext, RefreshDecision, TelegramExportLease,
};
use tgsum_core::project::{ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;

fn context<'a>(now: u64, launch_id: &'a str) -> RefreshContext<'a> {
    RefreshContext {
        now,
        launch_id,
        session_active_unlocked: true,
        client_not_before: None,
        any_export_in_flight: false,
    }
}

#[test]
fn default_manual_mode_never_starts_an_unattended_export() {
    let checkpoint = RefreshCheckpoint::default();
    assert_eq!(
        checkpoint.try_claim(RefreshCadence::Manual, context(0, "app-1")),
        Err(RefreshDecision::ManualOnly)
    );
}

#[test]
fn due_job_waits_for_unlock_and_global_export_lease() {
    let checkpoint = RefreshCheckpoint::default();
    let mut locked = context(100, "app-1");
    locked.session_active_unlocked = false;
    assert_eq!(
        checkpoint.try_claim(RefreshCadence::Daily, locked),
        Err(RefreshDecision::NeedsActiveSession)
    );
    let mut busy = context(101, "app-1");
    busy.any_export_in_flight = true;
    assert_eq!(
        checkpoint.try_claim(RefreshCadence::Daily, busy),
        Err(RefreshDecision::Busy)
    );
    let claimed = checkpoint
        .try_claim(RefreshCadence::Daily, context(102, "app-1"))
        .unwrap();
    assert!(claimed.unresolved_attempt);
    assert_eq!(claimed.last_started_at, Some(102));
}

#[test]
fn crash_or_timeout_does_not_replay_on_restart() {
    let checkpoint = RefreshCheckpoint::default()
        .try_claim(RefreshCadence::OnStart, context(10, "first-launch"))
        .unwrap();
    let persisted = serde_json::to_string(&checkpoint).unwrap();
    let after_restart: RefreshCheckpoint = serde_json::from_str(&persisted).unwrap();
    assert_eq!(
        after_restart.try_claim(RefreshCadence::OnStart, context(20, "second-launch")),
        Err(RefreshDecision::NeedsUserAction)
    );
    assert_eq!(
        after_restart.decide(RefreshCadence::Manual, context(20, "second-launch")),
        RefreshDecision::NeedsUserAction
    );
    let resolved = after_restart.resolved_successfully();
    assert_eq!(
        resolved.try_claim(RefreshCadence::OnStart, context(21, "first-launch")),
        Err(RefreshDecision::AlreadyAttempted)
    );
    assert!(resolved
        .try_claim(RefreshCadence::OnStart, context(21, "second-launch"))
        .is_ok());
}

#[test]
fn known_failure_and_client_delay_do_not_make_retries_faster() {
    let checkpoint = RefreshCheckpoint::default()
        .try_claim(RefreshCadence::EverySixHours, context(100, "app-1"))
        .unwrap()
        .resolved_with_backoff(200, 900);
    let mut early = context(1_000, "app-2");
    early.client_not_before = Some(30_000);
    assert_eq!(
        checkpoint.decide(RefreshCadence::EverySixHours, early),
        RefreshDecision::WaitUntil(30_000)
    );
    let mut due = context(30_000, "app-2");
    due.session_active_unlocked = false;
    assert_eq!(
        checkpoint.decide(RefreshCadence::EverySixHours, due),
        RefreshDecision::NeedsActiveSession
    );
    assert!(checkpoint
        .try_claim(RefreshCadence::EverySixHours, context(30_001, "app-2"))
        .is_ok());
}

#[test]
fn interval_counts_from_attempt_and_handles_backwards_clock() {
    let checkpoint = RefreshCheckpoint::default()
        .try_claim(RefreshCadence::Daily, context(86_400, "app-1"))
        .unwrap()
        .resolved_successfully();
    assert_eq!(
        checkpoint.decide(RefreshCadence::Daily, context(86_399, "app-2")),
        RefreshDecision::WaitUntil(172_800)
    );
    assert_eq!(
        checkpoint.decide(RefreshCadence::Daily, context(172_800, "app-2")),
        RefreshDecision::Ready
    );
}

fn configured_source(store: &ProjectStore, directory: &std::path::Path) -> String {
    let project = store.create("Synthetic Telegram").unwrap();
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
    assert_eq!(project.telegram_refresh.len(), 0);
    assert_eq!(
        store
            .update(
                &project.project_id,
                project.revision,
                ProjectChange::TelegramRefreshCadence {
                    source_id: "pilot".into(),
                    cadence: RefreshCadence::Daily,
                },
            )
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::AssistedExport {
                source_id: "pilot".into(),
                settings: Some(AssistedExportSettings {
                    directory: directory.to_owned(),
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
                cadence: RefreshCadence::OnStart,
            },
        )
        .unwrap();
    project.project_id
}

#[test]
fn persisted_claim_and_global_lease_prevent_replay_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let second_store = ProjectStore::new(root.path());
    let project_id = configured_source(&store, root.path());

    let lease = TelegramExportLease::try_acquire(&store).unwrap().unwrap();
    assert!(TelegramExportLease::try_acquire(&second_store)
        .unwrap()
        .is_none());
    let attempt = lease
        .claim(&store, &project_id, "pilot", context(100, "launch-1"))
        .unwrap()
        .unwrap();
    assert!(
        store.open(&project_id).unwrap().telegram_refresh["pilot"]
            .checkpoint
            .unresolved_attempt
    );
    drop(lease); // Synthetic process exit after an unknown client outcome.

    let recovered_lease = TelegramExportLease::try_acquire(&second_store)
        .unwrap()
        .unwrap();
    assert_eq!(
        recovered_lease
            .claim(
                &second_store,
                &project_id,
                "pilot",
                context(200, "launch-2")
            )
            .unwrap()
            .unwrap_err(),
        RefreshDecision::NeedsUserAction
    );
    let current = second_store.open(&project_id).unwrap();
    second_store
        .update(
            &project_id,
            current.revision,
            ProjectChange::Rename("Edited while the attempt was unresolved".into()),
        )
        .unwrap();
    recovered_lease
        .resolve_successfully(&second_store, &attempt)
        .unwrap();
    assert_eq!(
        recovered_lease
            .claim(
                &second_store,
                &project_id,
                "pilot",
                context(201, "launch-1")
            )
            .unwrap()
            .unwrap_err(),
        RefreshDecision::AlreadyAttempted
    );
    assert_eq!(
        recovered_lease
            .resolve_successfully(&second_store, &attempt)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    let second = recovered_lease
        .claim(
            &second_store,
            &project_id,
            "pilot",
            context(202, "launch-2"),
        )
        .unwrap()
        .unwrap();
    recovered_lease
        .resolve_with_backoff(&second_store, &second, 202, 900)
        .unwrap();
    assert_eq!(
        recovered_lease
            .claim(
                &second_store,
                &project_id,
                "pilot",
                context(203, "launch-3")
            )
            .unwrap()
            .unwrap_err(),
        RefreshDecision::WaitUntil(1102)
    );
}

#[test]
fn removing_assisted_settings_disables_cadence_without_erasing_unresolved_attempt() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project_id = configured_source(&store, root.path());
    let lease = TelegramExportLease::try_acquire(&store).unwrap().unwrap();
    let _attempt = lease
        .claim(&store, &project_id, "pilot", context(100, "launch-1"))
        .unwrap()
        .unwrap();
    let project = store.open(&project_id).unwrap();
    let removed = store
        .update(
            &project_id,
            project.revision,
            ProjectChange::AssistedExport {
                source_id: "pilot".into(),
                settings: None,
            },
        )
        .unwrap();
    assert_eq!(
        removed.telegram_refresh["pilot"].cadence,
        RefreshCadence::Manual
    );
    assert!(
        removed.telegram_refresh["pilot"]
            .checkpoint
            .unresolved_attempt
    );
    assert_eq!(
        lease
            .claim(&store, &project_id, "pilot", context(200, "launch-2"))
            .unwrap()
            .unwrap_err(),
        RefreshDecision::NeedsUserAction
    );
}

#[test]
fn schema_eight_migrates_without_rewriting_or_accepting_refresh_state() {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Legacy").unwrap();
    let path = root
        .path()
        .join(&project.project_id)
        .join("revisions/00000000000000000000.json");
    let mut value = serde_json::to_value(&project).unwrap();
    value["schema_version"] = 8.into();
    let bytes = serde_json::to_vec(&value).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let opened = store.open(&project.project_id).unwrap();
    assert_eq!(opened.schema_version, 9);
    assert!(opened.telegram_refresh.is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    value["telegram_refresh"] = serde_json::json!({"pilot":{"cadence":"daily"}});
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(store.open(&project.project_id).is_err());
}
