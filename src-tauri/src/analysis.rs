//! Desktop ownership of one prepared/running analysis. The frontend receives
//! an opaque run ID; Run cannot replace reviewed arguments or credentials.
use super::{run_blocking, CmdError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use tgsum_core::analysis::{AnalysisCoverage, AnalysisSpec, Completion, FailureCode, RunTicket};
use tgsum_core::project::ProjectStore;
use tgsum_core::recipe::{Recipe, RecipeOutput, RECIPE_VERSION};
use tgsum_runner::{Cancellation, PreparedContext, RunnerError};

pub struct AnalysisState {
    shared: Arc<Mutex<Session>>,
    fixtures: bool,
}
#[derive(Default)]
struct Session {
    generation: u64,
    active: Active,
}
#[derive(Default)]
enum Active {
    #[default]
    Idle,
    Working {
        project: String,
        cancel: Cancellation,
    },
    Ready(Box<Prepared>),
}
impl Default for AnalysisState {
    fn default() -> Self {
        Self {
            shared: Arc::new(Mutex::new(Session::default())),
            fixtures: false,
        }
    }
}
impl AnalysisState {
    #[cfg(all(feature = "analysis-fixtures", debug_assertions))]
    pub fn synthetic() -> Self {
        Self {
            fixtures: true,
            ..Self::default()
        }
    }

    pub(super) fn for_app() -> Self {
        #[cfg(all(feature = "analysis-fixtures", debug_assertions))]
        if std::env::var_os("TGSUM_ANALYSIS_FIXTURE").as_deref() == Some(std::ffi::OsStr::new("1"))
        {
            return Self::synthetic();
        }
        Self::default()
    }

    pub(super) fn before_edit(&self, store: &ProjectStore, project: &str) -> Result<(), CmdError> {
        let mut session = self.shared.lock().unwrap();
        match &session.active {
            Active::Working {
                project: active, ..
            } if active == project => return Err(busy()),
            Active::Ready(run) if run.ticket.request().project_id == project => {}
            _ => return Ok(()),
        }
        if let Active::Ready(run) = std::mem::take(&mut session.active) {
            store.cancel_analysis(&run.ticket)?;
        }
        Ok(())
    }

    pub(super) fn cancel(&self, store: &ProjectStore) -> Result<(), CmdError> {
        let mut session = self.shared.lock().unwrap();
        if let Active::Working { cancel, .. } = &session.active {
            cancel.cancel();
            return Ok(());
        }
        if let Active::Ready(run) = std::mem::take(&mut session.active) {
            store.cancel_analysis(&run.ticket)?;
        }
        Ok(())
    }

    fn reserve(&self, store: &ProjectStore, project: &str) -> Result<Lease, CmdError> {
        let mut session = self.shared.lock().unwrap();
        if matches!(session.active, Active::Working { .. }) {
            return Err(busy());
        }
        if let Active::Ready(run) = std::mem::take(&mut session.active) {
            store.cancel_analysis(&run.ticket)?;
        }
        session.generation = session.generation.checked_add(1).ok_or_else(busy)?;
        let cancel = Cancellation::default();
        session.active = Active::Working {
            project: project.into(),
            cancel: cancel.clone(),
        };
        Ok(Lease {
            shared: self.shared.clone(),
            generation: session.generation,
            cancel,
            keep: false,
        })
    }

    fn take_run(&self, id: &str) -> Result<(Box<Prepared>, Lease), CmdError> {
        let mut session = self.shared.lock().unwrap();
        if !matches!(&session.active,Active::Ready(run) if run.ticket.request().run_id==id) {
            return Err(CmdError::Conflict(
                "Подготовьте и проверьте этот запуск заново.".into(),
            ));
        }
        let Active::Ready(run) = std::mem::take(&mut session.active) else {
            unreachable!()
        };
        let cancel = Cancellation::default();
        session.active = Active::Working {
            project: run.ticket.request().project_id.clone(),
            cancel: cancel.clone(),
        };
        Ok((
            run,
            Lease {
                shared: self.shared.clone(),
                generation: session.generation,
                cancel,
                keep: false,
            },
        ))
    }
}

struct Lease {
    shared: Arc<Mutex<Session>>,
    generation: u64,
    cancel: Cancellation,
    keep: bool,
}
impl Lease {
    fn ready(mut self, store: &ProjectStore, run: Prepared) -> Result<(), CmdError> {
        let mut session = self.shared.lock().unwrap();
        if self.cancel.is_cancelled() {
            drop(session);
            store.cancel_analysis(&run.ticket)?;
            return Err(CmdError::Cancelled);
        }
        session.active = Active::Ready(Box::new(run));
        self.keep = true;
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if !self.keep {
            let mut session = self.shared.lock().unwrap();
            if session.generation == self.generation {
                session.active = Active::Idle;
            }
        }
    }
}
fn busy() -> CmdError {
    CmdError::Conflict("Дождитесь завершения анализа или отмените его.".into())
}
fn runner_error(error: RunnerError) -> CmdError {
    match error {
        RunnerError::Cancelled => CmdError::Cancelled,
        RunnerError::ExportOnly(reason)|RunnerError::InvalidRequest(reason) => CmdError::Failed(format!("Запуск недоступен: {reason}. Можно сохранить контекст в файл.")),
        RunnerError::Io(_) => CmdError::Failed("Не удалось подготовить файлы запуска. Проверьте выбранный агент и доступ к файлу авторизации.".into()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalysisOptions {
    agent: Agent,
    executable: String,
    auth_file: String,
    model: String,
    recipe: Recipe,
    destination: String,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Agent {
    Codex,
    Claude,
}
impl Agent {
    fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
        }
    }
    fn version(self) -> &'static str {
        match self {
            Self::Codex => "0.155.1",
            Self::Claude => tgsum_runner::claude::VERSION,
        }
    }
    fn profile(self) -> &'static str {
        match self {
            Self::Codex => "linux-x86_64-bwrap-codex-egress-v1",
            Self::Claude => "linux-x86_64-bwrap-claude-egress-v1",
        }
    }
    fn relay(self) -> &'static str {
        match self {
            Self::Codex => "tgsum-codex-relay",
            Self::Claude => "tgsum-claude-relay",
        }
    }
    fn auth_name(self) -> &'static str {
        match self {
            Self::Codex => "auth.json",
            Self::Claude => ".credentials.json",
        }
    }
    fn from_saved(id: &str) -> Result<Self, CmdError> {
        match id {
            "codex" | "synthetic" | "synthetic-codex" => Ok(Self::Codex),
            "claude" | "synthetic-claude" => Ok(Self::Claude),
            _ => Err(CmdError::Failed(
                "Неизвестный агент сохранённого анализа.".into(),
            )),
        }
    }
}

