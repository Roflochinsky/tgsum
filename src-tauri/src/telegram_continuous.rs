//! Saved selected-chat plans -> one local journal writer -> Project snapshots.
//! Starting this worker does not authenticate, export, or send Telegram data.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_dialog::DialogExt;
#[cfg(target_os = "linux")]
use tgsum_core::project::ProjectEntry;
use tgsum_core::project::{Project, ProjectChange, ProjectStore};
use tgsum_core::telegram_debug::capture::GapCount;
#[cfg(target_os = "linux")]
use tgsum_core::telegram_debug::continuous::ContinuousCapture;
use tgsum_core::telegram_debug::parser::{PeerKind, TypedPeer};
use tgsum_core::telegram_debug::settings::ContinuousSettings;

use crate::{analysis, run_blocking, CmdError};

type SourceKey = (String, String);

#[derive(Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    #[default]
    Stopped,
    Starting,
    #[cfg(target_os = "linux")]
    Watching,
    #[cfg(target_os = "linux")]
    WaitingForOperation,
    #[cfg(target_os = "linux")]
    Failed,
    Unavailable,
}

#[derive(Clone, Default, Serialize)]
struct Observation {
    phase: Phase,
    events: u64,
    applied_events: u64,
    counts_known: bool,
    #[serde(skip)]
    generation: Option<String>,
    gaps: Vec<GapCount>,
    last_poll: Option<u64>,
    last_observation: Option<u64>,
    message: Option<String>,
}

#[derive(Default)]
struct Host {
    #[cfg(target_os = "linux")]
    captures: BTreeMap<SourceKey, ContinuousCapture>,
    observations: BTreeMap<SourceKey, Observation>,
}

/// The mutex serializes the worker and Stop: a successful Stop has persisted
/// the disabled plan and dropped its writer before returning to the UI.
type Worker = Arc<Mutex<Option<std::thread::JoinHandle<()>>>>;

#[derive(Clone, Default)]
pub(crate) struct TelegramContinuousState(Arc<Mutex<Host>>, Worker);

#[derive(Serialize)]
pub(crate) struct ContinuousView {
    supported: bool,
    project: Project,
    observation: Observation,
    input_directory: Option<PathBuf>,
    journal_path: Option<PathBuf>,
}

impl TelegramContinuousState {
    fn status(
        &self,
        store: &ProjectStore,
        project_id: &str,
        source_id: &str,
    ) -> io::Result<ContinuousView> {
        let host = self.0.lock().map_err(poisoned)?;
        let project = store.open(project_id)?;
        source(&project, source_id)?;
        let plan = project.telegram_continuous.get(source_id);
        let enabled = plan.is_some_and(|p| p.settings.enabled);
        let mut observation = host
            .observations
            .get(&(project_id.into(), source_id.into()))
            .cloned()
            .unwrap_or_default();
        if !cfg!(target_os = "linux") {
            observation.phase = Phase::Unavailable;
        } else if !enabled {
            observation.phase = Phase::Stopped;
        } else if observation.phase == Phase::Stopped {
            observation.phase = Phase::Starting;
        }
        // Retained counters remain meaningful across application restarts.
        if let Some(checkpoint) = plan.and_then(|p| p.checkpoint.as_ref()) {
            observation.applied_events = observation.applied_events.max(checkpoint.sequence);
            observation.events = observation.events.max(checkpoint.sequence);
        }
        #[cfg(target_os = "linux")]
        let journal_path = plan
            .map(|_| store.telegram_journal_path(project_id, source_id))
            .transpose()?;
        #[cfg(not(target_os = "linux"))]
        let journal_path = None;
        let input_directory = plan.map(|p| p.settings.input_directory.clone());
        if plan.is_none() {
            observation.counts_known = true;
        }
        Ok(ContinuousView {
            supported: cfg!(target_os = "linux"),
            project,
            observation,
            input_directory,
            journal_path,
        })
    }

