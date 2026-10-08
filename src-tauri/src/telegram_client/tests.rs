use super::*;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct State {
    running: Option<Running>,
    calls: Vec<String>,
    persistent: bool,
    serial: u32,
    fail_quit: bool,
    fail_start_after_spawn: bool,
}

#[derive(Clone)]
struct Fake(Arc<Mutex<State>>);

fn identity(pid: u32) -> Identity {
    Identity {
        pid,
        start_ticks: 9007199254740993 + u64::from(pid),
        boot_id: "synthetic-boot".into(),
        executable_device: 5,
        executable_inode: 7,
    }
}

fn original() -> Running {
    Running {
        identity: identity(100),
        launch: Launch {
            executable: "/usr/bin/Telegram".into(),
            arguments: vec![
                "-startintray".into(),
                "-workdir".into(),
                "/synthetic/Ж 😀".into(),
            ],
            working_directory: "/synthetic/Ж 😀".into(),
        },
    }
}

impl Desktop for Fake {
    fn current(&mut self) -> io::Result<Option<Running>> {
        Ok(self.0.lock().unwrap().running.clone())
    }
    fn persistent_debug_exists(&mut self, _: &Launch) -> io::Result<bool> {
        Ok(self.0.lock().unwrap().persistent)
    }
    fn quit(&mut self, running: &Running) -> io::Result<()> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(state.running.as_ref(), Some(running));
        state.calls.push("quit".into());
        if state.fail_quit {
            return Err(error("synthetic quit timeout"));
        }
        state.running = None;
        Ok(())
    }
    fn start(&mut self, launch: &Launch) -> io::Result<Running> {
        let mut state = self.0.lock().unwrap();
        assert!(state.running.is_none());
        state.calls.push(
            if launch.debug() {
                "start-debug"
            } else {
                "start-original"
            }
            .into(),
        );
        state.serial += 1;
        let running = Running {
            identity: identity(100 + state.serial),
            launch: launch.clone(),
        };
        state.running = Some(running.clone());
        if state.fail_start_after_spawn {
            return Err(error("synthetic crash before claim"));
        }
        Ok(running)
    }
}

struct Fixture {
    root: tempfile::TempDir,
    fake: Fake,
}
impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            fake: Fake(Arc::new(Mutex::new(State {
                running: Some(original()),
                ..Default::default()
            }))),
        }
    }
    fn path(&self) -> PathBuf {
        self.root.path().join("client")
    }
    fn controller(&self) -> Controller<Fake> {
        Controller::new(self.fake.clone(), LeaseJournal::open(&self.path()).unwrap())
    }
    fn lease(&self, phase: Phase, managed: Option<Identity>) {
        LeaseJournal::open(&self.path())
            .unwrap()
            .append(&Lease {
                schema_version: 1,
                original: original().into(),
                phase,
                managed,
            })
            .unwrap();
    }
    fn calls(&self) -> Vec<String> {
        self.fake.0.lock().unwrap().calls.clone()
    }
}

#[test]
fn managed_start_reopen_resume_and_stop_restore_exact_launch() {
    let f = Fixture::new();
    let expected = original().launch;
    let mut controller = f.controller();
    assert_eq!(
        controller.ensure_debug(&expected.log_directory()).unwrap(),
        Mode::ManagedDebug
    );
    let managed = controller.detect().unwrap().unwrap();
    assert_ne!(managed.identity, original().identity);
    drop(controller);
    let mut reopened = f.controller();
    assert_eq!(
        reopened.ensure_debug(&expected.log_directory()).unwrap(),
        Mode::ManagedDebug
    );
    assert_eq!(reopened.detect().unwrap().unwrap(), managed);
    reopened.restore().unwrap();
    let restored = reopened.detect().unwrap().unwrap();
    assert_eq!(restored.launch, expected);
    assert_ne!(restored.identity, managed.identity);
    reopened.restore().unwrap();
    assert_eq!(f.calls(), ["quit", "start-debug", "quit", "start-original"]);
    assert_eq!(
        reopened.journal.latest().unwrap().unwrap().phase,
        Phase::Released
    );
}