enum RecipeRequest<'a> {
    Codex(tgsum_runner::codex::RecipeRequest<'a>),
    Claude(tgsum_runner::claude::RecipeRequest<'a>),
}
impl<'a> RecipeRequest<'a> {
    fn prepare(
        agent: Agent,
        context: &'a PreparedContext,
        model: &str,
        recipe: Recipe,
        cancel: &Cancellation,
    ) -> Result<Self, RunnerError> {
        match agent {
            Agent::Codex => {
                tgsum_runner::codex::RecipeRequest::prepare(context, model, recipe, cancel)
                    .map(Self::Codex)
            }
            Agent::Claude => {
                tgsum_runner::claude::RecipeRequest::prepare(context, model, recipe, cancel)
                    .map(Self::Claude)
            }
        }
    }
    fn validate(
        &self,
        value: &RecipeOutput,
    ) -> std::io::Result<Vec<tgsum_core::bundle::EvidenceRef>> {
        match self {
            Self::Codex(r) => r.validate(value),
            Self::Claude(r) => r.validate(value),
        }
    }
    #[cfg(all(feature = "analysis-fixtures", debug_assertions))]
    fn recipe(&self) -> Recipe {
        match self {
            Self::Codex(r) => r.recipe(),
            Self::Claude(r) => r.recipe(),
        }
    }
    #[cfg(all(feature = "analysis-fixtures", debug_assertions))]
    fn input(&self) -> Result<&[u8], RunnerError> {
        match self {
            Self::Codex(r) => Ok(&r.request().invocation()?.stdin),
            Self::Claude(r) => Ok(&r.request().invocation()?.stdin),
        }
    }
}
struct Prepared {
    context: PreparedContext,
    ticket: RunTicket,
    backend: Backend,
}
enum Backend {
    #[cfg(target_os = "linux")]
    Codex {
        runner: tgsum_runner::codex::CodexNetworkRunner,
        auth: tgsum_runner::codex::SelectedAuthFile,
    },
    #[cfg(target_os = "linux")]
    Claude {
        runner: tgsum_runner::claude::ClaudeNetworkRunner,
        auth: tgsum_runner::claude::SelectedAuthFile,
    },
    #[cfg(all(feature = "analysis-fixtures", debug_assertions))]
    Synthetic,
}

#[derive(Serialize)]
pub(crate) struct AnalysisReview {
    run_id: String,
    spec: AnalysisSpec,
    coverage: Vec<AnalysisCoverage>,
}
#[derive(Serialize)]
pub(crate) struct AnalysisView {
    run_id: String,
    spec: AnalysisSpec,
    coverage: Vec<AnalysisCoverage>,
    state: &'static str,
    failure: Option<FailureCode>,
    result: Option<Value>,
    can_commit: bool,
}
fn view(store: &ProjectStore, project: &str, id: &str) -> Result<AnalysisView, CmdError> {
    let saved = store.read_analysis(project, id)?;
    let (state, failure, result) = match saved.completion {
        None => ("interrupted", None, None),
        Some(Completion::Cancelled) => ("cancelled", None, None),
        Some(Completion::Failed { code }) => ("failed", Some(code), None),
        Some(Completion::Validated { value, .. }) => (
            if saved.committed_revision.is_some() {
                "succeeded"
            } else {
                "uncommitted"
            },
            None,
            Some(value),
        ),
    };
    let can_commit =
        state == "uncommitted" && store.open(project)?.revision == saved.request.project_revision;
    Ok(AnalysisView {
        run_id: id.into(),
        spec: saved.request.spec,
        coverage: saved.request.coverage,
        state,
        failure,
        result,
        can_commit,
    })
}

fn relay_path(agent: Agent) -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join(agent.relay());
    if path.is_file() {
        return Some(path);
    }
    tgsum_runner::discover(std::ffi::OsStr::new(agent.relay()), &search_dirs())
        .ok()?
        .into_iter()
        .next()
        .map(|x| x.path)
}
fn search_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|p| p.is_absolute())
                .collect()
        })
        .unwrap_or_default()
}

