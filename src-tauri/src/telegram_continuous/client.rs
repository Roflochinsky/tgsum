//! One client controller per application, shared by saved selected sources.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Default, Serialize)]
pub(crate) struct View {
    pub phase: Phase,
    pub log_directory: Option<PathBuf>,
    pub enabled_sources: usize,
    pub can_restore: bool,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    #[default]
    Unmanaged,
    #[cfg(target_os = "linux")]
    ManagedDebug,
    #[cfg(target_os = "linux")]
    UserDebug,
    #[cfg(target_os = "linux")]
    Failed,
}

#[cfg(target_os = "linux")]
mod native {
    use super::*;
    use crate::telegram_client::{Controller, Desktop, LeaseJournal, Mode, StockDesktop};
    use std::{io, path::Path};

    pub(super) type Backend = Box<dyn Desktop + Send>;

    pub(crate) struct Client {
        desktop: Option<Backend>,
        controller: Option<Controller<Backend>>,
        path: Option<PathBuf>,
        blocked: bool,
        pub view: View,
    }

    impl Default for Client {
        fn default() -> Self {
            Self::new(Box::new(StockDesktop::default()))
        }
    }

    impl Client {
        pub fn new(desktop: Backend) -> Self {
            Self {
                desktop: Some(desktop),
                controller: None,
                path: None,
                blocked: false,
                view: View::default(),
            }
        }

        /// Detection does not create control receipts or switch runtime mode.
        pub fn detect(&mut self) -> io::Result<Option<PathBuf>> {
            let running = match &mut self.controller {
                Some(controller) => controller.detect()?,
                None => self.desktop.as_mut().unwrap().current()?,
            };
            Ok(running.map(|running| running.launch.log_directory()))
        }

        pub fn preflight_binding(&mut self, requested: Option<&Path>) -> io::Result<PathBuf> {
            let logs = self.detect()?.ok_or_else(|| {
                io::Error::other(
                    "Откройте существующий Telegram Desktop перед первым запуском сбора.",
                )
            })?;
            if requested.is_some_and(|path| path != logs) {
                return Err(io::Error::other(
                    "Папка логов не совпадает с работающим Telegram. Найдите Telegram ещё раз.",
                ));
            }
            let profile = logs
                .parent()
                .ok_or_else(|| io::Error::other("Не найдена папка Telegram."))?;
            tgsum_core::telegram_debug::settings::validate_log_directory(profile)?;
            match std::fs::symlink_metadata(&logs) {
                Ok(_) => tgsum_core::telegram_debug::settings::validate_log_directory(&logs)?,
                // Only the verified running profile may create missing logs.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            Ok(logs)
        }

        fn initialize(&mut self, path: &Path) -> io::Result<()> {
            if let Some(original) = &self.path {
                if original != path {
                    return Err(io::Error::other(
                        "Папка управления Telegram изменилась. Перезапустите TGSUM.",
                    ));
                }
            }
            if self.controller.is_none() {
                let journal = LeaseJournal::open(path)?;
                self.controller = Some(Controller::new(self.desktop.take().unwrap(), journal));
                self.path = Some(path.into());
            }
            Ok(())
        }

        pub fn retry(&mut self) {
            self.blocked = false;
            if let Some(controller) = self.controller.take() {
                self.desktop = Some(controller.into_desktop());
            }
        }

        pub fn fail(&mut self, error: &io::Error, enabled_sources: usize) {
            if !self.blocked {
                if let Some(message) = crate::telegram_client::diagnostic(error) {
                    eprintln!("tgsum: Telegram client control failed: {message}");
                } else {
                    eprintln!("tgsum: Telegram client control failed ({:?})", error.kind());
                }
            }
            self.blocked = true;
            self.view.phase = Phase::Failed;
            self.view.enabled_sources = enabled_sources;
            self.view.can_restore = enabled_sources == 0;
            self.view.message = Some(error.to_string());
        }

        pub fn ensure(
            &mut self,
            path: &Path,
            logs: &Path,
            enabled_sources: usize,
        ) -> io::Result<()> {
            self.view.enabled_sources = enabled_sources;
            self.view.can_restore = false;
            let result = (|| {
                if self.blocked {
                    return Err(io::Error::other(self.view.message.clone().unwrap_or_else(
                        || "Повторите запуск сбора после устранения ошибки Telegram.".into(),
                    )));
                }
                self.initialize(path)?;
                let mode = self.controller.as_mut().unwrap().ensure_debug(logs)?;
                self.view.phase = match mode {
                    Mode::ManagedDebug => Phase::ManagedDebug,
                    Mode::UserDebug => Phase::UserDebug,
                };
                self.view.log_directory = Some(logs.into());
                self.view.message = None;
                Ok(())
            })();
            if let Err(error) = &result {
                self.fail(error, enabled_sources);
            }
            result
        }

        pub fn restore(&mut self, path: &Path) -> io::Result<()> {
            self.view.enabled_sources = 0;
            let result = (|| {
                if self.blocked {
                    return Err(io::Error::other(self.view.message.clone().unwrap_or_else(
                        || "Повторите восстановление режима Telegram.".into(),
                    )));
                }
                // Passive/empty installations never touch the actual client.
                if self.controller.is_none() && !path.try_exists()? {
                    return Ok(());
                }
                self.initialize(path)?;
                self.controller.as_mut().unwrap().restore()
            })();
            if let Err(error) = &result {
                self.fail(error, 0);
            } else {
                self.view.phase = Phase::Unmanaged;
                self.view.can_restore = false;
                self.view.message = None;
            }
            result
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) use native::Client;

#[cfg(all(
    target_os = "linux",
    any(test, all(debug_assertions, feature = "desktop-e2e"))
))]
pub(super) mod fixture;
