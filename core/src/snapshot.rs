//! Offline, immutable snapshots of one explicitly selected conversation.
//!
//! A full archive is streamed one chat at a time. Only the selected chat is
//! staged, and it is published after the entire JSON document passes validation.
//! No attachment bytes, account credentials, or client session files are read.
//! Snapshots contain private, unsanitized data; they are not agent workspaces.

mod metadata;
mod telegram;

pub use metadata::{
    CoverageGap, DeletionState, IdentityQuality, MessageMetadata, MessageProvenance,
    RecordLocatorKind, SnapshotMetadata, TimeRange, TimestampInfo, TimezoneStatus, UtcInstant,
};

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use crate::connector::{
    ArchiveImporter, ConnectorDescriptor, ConversationObservation, TelegramJson,
};
use serde::{Deserialize, Serialize};

use crate::{stream_chats, RawChat};

const SCHEMA_VERSION: u32 = 2;

/// Local namespace supplied by the user/project, not inferred from chat names.
/// A per-chat Telegram archive does not prove which account exported it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceScope {
    pub platform: String,
    pub account_local_id: String,
    pub conversation_id: String,
}

impl SourceScope {
    pub fn telegram(
        account_local_id: impl Into<String>,
        conversation_id: impl Into<String>,
    ) -> Self {
        Self {
            platform: "telegram".into(),
            account_local_id: account_local_id.into(),
            conversation_id: conversation_id.into(),
        }
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        for part in [
            &self.platform,
            &self.account_local_id,
            &self.conversation_id,
        ] {
            if part.trim().is_empty() || part.chars().any(char::is_control) {
                return Err(invalid(
                    "source identity must be nonempty and contain no control characters",
                ));
            }
        }
        Ok(())
    }
}

/// Stable native identity. Different accounts/chats may reuse a message ID.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MessageKey {
    pub source: SourceScope,
    pub message_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageLevel {
    Complete,
    Partial,
    OwnMessagesOnly,
    FutureOnly,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    pub level: CoverageLevel,
    pub reason: String,
    #[serde(default)]
    pub range: Option<TimeRange>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub known_gaps: Vec<CoverageGap>,
}

impl Coverage {
    pub fn unknown(reason: impl Into<String>) -> Self {
        Self {
            level: CoverageLevel::Unknown,
            reason: reason.into(),
            range: None,
            evidence: Vec::new(),
            known_gaps: Vec::new(),
        }
    }
}

/// Relative paths are references only; existence and safety for copying have
/// not been established. The future bundle packager must validate them anew.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub relative_path: Option<String>,
    pub original_name: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<u64>,
    pub media_type: String,
    pub availability: AttachmentAvailability,
    /// Native attachment identity, if the format exposes it.
    #[serde(default)]
    pub source_attachment_id: Option<String>,
    /// Only populated after hashing attachment bytes, never from its filename.
    #[serde(default)]
    pub content_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentAvailability {
    UnverifiedReference,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalMessage {
    pub key: MessageKey,
    /// Original exported timestamp; no implicit local-time to UTC conversion.
    pub timestamp: Option<String>,
    pub timestamp_unix: Option<String>,
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub text: String,
    pub reply_to: Option<String>,
    /// Resolved Telegram forum topic, or None when it cannot be established.
    pub thread_id: Option<String>,
    pub edited_at: Option<String>,
    pub edited_unix: Option<String>,
    pub is_service: bool,
    pub service_action: Option<String>,
    pub service_title: Option<String>,
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub metadata: Option<MessageMetadata>,
}

impl CanonicalMessage {
    /// Construct an observed record; normalization fills optional source fields.
    /// Store publication computes metadata and verifies its scope/identity.
    pub fn new(key: MessageKey, text: impl Into<String>) -> Self {
        Self {
            key,
            text: text.into(),
            timestamp: None,
            timestamp_unix: None,
            sender_id: None,
            sender_name: None,
            reply_to: None,
            thread_id: None,
            edited_at: None,
            edited_unix: None,
            is_service: false,
            service_action: None,
            service_title: None,
            attachments: Vec::new(),
            metadata: None,
        }
    }
}

/// Immutable observation, not an assertion that all historical messages exist.
/// Message keys plus snapshot_id reference the exact version stored here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub source: SourceScope,
    pub conversation_title: Option<String>,
    pub conversation_kind: String,
    pub coverage: Coverage,
    pub messages: Vec<CanonicalMessage>,
    #[serde(default)]
    pub metadata: Option<SnapshotMetadata>,
}