#[tauri::command]
pub(crate) fn analysis_catalog(state: State<'_, AnalysisState>) -> Value {
    let agents = [Agent::Codex,Agent::Claude].map(|agent| {
        let paths=tgsum_runner::discover(std::ffi::OsStr::new(agent.id()),&search_dirs()).unwrap_or_default();
        let destinations=match agent {
            Agent::Codex=>serde_json::json!([{"id":"chatgpt.com","title":"OpenAI · ChatGPT"},{"id":"api.openai.com","title":"OpenAI · API"}]),
            Agent::Claude=>serde_json::json!([{"id":"api.anthropic.com","title":"Anthropic · Claude"}]),
        };
        serde_json::json!({"id":agent.id(),"title":agent.title(),"version":agent.version(),
            "auth_name":agent.auth_name(),"destinations":destinations,
            "executables":paths.into_iter().map(|p|p.path).collect::<Vec<_>>(),
            "available":state.fixtures || (cfg!(all(target_os="linux",target_arch="x86_64")) && relay_path(agent).is_some())})
    });
    serde_json::json!({"recipes":Recipe::ALL.map(|r|serde_json::json!({"id":r.id(),"title":r.title(),"version":RECIPE_VERSION})),
        "agents":agents,"fixtures":state.fixtures})
}

#[tauri::command]
pub(crate) async fn pick_analysis_file<R: Runtime>(
    app: AppHandle<R>,
    agent: Agent,
    kind: String,
) -> Option<String> {
    let title = match kind.as_str() {
        "executable" => format!("Выберите исполняемый файл {}", agent.title()),
        "auth" => format!("Выберите {} {}", agent.auth_name(), agent.title()),
        _ => return None,
    };
    let mut dialog = app.dialog().file().set_title(title);
    if kind == "auth" {
        dialog = dialog.add_filter(agent.auth_name(), &["json"]);
    }
    dialog
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string())
}

