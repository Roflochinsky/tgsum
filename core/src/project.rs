//! Private project configuration with immutable, atomically published revisions.
//!
//! Archives remain at their selected locations; this store retains references,
//! settings, and selected snapshots. It neither copies archives nor opens client
//! sessions. Revision creation uses no-clobber publication for optimistic writes:
//! a stale editor must reload instead of overwriting another editor's changes.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::scope::{MessageFilter, SourceSelection};
use crate::snapshot::{validate_snapshot_id, SnapshotStore, SourceScope};

const SCHEMA_VERSION: u32 = 2;
const MAX_MANIFEST_BYTES: u64 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub default_agent: String,
    pub default_recipe: String,
    pub privacy_preset: String,
    /// None means no automatic Markdown part-size limit.
    pub max_tokens: Option<usize>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            default_agent: "export_only".into(),
            default_recipe: "summary".into(),
            privacy_preset: "secrets".into(),
            max_tokens: Some(crate::DEFAULT_MAX_TOKENS),
        }
    }
}

/// A selected source. Account identity is configured locally and is not proven
/// by a Telegram export. No credential value belongs in this manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSource {
    pub source_id: String,
    pub connector_id: String,
    pub scope: SourceScope,
    pub archive_path: Option<PathBuf>,
    pub latest_snapshot_id: Option<String>,
    #[serde(default)]
    pub selection: SourceSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisBaseline {
    pub source_id: String,
    pub source: SourceScope,
    pub snapshot_id: String,
    pub filter: MessageFilter,
    pub analysis_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisInput {
    pub source_id: String,
    pub source: SourceScope,
    pub snapshot_id: String,
    pub selection: SourceSelection,
    pub baseline: Option<AnalysisBaseline>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisRun {
    pub run_id: String,
    pub inputs: Vec<AnalysisInput>,
    pub outcome: Option<AnalysisOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub project_id: String,
    pub revision: u64,
    pub name: String,
    pub sources: Vec<ProjectSource>,
    pub settings: ProjectSettings,
    #[serde(default)]
    pub analysis_run: Option<AnalysisRun>,
    #[serde(default)]
    pub baselines: Vec<AnalysisBaseline>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ProjectChange {
    Rename(String),
    Source(ProjectSource),
    RemoveSource(String),
    Settings(ProjectSettings),
    Selection {
        source_id: String,
        selection: SourceSelection,
    },
    BeginAnalysis {
        run_id: String,
    },
    FinishAnalysis {
        run_id: String,
        outcome: AnalysisOutcome,
    },
    RecordSnapshot {
        source_id: String,
        snapshot_id: String,
    },
}

/// A corrupt project stays visible without preventing other projects opening.
#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ProjectEntry {
    Ready { project: Box<Project> },
    Unavailable { project_id: String, message: String },
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SourceAvailability {
    FileAvailable,
    FileMissing,
    NotFileBacked,
    Unavailable { message: String },
}

impl ProjectSource {
    /// Checks only the explicitly configured path's metadata. A moved archive
    /// does not stop the project opening or invalidate retained snapshots.
    pub fn availability(&self) -> SourceAvailability {
        let Some(path) = &self.archive_path else {
            return SourceAvailability::NotFileBacked;
        };
        match fs::metadata(path) {
            Ok(meta) if meta.is_file() => SourceAvailability::FileAvailable,
            Ok(_) => SourceAvailability::Unavailable {
                message: "source path is not a file".into(),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                SourceAvailability::FileMissing
            }
            Err(error) => SourceAvailability::Unavailable {
                message: error.to_string(),
            },
        }
    }
}

#[derive(Clone)]
pub struct ProjectStore {
    root: PathBuf,
}

impl ProjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn create(&self, name: &str) -> io::Result<Project> {
        validate_name(name)?;
        fs::create_dir_all(&self.root)?;
        let directory = tempfile::Builder::new()
            .prefix("project-")
            .tempdir_in(&self.root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
        }
        let project_id = directory
            .path()
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| invalid("project ID is not UTF-8"))?
            .to_owned();
        let project = Project {
            schema_version: SCHEMA_VERSION,
            project_id,
            revision: 0,
            name: name.trim().to_owned(),
            sources: Vec::new(),
            settings: ProjectSettings::default(),
            analysis_run: None,
            baselines: Vec::new(),
        };
        let revisions = directory.path().join("revisions");
        fs::create_dir(&revisions)?;
        write_revision(&revisions, &project)?;
        // A failed initial publication drops the entire newly created directory.
        let _retained = directory.keep();
        Ok(project)
    }

    pub fn open(&self, project_id: &str) -> io::Result<Project> {
        let directory = self.directory(project_id)?;
        let revisions = directory.join("revisions");
        require_directory(&revisions)?;
        let latest = fs::read_dir(&revisions)?
            .filter_map(|entry| {
                let entry = match entry {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
                let name = entry.file_name();
                let name = name.to_str()?;
                // tempfile staging names cannot become a committed head.
                if !name.ends_with(".json") {
                    return None;
                }
                Some(parse_revision_name(name).map(|revision| (revision, entry.path())))
            })
            .collect::<io::Result<Vec<_>>>()?
            .into_iter()
            .max_by_key(|(revision, _)| *revision)
            .ok_or_else(|| {
                invalid("project has no committed manifest; restore a known revision")
            })?;
        let metadata = fs::symlink_metadata(&latest.1)?;
        if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
            return Err(invalid(
                "project manifest must be a regular file of at most 1 MiB",
            ));
        }
        let mut bytes = Vec::new();
        File::open(latest.1)?
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(invalid("project manifest exceeds 1 MiB"));
        }
        // Check the version before interpreting fields. A future schema can
        // change their shape; opening it must never rewrite it as today's one.
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(invalid)?;
        if !matches!(value["schema_version"].as_u64(), Some(1) | Some(2)) {
            return Err(invalid(
                "unsupported project schema; use a compatible app or an explicit migration",
            ));
        }
        let mut project: Project = serde_json::from_value(value).map_err(invalid)?;
        // Version 1 had no scope or analysis ledger. Defaults preserve its
        // full-source behavior; a later update publishes v2 as a new revision.
        project.schema_version = SCHEMA_VERSION;
        validate_project(&project)?;
        if project.project_id != project_id || project.revision != latest.0 {
            return Err(invalid(
                "manifest identity/revision does not match its location",
            ));
        }
        Ok(project)
    }

    pub fn list(&self) -> io::Result<Vec<ProjectEntry>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            if let Some(id) = name.to_str().filter(|id| id.starts_with("project-")) {
                ids.push(id.to_owned());
            }
        }
        ids.sort();
        Ok(ids
            .into_iter()
            .map(|id| match self.open(&id) {
                Ok(project) => ProjectEntry::Ready {
                    project: Box::new(project),
                },
                Err(error) => ProjectEntry::Unavailable {
                    project_id: id,
                    message: error.to_string(),
                },
            })
            .collect())
    }

    /// The expected revision is the one the caller displayed/edited. Each
    /// successful edit writes the next immutable revision. WouldBlock means
    /// another editor won; callers must reload before retrying their change.
    pub fn update(
        &self,
        project_id: &str,
        expected_revision: u64,
        change: ProjectChange,
    ) -> io::Result<Project> {
        let mut project = self.open(project_id)?;
        if project.revision != expected_revision {
            return Err(conflict());
        }
        let changed_source_id = match &change {
            ProjectChange::Source(source) => Some(source.source_id.clone()),
            ProjectChange::RecordSnapshot { source_id, .. } => Some(source_id.clone()),
            _ => None,
        };
        match change {
            ProjectChange::Rename(name) => {
                validate_name(&name)?;
                project.name = name.trim().into();
            }
            ProjectChange::Source(source) => {
                project
                    .baselines
                    .retain(|b| b.source_id != source.source_id || b.source == source.scope);
                if let Some(old) = project
                    .sources
                    .iter_mut()
                    .find(|s| s.source_id == source.source_id)
                {
                    *old = source;
                } else {
                    project.sources.push(source);
                }
            }
            ProjectChange::RemoveSource(id) => {
                if !project.sources.iter().any(|s| s.source_id == id) {
                    return Err(invalid("source not connected to this project"));
                }
                project.sources.retain(|s| s.source_id != id);
                project.baselines.retain(|b| b.source_id != id);
            }
            ProjectChange::Settings(settings) => project.settings = settings,
            ProjectChange::Selection {
                source_id,
                selection,
            } => {
                selection.filter.validate()?;
                project
                    .sources
                    .iter_mut()
                    .find(|s| s.source_id == source_id)
                    .ok_or_else(|| invalid("source not connected to this project"))?
                    .selection = selection;
            }
            ProjectChange::BeginAnalysis { run_id } => {
                validate_snapshot_id(&run_id)?;
                if project
                    .analysis_run
                    .as_ref()
                    .is_some_and(|run| run.outcome.is_none() || run.run_id == run_id)
                {
                    return Err(invalid("analysis already active or run ID reused"));
                }
                let snapshots = self.snapshots(project_id)?;
                let mut inputs = Vec::new();
                for source in project.sources.iter().filter(|s| s.selection.enabled) {
                    let snapshot_id = source
                        .latest_snapshot_id
                        .as_ref()
                        .ok_or_else(|| invalid("refresh selected sources before analysis"))?;
                    if snapshots.load(snapshot_id)?.source != source.scope {
                        return Err(invalid("analysis snapshot source mismatch"));
                    }
                    inputs.push(AnalysisInput {
                        source_id: source.source_id.clone(),
                        source: source.scope.clone(),
                        snapshot_id: snapshot_id.clone(),
                        selection: source.selection.clone(),
                        baseline: project
                            .baselines
                            .iter()
                            .find(|b| b.source_id == source.source_id && b.source == source.scope)
                            .cloned(),
                    });
                }
                if inputs.is_empty() {
                    return Err(invalid("select at least one source before analysis"));
                }
                project.analysis_run = Some(AnalysisRun {
                    run_id,
                    inputs,
                    outcome: None,
                });
            }
            ProjectChange::FinishAnalysis { run_id, outcome } => {
                let run = project
                    .analysis_run
                    .as_mut()
                    .filter(|run| run.run_id == run_id && run.outcome.is_none())
                    .ok_or_else(|| invalid("analysis is not active or run ID does not match"))?;
                if outcome == AnalysisOutcome::Succeeded {
                    for input in &run.inputs {
                        // A disconnected/replaced source must not be reattached by
                        // a delayed result. An ordinary refresh is independent.
                        if !project
                            .sources
                            .iter()
                            .any(|s| s.source_id == input.source_id && s.scope == input.source)
                        {
                            continue;
                        }
                        project.baselines.retain(|b| b.source_id != input.source_id);
                        project.baselines.push(AnalysisBaseline {
                            source_id: input.source_id.clone(),
                            source: input.source.clone(),
                            snapshot_id: input.snapshot_id.clone(),
                            filter: input.selection.filter.clone(),
                            analysis_id: run_id.clone(),
                        });
                    }
                }
                run.outcome = Some(outcome);
            }
            ProjectChange::RecordSnapshot {
                source_id,
                snapshot_id,
            } => {
                let source = project
                    .sources
                    .iter_mut()
                    .find(|s| s.source_id == source_id)
                    .ok_or_else(|| invalid("source not connected to this project"))?;
                source.latest_snapshot_id = Some(snapshot_id);
            }
        }
        project.revision = project
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("project revision exhausted"))?;
        validate_project(&project)?;
        // Renames and preferences must not deserialize large chat snapshots.
        for source in project
            .sources
            .iter()
            .filter(|s| Some(&s.source_id) == changed_source_id.as_ref())
        {
            if let Some(snapshot_id) = &source.latest_snapshot_id {
                if self.snapshots(project_id)?.load(snapshot_id)?.source != source.scope {
                    return Err(invalid("snapshot does not belong to the connected source"));
                }
            }
        }
        write_revision(&self.directory(project_id)?.join("revisions"), &project).map_err(
            |error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    conflict()
                } else {
                    error
                }
            },
        )?;
        Ok(project)
    }

    pub fn snapshots(&self, project_id: &str) -> io::Result<SnapshotStore> {
        let directory = self.directory(project_id)?.join("snapshots");
        match fs::create_dir(&directory) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e),
        }
        require_directory(&directory)?;
        Ok(SnapshotStore::new(directory))
    }

    fn directory(&self, project_id: &str) -> io::Result<PathBuf> {
        validate_snapshot_id(project_id)?;
        if !project_id.starts_with("project-") {
            return Err(invalid("invalid project ID"));
        }
        let directory = self.root.join(project_id);
        require_directory(&directory)?;
        Ok(directory)
    }
}