#[test]
fn user_debug_and_unknown_persistent_setting_are_never_switched_off() {
    let f = Fixture::new();
    f.fake
        .0
        .lock()
        .unwrap()
        .running
        .as_mut()
        .unwrap()
        .launch
        .arguments
        .push("-debug".into());
    let mut controller = f.controller();
    assert_eq!(
        controller
            .ensure_debug(&original().launch.log_directory())
            .unwrap(),
        Mode::UserDebug
    );
    controller.restore().unwrap();
    assert!(f.calls().is_empty());
    assert!(controller.journal.latest().unwrap().is_none());
    f.fake.0.lock().unwrap().running = Some(original());
    f.fake.0.lock().unwrap().persistent = true;
    assert!(controller
        .ensure_debug(&original().launch.log_directory())
        .is_err());
    assert!(f.calls().is_empty());
}

#[test]
fn wrong_log_directory_never_quits_existing_client() {
    let f = Fixture::new();
    assert!(f
        .controller()
        .ensure_debug(Path::new("/different/DebugLogs"))
        .is_err());
    assert!(f.calls().is_empty());
}

#[test]
fn reused_pid_and_changed_start_identity_are_not_owned() {
    let f = Fixture::new();
    let mut controller = f.controller();
    controller
        .ensure_debug(&original().launch.log_directory())
        .unwrap();
    let before = f.calls();
    f.fake
        .0
        .lock()
        .unwrap()
        .running
        .as_mut()
        .unwrap()
        .identity
        .start_ticks += 1;
    assert!(controller
        .ensure_debug(&original().launch.log_directory())
        .is_err());
    assert!(controller.restore().is_err());
    assert_eq!(before, f.calls());
}

#[test]
fn disappeared_claimed_client_restarts_with_same_original_launch() {
    let f = Fixture::new();
    let mut controller = f.controller();
    controller
        .ensure_debug(&original().launch.log_directory())
        .unwrap();
    let first = controller.detect().unwrap().unwrap().identity;
    f.fake.0.lock().unwrap().running = None;
    controller
        .ensure_debug(&original().launch.log_directory())
        .unwrap();
    assert_ne!(controller.detect().unwrap().unwrap().identity, first);
    controller.restore().unwrap();
    assert_eq!(
        controller.detect().unwrap().unwrap().launch,
        original().launch
    );
}

#[test]
fn prepared_receipt_recovers_before_and_after_graceful_quit() {
    for running in [Some(original()), None] {
        let f = Fixture::new();
        f.lease(Phase::Prepared, None);
        f.fake.0.lock().unwrap().running = running.clone();
        let mut controller = f.controller();
        controller
            .ensure_debug(&original().launch.log_directory())
            .unwrap();
        assert_eq!(
            f.calls().iter().filter(|call| *call == "quit").count(),
            usize::from(running.is_some())
        );
        controller.restore().unwrap();
        assert_eq!(
            controller.detect().unwrap().unwrap().launch,
            original().launch
        );
    }
}

#[test]
fn pending_launch_crash_does_not_adopt_an_unclaimed_process() {
    let f = Fixture::new();
    f.fake.0.lock().unwrap().fail_start_after_spawn = true;
    let mut controller = f.controller();
    assert!(controller
        .ensure_debug(&original().launch.log_directory())
        .is_err());
    assert_eq!(
        controller.journal.latest().unwrap().unwrap().phase,
        Phase::DebugPending
    );
    let before = f.calls();
    drop(controller);
    let mut reopened = f.controller();
    assert!(reopened
        .ensure_debug(&original().launch.log_directory())
        .is_err());
    assert!(reopened.restore().is_err());
    assert_eq!(f.calls(), before);
    // User restoration to the exact normal launch needs no further mutation.
    f.fake.0.lock().unwrap().running = Some(original());
    reopened.restore().unwrap();
    assert_eq!(f.calls(), before);
}

#[test]
fn restoration_recovers_after_quit_and_after_original_launch() {
    for phase in [Phase::Restoring, Phase::RestorePending] {
        for current in [None, Some(original())] {
            let f = Fixture::new();
            f.lease(phase, (phase == Phase::Restoring).then(|| identity(101)));
            f.fake.0.lock().unwrap().running = current.clone();
            let mut controller = f.controller();
            controller.restore().unwrap();
            assert_eq!(
                controller.detect().unwrap().unwrap().launch,
                original().launch
            );
            assert_eq!(f.calls().len(), usize::from(current.is_none()));
        }
    }
}