#[tauri::command]
pub(crate) async fn prepare_project_analysis(
    state: State<'_, AnalysisState>,
    store: State<'_, ProjectStore>,
    project_id: String,
    bundle_id: String,
    expected_revision: u64,
    options: AnalysisOptions,
) -> Result<AnalysisReview, CmdError> {
    let lease = state.reserve(&store, &project_id)?;
    let fixtures = state.fixtures;
    let store = store.inner().clone();
    run_blocking(move || {
        let context = PreparedContext::from_store(
            store.clone(),
            &project_id,
            &bundle_id,
            expected_revision,
            &lease.cancel,
        )
        .map_err(runner_error)?;
        let request = RecipeRequest::prepare(
            options.agent,
            &context,
            &options.model,
            options.recipe,
            &lease.cancel,
        )
        .map_err(runner_error)?;
        let (backend, spec) = prepare_backend(&request, options, fixtures, &lease.cancel)?;
        let ticket =
            store.begin_analysis(&project_id, &bundle_id, expected_revision, spec, || {
                lease.cancel.is_cancelled()
            })?;
        let review = AnalysisReview {
            run_id: ticket.request().run_id.clone(),
            spec: ticket.request().spec.clone(),
            coverage: ticket.request().coverage.clone(),
        };
        drop(request);
        if lease.cancel.is_cancelled() {
            store.cancel_analysis(&ticket)?;
            return Err(CmdError::Cancelled);
        }
        lease.ready(
            &store,
            Prepared {
                context,
                ticket,
                backend,
            },
        )?;
        Ok(review)
    })
    .await
}

fn prepare_backend(
    request: &RecipeRequest<'_>,
    options: AnalysisOptions,
    fixtures: bool,
    cancel: &Cancellation,
) -> Result<(Backend, AnalysisSpec), CmdError> {
    let spec = AnalysisSpec {
        agent: options.agent.id().into(),
        agent_version: options.agent.version().into(),
        isolation_profile: options.agent.profile().into(),
        destination: options.destination,
        model: options.model,
        recipe: options.recipe.id().into(),
        recipe_version: RECIPE_VERSION,
    };
    if fixtures {
        #[cfg(all(feature = "analysis-fixtures", debug_assertions))]
        {
            let mut spec = spec;
            if ![
                "fixture-success",
                "fixture-failure",
                "fixture-wait",
                "fixture-saved",
            ]
            .contains(&spec.model.as_str())
            {
                return Err(CmdError::Failed(
                    "Выберите модель локального стенда.".into(),
                ));
            }
            spec.agent = format!("synthetic-{}", options.agent.id());
            spec.agent_version = "1".into();
            spec.isolation_profile = "synthetic-local".into();
            spec.destination = "local_fixture".into();
            return Ok((Backend::Synthetic, spec));
        }
        #[cfg(not(all(feature = "analysis-fixtures", debug_assertions)))]
        return Err(CmdError::Failed(
            "Локальный стенд отсутствует в этой сборке.".into(),
        ));
    }
    // Validate the reviewed recipient on every OS before selecting a runtime
    // or opening auth. Unsupported hosts must preserve this same boundary.
    let destination_valid = match options.agent {
        Agent::Codex => ["api.openai.com", "chatgpt.com"].contains(&spec.destination.as_str()),
        Agent::Claude => spec.destination == "api.anthropic.com",
    };
    if !destination_valid {
        return Err(CmdError::Failed(
            "Получатель не соответствует выбранному агенту.".into(),
        ));
    }
    #[cfg(target_os = "linux")]
    {
        let relay = relay_path(options.agent).ok_or_else(|| {
            CmdError::Failed(format!(
                "Компонент запуска {} не установлен. Сохранение контекста доступно.",
                options.agent.title()
            ))
        })?;
        let backend = match request {
            RecipeRequest::Codex(request) => {
                let runner = tgsum_runner::codex::CodexNetworkRunner::qualify_installed(
                    request.request(),
                    relay,
                    options.executable.into(),
                    cancel,
                )
                .map_err(runner_error)?;
                let auth =
                    tgsum_runner::codex::SelectedAuthFile::select(Path::new(&options.auth_file))
                        .map_err(runner_error)?;
                Backend::Codex { runner, auth }
            }
            RecipeRequest::Claude(_) => {
                let policy =
                    tgsum_runner::claude::EndpointPolicy::capture(cancel).map_err(runner_error)?;
                let runner = tgsum_runner::claude::ClaudeNetworkRunner::qualify_installed(
                    relay,
                    options.executable.into(),
                    policy,
                    cancel,
                )
                .map_err(runner_error)?;
                let auth =
                    tgsum_runner::claude::SelectedAuthFile::select(Path::new(&options.auth_file))
                        .map_err(runner_error)?;
                Backend::Claude { runner, auth }
            }
        };
        Ok((backend, spec))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (request, options.executable, options.auth_file, spec, cancel);
        Err(CmdError::Failed(
            "Запуск агента на этой ОС ещё не квалифицирован. Сохраните контекст.".into(),
        ))
    }
}

