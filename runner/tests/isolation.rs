//! Opt-in qualification tests: they require an actual strict namespace backend.
//! Run explicitly; a skipped test is never evidence of platform support.
#![cfg(all(target_os = "linux", target_arch = "x86_64", feature = "test-fixtures"))]

use std::fs;
use std::io::Cursor;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use tgsum_core::bundle::BundleOptions;
use tgsum_core::project::{ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;
use tgsum_runner::{
    AdapterContract, Cancellation, Invocation, OfflineRunner, PreparedContext, RunLimits,
    RunnerError, RuntimeFile, RuntimeSpec, Termination, LINUX_OFFLINE_PROFILE,
};

fn contract() -> AdapterContract {
    AdapterContract {
        id: "synthetic-fixture".into(),
        isolation_profile: LINUX_OFFLINE_PROFILE.into(),
        version_probe: invocation(&["--version"]),
        expected_version_output: b"tgsum-fixture 1\n".to_vec(),
    }
}

fn runtime() -> RuntimeSpec {
    let files = [
        ("libc.so.6", "/usr/lib/libc.so.6"),
        ("libgcc_s.so.1", "/usr/lib/libgcc_s.so.1"),
        ("ld-linux-x86-64.so.2", "/lib64/ld-linux-x86-64.so.2"),
    ]
    .into_iter()
    .map(|(name, guest)| {
        let source = ["/usr/lib", "/lib/x86_64-linux-gnu", "/usr/lib64"]
            .into_iter()
            .map(|p| PathBuf::from(p).join(name))
            .find(|p| p.is_file())
            .unwrap();
        RuntimeFile {
            source,
            guest: guest.into(),
        }
    })
    .collect();
    RuntimeSpec {
        executable: env!("CARGO_BIN_EXE_tgsum-runner-fixture").into(),
        files,
    }
}

fn runner() -> OfflineRunner {
    OfflineRunner::qualify(contract(), runtime(), &Cancellation::default()).unwrap()
}

fn invocation(args: &[&str]) -> Invocation {
    Invocation {
        args: args.iter().map(|s| (*s).into()).collect(),
        stdin: Vec::new(),
    }
}

struct ContextFixture {
    root: tempfile::TempDir,
    context: PreparedContext,
    project: tgsum_core::project::Project,
}

fn context() -> ContextFixture {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("Synthetic Project").unwrap();
    let scope = SourceScope::telegram("private-account", "77");
    store.snapshots(&project.project_id).unwrap().import_telegram(
        "snapshot", &scope,
        Cursor::new(r#"{"id":77,"name":"Synthetic","messages":[{"id":1,"date":"2026-06-18T10:00:00","text":"SELECTED token=SYNTHETIC_SECRET"}]}"#),
    ).unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "source".into(),
                connector_id: "telegram_json".into(),
                scope,
                archive_path: Some(root.path().join("raw.json")),
                latest_snapshot_id: Some("snapshot".into()),
                selection: Default::default(),
            }),
        )
        .unwrap();
    let review = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions {
                redact_candidates: true,
            },
            || false,
        )
        .unwrap();
    let context = PreparedContext::from_bundle(
        root.path().into(),
        &project.project_id,
        &review.bundle_id,
        project.revision,
        &Cancellation::default(),
    )
    .unwrap();
    ContextFixture {
        root,
        context,
        project,
    }
}

const REQUIRES: &str = "requires bubblewrap 0.12.0, Linux namespaces and close_range";