#[test]
fn quit_timeout_preserves_original_and_can_be_stopped_without_restart() {
    let f = Fixture::new();
    f.fake.0.lock().unwrap().fail_quit = true;
    let mut controller = f.controller();
    assert!(controller
        .ensure_debug(&original().launch.log_directory())
        .is_err());
    assert_eq!(controller.detect().unwrap().unwrap(), original());
    controller.restore().unwrap();
    assert_eq!(f.calls(), ["quit"]);
}

#[test]
fn journal_is_private_single_writer_and_preserves_unbranded_directory() {
    let f = Fixture::new();
    fs::create_dir(f.path()).unwrap();
    let foreign = f.path().join("user.txt");
    fs::write(&foreign, "untouched").unwrap();
    assert!(LeaseJournal::open(&f.path()).is_err());
    assert_eq!(fs::read_to_string(foreign).unwrap(), "untouched");
    let owned = f.root.path().join("owned");
    let journal = LeaseJournal::open(&owned).unwrap();
    assert!(LeaseJournal::open(&owned).is_err());
    assert_eq!(
        fs::metadata(&owned).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(owned.join("owner.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    drop(journal);
    assert!(LeaseJournal::open(&owned).is_ok());
}

#[test]
fn replaced_directory_or_marker_stops_before_client_actions() {
    for marker in [false, true] {
        let f = Fixture::new();
        let mut controller = f.controller();
        if marker {
            let path = f.path().join("owner.json");
            fs::rename(&path, f.path().join("saved-owner")).unwrap();
            fs::write(&path, "{}").unwrap();
        } else {
            fs::rename(f.path(), f.root.path().join("saved")).unwrap();
            fs::create_dir(f.path()).unwrap();
        }
        assert!(controller
            .ensure_debug(&original().launch.log_directory())
            .is_err());
        assert!(controller.restore().is_err());
        assert!(f.calls().is_empty());
    }
}

#[test]
fn symlink_and_fifo_records_are_rejected_without_reading_or_blocking() {
    let f = Fixture::new();
    let journal = LeaseJournal::open(&f.path()).unwrap();
    let record = f.path().join("00000000000000000000.json");
    symlink("/does-not-exist", &record).unwrap();
    assert!(journal.latest().is_err());
    fs::remove_file(&record).unwrap(); // Own fixture only.
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &record,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        0,
    )
    .unwrap();
    assert!(journal.latest().is_err());
}

#[test]
fn stat_parentheses_and_unsafe_launch_arguments_are_handled() {
    let stat = format!(
        "123 (Telegram (space)) S {} 9007199254740993 0",
        vec!["0"; 18].join(" ")
    );
    assert_eq!(linux::start_ticks(&stat).unwrap(), 9007199254740993);
    assert!(linux::start_ticks("broken").is_err());
    let dir = Path::new("/synthetic/Ж 😀");
    linux::validate_arguments(&original().launch.arguments, dir).unwrap();
    for args in [
        vec!["-many"],
        vec!["-key", "secret"],
        vec!["--", "tg://action"],
        vec!["-workdir", "/other"],
        vec!["-debug", "-debug"],
        vec!["-quit"],
    ] {
        assert!(linux::validate_arguments(
            &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            dir
        )
        .is_err());
    }
}

#[test]
fn profile_is_explicitly_pinned_without_reading_environment() {
    let dir = Path::new("/synthetic/Ж 😀");
    assert_eq!(
        linux::pinned_arguments(&[], dir).unwrap(),
        ["-workdir", "/synthetic/Ж 😀"]
    );
    let existing = original().launch.arguments;
    assert_eq!(linux::pinned_arguments(&existing, dir).unwrap(), existing);
}

#[test]
fn released_profile_can_resume_with_no_process_but_unknown_first_launch_cannot() {
    let f = Fixture::new();
    let mut controller = f.controller();
    controller
        .ensure_debug(&original().launch.log_directory())
        .unwrap();
    controller.restore().unwrap();
    f.fake.0.lock().unwrap().running = None;
    controller
        .ensure_debug(&original().launch.log_directory())
        .unwrap();
    assert_eq!(
        f.calls(),
        [
            "quit",
            "start-debug",
            "quit",
            "start-original",
            "start-debug"
        ]
    );
    controller.restore().unwrap();
    let first = Fixture::new();
    first.fake.0.lock().unwrap().running = None;
    assert!(first
        .controller()
        .ensure_debug(&original().launch.log_directory())
        .is_err());
    assert!(first.calls().is_empty());
}

#[test]
fn ipc_readiness_requires_owned_listening_stock_endpoint() {
    let owned = std::collections::BTreeSet::from([123]);
    let line = "0000000000000000: 00000002 00000000 00010000 0001 01 123 @0123456789abcdef0123456789abcdef-TelegramDesktop";
    assert!(linux::owned_ipc_endpoint(line, &owned).is_some());
    for changed in [
        line.replace(" 123 ", " 999 "),
        line.replace("00010000", "00000000"),
        line.replace("0001 01", "0002 01"),
        line.replace("-TelegramDesktop", "-Other"),
        line.replace("abcdef-TelegramDesktop", "abcdeg-TelegramDesktop"),
    ] {
        assert!(linux::owned_ipc_endpoint(&changed, &owned).is_none());
    }
}

#[test]
fn uncertain_publication_stops_same_controller_until_durable_reopen() {
    let f = Fixture::new();
    let mut controller = f.controller();
    controller.journal.fail_after_link.set(true);
    assert!(controller
        .ensure_debug(&original().launch.log_directory())
        .is_err());
    assert!(f.calls().is_empty());
    assert!(controller
        .ensure_debug(&original().launch.log_directory())
        .is_err());
    assert!(controller.restore().is_err());
    assert!(f.calls().is_empty());
    drop(controller);
    let mut recovered = f.controller();
    recovered
        .ensure_debug(&original().launch.log_directory())
        .unwrap();
    assert_eq!(f.calls(), ["quit", "start-debug"]);
    recovered.restore().unwrap();
}

#[test]
fn late_foreign_directory_never_receives_a_control_marker() {
    let f = Fixture::new();
    let parent =
        cap_std::fs::Dir::open_ambient_dir(f.root.path(), cap_std::ambient_authority()).unwrap();
    assert!(journal::create_branded(&parent, Path::new("client"), || {
        fs::create_dir(f.path())?;
        fs::set_permissions(f.path(), fs::Permissions::from_mode(0o700))?;
        fs::write(f.path().join("user.txt"), "foreign contents")?;
        Ok(())
    })
    .is_err());
    assert!(!f.path().join("owner.json").exists());
    assert_eq!(
        fs::read_to_string(f.path().join("user.txt")).unwrap(),
        "foreign contents"
    );
    assert!(LeaseJournal::open(&f.path()).is_err());
    let stage = fs::read_dir(f.root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".tgsum-client-")
        })
        .unwrap();
    assert!(stage.join("owner.json").is_file());
}