    fn set_enabled(
        &self,
        store: &ProjectStore,
        project_id: &str,
        source_id: &str,
        expected_revision: u64,
        options: StartOptions,
    ) -> io::Result<Project> {
        let mut host = self.0.lock().map_err(poisoned)?;
        let project = store.open(project_id)?;
        if project.revision != expected_revision {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Проект обновился. Перезагрузите его и повторите действие.",
            ));
        }
        source(&project, source_id)?;
        if options.enabled && !cfg!(target_os = "linux") {
            return Err(io::Error::other(
                "Сбор диагностических логов пока доступен только на Linux.",
            ));
        }
        let settings = settings(store, &project, source_id, options)?;
        #[cfg(target_os = "linux")]
        if settings.enabled
            && project
                .telegram_continuous
                .get(source_id)
                .is_some_and(|plan| plan.settings.enabled)
            && host
                .captures
                .contains_key(&(project_id.into(), source_id.into()))
        {
            return Ok(project);
        }
        let updated = store.update(
            project_id,
            expected_revision,
            ProjectChange::TelegramContinuous {
                source_id: source_id.into(),
                settings,
            },
        )?;
        let key = (project_id.into(), source_id.into());
        #[cfg(target_os = "linux")]
        host.captures.remove(&key);
        let observation = host.observations.entry(key).or_default();
        observation.phase = if updated.telegram_continuous[source_id].settings.enabled {
            Phase::Starting
        } else {
            Phase::Stopped
        };
        observation.message = None;
        Ok(updated)
    }

    /// Called after Disconnect has durably removed its Project source.
    pub(crate) fn disconnected(&self, project_id: &str, source_id: &str) -> io::Result<()> {
        let mut host = self.0.lock().map_err(poisoned)?;
        let key = (project_id.into(), source_id.into());
        #[cfg(target_os = "linux")]
        host.captures.remove(&key);
        host.observations.remove(&key);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn tick(
        &self,
        store: &ProjectStore,
        apply: impl Fn(&str, &str, &mut ContinuousCapture) -> io::Result<bool>,
    ) -> io::Result<()> {
        let mut host = self.0.lock().map_err(poisoned)?;
        let mut enabled = BTreeMap::new();
        for entry in store.list()? {
            if let ProjectEntry::Ready { project } = entry {
                for (source_id, plan) in &project.telegram_continuous {
                    if plan.settings.enabled {
                        enabled.insert(
                            (project.project_id.clone(), source_id.clone()),
                            plan.settings.generation.clone(),
                        );
                    }
                }
            }
        }
        host.captures.retain(|key, _| enabled.contains_key(key));
        for (key, observation) in &mut host.observations {
            if !enabled.contains_key(key) {
                observation.phase = Phase::Stopped;
            }
        }
        for (key, generation) in &enabled {
            if host
                .observations
                .get(key)
                .and_then(|state| state.generation.as_ref())
                != Some(generation)
            {
                host.captures.remove(key);
                host.observations.insert(
                    key.clone(),
                    Observation {
                        generation: Some(generation.clone()),
                        phase: Phase::Starting,
                        ..Default::default()
                    },
                );
            }
            if host
                .observations
                .get(key)
                .is_some_and(|state| state.phase == Phase::Failed)
            {
                // A failed session cannot guess the outcome of an I/O commit.
                // Explicit Start or a process restart opens and recovers it.
                continue;
            }
            let mut capture = match host.captures.remove(key) {
                Some(capture) => capture,
                None => match store
                    .telegram_journal_path(&key.0, &key.1)
                    .and_then(|path| path.try_exists())
                    .and_then(|existing| {
                        let mut capture = store.open_telegram_capture(&key.0, &key.1)?;
                        if existing {
                            capture
                                .record_collector_restart()
                                .map_err(io::Error::other)?;
                        }
                        Ok(capture)
                    }) {
                    Ok(capture) => capture,
                    Err(_) => {
                        failed(&mut host, key, "Не удалось открыть журнал. Проверьте папку логов, доступ к диску и отсутствие второго сборщика; затем повторите запуск.");
                        continue;
                    }
                },
            };
            let poll = match capture.poll() {
                Ok(poll) => poll,
                Err(error) => {
                    let durable = if error
                        == tgsum_core::telegram_debug::capture::CaptureError::SourceChanged
                    {
                        capture.status().ok()
                    } else {
                        None
                    };
                    failed(&mut host, key, "Чтение логов остановлено. Данные сохранены до последней подтверждённой записи. Проверьте доступ к папке и свободное место; затем повторите запуск.");
                    if let Some(status) = durable {
                        let state = host.observations.entry(key.clone()).or_default();
                        state.events = status.events;
                        state.applied_events = status.applied.as_ref().map_or(0, |c| c.sequence);
                        state.gaps = status.gaps;
                        state.counts_known = true;
                    }
                    continue;
                }
            };
            let state = host.observations.entry(key.clone()).or_default();
            state.last_poll = Some(now());
            if poll.added_events > 0 {
                state.last_observation = state.last_poll;
            }
            let pending = match capture.status() {
                Ok(status) => status,
                Err(_) => {
                    failed(&mut host, key, "Состояние журнала недоступно. Повторите запуск для восстановления сохранённых записей.");
                    continue;
                }
            };
            let state = host.observations.entry(key.clone()).or_default();
            state.events = pending.events;
            state.counts_known = true;
            state.applied_events = pending.applied.as_ref().map_or(0, |c| c.sequence);
            state.gaps = pending.gaps.clone();
            let sequence = pending.applied.as_ref().map_or(0, |c| c.sequence);
            let revision = pending
                .applied
                .as_ref()
                .map_or(0, |c| c.observation_revision);
            let needs_apply =
                sequence != pending.events || revision != pending.observation_revision;
            state.phase = Phase::Watching;
            state.message = None;
            if needs_apply {
                match apply(&key.0, &key.1, &mut capture) {
                    Ok(true) => {}
                    Ok(false) => state.phase = Phase::WaitingForOperation,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        state.phase = Phase::WaitingForOperation
                    }
                    Err(_) => {
                        failed(&mut host, key, "Журнал сохранён, но обновление проекта остановлено. Проверьте доступ к диску и повторите запуск. Предыдущая история проекта сохранена.");
                        continue;
                    }
                }
            }
            let status = match capture.status() {
                Ok(status) => status,
                Err(_) => {
                    failed(&mut host, key, "Состояние журнала недоступно. Повторите запуск для восстановления сохранённых записей.");
                    continue;
                }
            };
            let state = host.observations.entry(key.clone()).or_default();
            state.events = status.events;
            state.applied_events = status.applied.map_or(0, |checkpoint| checkpoint.sequence);
            state.gaps = status.gaps;
            host.captures.insert(key.clone(), capture);
        }
        Ok(())
    }

    fn shutdown(&self) {
        if let Ok(mut host) = self.0.lock() {
            #[cfg(target_os = "linux")]
            host.captures.clear();
            for state in host.observations.values_mut() {
                state.phase = Phase::Stopped;
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn failed(host: &mut Host, key: &SourceKey, message: &str) {
    let state = host.observations.entry(key.clone()).or_default();
    state.phase = Phase::Failed;
    state.counts_known = false;
    state.message = Some(message.into());
}

struct StartOptions {
    enabled: bool,
    input_directory: Option<PathBuf>,
    confirmed_single_account: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContinuousRequest {
    project_id: String,
    source_id: String,
    expected_revision: u64,
    enabled: bool,
    input_directory: Option<PathBuf>,
    confirmed_single_account: bool,
}

fn settings(
    store: &ProjectStore,
    project: &Project,
    source_id: &str,
    options: StartOptions,
) -> io::Result<ContinuousSettings> {
    if let Some(plan) = project.telegram_continuous.get(source_id) {
        if options
            .input_directory
            .as_ref()
            .is_some_and(|directory| *directory != plan.settings.input_directory)
        {
            return Err(io::Error::other(
                "Этот журнал уже связан с другой папкой логов. Его привязку нельзя менять.",
            ));
        }
        let mut settings = plan.settings.clone();
        settings.enabled = options.enabled;
        return Ok(settings);
    }
    if !options.enabled || !options.confirmed_single_account {
        return Err(io::Error::other(
            "Для первого запуска подтвердите один аккаунт в Telegram Desktop.",
        ));
    }
    let source = source(project, source_id)?;
    let bootstrap_snapshot_id = source
        .latest_snapshot_id
        .clone()
        .ok_or_else(|| io::Error::other("Сначала подключите начальную историю этого чата."))?;
    let snapshot = store
        .snapshots(&project.project_id)?
        .load(&bootstrap_snapshot_id)?;
    let kind = match snapshot.conversation_kind.as_str() {
        "private_supergroup" | "public_supergroup" | "private_channel" | "public_channel" => {
            PeerKind::Channel
        }
        "private_group" | "public_group" => PeerKind::Chat,
        "personal_chat" => PeerKind::User,
        _ => return Err(io::Error::other("Тип этого чата пока не поддерживается.")),
    };
    let directory = options
        .input_directory
        .filter(|directory| directory.is_absolute())
        .ok_or_else(|| io::Error::other("Выберите абсолютный путь к папке DebugLogs."))?;
    tgsum_core::telegram_debug::settings::validate_log_directory(&directory).map_err(|_| {
        io::Error::other("Папка логов недоступна или содержит символьную ссылку. Выберите существующую локальную папку DebugLogs.")
    })?;
    Ok(ContinuousSettings {
        enabled: true,
        generation: format!(
            "capture-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ),
        input_directory: directory,
        peer: TypedPeer {
            kind,
            id: source.scope.conversation_id.clone(),
        },
        self_user_id: None,
        confirmed_single_account: true,
        bootstrap_snapshot_id,
    })
}

fn source<'a>(
    project: &'a Project,
    source_id: &str,
) -> io::Result<&'a tgsum_core::project::ProjectSource> {
    project
        .sources
        .iter()
        .find(|source| {
            source.source_id == source_id
                && source.scope.platform == "telegram"
                && source.connector_id == "telegram_json"
        })
        .ok_or_else(|| io::Error::other("Этот источник Telegram не подключён к проекту."))
}

#[cfg(target_os = "linux")]
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other("Состояние сборщика недоступно. Перезапустите TGSUM.")
}

#[tauri::command]
pub(crate) async fn telegram_continuous_status(
    runtime: State<'_, TelegramContinuousState>,
    store: State<'_, ProjectStore>,
    project_id: String,
    source_id: String,
) -> Result<ContinuousView, CmdError> {
    let runtime = runtime.inner().clone();
    let store = store.inner().clone();
    run_blocking(move || Ok(runtime.status(&store, &project_id, &source_id)?)).await
}

#[tauri::command]
pub(crate) async fn set_telegram_continuous(
    runtime: State<'_, TelegramContinuousState>,
    store: State<'_, ProjectStore>,
    analysis: State<'_, analysis::AnalysisState>,
    request: ContinuousRequest,
) -> Result<Project, CmdError> {
    analysis.before_edit(&store, &request.project_id)?;
    let runtime = runtime.inner().clone();
    let store = store.inner().clone();
    run_blocking(move || {
        Ok(runtime.set_enabled(
            &store,
            &request.project_id,
            &request.source_id,
            request.expected_revision,
            StartOptions {
                enabled: request.enabled,
                input_directory: request.input_directory,
                confirmed_single_account: request.confirmed_single_account,
            },
        )?)
    })
    .await
}

#[tauri::command]
pub(crate) async fn pick_telegram_log_directory<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<String>, CmdError> {
    #[cfg(all(debug_assertions, feature = "desktop-e2e"))]
    if app.try_state::<crate::desktop_e2e::Harness>().is_some() {
        return Err(CmdError::Failed(
            "Выбор реальной папки отключён в синтетическом стенде.".into(),
        ));
    }
    run_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("Папка DebugLogs Telegram Desktop")
            .blocking_pick_folder();
        selected
            .map(|path| {
                path.into_path()
                    .map_err(|error| CmdError::Failed(error.to_string()))
                    .and_then(|path| {
                        path.canonicalize()?
                            .into_os_string()
                            .into_string()
                            .map_err(|_| CmdError::Failed("Путь не является UTF-8.".into()))
                    })
            })
            .transpose()
    })
    .await
}