#[tauri::command]
pub(crate) async fn run_project_analysis(
    state: State<'_, AnalysisState>,
    store: State<'_, ProjectStore>,
    run_id: String,
) -> Result<AnalysisView, CmdError> {
    let (run, lease) = state.take_run(&run_id)?;
    let store = store.inner().clone();
    run_blocking(move || {
        let lease = lease;
        let result = execute(&store, &run, &lease.cancel);
        if result.is_err() {
            // The managed adapter normally wrote a terminal record already.
            // A failure before its entry point still must not leave an apparent success.
            let _ = if lease.cancel.is_cancelled() {
                store.cancel_analysis(&run.ticket)
            } else {
                store.fail_analysis(&run.ticket, FailureCode::Agent)
            };
        }
        view(&store, &run.ticket.request().project_id, &run_id)
    })
    .await
}

#[cfg(any(
    target_os = "linux",
    all(feature = "analysis-fixtures", debug_assertions)
))]
fn execute(store: &ProjectStore, run: &Prepared, cancel: &Cancellation) -> Result<(), CmdError> {
    store.check_pending_analysis(&run.ticket, || cancel.is_cancelled())?;
    let spec = &run.ticket.request().spec;
    let recipe = Recipe::from_version(&spec.recipe, spec.recipe_version)?;
    let request = RecipeRequest::prepare(
        Agent::from_saved(&spec.agent)?,
        &run.context,
        &spec.model,
        recipe,
        cancel,
    )
    .map_err(runner_error)?;
    match (&run.backend, &request) {
        #[cfg(target_os = "linux")]
        (Backend::Codex { runner, auth }, RecipeRequest::Codex(request)) => {
            runner
                .run_recipe(request, &run.ticket, auth, cancel)
                .map_err(|_| {
                    CmdError::Failed("Анализ не завершён. Проверьте запись запуска.".into())
                })?;
        }
        #[cfg(target_os = "linux")]
        (Backend::Claude { runner, auth }, RecipeRequest::Claude(request)) => {
            runner
                .run_recipe(request, &run.ticket, auth, cancel)
                .map_err(|_| {
                    CmdError::Failed("Анализ не завершён. Проверьте запись запуска.".into())
                })?;
        }
        #[cfg(all(feature = "analysis-fixtures", debug_assertions))]
        (Backend::Synthetic, _) => synthetic_result(store, run, &request, cancel)?,
        #[cfg(target_os = "linux")]
        _ => {
            return Err(CmdError::Failed(
                "Агент не соответствует проверенному запуску.".into(),
            ))
        }
    }
    Ok(())
}

#[cfg(not(any(
    target_os = "linux",
    all(feature = "analysis-fixtures", debug_assertions)
)))]
fn execute(_: &ProjectStore, _: &Prepared, _: &Cancellation) -> Result<(), CmdError> {
    Err(CmdError::Failed(
        "На этой платформе доступно сохранение контекста.".into(),
    ))
}

