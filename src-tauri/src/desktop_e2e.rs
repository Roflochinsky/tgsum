//! Opt-in real-renderer harness. This module is absent from release builds.
//! Only native picker results are simulated; the application IPC stays real.
use std::{collections::VecDeque, fs, io, path::PathBuf, sync::Mutex};

use serde::Deserialize;
use tauri::{AppHandle, Builder, Manager, Runtime};
use tgsum_core::project::ProjectStore;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    nonce: String,
    port: u16,
    picks: VecDeque<Pick>,
    #[serde(default)]
    refresh_fixture: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pick {
    kind: String,
    path: Option<PathBuf>,
}

pub(crate) struct Harness {
    pub root: PathBuf,
    pub nonce: String,
    picks: Mutex<VecDeque<Pick>>,
}

#[cfg(target_os = "linux")]
pub(crate) fn requested() -> bool {
    std::env::var_os("TGSUM_DESKTOP_E2E_ROOT").is_some()
}

pub(crate) fn configure<R: Runtime>(builder: Builder<R>) -> io::Result<Builder<R>> {
    let Some(root) = std::env::var_os("TGSUM_DESKTOP_E2E_ROOT") else {
        return Ok(builder);
    };
    let root = fs::canonicalize(root)?;
    let temp = fs::canonicalize(std::env::temp_dir())?;
    if !root.starts_with(temp)
        || !root
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("tgsum-desktop-e2e-"))
        || std::env::var("TGSUM_ANALYSIS_FIXTURE").as_deref() != Ok("1")
    {
        return Err(io::Error::other(
            "desktop E2E requires an owned temporary root and synthetic agents",
        ));
    }
    let config: Config = serde_json::from_slice(&fs::read(root.join("harness.json"))?)?;
    if config.nonce.len() < 16 || config.port < 1024 {
        return Err(io::Error::other("invalid desktop E2E nonce/port"));
    }
    // Predeclared responses can only name files/directories inside this run.
    let mut picks = config.picks;
    for pick in &mut picks {
        if !matches!(pick.kind.as_str(), "export" | "output") {
            return Err(io::Error::other("unknown desktop E2E picker"));
        }
        if let Some(path) = &pick.path {
            let path = fs::canonicalize(root.join(path))?;
            if !path.starts_with(&root) {
                return Err(io::Error::other("desktop E2E picker outside owned root"));
            }
            pick.path = Some(path);
        }
    }
    // A second launch cannot silently reuse an existing Project store.
    let projects = root.join("projects");
    fs::create_dir(&projects)?;
    let store = ProjectStore::new(projects);
    let builder = if config.refresh_fixture {
        builder.manage(crate::telegram_refresh::fixture::state(
            root.clone(),
            store.clone(),
        )?)
    } else {
        builder
    };
    Ok(builder
        .manage(store)
        .manage(Harness {
            root,
            nonce: config.nonce,
            picks: Mutex::new(picks),
        })
        .plugin(tauri_plugin_wdio_webdriver::init_with_port(config.port)))
}

pub(crate) fn pick<R: Runtime>(app: &AppHandle<R>, kind: &str) -> Option<Option<String>> {
    let harness = app.try_state::<Harness>()?;
    let pick = harness
        .picks
        .lock()
        .unwrap()
        .pop_front()
        .expect("unexpected E2E picker invocation");
    assert_eq!(pick.kind, kind, "E2E picker order changed");
    Some(pick.path.map(|path| path.to_string_lossy().into_owned()))
}