#[test]
fn failed_stage_publication_preserves_unknown_contents_and_marker() {
    let f = Fixture::new();
    let parent =
        cap_std::fs::Dir::open_ambient_dir(f.root.path(), cap_std::ambient_authority()).unwrap();
    let mut stage = None;
    assert!(journal::create_branded(&parent, Path::new("client"), || {
        let path = fs::read_dir(f.root.path())?.next().unwrap()?.path();
        fs::write(path.join("user.txt"), "unexpected contents")?;
        stage = Some(path);
        fs::create_dir(f.path())?;
        Ok(())
    })
    .is_err());
    let stage = stage.unwrap();
    assert_eq!(
        fs::read_to_string(stage.join("user.txt")).unwrap(),
        "unexpected contents"
    );
    assert!(stage.join("owner.json").is_file());
    assert!(!f.path().join("owner.json").exists());
}

#[test]
fn failed_stage_publication_never_deletes_a_replacement_marker() {
    let f = Fixture::new();
    let parent =
        cap_std::fs::Dir::open_ambient_dir(f.root.path(), cap_std::ambient_authority()).unwrap();
    let mut stage = None;
    assert!(journal::create_branded(&parent, Path::new("client"), || {
        let path = fs::read_dir(f.root.path())?.next().unwrap()?.path();
        let replacement = path.join("replacement.json");
        fs::write(&replacement, "unexpected replacement marker")?;
        fs::rename(replacement, path.join("owner.json"))?;
        stage = Some(path);
        fs::create_dir(f.path())?;
        Ok(())
    })
    .is_err());
    assert_eq!(
        fs::read_to_string(stage.unwrap().join("owner.json")).unwrap(),
        "unexpected replacement marker"
    );
    assert!(!f.path().join("owner.json").exists());
}

