//! Synthetic driver enabled only inside the opt-in desktop E2E harness.
use super::TelegramRefreshState;
use serde::Deserialize;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tgsum_core::bridge_refresh::{
    DriverEvent, DriverRequest, DriverStatus, RefreshDriver, RefreshObservation,
};
use tgsum_core::project::ProjectStore;

#[derive(Deserialize)]
struct Control {
    mode: String,
    now: u64,
}
fn control(root: &std::path::Path) -> io::Result<Control> {
    Ok(serde_json::from_slice(&fs::read(
        root.join("refresh-control.json"),
    )?)?)
}
struct Driver {
    root: PathBuf,
    path: Option<PathBuf>,
    exporting: bool,
    cancelled: bool,
}
impl RefreshDriver for Driver {
    fn available(&self) -> bool {
        true
    }
    fn start(&mut self, request: &DriverRequest) -> io::Result<()> {
        let directory = request.settings.directory.canonicalize()?;
        if !directory.starts_with(&self.root) {
            return Err(io::Error::other("synthetic export outside harness"));
        }
        let output = directory.join(&request.run_id);
        fs::create_dir(&output)?;
        let file = output.join("result.json");
        let id: u64 = request
            .source
            .conversation_id
            .parse()
            .map_err(|_| io::Error::other("synthetic chat ID"))?;
        fs::write(
            &file,
            serde_json::to_vec(&serde_json::json!({
                "id":id,"name":"Synthetic refresh","type":"private_group",
                "messages":[{"id":77,"type":"message","text":"Synthetic refreshed context"}]
            }))?,
        )?;
        writeln!(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.root.join("refresh-starts.txt"))?,
            "{}",
            request.run_id
        )?;
        self.path = Some(file);
        self.exporting = false;
        self.cancelled = false;
        Ok(())
    }
    fn poll(&mut self, request: &DriverRequest) -> io::Result<Option<DriverEvent>> {
        let status = if self.cancelled {
            DriverStatus::Stopped {
                cancelled: true,
                retry_after: 900,
            }
        } else if !self.exporting {
            self.exporting = true;
            DriverStatus::Exporting
        } else {
            match control(&self.root)?.mode.as_str() {
                "hold" => return Ok(None),
                "failure" => DriverStatus::Stopped {
                    cancelled: false,
                    retry_after: 900,
                },
                "success" => DriverStatus::Completed {
                    archive_path: self.path.clone().unwrap(),
                },
                _ => DriverStatus::NeedsUserAction,
            }
        };
        Ok(Some(DriverEvent {
            request: request.clone(),
            status,
        }))
    }
    fn cancel(&mut self, _: &DriverRequest) -> io::Result<()> {
        self.cancelled = true;
        Ok(())
    }
}
pub(crate) fn state(root: PathBuf, store: ProjectStore) -> io::Result<TelegramRefreshState> {
    let clock_root = root.clone();
    TelegramRefreshState::with_driver(
        store,
        Box::new(Driver {
            root,
            path: None,
            exporting: false,
            cancelled: false,
        }),
        Arc::new(move || {
            let control = control(&clock_root).ok();
            RefreshObservation {
                unix_seconds: control.as_ref().map_or(100, |c| c.now),
                active_unlocked: control.is_some_and(|c| c.mode != "locked"),
                at: Instant::now(),
            }
        }),
        "desktop-fixture-launch".into(),
    )
}
