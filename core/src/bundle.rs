//! Selected, sanitized context with a private evidence index. Export only never
//! launches an agent or advances a successful-analysis baseline.

mod files;

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::infrastructure::{InfrastructureCategory, InfrastructureDetector, InfrastructurePolicy};
use crate::pii::{PiiDetector, PiiPolicy, PiiSummary};
use crate::project::{AnalysisInput, Project, ProjectStore};
use crate::pseudonyms::{MappingDraft, MappingRef};
use crate::sanitize::{sanitize, FindingAction, ReviewPolicy, SecretRule, RULES_VERSION};
use crate::scope::{select_messages, DateRange, ScopeStats};
use crate::snapshot::{CanonicalMessage, CoverageLevel, MessageKey, SourceScope};
use files::{
    check_cancel, invalid, load_json, private_dir, read_regular, require_dir, write_json,
    EvidenceKey, MarkdownWriter,
};

const PREVIEW_BYTES: usize = 24 * 1024;
const INDEX_LINE_BYTES: u64 = 4 * 1024 * 1024;
const REVIEW_FINDINGS: usize = 20;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct BundleOptions {
    pub redact_candidates: bool,
    /// Private policy; names must never be copied into the public manifest.
    #[serde(default)]
    pub infrastructure: InfrastructurePolicy,
    #[serde(default)]
    pub pii: PiiPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub id: String,
    pub revision: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrivacySummary {
    pub redacted: usize,
    pub needs_review: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleSource {
    pub id: String,
    pub title: String,
    pub platform: String,
    pub coverage: CoverageLevel,
    pub known_gaps: usize,
    pub dates: Option<DateRange>,
    pub selected_topics: Option<usize>,
    pub only_changes: bool,
    pub stats: ScopeStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleFile {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
    pub schema_version: u32,
    /// Opaque version binding, not a claim that any pseudonym detector ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pseudonym_mapping_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub infrastructure: Option<InfrastructureSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pii: Option<PiiSummary>,
    pub sanitizer_version: String,
    pub destination: String,
    pub project_title: String,
    pub sources: Vec<BundleSource>,
    pub messages: usize,
    pub attachment_references: usize,
    pub included_attachments: usize,
    pub privacy: PrivacySummary,
    pub files: Vec<BundleFile>,
}

/// Public execution metadata contains categories/counts, never configured names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfrastructureSummary {
    pub rules_version: String,
    pub categories: std::collections::BTreeSet<InfrastructureCategory>,
    pub replacements: usize,
    pub by_category: BTreeMap<InfrastructureCategory, usize>,
}

#[derive(Serialize)]
pub struct ReviewFinding {
    pub field: &'static str,
    pub rule: SecretRule,
    pub evidence: Option<EvidenceRef>,
    /// Local review only. This can contain medium-confidence candidates.
    pub excerpt: String,
}

#[derive(Serialize)]
pub struct BundleReview {
    pub bundle_id: String,
    pub project_revision: u64,
    pub manifest: BundleManifest,
    pub preview: String,
    pub preview_truncated: bool,
    pub findings: Vec<ReviewFinding>,
    pub omitted_findings: usize,
}

#[derive(Debug, Serialize)]
pub struct BundleExport {
    pub directory: PathBuf,
    pub files: Vec<BundleFile>,
}

#[derive(Serialize, Deserialize)]
struct PrivateBundle {
    schema_version: u32,
    project_id: String,
    project_revision: u64,
    manifest_sha256: String,
    index_sha256: String,
    inputs: Vec<AnalysisInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pseudonyms: Option<crate::pseudonyms::MappingRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    infrastructure_policy: Option<InfrastructurePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pii_policy: Option<PiiPolicy>,
}

#[derive(Serialize, Deserialize)]
struct EvidenceEntry {
    reference: EvidenceRef,
    snapshot_id: String,
    message_key: MessageKey,
    source_revision: String,
}

pub struct ResolvedEvidence {
    pub snapshot_id: String,
    pub message: CanonicalMessage,
}

pub(crate) struct AnalysisBundle {
    pub revision: u64,
    pub manifest_sha256: String,
    pub inputs: Vec<AnalysisInput>,
    pub manifest: BundleManifest,
}

impl ProjectStore {
    pub(crate) fn analysis_bundle(
        &self,
        project_id: &str,
        bundle_id: &str,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<AnalysisBundle> {
        let (directory, private, manifest) =
            self.checked_bundle(project_id, bundle_id, cancelled)?;
        if manifest.privacy.needs_review != 0 {
            return Err(invalid("resolve privacy findings before analysis"));
        }
        for file in &manifest.files {
            check_cancel(cancelled)?;
            let path = directory.join("context").join(&file.name);
            if read_regular(&path)?.metadata()?.len() != file.bytes
                || files::digest_file(&path, cancelled)? != file.sha256
            {
                return Err(invalid("reviewed context file changed"));
            }
        }
        Ok(AnalysisBundle {
            revision: private.project_revision,
            manifest_sha256: private.manifest_sha256,
            inputs: private.inputs,
            manifest,
        })
    }

    pub(crate) fn check_analysis_evidence(
        &self,
        project_id: &str,
        bundle_id: &str,
        references: &[EvidenceRef],
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<()> {
        use std::collections::BTreeSet;
        use std::io::Read;
        if references.len() > 512
            || references
                .iter()
                .any(|r| r.id.len() > 128 || r.revision.len() > 128)
        {
            return Err(invalid(
                "analysis evidence exceeds count or identity limits",
            ));
        }
        let mut remaining: BTreeSet<_> = references
            .iter()
            .map(|r| (r.id.clone(), r.revision.clone()))
            .collect();
        if remaining.len() != references.len() {
            return Err(invalid("duplicate analysis evidence"));
        }
        let (directory, _, _) = self.checked_bundle(project_id, bundle_id, cancelled)?;
        let mut reader = BufReader::new(read_regular(&directory.join("evidence.jsonl"))?);
        loop {
            check_cancel(cancelled)?;
            let mut line = String::new();
            if reader
                .by_ref()
                .take(INDEX_LINE_BYTES + 1)
                .read_line(&mut line)?
                == 0
            {
                break;
            }
            if line.len() as u64 > INDEX_LINE_BYTES {
                return Err(invalid("evidence entry is too large"));
            }
            let entry: EvidenceEntry =
                serde_json::from_str(&line).map_err(|_| invalid("invalid evidence entry"))?;
            remaining.remove(&(entry.reference.id, entry.reference.revision));
        }
        if !remaining.is_empty() {
            return Err(invalid("analysis evidence is outside reviewed bundle"));
        }
        Ok(())
    }

    /// Prepare a private draft for local review. Every emitted data field is
    /// scanned before Markdown formatting; pending medium findings block export.
    pub fn prepare_bundle(
        &self,
        project_id: &str,
        expected_revision: u64,
        options: BundleOptions,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<BundleReview> {
        let project = self.open(project_id)?;
        check_revision(project.revision, expected_revision)?;
        check_cancel(&cancelled)?;
        let directory = self.directory(project_id)?;
        let mut scan = ScanReview::new(options, self, &project)?;
        scan.discover_participants(self, &project, &cancelled)?;
        let key = EvidenceKey::load_or_create(&directory)?;
        let drafts = private_dir(&directory.join("bundles"))?;
        let staging = tempfile::Builder::new()
            .prefix("bundle-")
            .tempdir_in(drafts)?;
        let bundle_id = staging
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| invalid("invalid bundle ID"))?
            .to_owned();
        let public = private_dir(&staging.path().join("context"))?;
        let mut index = BufWriter::new(files::create_private(
            &staging.path().join("evidence.jsonl"),
        )?);
        let mut manifest = BundleManifest {
            schema_version: 1,
            pseudonym_mapping_id: project.pseudonyms.as_ref().map(|r| r.id().to_owned()),
            infrastructure: None,
            pii: None,
            sanitizer_version: RULES_VERSION.into(),
            destination: "export_only".into(),
            project_title: String::new(),
            sources: Vec::new(),
            messages: 0,
            attachment_references: 0,
            included_attachments: 0,
            privacy: PrivacySummary::default(),
            files: Vec::new(),
        };
        manifest.project_title = scan.clean(&project.name, "project_title", None, None)?;
        let mut output = MarkdownWriter::new(&public, project.settings.max_tokens);
        let mut inputs = Vec::new();
        let snapshots = self.snapshots(project_id)?;
        for source in project.sources.iter().filter(|s| s.selection.enabled) {
            check_cancel(&cancelled)?;
            let snapshot_id = source
                .latest_snapshot_id
                .as_ref()
                .ok_or_else(|| invalid("refresh every selected source before preparing context"))?;
            let snapshot = snapshots.load(snapshot_id)?;
            if snapshot.source != source.scope {
                return Err(invalid("bundle source mismatch"));
            }
            let baseline = project
                .baselines
                .iter()
                .find(|b| b.source_id == source.source_id);
            let previous = baseline
                .map(|b| snapshots.load(&b.snapshot_id))
                .transpose()?;
            let selected = select_messages(
                &snapshot,
                &source.selection,
                previous.as_ref().zip(baseline.map(|b| &b.filter)),
            )?;
            let source_ref = key.opaque("source", &source.scope)?;
            let title = scan.clean(
                snapshot
                    .conversation_title
                    .as_deref()
                    .unwrap_or("Conversation"),
                "source_title",
                None,
                Some(&source.scope),
            )?;
            let platform = scan.clean(
                &source.scope.platform,
                "platform",
                None,
                Some(&source.scope),
            )?;
            // Replies may point only to messages actually included in this bundle.
            let references: BTreeMap<_, _> = selected
                .messages
                .iter()
                .map(|m| Ok((m.key.message_id.clone(), key.reference(m)?)))
                .collect::<io::Result<_>>()?;
            for message in &selected.messages {
                check_cancel(&cancelled)?;
                let reference = &references[&message.key.message_id];
                let mut block =
                    format!("\n## Evidence {}@{}\n\n", reference.id, reference.revision);
                block.push_str(&format!("Source: {} · {}\n\n", source_ref, inline(&title)));
                let sender = scan.clean_sender(message, reference)?;
                let timestamp = scan.clean(
                    message.timestamp.as_deref().unwrap_or("Unknown date"),
                    "timestamp",
                    Some(reference),
                    Some(&source.scope),
                )?;
                block.push_str(&format!(
                    "Sender: {}\n\nDate: {}\n\n",
                    inline(&sender),
                    inline(&timestamp)
                ));
                if let Some(edited) = &message.edited_at {
                    let edited =
                        scan.clean(edited, "edited_at", Some(reference), Some(&source.scope))?;
                    block.push_str(&format!("Edited: {}\n\n", inline(&edited)));
                }
                if message
                    .metadata
                    .as_ref()
                    .is_some_and(|m| m.deletion_state == crate::snapshot::DeletionState::Deleted)
                {
                    block.push_str("Message state: explicitly deleted.\n\n");
                }
                if let Some(thread) = &message.thread_id {
                    block.push_str(&format!(
                        "Thread: {}\n\n",
                        key.opaque("thread", &(&source.scope, thread))?
                    ));
                }
                if let Some(reply) = message.reply_to.as_ref() {
                    if let Some(target) = references.get(reply) {
                        block.push_str(&format!("Reply to: {}@{}\n\n", target.id, target.revision));
                    } else {
                        block.push_str("Reply target: outside this context or unresolved.\n\n");
                    }
                }
                if message.is_service {
                    let action = scan.clean(
                        message.service_action.as_deref().unwrap_or("service"),
                        "service_action",
                        Some(reference),
                        Some(&source.scope),
                    )?;
                    block.push_str(&format!("Service: {}\n\n", inline(&action)));
                    if let Some(title) = &message.service_title {
                        let title = scan.clean(
                            title,
                            "service_title",
                            Some(reference),
                            Some(&source.scope),
                        )?;
                        block.push_str(&format!("Service title: {}\n\n", inline(&title)));
                    }
                }
                let text =
                    scan.clean(&message.text, "text", Some(reference), Some(&source.scope))?;
                // Quote all source lines, including CR-only line endings. This
                // marks provenance; it is not a prompt-injection defense.
                for line in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
                    block.push_str("> ");
                    block.push_str(line);
                    block.push('\n');
                }
                for (position, _) in message.attachments.iter().enumerate() {
                    let attachment = key.opaque("attachment", &(&message.key, position))?;
                    block.push_str(&format!(
                        "\nAttachment reference: {attachment} (file not included).\n"
                    ));
                }
                manifest.attachment_references += message.attachments.len();
                output.write_block(&block)?;
                let entry = EvidenceEntry {
                    reference: reference.clone(),
                    snapshot_id: snapshot_id.clone(),
                    message_key: message.key.clone(),
                    source_revision: message
                        .metadata
                        .as_ref()
                        .ok_or_else(|| invalid("message revision is missing"))?
                        .revision_id
                        .clone(),
                };
                let bytes = serde_json::to_vec(&entry).map_err(invalid)?;
                if bytes.len() as u64 >= INDEX_LINE_BYTES {
                    return Err(invalid("evidence entry is too large"));
                }
                index.write_all(&bytes)?;
                index.write_all(b"\n")?;
                manifest.messages += 1;
            }
            manifest.sources.push(BundleSource {
                id: source_ref,
                title,
                platform,
                coverage: snapshot.coverage.level.clone(),
                known_gaps: snapshot.coverage.known_gaps.len(),
                dates: source.selection.filter.dates.clone(),
                selected_topics: source.selection.filter.topic_ids.as_ref().map(Vec::len),
                only_changes: source.selection.only_changes,
                stats: selected.stats,
            });
            inputs.push(AnalysisInput {
                source_id: source.source_id.clone(),
                source: source.scope.clone(),
                snapshot_id: snapshot_id.clone(),
                selection: source.selection.clone(),
                baseline: baseline.cloned(),
            });
        }
        if inputs.is_empty() {
            return Err(invalid(
                "select at least one source before preparing context",
            ));
        }
        if manifest.messages == 0 {
            return Err(invalid("no messages match the current selection"));
        }
        manifest.files = output.finish()?;
        index.flush()?;
        index.get_ref().sync_all()?;
        drop(index);
        check_cancel(&cancelled)?;
        let privacy = scan.finish(self, &project)?;
        let changed_mapping = privacy.reference != project.pseudonyms;
        let reviewed_revision = if changed_mapping {
            expected_revision
                .checked_add(1)
                .ok_or_else(|| invalid("project revision exhausted"))?
        } else {
            expected_revision
        };
        let omitted_findings = privacy
            .summary
            .needs_review
            .saturating_sub(privacy.findings.len());
        manifest.privacy = privacy.summary;
        manifest.infrastructure = privacy.infrastructure;
        manifest.pii = privacy.pii;
        manifest.pseudonym_mapping_id = privacy.reference.as_ref().map(|r| r.id().to_owned());
        write_json(&public.join("manifest.json"), &manifest)?;
        let private = PrivateBundle {
            schema_version: 4,
            project_id: project_id.into(),
            project_revision: reviewed_revision,
            manifest_sha256: files::digest_file(&public.join("manifest.json"), &cancelled)?,
            index_sha256: files::digest_file(&staging.path().join("evidence.jsonl"), &cancelled)?,
            inputs,
            pseudonyms: privacy.reference.clone(),
            infrastructure_policy: privacy.policy,
            pii_policy: privacy.pii_policy,
        };
        write_json(&staging.path().join("private.json"), &private)?;
        let (preview, preview_truncated) = files::preview(&public, &manifest.files, PREVIEW_BYTES)?;
        // Nothing fallible follows a successful Project publication. Until this
        // point, errors/cancellation discard the bundle and leave the pointer.
        check_cancel(&cancelled)?;
        if changed_mapping {
            self.publish_pseudonyms(
                project_id,
                expected_revision,
                privacy.reference.expect("changed mapping exists"),
            )?;
        } else {
            check_revision(self.open(project_id)?.revision, expected_revision)?;
        }
        let _retained = staging.keep();
        Ok(BundleReview {
            bundle_id,
            project_revision: reviewed_revision,
            manifest,
            preview,
            preview_truncated,
            findings: privacy.findings,
            omitted_findings,
        })
    }

    /// Copy only validated public files to a new folder. A draft with pending
    /// review findings, changed settings, corrupt files, or cancellation fails.
    /// The manifest is written last; failed export folders are removed.
    pub fn export_bundle(
        &self,
        project_id: &str,
        bundle_id: &str,
        expected_revision: u64,
        destination: &Path,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<BundleExport> {
        let (directory, private, manifest) =
            self.checked_bundle(project_id, bundle_id, &cancelled)?;
        check_revision(private.project_revision, expected_revision)?;
        check_revision(self.open(project_id)?.revision, expected_revision)?;
        if manifest.privacy.needs_review != 0 {
            return Err(invalid("resolve privacy findings before export"));
        }
        if !destination.is_absolute() {
            return Err(invalid("export destination must be absolute"));
        }
        fs::create_dir_all(destination)?;
        let destination = fs::canonicalize(destination)?;
        let root = fs::canonicalize(
            self.directory(project_id)?
                .parent()
                .ok_or_else(|| invalid("missing project root"))?,
        )?;
        if destination.starts_with(root) {
            return Err(invalid("export must be outside private Project storage"));
        }
        let output = tempfile::Builder::new()
            .prefix("tgsum-context-")
            .tempdir_in(destination)?;
        let public = directory.join("context");
        for file in &manifest.files {
            check_cancel(&cancelled)?;
            files::copy_checked(
                &public.join(&file.name),
                &output.path().join(&file.name),
                file,
                &cancelled,
            )?;
        }
        check_revision(self.open(project_id)?.revision, expected_revision)?;
        check_cancel(&cancelled)?;
        write_json(&output.path().join("manifest.json"), &manifest)?;
        Ok(BundleExport {
            directory: output.keep(),
            files: manifest.files,
        })
    }

    /// Resolve a public reference only through this Project's private index and
    /// the exact immutable snapshot. Historical bundles survive later imports.
    pub fn resolve_evidence(
        &self,
        project_id: &str,
        bundle_id: &str,
        reference: &EvidenceRef,
    ) -> io::Result<ResolvedEvidence> {
        let (directory, _, _) = self.checked_bundle(project_id, bundle_id, &|| false)?;
        let key = EvidenceKey::load(&self.directory(project_id)?)?;
        let mut reader = BufReader::new(read_regular(&directory.join("evidence.jsonl"))?);
        loop {
            use std::io::Read;
            let mut line = String::new();
            if reader
                .by_ref()
                .take(INDEX_LINE_BYTES + 1)
                .read_line(&mut line)?
                == 0
            {
                break;
            }
            if line.len() as u64 > INDEX_LINE_BYTES {
                return Err(invalid("evidence entry is too large"));
            }
            let entry: EvidenceEntry = serde_json::from_str(&line).map_err(invalid)?;
            if &entry.reference != reference {
                continue;
            }
            let snapshot = self.snapshots(project_id)?.load(&entry.snapshot_id)?;
            let message = snapshot
                .messages
                .into_iter()
                .find(|m| m.key == entry.message_key)
                .ok_or_else(|| invalid("evidence message is missing"))?;
            if key.reference(&message)? != *reference
                || message
                    .metadata
                    .as_ref()
                    .is_none_or(|m| m.revision_id != entry.source_revision)
            {
                return Err(invalid("evidence revision mismatch"));
            }
            return Ok(ResolvedEvidence {
                snapshot_id: entry.snapshot_id,
                message,
            });
        }
        Err(invalid("evidence reference does not belong to this bundle"))
    }

    /// Resolve labels locally using this bundle's immutable mapping version.
    /// Legacy bundles without a mapping remain unbound after later allocation.
    pub fn bundle_pseudonyms(
        &self,
        project_id: &str,
        bundle_id: &str,
    ) -> io::Result<Option<crate::pseudonyms::PseudonymMapping>> {
        let (_, private, _) = self.checked_bundle(project_id, bundle_id, &|| false)?;
        private
            .pseudonyms
            .as_ref()
            .map(|r| self.load_pseudonyms(project_id, r))
            .transpose()
    }

    fn checked_bundle(
        &self,
        project_id: &str,
        bundle_id: &str,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<(PathBuf, PrivateBundle, BundleManifest)> {
        crate::snapshot::validate_snapshot_id(bundle_id)?;
        if !bundle_id.starts_with("bundle-") {
            return Err(invalid("invalid bundle ID"));
        }
        let root = self.directory(project_id)?.join("bundles");
        require_dir(&root)?;
        let directory = root.join(bundle_id);
        require_dir(&directory)?;
        let private: PrivateBundle = load_json(&directory.join("private.json"))?;
        if !matches!(private.schema_version, 1..=4) || private.project_id != project_id {
            return Err(invalid("private bundle identity/version mismatch"));
        }
        let project = self.read_revision(project_id, private.project_revision)?;
        if private.pseudonyms != project.pseudonyms {
            return Err(invalid("bundle private mapping changed"));
        }
        if let Some(reference) = &private.pseudonyms {
            if private.schema_version < 2 {
                return Err(invalid("legacy bundle cannot bind a private mapping"));
            }
            self.load_pseudonyms(project_id, reference)?;
        }
        let public = directory.join("context");
        require_dir(&public)?;
        if files::digest_file(&public.join("manifest.json"), cancelled)? != private.manifest_sha256
            || files::digest_file(&directory.join("evidence.jsonl"), cancelled)?
                != private.index_sha256
        {
            return Err(invalid("bundle manifest or evidence index changed"));
        }
        let manifest: BundleManifest = load_json(&public.join("manifest.json"))?;
        if manifest.schema_version != 1 || manifest.destination != "export_only" {
            return Err(invalid("unsupported bundle format"));
        }
        if manifest.pseudonym_mapping_id.as_deref() != private.pseudonyms.as_ref().map(|r| r.id()) {
            return Err(invalid("bundle public mapping reference changed"));
        }
        match (&private.infrastructure_policy, &manifest.infrastructure) {
            (None, None) => {}
            (Some(policy), Some(summary)) if private.schema_version >= 3 => {
                InfrastructureDetector::new(policy.clone())?;
                if summary.rules_version != "infrastructure/1"
                    || policy.categories.is_empty()
                    || summary.categories != policy.categories
                    || summary
                        .by_category
                        .keys()
                        .any(|c| !summary.categories.contains(c))
                    || summary
                        .by_category
                        .values()
                        .try_fold(0usize, |n, v| n.checked_add(*v))
                        != Some(summary.replacements)
                    || (summary.replacements > 0 && private.pseudonyms.is_none())
                {
                    return Err(invalid("bundle infrastructure metadata mismatch"));
                }
            }
            _ => return Err(invalid("bundle infrastructure policy mismatch")),
        }
        match (&private.pii_policy, &manifest.pii) {
            (None, None) => {}
            (Some(policy), Some(summary)) if private.schema_version >= 4 => {
                let generic = summary.ambiguous.checked_add(summary.unresolved);
                if summary.rules_version != "pii/1"
                    || policy.categories.is_empty()
                    || summary.categories != policy.categories
                    || summary
                        .by_category
                        .keys()
                        .any(|c| !summary.categories.contains(c))
                    || summary
                        .by_category
                        .values()
                        .try_fold(0usize, |n, v| n.checked_add(*v))
                        != Some(summary.replacements)
                    || generic.is_none_or(|n| n > summary.replacements)
                    || (summary.replacements > generic.unwrap_or(0) && private.pseudonyms.is_none())
                {
                    return Err(invalid("bundle PII metadata mismatch"));
                }
            }
            _ => return Err(invalid("bundle PII policy mismatch")),
        }
        files::validate_files(&manifest.files)?;
        Ok((directory, private, manifest))
    }
}

struct ScanReview {
    policy: ReviewPolicy,
    summary: PrivacySummary,
    findings: Vec<ReviewFinding>,
    infrastructure: Option<InfrastructureReview>,
    pii: Option<PiiReview>,
    mapping: Option<MappingDraft>,
}

struct InfrastructureReview {
    policy: InfrastructurePolicy,
    detector: InfrastructureDetector,
    summary: InfrastructureSummary,
}

struct PiiReview {
    policy: PiiPolicy,
    detector: PiiDetector,
    summary: PiiSummary,
}

struct PreparedPrivacy {
    summary: PrivacySummary,
    findings: Vec<ReviewFinding>,
    reference: Option<MappingRef>,
    infrastructure: Option<InfrastructureSummary>,
    policy: Option<InfrastructurePolicy>,
    pii: Option<PiiSummary>,
    pii_policy: Option<PiiPolicy>,
}

impl ScanReview {
    fn new(options: BundleOptions, store: &ProjectStore, project: &Project) -> io::Result<Self> {
        let detector = InfrastructureDetector::new(options.infrastructure.clone())?;
        let mapping =
            if options.infrastructure.categories.is_empty() && options.pii.categories.is_empty() {
                if let Some(reference) = &project.pseudonyms {
                    store.load_pseudonyms(&project.project_id, reference)?;
                }
                None
            } else {
                Some(store.draft_pseudonyms(project)?)
            };
        let infrastructure = if options.infrastructure.categories.is_empty() {
            None
        } else {
            Some(InfrastructureReview {
                summary: InfrastructureSummary {
                    rules_version: crate::infrastructure::RULES_VERSION.into(),
                    categories: options.infrastructure.categories.clone(),
                    replacements: 0,
                    by_category: BTreeMap::new(),
                },
                policy: options.infrastructure,
                detector,
            })
        };
        let pii = (!options.pii.categories.is_empty()).then(|| PiiReview {
            summary: PiiSummary::new(&options.pii),
            detector: PiiDetector::new(options.pii.clone()),
            policy: options.pii,
        });
        Ok(Self {
            policy: if options.redact_candidates {
                ReviewPolicy::RedactCandidates
            } else {
                ReviewPolicy::KeepForReview
            },
            summary: PrivacySummary::default(),
            findings: Vec::new(),
            infrastructure,
            pii,
            mapping,
        })
    }

    fn discover_participants(
        &mut self,
        store: &ProjectStore,
        project: &Project,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<()> {
        let Some(pii) = &mut self.pii else {
            return Ok(());
        };
        if !pii.detector.participants_enabled() {
            return Ok(());
        }
        let mapping = self.mapping.as_mut().expect("PII enables mapping");
        let snapshots = store.snapshots(&project.project_id)?;
        for source in project.sources.iter().filter(|s| s.selection.enabled) {
            check_cancel(cancelled)?;
            let id = source
                .latest_snapshot_id
                .as_ref()
                .ok_or_else(|| invalid("refresh every selected source before preparing context"))?;
            let snapshot = snapshots.load(id)?;
            if snapshot.source != source.scope {
                return Err(invalid("bundle source mismatch"));
            }
            let baseline = project
                .baselines
                .iter()
                .find(|b| b.source_id == source.source_id);
            let previous = baseline
                .map(|b| snapshots.load(&b.snapshot_id))
                .transpose()?;
            let selected = select_messages(
                &snapshot,
                &source.selection,
                previous.as_ref().zip(baseline.map(|b| &b.filter)),
            )?;
            for message in selected.messages {
                check_cancel(cancelled)?;
                pii.detector.observe(message, mapping, self.policy)?;
            }
        }
        pii.detector.finish_discovery(mapping.mapping())
    }

    fn finish(self, store: &ProjectStore, project: &Project) -> io::Result<PreparedPrivacy> {
        let reference = match self.mapping {
            Some(mapping) => store.stage_pseudonyms(mapping)?,
            None => project.pseudonyms.clone(),
        };
        let (infrastructure, policy) = if let Some(infrastructure) = self.infrastructure {
            (Some(infrastructure.summary), Some(infrastructure.policy))
        } else {
            (None, None)
        };
        let (pii, pii_policy) = self
            .pii
            .map(|p| (Some(p.summary), Some(p.policy)))
            .unwrap_or_default();
        Ok(PreparedPrivacy {
            summary: self.summary,
            findings: self.findings,
            reference,
            infrastructure,
            policy,
            pii,
            pii_policy,
        })
    }

    fn clean(
        &mut self,
        value: &str,
        field: &'static str,
        evidence: Option<&EvidenceRef>,
        scope: Option<&SourceScope>,
    ) -> io::Result<String> {
        self.clean_field(value, field, evidence, None, scope)
    }

    fn clean_sender(
        &mut self,
        message: &CanonicalMessage,
        reference: &EvidenceRef,
    ) -> io::Result<String> {
        self.clean_field(
            message.sender_name.as_deref().unwrap_or("Unknown sender"),
            "sender",
            Some(reference),
            Some(message),
            Some(&message.key.source),
        )
    }

    fn clean_field(
        &mut self,
        value: &str,
        field: &'static str,
        evidence: Option<&EvidenceRef>,
        sender: Option<&CanonicalMessage>,
        scope: Option<&SourceScope>,
    ) -> io::Result<String> {
        let result = sanitize(value, self.policy)?;
        self.summary.redacted += result.report.redacted;
        self.summary.needs_review += result.report.needs_review;
        let mut text = result.text;
        let mut pii_shifts = Vec::new();
        if let Some(pii) = &mut self.pii {
            let mapping = self.mapping.as_mut().expect("PII enables mapping");
            let output = pii.detector.replace(&text, sender, scope, mapping)?;
            pii.summary.record(&output.findings);
            text = output.text;
            pii_shifts = output.findings;
        }
        let mut shifts = Vec::new();
        if let Some(infrastructure) = &mut self.infrastructure {
            let mapping = self
                .mapping
                .as_mut()
                .expect("infrastructure enables mapping");
            let scan = infrastructure.detector.scan(&text)?;
            mapping.allocate(&scan.inputs())?;
            let output = scan.apply(mapping.mapping())?;
            for finding in &output.findings {
                infrastructure.summary.replacements += 1;
                *infrastructure
                    .summary
                    .by_category
                    .entry(finding.category)
                    .or_default() += 1;
            }
            text = output.text;
            shifts = output.findings;
        }
        for finding in result
            .report
            .findings
            .iter()
            .filter(|f| f.action == FindingAction::NeedsReview)
        {
            if self.findings.len() == REVIEW_FINDINGS {
                break;
            }
            // Slice sanitized UTF-8, never the raw input, so nearby high-confidence
            // values stay hidden. Bound both the number and size of excerpts.
            let position =
                transformed_position(&pii_shifts, finding.output.start, |f| (&f.input, &f.output));
            let position = transformed_position(&shifts, position, |f| (&f.input, &f.output));
            let start = text[..position]
                .char_indices()
                .rev()
                .nth(48)
                .map_or(0, |(i, _)| i);
            let end = text[position..]
                .char_indices()
                .nth(200)
                .map_or(text.len(), |(i, _)| position + i);
            self.findings.push(ReviewFinding {
                field,
                rule: finding.rule,
                evidence: evidence.cloned(),
                excerpt: format!(
                    "{}{}{}",
                    if start > 0 { "…" } else { "" },
                    &text[start..end],
                    if end < text.len() { "…" } else { "" }
                ),
            });
        }
        Ok(text)
    }
}

fn transformed_position<T>(
    findings: &[T],
    original: usize,
    ranges: impl Fn(&T) -> (&Range<usize>, &Range<usize>),
) -> usize {
    let index = findings.partition_point(|f| ranges(f).0.end <= original);
    if let Some(finding) = findings
        .get(index)
        .filter(|f| ranges(f).0.start <= original)
    {
        return ranges(finding).1.start;
    }
    if index == 0 {
        original
    } else {
        let preceding = &findings[index - 1];
        ranges(preceding).1.end + (original - ranges(preceding).0.end)
    }
}

fn inline(text: &str) -> String {
    // JSON quoting preserves controls/newlines without creating Markdown structure.
    serde_json::to_string(text).expect("strings serialize")
}

fn check_revision(actual: u64, expected: u64) -> io::Result<()> {
    if actual != expected {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "Project changed; prepare a new privacy review",
        ));
    }
    Ok(())
}