#[test]
fn ipc_wrong_peer_or_identity_receives_zero_bytes() {
    use std::io::Read as _;
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixListener};
    for failure in 0..3 {
        let f = Fixture::new();
        let endpoint = format!(
            "tgsum-peer-{}-{failure}",
            f.root.path().file_name().unwrap().to_string_lossy()
        );
        let address = SocketAddr::from_abstract_name(endpoint.as_bytes()).unwrap();
        let listener = UnixListener::bind_addr(&address).unwrap();
        let pid = std::process::id();
        let uid = rustix::process::geteuid().as_raw();
        assert!(linux::send_owned_quit(
            &endpoint,
            pid + u32::from(failure == 0),
            uid + u32::from(failure == 1),
            || {
                if failure == 2 {
                    return Err(error("synthetic changed identity"));
                }
                Ok(())
            }
        )
        .is_err());
        let mut stream = listener.accept().unwrap().0;
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        assert!(bytes.is_empty());
    }
}

#[test]
fn held_ipc_connection_never_redirects_to_replacement_endpoint() {
    use std::io::Read as _;
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixListener};
    for close_old in [false, true] {
        let f = Fixture::new();
        let endpoint = format!(
            "tgsum-held-{}",
            f.root.path().file_name().unwrap().to_string_lossy()
        );
        let address = SocketAddr::from_abstract_name(endpoint.as_bytes()).unwrap();
        let listener = UnixListener::bind_addr(&address).unwrap();
        let mut old = None;
        let mut replacement = None;
        let result = linux::send_owned_quit(
            &endpoint,
            std::process::id(),
            rustix::process::geteuid().as_raw(),
            || {
                let accepted = listener.accept()?.0;
                drop(listener);
                replacement = Some(UnixListener::bind_addr(&address)?);
                if close_old {
                    accepted.shutdown(std::net::Shutdown::Both)?;
                } else {
                    old = Some(accepted);
                }
                Ok(())
            },
        );
        if close_old {
            assert!(result.is_err());
        } else {
            result.unwrap();
            let mut bytes = [0u8; 9];
            old.as_mut()
                .unwrap()
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            old.as_mut().unwrap().read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"CMD:quit;");
        }
        let replacement = replacement.unwrap();
        replacement.set_nonblocking(true).unwrap();
        assert_eq!(
            replacement.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }
}

#[test]
fn desktop_launch_uses_its_own_scope_and_literal_profile_arguments() {
    let mut launch = original().launch;
    launch.working_directory = "/synthetic/Ж 😀/${TGSUM_SCOPE_LITERAL}".into();
    launch.arguments[2] = launch.working_directory.to_str().unwrap().into();
    let command = linux::scope_command(&launch, "tgsum-synthetic.scope");
    assert_eq!(command.get_program(), "/usr/bin/systemd-run");
    let args: Vec<_> = command
        .get_args()
        .map(|arg| arg.to_str().unwrap())
        .collect();
    assert_eq!(
        args,
        [
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            "--no-ask-password",
            "--expand-environment=no",
            "--unit=tgsum-synthetic.scope",
            "--",
            "/usr/bin/Telegram",
            "-startintray",
            "-workdir",
            "/synthetic/Ж 😀/${TGSUM_SCOPE_LITERAL}"
        ]
    );
    assert_eq!(
        command.get_current_dir(),
        Some(launch.working_directory.as_path())
    );
}