impl Snapshot {
    fn validate(&self) -> io::Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(invalid("unsupported snapshot schema"));
        }
        validate_snapshot_id(&self.snapshot_id)?;
        self.source.validate()?;
        let mut ids = BTreeSet::new();
        for message in &self.messages {
            if message.key.source != self.source
                || message.key.message_id.trim().is_empty()
                || message.key.message_id.chars().any(char::is_control)
                || !ids.insert(&message.key.message_id)
            {
                return Err(invalid(
                    "invalid, duplicate or out-of-scope message identity",
                ));
            }
        }
        self.validate_metadata()
    }

    /// Compare observations of the same source. Missing records are never
    /// reported as deletions, including when a caller declares full coverage.
    /// Lists are sorted by native ID as strings; unchanged is a count.
    pub fn diff(&self, previous: &Self) -> io::Result<SnapshotDiff> {
        self.validate()?;
        previous.validate()?;
        if self.source != previous.source {
            return Err(invalid("cannot diff different source namespaces"));
        }
        if self.snapshot_id != previous.snapshot_id
            && self.messages.iter().chain(&previous.messages).any(|m| {
                m.metadata
                    .as_ref()
                    .is_some_and(|meta| meta.identity_quality == IdentityQuality::SnapshotLocal)
            })
        {
            return Err(invalid(
                "snapshot-local identities require an explicit cross-snapshot matching strategy",
            ));
        }
        let old: BTreeMap<_, _> = previous.messages.iter().map(|m| (&m.key, m)).collect();
        let new: BTreeMap<_, _> = self.messages.iter().map(|m| (&m.key, m)).collect();
        let mut diff = SnapshotDiff::default();
        for (key, message) in &new {
            match old.get(key) {
                Some(before)
                    if before.metadata.as_ref().map(|m| &m.revision_id)
                        == message.metadata.as_ref().map(|m| &m.revision_id) =>
                {
                    diff.unchanged += 1
                }
                _ if message
                    .metadata
                    .as_ref()
                    .is_some_and(|meta| meta.deletion_state == DeletionState::Deleted) =>
                {
                    diff.deleted.push((*key).clone())
                }
                None => diff.created.push((*key).clone()),
                Some(_) => diff.edited.push((*key).clone()),
            }
        }
        diff.missing = old
            .keys()
            .filter(|key| !new.contains_key(*key))
            .map(|key| (*key).clone())
            .collect();
        Ok(diff)
    }
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotDiff {
    pub created: Vec<MessageKey>,
    pub edited: Vec<MessageKey>,
    /// Only explicit source tombstones, never inferred from archive absence.
    #[serde(default)]
    pub deleted: Vec<MessageKey>,
    pub missing: Vec<MessageKey>,
    pub unchanged: usize,
}

/// A private directory of immutable `<snapshot_id>.json` files. IDs are local
/// portable names, never source-provided paths. Callers supply unique run IDs.
/// Publication exposes only a fully written file and refuses replacement.
/// On systems using link/unlink fallback, a crash can leave a staging name as
/// well as the published name. This is not a guarantee of directory-entry
/// durability across power loss, or protection against other local processes
/// modifying a user-owned directory. Keep the store outside shared temp roots.
pub struct SnapshotStore {
    root: PathBuf,
}