fn validate_project(project: &Project) -> io::Result<()> {
    if project.schema_version != SCHEMA_VERSION {
        return Err(invalid(
            "unsupported project schema; use a compatible app or an explicit migration",
        ));
    }
    validate_snapshot_id(&project.project_id)?;
    validate_name(&project.name)?;
    let mut ids = BTreeSet::new();
    for source in &project.sources {
        validate_snapshot_id(&source.source_id)?;
        validate_snapshot_id(&source.connector_id)?;
        source.scope.validate()?;
        source.selection.filter.validate()?;
        if !ids.insert(&source.source_id) {
            return Err(invalid("duplicate source ID"));
        }
        if source
            .archive_path
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err(invalid("archive reference must be an absolute path"));
        }
        if let Some(id) = &source.latest_snapshot_id {
            validate_snapshot_id(id)?;
        }
    }
    let mut baseline_ids = BTreeSet::new();
    for baseline in &project.baselines {
        validate_baseline(baseline)?;
        if !baseline_ids.insert(&baseline.source_id)
            || !project
                .sources
                .iter()
                .any(|s| s.source_id == baseline.source_id && s.scope == baseline.source)
        {
            return Err(invalid("baseline source is duplicated or disconnected"));
        }
    }
    if let Some(run) = &project.analysis_run {
        validate_snapshot_id(&run.run_id)?;
        if run.inputs.is_empty() {
            return Err(invalid("analysis has no inputs"));
        }
        let mut ids = BTreeSet::new();
        for input in &run.inputs {
            validate_snapshot_id(&input.source_id)?;
            validate_snapshot_id(&input.snapshot_id)?;
            input.source.validate()?;
            input.selection.filter.validate()?;
            if !ids.insert(&input.source_id) {
                return Err(invalid("duplicate analysis source"));
            }
            if let Some(baseline) = &input.baseline {
                validate_baseline(baseline)?;
                if baseline.source_id != input.source_id || baseline.source != input.source {
                    return Err(invalid("analysis baseline source mismatch"));
                }
            }
        }
    }
    for id in [
        &project.settings.default_agent,
        &project.settings.default_recipe,
        &project.settings.privacy_preset,
    ] {
        validate_snapshot_id(id)?;
    }
    if project.settings.max_tokens == Some(0) {
        return Err(invalid("max_tokens must be positive or null"));
    }
    Ok(())
}