pub(crate) fn start_worker<R: Runtime>(app: AppHandle<R>, stop: Arc<AtomicBool>) {
    let runtime = app.state::<TelegramContinuousState>().inner().clone();
    let mut worker = runtime.1.lock().expect("Telegram worker handle poisoned");
    if worker.is_some() {
        return;
    }
    let state = runtime.clone();
    *worker = Some(std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        let store = app.state::<ProjectStore>().inner().clone();
        while !stop.load(Ordering::Relaxed) {
            #[cfg(target_os = "linux")]
            if state
                .tick(&store, |project_id, source_id, capture| {
                    if let Some(analysis) = app.try_state::<analysis::AnalysisState>() {
                        analysis.background_edit(project_id, || {
                            store
                                .apply_telegram_observations(project_id, source_id, capture)
                                .map(|_| ())
                        })
                    } else {
                        Ok(false)
                    }
                })
                .is_err()
            {
                // Never put a raw log line, source path, or account in stderr.
                eprintln!("tgsum: Telegram journal worker could not complete its check");
            }
            for _ in 0..10 {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        state.shutdown();
    }));
}

pub(crate) fn finish_worker<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<TelegramContinuousState>();
    let worker = state.1.lock().ok().and_then(|mut handle| handle.take());
    if let Some(worker) = worker {
        let _ = worker.join();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
