//! Durable analysis records. A checked result is published before a single
//! Project revision advances its baselines; interruption never invents success.
//! Runner/recipe code owns process, schema and semantic validation. This module
//! binds that validated value to the reviewed bundle and its evidence index.

use crate::bundle::{BundleManifest, EvidenceRef};
use crate::project::{AnalysisInput, AnalysisOutcome, AnalysisRun, ProjectStore};
use crate::snapshot::validate_snapshot_id;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const RECORD_LIMIT: usize = 4 * 1024 * 1024;
const RESULT_LIMIT: usize = 1024 * 1024;

/// Trusted adapter/recipe metadata shown at Review. These strings never select
/// executables, network endpoints or commands in this offline module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisSpec {
    pub agent: String,
    pub agent_version: String,
    pub isolation_profile: String,
    pub destination: String,
    pub model: String,
    pub recipe: String,
    pub recipe_version: u32,
}

impl AnalysisSpec {
    fn validate(&self) -> io::Result<()> {
        for value in [
            &self.agent,
            &self.agent_version,
            &self.isolation_profile,
            &self.destination,
            &self.model,
            &self.recipe,
        ] {
            if value.is_empty()
                || value.len() > 128
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._:".contains(&b))
            {
                return Err(invalid("invalid analysis descriptor"));
            }
        }
        if self.recipe_version == 0 {
            return Err(invalid("recipe version must be positive"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRequest {
    pub schema_version: u32,
    pub run_id: String,
    pub project_id: String,
    pub project_revision: u64,
    pub bundle_id: String,
    pub manifest_sha256: String,
    pub spec: AnalysisSpec,
    pub inputs: Vec<AnalysisInput>,
    pub coverage: Vec<AnalysisCoverage>,
}

/// Coverage comes from the reviewed manifest, never a model's completeness claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisCoverage {
    pub source_id: String,
    pub level: crate::snapshot::CoverageLevel,
    pub known_gaps: usize,
    pub only_changes: bool,
    pub messages: usize,
}

fn coverage(manifest: &BundleManifest) -> Vec<AnalysisCoverage> {
    manifest
        .sources
        .iter()
        .map(|s| AnalysisCoverage {
            source_id: s.id.clone(),
            level: s.coverage.clone(),
            known_gaps: s.known_gaps,
            only_changes: s.only_changes,
            messages: s.stats.selected,
        })
        .collect()
}

/// Cannot be constructed or edited from serialized chat data. Resuming a ticket
/// verifies its storage location and the original bundle binding again.
pub struct RunTicket {
    directory: PathBuf,
    request: RunRequest,
}
impl RunTicket {
    pub fn request(&self) -> &RunRequest {
        &self.request
    }
}
impl fmt::Debug for RunTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunTicket")
            .field("run_id", &self.request.run_id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultRef {
    pub run_id: String,
    pub bundle_id: String,
    pub sha256: String,
}
impl ResultRef {
    pub(crate) fn validate(&self) -> io::Result<()> {
        run_id(&self.run_id)?;
        validate_snapshot_id(&self.bundle_id)?;
        if !self.bundle_id.starts_with("bundle-")
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid("invalid analysis result reference"));
        }
        Ok(())
    }
}

/// Static categories only; provider text and credentials never belong in a
/// durable failure diagnostic. Authentication is set only by qualified adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    Agent,
    Authentication,
    Transport,
    InvalidResult,
    Interrupted,
    TimedOut,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Completion {
    Validated {
        value: Value,
        evidence: Vec<EvidenceRef>,
    },
    Failed {
        code: FailureCode,
    },
    Cancelled,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompletionRecord {
    schema_version: u32,
    request_sha256: String,
    completion: Completion,
    sha256: String,
}

fn pack_completion(request: &RunRequest, completion: Completion) -> io::Result<Vec<u8>> {
    let bytes = encode(&completion, RECORD_LIMIT)?;
    encode(
        &CompletionRecord {
            schema_version: 1,
            request_sha256: format!("{:x}", Sha256::digest(encode(request, RECORD_LIMIT)?)),
            completion,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        },
        RECORD_LIMIT,
    )
}

fn unpack_completion(request: &RunRequest, bytes: &[u8]) -> io::Result<Completion> {
    let record: CompletionRecord =
        serde_json::from_slice(bytes).map_err(|_| invalid("invalid analysis completion"))?;
    if record.schema_version != 1
        || record.request_sha256 != format!("{:x}", Sha256::digest(encode(request, RECORD_LIMIT)?))
        || record.sha256
            != format!(
                "{:x}",
                Sha256::digest(encode(&record.completion, RECORD_LIMIT)?)
            )
    {
        return Err(invalid("analysis completion checksum or version mismatch"));
    }
    if let Completion::Validated { value, .. } = &record.completion {
        encode(value, RESULT_LIMIT)?;
        if !value.is_object() {
            return Err(invalid("analysis result must be a JSON object"));
        }
    }
    Ok(record.completion)
}

impl fmt::Debug for Completion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validated { evidence, .. } => f
                .debug_struct("Validated")
                .field("evidence_count", &evidence.len())
                .finish_non_exhaustive(),
            Self::Failed { code } => f.debug_tuple("Failed").field(code).finish(),
            Self::Cancelled => f.write_str("Cancelled"),
        }
    }
}

#[derive(Debug)]
pub struct StoredAnalysis {
    pub request: RunRequest,
    /// None is pending/interrupted, never implied success after restart.
    pub completion: Option<Completion>,
    /// A validated artifact can exist without a committed baseline, e.g. after
    /// a crash or concurrent edit. Recovery must explicitly commit the ticket.
    pub committed_revision: Option<u64>,
}

impl ProjectStore {
    /// Recheck immediately before a launch; terminal runs cannot be replayed.
    pub fn check_pending_analysis(
        &self,
        ticket: &RunTicket,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<()> {
        self.check_ticket(ticket, &cancelled)?;
        match fs::symlink_metadata(ticket.directory.join("completion.json")) {
            Ok(_) => Err(invalid("analysis already has a terminal result")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
    /// Create a pending run for the exact revision reviewed by the user. This
    /// does not change Project revision, so PreparedContext stays bound to it.
    pub fn begin_analysis(
        &self,
        project_id: &str,
        bundle_id: &str,
        expected_revision: u64,
        spec: AnalysisSpec,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<RunTicket> {
        check_cancel(&cancelled)?;
        spec.validate()?;
        let project = self.open(project_id)?;
        if project.revision != expected_revision {
            return Err(conflict());
        }
        if project
            .analysis_run
            .as_ref()
            .is_some_and(|r| r.outcome.is_none())
        {
            return Err(invalid("legacy analysis is still active"));
        }
        let bundle = self.analysis_bundle(project_id, bundle_id, &cancelled)?;
        if bundle.revision != expected_revision {
            return Err(conflict());
        }
        let root = private_dir(&self.directory(project_id)?.join("analyses"))?;
        let staged = tempfile::Builder::new().prefix("run-").tempdir_in(&root)?;
        set_private_directory(staged.path())?;
        let request = RunRequest {
            schema_version: 1,
            run_id: staged
                .path()
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| invalid("invalid analysis ID"))?
                .into(),
            project_id: project_id.into(),
            project_revision: expected_revision,
            bundle_id: bundle_id.into(),
            manifest_sha256: bundle.manifest_sha256,
            spec,
            inputs: bundle.inputs,
            coverage: coverage(&bundle.manifest),
        };
        publish(
            &staged.path().join("request.json"),
            &encode(&request, RECORD_LIMIT)?,
        )?;
        check_cancel(&cancelled)?;
        if self.open(project_id)?.revision != expected_revision {
            return Err(conflict());
        }
        let directory = staged.keep();
        #[cfg(unix)]
        File::open(&root)?.sync_all()?;
        Ok(RunTicket { directory, request })
    }

    pub fn resume_analysis(&self, project_id: &str, id: &str) -> io::Result<RunTicket> {
        let directory = self.analysis_directory(project_id, id)?;
        let request: RunRequest = load(&directory.join("request.json"))?;
        let ticket = RunTicket { directory, request };
        if ticket.request.project_id != project_id || ticket.request.run_id != id {
            return Err(invalid("analysis request identity mismatch"));
        }
        self.check_ticket(&ticket, &|| false)?;
        Ok(ticket)
    }

    /// Call ONLY after the runner accepted process/transport/JSONL outcomes.
    /// The mandatory trusted recipe validator rejects schema/semantic failures
    /// and returns every reference derived from the typed value (not a model's
    /// separate list). Core verifies membership/revisions in the same bundle.
    pub fn save_analysis_result<T: Serialize>(
        &self,
        ticket: &RunTicket,
        value: &T,
        validate: impl FnOnce(&T) -> io::Result<Vec<EvidenceRef>>,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<ResultRef> {
        self.check_ticket(ticket, &cancelled)?;
        check_cancel(&cancelled)?;
        let evidence = validate(value)?;
        self.check_analysis_evidence(
            &ticket.request.project_id,
            &ticket.request.bundle_id,
            &evidence,
            &cancelled,
        )?;
        let bytes = encode(value, RESULT_LIMIT)?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| invalid("invalid result JSON"))?;
        if !value.is_object() {
            return Err(invalid("analysis result must be a JSON object"));
        }
        let bytes = pack_completion(&ticket.request, Completion::Validated { value, evidence })?;
        check_cancel(&cancelled)?;
        publish(&ticket.directory.join("completion.json"), &bytes)?;
        Ok(receipt(&ticket.request, &bytes))
    }

    pub fn fail_analysis(&self, ticket: &RunTicket, code: FailureCode) -> io::Result<()> {
        self.finish_without_result(ticket, Completion::Failed { code })
    }
    pub fn cancel_analysis(&self, ticket: &RunTicket) -> io::Result<()> {
        self.finish_without_result(ticket, Completion::Cancelled)
    }
    fn finish_without_result(&self, ticket: &RunTicket, completion: Completion) -> io::Result<()> {
        // Can record a failure even if the reviewed bundle was subsequently
        // removed/corrupted; only ticket identity, never source data, is needed.
        self.check_ticket_identity(ticket)?;
        publish(
            &ticket.directory.join("completion.json"),
            &pack_completion(&ticket.request, completion)?,
        )
    }

    /// Atomically commit result provenance + all input baselines in one Project
    /// revision. Idempotent after a crash following publication; a conflicting
    /// Project edit never rebinds the result to newer inputs or rolls back scope.
    pub fn commit_analysis(
        &self,
        ticket: &RunTicket,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<u64> {
        self.check_ticket(ticket, &cancelled)?;
        let bytes = read(&ticket.directory.join("completion.json"))?;
        let completion = unpack_completion(&ticket.request, &bytes)?;
        let Completion::Validated { evidence, .. } = completion else {
            return Err(invalid("analysis has no validated result"));
        };
        self.check_analysis_evidence(
            &ticket.request.project_id,
            &ticket.request.bundle_id,
            &evidence,
            &cancelled,
        )?;
        let result = receipt(&ticket.request, &bytes);
        if let Some(revision) = self.committed_revision(&ticket.request, Some(&result))? {
            return Ok(revision);
        }
        check_cancel(&cancelled)?;
        let project = self.publish_analysis(
            &ticket.request.project_id,
            ticket.request.project_revision,
            AnalysisRun {
                run_id: ticket.request.run_id.clone(),
                inputs: ticket.request.inputs.clone(),
                outcome: Some(AnalysisOutcome::Succeeded),
                result: Some(result),
            },
        )?;
        Ok(project.revision)
    }

    pub fn read_analysis(&self, project_id: &str, id: &str) -> io::Result<StoredAnalysis> {
        let directory = self.analysis_directory(project_id, id)?;
        let request: RunRequest = load(&directory.join("request.json"))?;
        if request.project_id != project_id || request.run_id != id {
            return Err(invalid("analysis request identity mismatch"));
        }
        validate_request(&request)?;
        let bytes = match read(&directory.join("completion.json")) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let completion = bytes
            .as_ref()
            .map(|b| unpack_completion(&request, b))
            .transpose()?;
        let result = bytes
            .as_ref()
            .filter(|_| matches!(completion, Some(Completion::Validated { .. })))
            .map(|b| receipt(&request, b));
        let committed_revision = self.committed_revision(&request, result.as_ref())?;
        Ok(StoredAnalysis {
            request,
            completion,
            committed_revision,
        })
    }

    fn analysis_directory(&self, project_id: &str, id: &str) -> io::Result<PathBuf> {
        run_id(id)?;
        let root = self.directory(project_id)?.join("analyses");
        require_dir(&root)?;
        let directory = root.join(id);
        require_dir(&directory)?;
        Ok(directory)
    }
    fn check_ticket_identity(&self, ticket: &RunTicket) -> io::Result<()> {
        validate_request(&ticket.request)?;
        let directory =
            self.analysis_directory(&ticket.request.project_id, &ticket.request.run_id)?;
        if fs::canonicalize(&directory)? != fs::canonicalize(&ticket.directory)?
            || load::<RunRequest>(&directory.join("request.json"))? != ticket.request
        {
            return Err(invalid(
                "analysis ticket changed or belongs to another store",
            ));
        }
        Ok(())
    }
    fn check_ticket(&self, ticket: &RunTicket, cancelled: &impl Fn() -> bool) -> io::Result<()> {
        check_cancel(cancelled)?;
        self.check_ticket_identity(ticket)?;
        let bundle = self.analysis_bundle(
            &ticket.request.project_id,
            &ticket.request.bundle_id,
            cancelled,
        )?;
        if bundle.revision != ticket.request.project_revision
            || bundle.manifest_sha256 != ticket.request.manifest_sha256
            || bundle.inputs != ticket.request.inputs
            || coverage(&bundle.manifest) != ticket.request.coverage
        {
            return Err(invalid("analysis no longer matches the reviewed bundle"));
        }
        Ok(())
    }
    fn committed_revision(
        &self,
        request: &RunRequest,
        result: Option<&ResultRef>,
    ) -> io::Result<Option<u64>> {
        let revision = request
            .project_revision
            .checked_add(1)
            .ok_or_else(|| invalid("revision exhausted"))?;
        let project = match self.read_revision(&request.project_id, revision) {
            Ok(project) => project,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if let Some(run) = project
            .analysis_run
            .filter(|run| run.run_id == request.run_id)
        {
            if result.is_none() || run.result.as_ref() != result || run.inputs != request.inputs {
                return Err(invalid("committed analysis artifact is missing or changed"));
            }
            return Ok(Some(revision));
        }
        Ok(None)
    }
}

fn receipt(request: &RunRequest, bytes: &[u8]) -> ResultRef {
    ResultRef {
        run_id: request.run_id.clone(),
        bundle_id: request.bundle_id.clone(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}
fn validate_request(request: &RunRequest) -> io::Result<()> {
    run_id(&request.run_id)?;
    validate_snapshot_id(&request.project_id)?;
    receipt(request, &[]).validate()?;
    request.spec.validate()?;
    if request.schema_version != 1
        || request.inputs.is_empty()
        || request.coverage.len() != request.inputs.len()
        || request.manifest_sha256.len() != 64
    {
        return Err(invalid("invalid analysis request"));
    }
    Ok(())
}
fn run_id(value: &str) -> io::Result<()> {
    validate_snapshot_id(value)?;
    if !value.starts_with("run-") {
        return Err(invalid("invalid analysis ID"));
    }
    Ok(())
}
fn read(path: &Path) -> io::Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(invalid("analysis file must be regular"));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(RECORD_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > RECORD_LIMIT {
        return Err(invalid("analysis record exceeds limit"));
    }
    Ok(bytes)
}
fn load<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&read(path)?).map_err(|_| invalid("invalid analysis record"))
}
fn publish(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| invalid("missing analysis directory"))?;
    require_dir(directory)?;
    let mut staged = tempfile::NamedTempFile::new_in(directory)?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;
    staged.persist_noclobber(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(directory)?.sync_all()?;
    Ok(())
}
fn encode(value: &impl Serialize, limit: usize) -> io::Result<Vec<u8>> {
    struct Bounded {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit - self.bytes.len() {
                return Err(invalid("analysis size limit exceeded"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut output, value)
        .map_err(|_| invalid("analysis serialization or size limit failed"))?;
    Ok(output.bytes)
}
fn private_dir(path: &Path) -> io::Result<PathBuf> {
    match fs::create_dir(path) {
        Ok(()) => {
            set_private_directory(path)?;
            #[cfg(unix)]
            if let Some(parent) = path.parent() {
                File::open(parent)?.sync_all()?;
            }
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    require_dir(path)?;
    Ok(path.into())
}
fn set_private_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn require_dir(path: &Path) -> io::Result<()> {
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(invalid("analysis path must be a directory, not a symlink"));
    }
    Ok(())
}
fn check_cancel(cancelled: &impl Fn() -> bool) -> io::Result<()> {
    if cancelled() {
        Err(crate::cancelled())
    } else {
        Ok(())
    }
}
fn conflict() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "Project changed; review again before committing analysis",
    )
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