#[cfg(all(feature = "analysis-fixtures", debug_assertions))]
fn synthetic_result(
    store: &ProjectStore,
    run: &Prepared,
    request: &RecipeRequest<'_>,
    cancel: &Cancellation,
) -> Result<(), CmdError> {
    use serde_json::json;
    let count = if run.ticket.request().spec.model == "fixture-wait" {
        500
    } else {
        10
    };
    for _ in 0..count {
        if cancel.is_cancelled() {
            return Err(CmdError::Cancelled);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if run.ticket.request().spec.model == "fixture-failure" {
        return Err(CmdError::Failed("Synthetic failure".into()));
    }
    let input: Value = serde_json::from_slice(request.input().map_err(runner_error)?).unwrap();
    let (id, revision) = input["untrusted_documents"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|d| d["text"].as_str().unwrap().lines())
        .find_map(|l| l.strip_prefix("## Evidence "))
        .unwrap()
        .split_once('@')
        .unwrap();
    let value:RecipeOutput=serde_json::from_value(json!({"recipe":request.recipe().id(),"version":1,
        "sections":request.recipe().sections().iter().enumerate().map(|(i,id_section)|json!({"id":id_section,"claims":if i==0 {vec![json!({"text":"<img src=x onerror=alert(1)> Synthetic local result","evidence":[{"id":id,"revision":revision}]})]} else {vec![]}})).collect::<Vec<_>>(),"actions":[]})).unwrap();
    store.save_analysis_result(
        &run.ticket,
        &value,
        |v| request.validate(v),
        || cancel.is_cancelled(),
    )?;
    if run.ticket.request().spec.model == "fixture-saved" {
        return Ok(());
    }
    store.commit_analysis(&run.ticket, || cancel.is_cancelled())?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn list_project_analyses(
    store: State<'_, ProjectStore>,
    project_id: String,
) -> Result<Vec<Value>, CmdError> {
    let store = store.inner().clone();
    run_blocking(move || {
        Ok(store
            .recent_analysis_ids(&project_id, 20)?
            .into_iter()
            .map(|id| match view(&store, &project_id, &id) {
                Ok(v) => serde_json::json!({"run_id":id,"agent":v.spec.agent,"recipe":v.spec.recipe,"state":v.state}),
                Err(_) => serde_json::json!({"run_id":id,"recipe":"","state":"unavailable"}),
            })
            .collect())
    })
    .await
}
#[tauri::command]
pub(crate) async fn read_project_analysis(
    store: State<'_, ProjectStore>,
    project_id: String,
    run_id: String,
) -> Result<AnalysisView, CmdError> {
    let store = store.inner().clone();
    run_blocking(move || view(&store, &project_id, &run_id)).await
}
#[tauri::command]
pub(crate) async fn recover_project_analysis(
    state: State<'_, AnalysisState>,
    store: State<'_, ProjectStore>,
    project_id: String,
    run_id: String,
    commit: bool,
) -> Result<AnalysisView, CmdError> {
    let lease = state.reserve(&store, &project_id)?;
    let store = store.inner().clone();
    run_blocking(move || {
        let lease = lease;
        let ticket = store.resume_analysis(&project_id, &run_id)?;
        let saved = store.read_analysis(&project_id, &run_id)?;
        if commit {
            if saved.committed_revision.is_none() {
                let Some(Completion::Validated { value, evidence }) = saved.completion else {
                    return Err(CmdError::Failed(
                        "Нет проверенного результата для восстановления.".into(),
                    ));
                };
                let spec = &ticket.request().spec;
                let context = PreparedContext::from_store(
                    store.clone(),
                    &project_id,
                    &ticket.request().bundle_id,
                    ticket.request().project_revision,
                    &lease.cancel,
                )
                .map_err(runner_error)?;
                let request = RecipeRequest::prepare(
                    Agent::from_saved(&spec.agent)?,
                    &context,
                    &spec.model,
                    Recipe::from_version(&spec.recipe, spec.recipe_version)?,
                    &lease.cancel,
                )
                .map_err(runner_error)?;
                let value: RecipeOutput = serde_json::from_value(value)
                    .map_err(|_| CmdError::Failed("Неизвестный формат результата.".into()))?;
                if request.validate(&value)? != evidence {
                    return Err(CmdError::Failed("Ссылки результата повреждены.".into()));
                }
                store.commit_analysis(&ticket, || lease.cancel.is_cancelled())?;
            }
        } else if saved.completion.is_none() {
            store.cancel_analysis(&ticket)?;
        } else {
            return Err(CmdError::Failed("Этот запуск уже завершён.".into()));
        }
        view(&store, &project_id, &run_id)
    })
    .await
}

#[tauri::command]
pub(crate) fn discard_analysis_review(
    state: State<'_, AnalysisState>,
    store: State<'_, ProjectStore>,
) -> Result<(), CmdError> {
    state.cancel(&store)
}
