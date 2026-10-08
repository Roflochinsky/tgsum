//! Controlled synthetic Desktop. No process, socket, account or native adapter.
use crate::telegram_client::{Desktop, Identity, Launch, Running};
use std::{
    fs, io,
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};

pub(crate) struct State {
    pub running: Option<Running>,
    pub calls: Vec<String>,
    pub fail_restore: bool,
    pub persistent: bool,
    next: u32,
    record: std::path::PathBuf,
}

pub(crate) struct Fake(pub Arc<Mutex<State>>);

impl Fake {
    pub fn new(profile: &Path) -> io::Result<Self> {
        let profile = profile.canonicalize()?;
        Ok(Self(Arc::new(Mutex::new(State {
            running: Some(Running {
                identity: identity(100_000),
                launch: Launch {
                    executable: "/usr/bin/Telegram".into(),
                    arguments: vec!["-workdir".into(), profile.to_str().unwrap().into()],
                    working_directory: profile.clone(),
                },
            }),
            calls: Vec::new(),
            fail_restore: false,
            persistent: false,
            next: 100_001,
            record: profile.join("synthetic-client-calls.jsonl"),
        }))))
    }
}

fn identity(pid: u32) -> Identity {
    Identity {
        pid,
        start_ticks: u64::from(pid),
        boot_id: "synthetic-runtime".into(),
        executable_device: 5,
        executable_inode: 7,
    }
}

fn call(state: &mut State, action: &str) -> io::Result<()> {
    state.calls.push(action.into());
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.record)?;
    writeln!(file, "{}", serde_json::to_string(action)?)
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
        if state.running.as_ref() != Some(running) {
            return Err(io::Error::other("Synthetic process changed"));
        }
        call(&mut state, "quit")?;
        state.running = None;
        Ok(())
    }
    fn start(&mut self, launch: &Launch) -> io::Result<Running> {
        let mut state = self.0.lock().unwrap();
        if state.running.is_some() {
            return Err(io::Error::other("Synthetic client is already running"));
        }
        let debug = launch.arguments.iter().any(|arg| arg == "-debug");
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Control {
            fail_restore: bool,
        }
        let control = state.record.with_file_name("synthetic-client-control.json");
        match fs::read(control) {
            Ok(bytes) => {
                state.fail_restore = serde_json::from_slice::<Control>(&bytes)?.fail_restore
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if !debug && state.fail_restore {
            return Err(io::Error::other(
                "Synthetic original-mode restoration failed",
            ));
        }
        call(
            &mut state,
            if debug {
                "start-debug"
            } else {
                "start-original"
            },
        )?;
        if debug {
            fs::create_dir_all(launch.log_directory())?;
        }
        let running = Running {
            identity: identity(state.next),
            launch: launch.clone(),
        };
        state.next += 1;
        state.running = Some(running.clone());
        Ok(running)
    }
}