impl SnapshotStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn load(&self, snapshot_id: &str) -> io::Result<Snapshot> {
        let mut snapshot: Snapshot =
            serde_json::from_reader(BufReader::new(File::open(self.path(snapshot_id)?)?))
                .map_err(invalid)?;
        if snapshot.schema_version == 1 {
            // Pure in-memory upgrade. Legacy observations cannot acquire an
            // invented import time or a claim to raw source bytes.
            if snapshot.source.platform != "telegram" {
                return Err(invalid("unsupported legacy snapshot platform"));
            }
            snapshot.add_metadata(TelegramJson.descriptor(), true, &[])?;
        }
        snapshot.validate()?;
        if snapshot.snapshot_id != snapshot_id {
            return Err(invalid("snapshot filename/identity mismatch"));
        }
        Ok(snapshot)
    }

    /// Import one selected chat from either Telegram JSON shape. Every byte
    /// must parse, including data after the selected chat. Temporary files are
    /// cleaned up on errors; an existing snapshot is never changed. Account
    /// ownership and history completeness cannot be proven from this format.
    pub fn import_telegram<R: Read>(
        &self,
        snapshot_id: &str,
        source: &SourceScope,
        mut reader: R,
    ) -> io::Result<Snapshot> {
        self.import(&TelegramJson, snapshot_id, source, &mut reader)
    }

    /// Import a selected conversation through any installed file/local adapter.
    /// The adapter emits source facts; the store owns validation and publication.
    pub fn import(
        &self,
        importer: &dyn ArchiveImporter,
        snapshot_id: &str,
        source: &SourceScope,
        reader: &mut dyn Read,
    ) -> io::Result<Snapshot> {
        self.publish_with(importer.descriptor(), snapshot_id, source, |emit| {
            importer.normalize(reader, source, emit)?;
            let mut trailing = [0];
            if reader.read(&mut trailing)? != 0 {
                return Err(invalid("importer returned before consuming the source"));
            }
            Ok(())
        })
    }

    /// Publish one already acquired observation (for API/bot adapters). This
    /// method performs no network call and does not advance any API cursor.
    pub fn publish_observation(
        &self,
        descriptor: ConnectorDescriptor,
        snapshot_id: &str,
        source: &SourceScope,
        observation: ConversationObservation,
    ) -> io::Result<Snapshot> {
        self.publish_with(descriptor, snapshot_id, source, |emit| emit(observation))
    }

    fn publish_with(
        &self,
        descriptor: ConnectorDescriptor,
        snapshot_id: &str,
        source: &SourceScope,
        normalize: impl FnOnce(
            &mut dyn FnMut(ConversationObservation) -> io::Result<()>,
        ) -> io::Result<()>,
    ) -> io::Result<Snapshot> {
        source.validate()?;
        validate_snapshot_id(descriptor.id)?;
        if descriptor.revision.is_empty() || descriptor.format_id.is_empty() {
            return Err(invalid("connector revision and format must be explicit"));
        }
        if descriptor.platform != source.platform {
            return Err(invalid("connector platform does not match selected source"));
        }
        let destination = self.path(snapshot_id)?;
        fs::create_dir_all(&self.root)?;
        let mut staged = tempfile::NamedTempFile::new_in(&self.root)?;
        let mut found = false;
        let mut rejected = false;
        normalize(&mut |observation| {
            let result = (|| {
                if found {
                    return Err(invalid("selected conversation appears more than once"));
                }
                found = true;
                if &observation.source != source {
                    return Err(invalid("observation is outside selected source"));
                }
                let mut messages = Vec::with_capacity(observation.messages.len());
                let mut facts = Vec::with_capacity(observation.messages.len());
                for observed in observation.messages {
                    if !descriptor.capabilities.stable_message_ids
                        && observed.identity_quality == IdentityQuality::Native
                    {
                        return Err(invalid("connector cannot claim native message identity"));
                    }
                    facts.push((observed.identity_quality, observed.deletion_state));
                    messages.push(observed.message);
                }
                let mut snapshot = Snapshot {
                    schema_version: SCHEMA_VERSION,
                    snapshot_id: snapshot_id.into(),
                    source: observation.source,
                    conversation_title: observation.title,
                    conversation_kind: observation.kind,
                    coverage: observation.coverage,
                    messages,
                    metadata: None,
                };
                snapshot.add_metadata(descriptor, false, &facts)?;
                snapshot.validate()?;
                let mut writer = BufWriter::new(staged.as_file_mut());
                serde_json::to_writer(&mut writer, &snapshot).map_err(invalid)?;
                writer.flush()
            })();
            if result.is_err() {
                rejected = true;
            }
            result
        })?;
        if rejected {
            return Err(invalid("importer ignored a rejected observation"));
        }
        if !found {
            return Err(invalid("selected conversation not present in export"));
        }
        // Re-read the staged representation before publishing: the returned
        // object and the persisted bytes cross the same validation interface.
        let snapshot: Snapshot =
            serde_json::from_reader(BufReader::new(File::open(staged.path())?)).map_err(invalid)?;
        snapshot.validate()?;
        staged.as_file().sync_all()?;
        staged.persist_noclobber(destination).map_err(|e| e.error)?;
        Ok(snapshot)
    }

    fn path(&self, id: &str) -> io::Result<PathBuf> {
        validate_snapshot_id(id)?;
        Ok(self.root.join(format!("{id}.json")))
    }

    pub fn directory(&self) -> &Path {
        &self.root
    }
}

pub(crate) fn normalize_telegram(
    reader: &mut dyn Read,
    source: &SourceScope,
    emit: &mut dyn FnMut(ConversationObservation) -> io::Result<()>,
) -> io::Result<()> {
    if source.platform != "telegram" {
        return Err(invalid("Telegram importer requires a Telegram source"));
    }
    let mut result = Ok(());
    stream_chats(reader, |chat: RawChat<telegram::Record>| {
        if chat.id == source.conversation_id && result.is_ok() {
            result = telegram::normalize(source, chat).and_then(&mut *emit);
        }
        ControlFlow::Continue(())
    })?;
    result
}

pub(crate) fn validate_snapshot_id(id: &str) -> io::Result<()> {
    if id.is_empty()
        || id.len() > 100
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(invalid(
            "snapshot ID must contain 1–100 ASCII letters, digits, underscores or hyphens",
        ));
    }
    // These device names remain reserved with an extension on Windows.
    let upper = id.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(invalid("snapshot ID is a reserved Windows device name"));
    }
    Ok(())
}

fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
