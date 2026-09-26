//! Selected, sanitized context with a private evidence index. Export only never
//! launches an agent or advances a successful-analysis baseline.

mod files;

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::project::{AnalysisInput, ProjectStore};
use crate::sanitize::{sanitize, FindingAction, ReviewPolicy, SecretRule, RULES_VERSION};
use crate::scope::{select_messages, DateRange, ScopeStats};
use crate::snapshot::{CanonicalMessage, CoverageLevel, MessageKey};
use files::{
    check_cancel, invalid, load_json, private_dir, read_regular, require_dir, write_json,
    EvidenceKey, MarkdownWriter,
};

const PREVIEW_BYTES: usize = 24 * 1024;
const INDEX_LINE_BYTES: u64 = 4 * 1024 * 1024;
const REVIEW_FINDINGS: usize = 20;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct BundleOptions {
    pub redact_candidates: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl ProjectStore {
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
        let mut scan = ScanReview::new(options);
        manifest.project_title = scan.clean(&project.name, "project_title", None)?;
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
            )?;
            let platform = scan.clean(&source.scope.platform, "platform", None)?;
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
                let sender = scan.clean(
                    message.sender_name.as_deref().unwrap_or("Unknown sender"),
                    "sender",
                    Some(reference),
                )?;
                let timestamp = scan.clean(
                    message.timestamp.as_deref().unwrap_or("Unknown date"),
                    "timestamp",
                    Some(reference),
                )?;
                block.push_str(&format!(
                    "Sender: {}\n\nDate: {}\n\n",
                    inline(&sender),
                    inline(&timestamp)
                ));
                if let Some(edited) = &message.edited_at {
                    let edited = scan.clean(edited, "edited_at", Some(reference))?;
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
                    )?;
                    block.push_str(&format!("Service: {}\n\n", inline(&action)));
                    if let Some(title) = &message.service_title {
                        let title = scan.clean(title, "service_title", Some(reference))?;
                        block.push_str(&format!("Service title: {}\n\n", inline(&title)));
                    }
                }
                let text = scan.clean(&message.text, "text", Some(reference))?;
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
        let omitted_findings = scan
            .summary
            .needs_review
            .saturating_sub(scan.findings.len());
        manifest.privacy = scan.summary;
        index.flush()?;
        index.get_ref().sync_all()?;
        drop(index);
        write_json(&public.join("manifest.json"), &manifest)?;
        let private = PrivateBundle {
            schema_version: 1,
            project_id: project_id.into(),
            project_revision: expected_revision,
            manifest_sha256: files::digest_file(&public.join("manifest.json"), &cancelled)?,
            index_sha256: files::digest_file(&staging.path().join("evidence.jsonl"), &cancelled)?,
            inputs,
        };
        write_json(&staging.path().join("private.json"), &private)?;
        check_cancel(&cancelled)?;
        check_revision(self.open(project_id)?.revision, expected_revision)?;
        let (preview, preview_truncated) = files::preview(&public, &manifest.files, PREVIEW_BYTES)?;
        let _retained = staging.keep();
        Ok(BundleReview {
            bundle_id,
            project_revision: expected_revision,
            manifest,
            preview,
            preview_truncated,
            findings: scan.findings,
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
        if private.schema_version != 1 || private.project_id != project_id {
            return Err(invalid("private bundle identity/version mismatch"));
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
        files::validate_files(&manifest.files)?;
        Ok((directory, private, manifest))
    }
}

struct ScanReview {
    policy: ReviewPolicy,
    summary: PrivacySummary,
    findings: Vec<ReviewFinding>,
}

impl ScanReview {
    fn new(options: BundleOptions) -> Self {
        Self {
            policy: if options.redact_candidates {
                ReviewPolicy::RedactCandidates
            } else {
                ReviewPolicy::KeepForReview
            },
            summary: PrivacySummary::default(),
            findings: Vec::new(),
        }
    }

    fn clean(
        &mut self,
        value: &str,
        field: &'static str,
        evidence: Option<&EvidenceRef>,
    ) -> io::Result<String> {
        let result = sanitize(value, self.policy)?;
        self.summary.redacted += result.report.redacted;
        self.summary.needs_review += result.report.needs_review;
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
            let start = result.text[..finding.output.start]
                .char_indices()
                .rev()
                .nth(48)
                .map_or(0, |(i, _)| i);
            let end = result.text[finding.output.start..]
                .char_indices()
                .nth(200)
                .map_or(result.text.len(), |(i, _)| finding.output.start + i);
            self.findings.push(ReviewFinding {
                field,
                rule: finding.rule,
                evidence: evidence.cloned(),
                excerpt: format!(
                    "{}{}{}",
                    if start > 0 { "…" } else { "" },
                    &result.text[start..end],
                    if end < result.text.len() { "…" } else { "" }
                ),
            });
        }
        Ok(result.text)
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
