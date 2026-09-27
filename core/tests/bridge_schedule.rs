use tgsum_core::bridge_schedule::{
    RefreshCadence, RefreshCheckpoint, RefreshContext, RefreshDecision,
};

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