#[test]
#[ignore = "requires bubblewrap 0.12.0, Linux namespaces and close_range"]
fn selected_context_is_read_only_and_host_files_env_sockets_are_absent() {
    let runner = runner();
    let fixture = context();
    let raw = fixture.root.path().join("raw.json");
    let credentials = fixture.root.path().join("connector-secret");
    fs::write(&raw, "SYNTHETIC_RAW").unwrap();
    fs::write(&credentials, "SYNTHETIC_CREDENTIAL").unwrap();
    let private = fixture.root.path().join(&fixture.project.project_id);
    let mut args = invocation(&["inspect"]);
    args.args.extend([
        raw.as_os_str().into(),
        credentials.as_os_str().into(),
        private.as_os_str().into(),
    ]);
    let output = runner
        .run(
            &fixture.context,
            &args,
            RunLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(output.process_succeeded(), "{output:?}");
    assert_eq!(output.stdout, b"isolated\n");
    assert_eq!(fs::read_to_string(&raw).unwrap(), "SYNTHETIC_RAW");
    let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let socket_path = fixture.root.path().join("host.sock");
    let _unix = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
    let output = runner
        .run(
            &fixture.context,
            &invocation(&[
                "network",
                &tcp.local_addr().unwrap().to_string(),
                socket_path.to_str().unwrap(),
            ]),
            RunLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(output.process_succeeded(), "{output:?}");
    assert!(ProjectStore::new(fixture.root.path())
        .open(&fixture.project.project_id)
        .unwrap()
        .baselines
        .is_empty());
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            runner
                .run(
                    &fixture.context,
                    &args,
                    RunLimits::default(),
                    &Cancellation::default(),
                )
                .unwrap()
        });
        let second = scope.spawn(|| {
            runner
                .run(
                    &fixture.context,
                    &args,
                    RunLimits::default(),
                    &Cancellation::default(),
                )
                .unwrap()
        });
        assert!(first.join().unwrap().process_succeeded());
        assert!(second.join().unwrap().process_succeeded());
    });
}

#[test]
#[ignore = "requires bubblewrap 0.12.0, Linux namespaces and close_range"]
fn arguments_bytes_exit_codes_and_runtime_snapshot_are_preserved() {
    let source = tempfile::tempdir().unwrap();
    let copied = source.path().join("agent with spaces");
    fs::copy(runtime().executable, &copied).unwrap();
    let mut spec = runtime();
    spec.executable = copied.clone();
    let runner = OfflineRunner::qualify(contract(), spec, &Cancellation::default()).unwrap();
    fs::write(copied, "replaced after qualification").unwrap();
    let fixture = context();
    let cancel = Cancellation::default();
    let mut input = invocation(&["echo"]);
    input.stdin = vec![0, 255, b'a', b'\n'];
    let output = runner
        .run(&fixture.context, &input, RunLimits::default(), &cancel)
        .unwrap();
    assert!(output.process_succeeded());
    assert_eq!(output.stdout, input.stdin);
    assert_eq!(output.stderr, b"diagnostic");
    assert!(!format!("{output:?}").contains("diagnostic"));
    let literal = "$(touch /tmp/injected); `whoami` | cat > /context/x";
    let output = runner
        .run(
            &fixture.context,
            &invocation(&["args", literal, "space value"]),
            RunLimits::default(),
            &cancel,
        )
        .unwrap();
    assert!(output.process_succeeded());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}:{literal}\n11:space value\n", literal.len())
    );
    let output = runner
        .run(
            &fixture.context,
            &invocation(&["exit"]),
            RunLimits::default(),
            &cancel,
        )
        .unwrap();
    assert_eq!(output.termination, Termination::Exited);
    assert_eq!(output.exit_code, Some(17));
    assert!(!output.process_succeeded());
}