fn write_revision(directory: &Path, project: &Project) -> io::Result<()> {
    let mut staged = tempfile::NamedTempFile::new_in(directory)?;
    {
        let mut writer = BufWriter::new(staged.as_file_mut());
        serde_json::to_writer(&mut writer, project).map_err(invalid)?;
        writer.flush()?;
    }
    if staged.as_file().metadata()?.len() > MAX_MANIFEST_BYTES {
        return Err(invalid("project manifest exceeds 1 MiB"));
    }
    staged.as_file().sync_all()?;
    staged
        .persist_noclobber(directory.join(format!("{:020}.json", project.revision)))
        .map_err(|e| e.error)?;
    Ok(())
}

fn validate_baseline(baseline: &AnalysisBaseline) -> io::Result<()> {
    validate_snapshot_id(&baseline.source_id)?;
    validate_snapshot_id(&baseline.snapshot_id)?;
    validate_snapshot_id(&baseline.analysis_id)?;
    baseline.source.validate()?;
    baseline.filter.validate()
}

fn parse_revision_name(name: &str) -> io::Result<u64> {
    let digits = name
        .strip_suffix(".json")
        .ok_or_else(|| invalid("invalid revision filename"))?;
    if digits.len() != 20 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid("invalid revision filename"));
    }
    digits.parse().map_err(invalid)
}

fn require_directory(path: &Path) -> io::Result<()> {
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(invalid("project path must be a directory, not a symlink"));
    }
    Ok(())
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.trim().is_empty() || name.len() > 512 || name.chars().any(char::is_control) {
        return Err(invalid(
            "project name must be nonempty, at most 512 bytes and contain no control characters",
        ));
    }
    Ok(())
}

fn conflict() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "project changed; reload before saving",
    )
}
fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