#[test]
#[ignore = "requires bubblewrap 0.12.0, Linux namespaces and close_range"]
fn timeout_cancel_overflow_and_success_kill_pipe_holding_descendants() {
    let runner = runner();
    let fixture = context();
    let mut limits = RunLimits {
        timeout: Duration::from_millis(250),
        stdout_bytes: 16384,
        stderr_bytes: 8192,
        ..RunLimits::default()
    };
    for (mode, termination) in [
        ("tree-sleep", Termination::TimedOut),
        ("stderr-flood", Termination::StderrLimit),
    ] {
        let started = Instant::now();
        let output = runner
            .run(
                &fixture.context,
                &invocation(&[mode]),
                limits,
                &Cancellation::default(),
            )
            .unwrap();
        assert_eq!(output.termination, termination, "{mode}: {output:?}");
        if mode == "tree-sleep" {
            assert_eq!(output.stdout, b"descendant-ready\n");
        }
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(
            output.stdout.len() <= limits.stdout_bytes
                && output.stderr.len() <= limits.stderr_bytes
        );
    }
    limits.stdout_bytes = 4096;
    limits.stderr_bytes = 65536;
    let output = runner
        .run(
            &fixture.context,
            &invocation(&["flood"]),
            limits,
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(output.termination, Termination::StdoutLimit);
    let mut input = invocation(&["sleep"]);
    input.stdin = vec![b'x'; 65536];
    let output = runner
        .run(&fixture.context, &input, limits, &Cancellation::default())
        .unwrap();
    assert_eq!(output.termination, Termination::TimedOut);
    let cancel = Cancellation::default();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(150));
            cancel.cancel();
        });
        let output = runner
            .run(
                &fixture.context,
                &invocation(&["tree-sleep"]),
                RunLimits::default(),
                &cancel,
            )
            .unwrap();
        assert_eq!(output.termination, Termination::Cancelled);
        assert_eq!(output.stdout, b"descendant-ready\n");
    });
    let started = Instant::now();
    let output = runner
        .run(
            &fixture.context,
            &invocation(&["orphan"]),
            RunLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(output.process_succeeded(), "{output:?}");
    assert_eq!(output.stdout, b"descendant-ready\n");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
#[ignore = "requires bubblewrap 0.12.0, Linux namespaces and close_range"]
fn inheritable_file_descriptors_are_closed_in_payload() {
    use rustix::io::{fcntl_setfd, FdFlags};
    let sentinel = tempfile::tempfile().unwrap();
    fcntl_setfd(&sentinel, FdFlags::empty()).unwrap();
    let runner = runner();
    let fixture = context();
    let output = runner
        .run(
            &fixture.context,
            &invocation(&["fd", &sentinel.as_raw_fd().to_string()]),
            RunLimits::default(),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(output.process_succeeded(), "{output:?}");
    assert_eq!(output.stdout, b"fd closed\n");
    sentinel.metadata().unwrap(); // Parent ownership is unaffected.
}

#[test]
#[ignore = "requires bubblewrap 0.12.0, Linux namespaces and close_range"]
fn changed_project_unknown_version_and_incomplete_runtime_refuse_to_run() {
    let mut wrong = contract();
    wrong.expected_version_output = b"tgsum-fixture 2\n".to_vec();
    assert!(matches!(
        OfflineRunner::qualify(wrong, runtime(), &Cancellation::default()),
        Err(RunnerError::ExportOnly(_))
    ));
    let incomplete = RuntimeSpec {
        executable: runtime().executable,
        files: vec![],
    };
    assert!(matches!(
        OfflineRunner::qualify(contract(), incomplete, &Cancellation::default()),
        Err(RunnerError::ExportOnly(_))
    ));
    let runner = runner();
    let fixture = context();
    ProjectStore::new(fixture.root.path())
        .update(
            &fixture.project.project_id,
            fixture.project.revision,
            ProjectChange::Rename("changed".into()),
        )
        .unwrap();
    assert!(runner
        .run(
            &fixture.context,
            &invocation(&["echo"]),
            RunLimits::default(),
            &Cancellation::default()
        )
        .is_err());
}

#[test]
#[ignore = "requires bubblewrap 0.12.0, Linux namespaces and close_range"]
fn hostile_parent_environment_cannot_enter_sandbox() {
    if std::env::var_os("TGSUM_RUNNER_ENV_CHILD").is_some() {
        let runner = runner();
        let fixture = context();
        let output = runner
            .run(
                &fixture.context,
                &invocation(&["inspect"]),
                RunLimits::default(),
                &Cancellation::default(),
            )
            .unwrap();
        assert!(output.process_succeeded(), "{output:?}");
        return;
    }
    // Environment mutation is confined to a new test process, not the concurrent
    // test harness. The bogus preload yields a loader diagnostic in that process
    // only; env_clear must remove it before bwrap's own loader is reached.
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "hostile_parent_environment_cannot_enter_sandbox",
            "--nocapture",
        ])
        .env("TGSUM_RUNNER_ENV_CHILD", "1")
        .env("TGSUM_SECRET_SENTINEL", "SYNTHETIC_SECRET")
        .env("LD_PRELOAD", "/tgsum-synthetic-absent-preload.so")
        .env("SSH_AUTH_SOCK", "/tgsum-synthetic-agent.sock")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{REQUIRES}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
