//! Bounded journal pages -> immutable canonical observations -> Project CAS.
//! The journal acknowledges only a durably published Project frontier. Original
//! messages/media remain in snapshots; no client action or external handoff.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use super::capture::{CaptureConfig, CaptureError};
use super::continuous::{AppliedCheckpoint, ContinuousCapture, JournalEvent, JournalStatus};
use super::parser::{MessageKind, ParsedEvent, PeerKind, TypedPeer};
use super::settings::{ContinuousPlan, ProjectionCheckpoint};
use crate::connector::{
    AttachmentAccess, ConnectorCapabilities, ConnectorDescriptor, ConnectorKind,
    ConversationObservation, CredentialKind, MessageObservation, RefreshMethod,
};
use crate::project::{Project, ProjectSource, ProjectStore};
use crate::snapshot::{
    CanonicalMessage, CoverageGap, CoverageLevel, DeletionState, IdentityQuality, MessageKey,
    Snapshot, SnapshotStore,
};

pub(crate) const DESCRIPTOR: ConnectorDescriptor = ConnectorDescriptor {
    id: "telegram_debug_observation",
    revision: concat!(env!("CARGO_PKG_VERSION"), ":desktop-7.2.5-project-1"),
    platform: "telegram",
    kind: ConnectorKind::LocalData,
    format_id: "telegram_desktop_debug_7_2_5",
    capabilities: ConnectorCapabilities {
        history: false,
        refresh: RefreshMethod::Events,
        stable_message_ids: true,
        attachments: AttachmentAccess::ReferencesOnly,
        credentials: CredentialKind::None,
    },
};

impl ProjectStore {
    /// Open the saved journal inside this private Project, never at a UI-supplied
    /// output path. Stop keeps both the binding and its previously applied data.
    pub fn open_telegram_capture(
        &self,
        project_id: &str,
        source_id: &str,
    ) -> io::Result<ContinuousCapture> {
        let project = self.open(project_id)?;
        let (source, plan) = binding(&project, source_id)?;
        let parent = self
            .directory(project_id)?
            .canonicalize()?
            .join("telegram-observations");
        match fs::create_dir(&parent) {
            Ok(()) => {
                fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))?;
                fs::File::open(&parent)?.sync_all()?;
                fs::File::open(self.directory(project_id)?)?.sync_all()?;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        let mut config = CaptureConfig::new(
            plan.settings.input_directory.clone(),
            parent.join(&plan.settings.generation),
            source.scope.account_local_id.clone(),
            plan.settings.peer.clone(),
        );
        config.confirmed_single_account = plan.settings.confirmed_single_account;
        config.self_user_id = plan.settings.self_user_id.clone();
        ContinuousCapture::open(config).map_err(capture_error)
    }

    /// Apply at most one journal page. The worker repeats while events remain;
    /// snapshot normalization holds one selected conversation, not the journal.
    pub fn apply_telegram_observations(
        &self,
        project_id: &str,
        source_id: &str,
        capture: &mut ContinuousCapture,
    ) -> io::Result<Project> {
        self.apply_telegram_with(project_id, source_id, capture, |_| Ok(()), |_| Ok(()))
    }

    fn apply_telegram_with(
        &self,
        project_id: &str,
        source_id: &str,
        capture: &mut ContinuousCapture,
        before_publication: impl FnOnce(&Project) -> io::Result<()>,
        after_publication: impl FnOnce(&Project) -> io::Result<()>,
    ) -> io::Result<Project> {
        let project = self.open(project_id)?;
        let (source, plan) = binding(&project, source_id)?;
        validate_capture(self, &project, source, plan, capture)?;
        let status = capture.status().map_err(capture_error)?;
        recover_ack(plan, &status, capture, || {
            self.sync_telegram_frontier(&project, source_id)
        })?;
        let sequence = plan.checkpoint.as_ref().map_or(0, |c| c.sequence);
        let revision = plan
            .checkpoint
            .as_ref()
            .map_or(0, |c| c.observation_revision);
        if sequence == status.events && revision == status.observation_revision {
            return Ok(project);
        }
        let events = capture.events_after(sequence, 512).map_err(capture_error)?;
        let next_sequence = events.last().map_or(sequence, |e| e.sequence);
        // Gaps and the poll revision become current only after ALL of that
        // poll's message events have reached the Project. Partial pages must
        // not acknowledge a future frontier or use the journal's latest index.
        let next_revision = if next_sequence == status.events {
            status.observation_revision
        } else {
            revision
        };
        let previous_id = source
            .latest_snapshot_id
            .as_deref()
            .ok_or_else(|| invalid("missing continuous bootstrap"))?;
        let snapshot_id = format!(
            "debug-{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    &source.scope,
                    &plan.settings.generation,
                    previous_id,
                    next_sequence,
                    next_revision,
                ))
                .map_err(invalid)?
            )
        );
        let snapshots = self.snapshots(project_id)?;
        let previous = snapshots.load(previous_id)?;
        let observation = project_page(
            previous,
            &events,
            &plan.settings.peer,
            &status,
            next_sequence == status.events,
        )?;
        publish_or_reuse(&snapshots, &snapshot_id, observation)?;
        // Sync both the published file name and the snapshots directory entry
        // before a durable Project revision can refer to it.
        fs::File::open(snapshots.directory())?.sync_all()?;
        fs::File::open(self.directory(project_id)?)?.sync_all()?;
        let checkpoint = ProjectionCheckpoint {
            sequence: next_sequence,
            observation_revision: next_revision,
            snapshot_id,
        };
        before_publication(&project)?;
        let published = self.publish_telegram_observation(
            project_id,
            project.revision,
            source_id,
            checkpoint.clone(),
        )?;
        after_publication(&published)?;
        self.sync_telegram_frontier(&published, source_id)?;
        capture
            .acknowledge(ack(&checkpoint))
            .map_err(capture_error)?;
        Ok(published)
    }

    fn sync_telegram_frontier(&self, project: &Project, source_id: &str) -> io::Result<()> {
        let plan = project
            .telegram_continuous
            .get(source_id)
            .ok_or_else(|| invalid("continuous source is disconnected"))?;
        let checkpoint = plan
            .checkpoint
            .as_ref()
            .ok_or_else(|| invalid("missing published frontier"))?;
        let source = project
            .sources
            .iter()
            .find(|s| s.source_id == source_id)
            .ok_or_else(|| invalid("continuous source is disconnected"))?;
        let snapshots = self.snapshots(&project.project_id)?;
        let snapshot = snapshots.load(&checkpoint.snapshot_id)?;
        if snapshot.source != source.scope
            || snapshot
                .metadata
                .as_ref()
                .is_none_or(|m| m.connector_id != DESCRIPTOR.id)
        {
            return Err(invalid(
                "recovered snapshot does not belong to the frontier",
            ));
        }
        let directory = self.directory(&project.project_id)?;
        let revisions = directory.join("revisions");
        if self.read_revision(&project.project_id, project.revision)? != *project {
            return Err(invalid(
                "recovered Project revision changed before acknowledgement",
            ));
        }
        // A rename can be visible after a process crash even if its directory
        // fsync failed. Redo both file and directory barriers before redb ack.
        fs::File::open(
            snapshots
                .directory()
                .join(format!("{}.json", checkpoint.snapshot_id)),
        )?
        .sync_all()?;
        fs::File::open(snapshots.directory())?.sync_all()?;
        fs::File::open(revisions.join(format!("{:020}.json", project.revision)))?.sync_all()?;
        fs::File::open(revisions)?.sync_all()?;
        fs::File::open(directory)?.sync_all()
    }

    pub fn telegram_journal_path(&self, project_id: &str, source_id: &str) -> io::Result<PathBuf> {
        let project = self.open(project_id)?;
        let plan = project
            .telegram_continuous
            .get(source_id)
            .ok_or_else(|| invalid("continuous source is disconnected"))?;
        Ok(self
            .directory(project_id)?
            .canonicalize()?
            .join("telegram-observations")
            .join(&plan.settings.generation)
            .join("observations.redb"))
    }
}

fn binding<'a>(
    project: &'a Project,
    source_id: &str,
) -> io::Result<(&'a ProjectSource, &'a ContinuousPlan)> {
    let source = project
        .sources
        .iter()
        .find(|s| s.source_id == source_id)
        .ok_or_else(|| invalid("continuous source is disconnected"))?;
    let plan = project
        .telegram_continuous
        .get(source_id)
        .ok_or_else(|| invalid("continuous source is not configured"))?;
    if !plan.settings.enabled {
        return Err(invalid("continuous source is stopped"));
    }
    plan.validate(source)?;
    Ok((source, plan))
}

fn validate_capture(
    store: &ProjectStore,
    project: &Project,
    source: &ProjectSource,
    plan: &ContinuousPlan,
    capture: &ContinuousCapture,
) -> io::Result<()> {
    let expected_path = store.telegram_journal_path(&project.project_id, &source.source_id)?;
    let input = fs::canonicalize(&plan.settings.input_directory)?;
    let expected_hash = format!("{:x}", Sha256::digest(input.as_os_str().as_encoded_bytes()));
    let actual = capture.binding();
    if capture.database_path() != expected_path
        || actual.account_namespace != source.scope.account_local_id
        || actual.peer != plan.settings.peer
        || actual.self_user_id != plan.settings.self_user_id
        || actual.input_directory_sha256 != expected_hash
        || actual.client_version != "7.2.5"
    {
        return Err(invalid("journal does not match the saved Project source"));
    }
    Ok(())
}

fn recover_ack(
    plan: &ContinuousPlan,
    status: &JournalStatus,
    capture: &mut ContinuousCapture,
    before_ack: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    if let Some(checkpoint) = &plan.checkpoint {
        if checkpoint.sequence > status.events
            || checkpoint.observation_revision > status.observation_revision
        {
            return Err(invalid(
                "Project frontier is ahead of the recovered journal",
            ));
        }
        let expected = ack(checkpoint);
        if status.applied.as_ref() != Some(&expected) {
            before_ack()?;
            capture.acknowledge(expected).map_err(capture_error)?;
        }
    } else if status.applied.is_some() {
        return Err(invalid(
            "journal is already attached to a different Project frontier",
        ));
    }
    Ok(())
}

fn ack(c: &ProjectionCheckpoint) -> AppliedCheckpoint {
    AppliedCheckpoint {
        sequence: c.sequence,
        observation_revision: c.observation_revision,
        snapshot_id: c.snapshot_id.clone(),
    }
}

fn project_page(
    previous: Snapshot,
    events: &[JournalEvent],
    selected: &TypedPeer,
    status: &JournalStatus,
    include_gaps: bool,
) -> io::Result<ConversationObservation> {
    let source = previous.source;
    // The JSON importer only assigns thread IDs when it has evidence of a
    // forum. Telegram's default topic there is General (native root ID 1).
    let forum = previous
        .messages
        .iter()
        .any(|m| m.thread_id.is_some() || m.service_action.as_deref() == Some("topic_created"))
        || events.iter().any(|item| {
            matches!(&item.event,
                ParsedEvent::Message { details: Some(details), .. } if details.thread_id.is_some()
            )
        });
    let names: BTreeMap<_, _> = previous
        .messages
        .iter()
        .filter_map(|m| {
            m.sender_id
                .as_ref()
                .zip(m.sender_name.as_ref())
                .map(|(id, name)| (id.clone(), name.clone()))
        })
        .collect();
    let mut messages = previous.messages;
    let mut positions: BTreeMap<_, _> = messages
        .iter()
        .enumerate()
        .map(|(i, m)| (m.key.message_id.clone(), i))
        .collect();
    let mut deletions: Vec<_> = messages
        .iter()
        .map(|m| {
            m.metadata
                .as_ref()
                .map_or(DeletionState::Unknown, |meta| meta.deletion_state.clone())
        })
        .collect();
    for item in events {
        match &item.event {
            ParsedEvent::Message {
                kind,
                peer,
                message_id,
                sender,
                timestamp,
                text,
                details,
            } => {
                if peer != selected {
                    return Err(invalid("out-of-scope journal event"));
                }
                let index = if let Some(index) = positions.get(message_id) {
                    *index
                } else {
                    let index = messages.len();
                    messages.push(CanonicalMessage::new(
                        MessageKey {
                            source: source.clone(),
                            message_id: message_id.clone(),
                        },
                        "",
                    ));
                    deletions.push(DeletionState::Present);
                    positions.insert(message_id.clone(), index);
                    index
                };
                let message = &mut messages[index];
                let old_timestamp = message
                    .edited_unix
                    .as_deref()
                    .or(message.timestamp_unix.as_deref())
                    .and_then(|t| t.parse::<i64>().ok())
                    .or_else(|| {
                        message.metadata.as_ref().and_then(|m| {
                            m.edited_timestamp
                                .utc
                                .or(m.timestamp.utc)
                                .map(|t| t.seconds)
                        })
                    });
                if old_timestamp.is_some_and(|old| {
                    *timestamp < old || (*timestamp == old && *kind != MessageKind::Edit)
                }) {
                    continue;
                }
                message.text.clone_from(text);
                if let Some(sender) = sender {
                    let sender_id = format!(
                        "{}{}",
                        match sender.kind {
                            PeerKind::User => "user",
                            PeerKind::Chat => "chat",
                            PeerKind::Channel => "channel",
                        },
                        sender.id
                    );
                    message.sender_name = names.get(&sender_id).cloned();
                    message.sender_id = Some(sender_id);
                }
                if let Some(details) = details {
                    if message.timestamp_unix.is_none() {
                        message.timestamp_unix = Some(details.sent_timestamp.to_string());
                    }
                    if message.timestamp.is_none() {
                        message.timestamp = utc_text(details.sent_timestamp);
                    }
                    if let Some(edited) = details.edited_timestamp {
                        message.edited_at = utc_text(edited);
                        message.edited_unix = Some(edited.to_string());
                    } else if *kind == MessageKind::Edit {
                        message.edited_at = utc_text(*timestamp);
                        message.edited_unix = Some(timestamp.to_string());
                    }
                    if details.reply_to.is_some() {
                        message.reply_to.clone_from(&details.reply_to);
                    }
                    if details.thread_id.is_some() {
                        message.thread_id.clone_from(&details.thread_id);
                    }
                } else if *kind == MessageKind::Edit || old_timestamp.is_some() {
                    message.edited_at = utc_text(*timestamp);
                    message.edited_unix = Some(timestamp.to_string());
                } else if message.timestamp.is_none() {
                    message.timestamp_unix = Some(timestamp.to_string());
                    message.timestamp = utc_text(*timestamp);
                }
                if forum && message.thread_id.is_none() {
                    message.thread_id = Some("1".into());
                }
                // A transport message after a delete cannot resurrect a body.
            }
            ParsedEvent::Delete { peer, message_ids } => {
                if peer != selected {
                    return Err(invalid("out-of-scope journal deletion"));
                }
                for id in message_ids {
                    let index = if let Some(index) = positions.get(id) {
                        *index
                    } else {
                        let index = messages.len();
                        messages.push(CanonicalMessage::new(
                            MessageKey {
                                source: source.clone(),
                                message_id: id.clone(),
                            },
                            "",
                        ));
                        deletions.push(DeletionState::Unknown);
                        positions.insert(id.clone(), index);
                        index
                    };
                    deletions[index] = DeletionState::Deleted;
                }
            }
        }
    }
    let mut coverage = previous.coverage;
    coverage.level = CoverageLevel::Partial;
    coverage.reason = "Imported history plus selected Telegram diagnostic observations; transport records do not prove accepted updates or complete history".into();
    if !coverage
        .evidence
        .iter()
        .any(|e| e == "telegram_desktop_debug_7_2_5")
    {
        coverage
            .evidence
            .push("telegram_desktop_debug_7_2_5".into());
    }
    if include_gaps {
        for gap in &status.gaps {
            // Counts belong in local status. A recurring global unsupported
            // packet must not multiply the canonical coverage descriptions.
            let reason = format!("Telegram diagnostics: {:?}", gap.gap);
            if !coverage.known_gaps.iter().any(|g| g.reason == reason) {
                coverage.known_gaps.push(CoverageGap {
                    range: None,
                    reason,
                });
            }
        }
    }
    Ok(ConversationObservation {
        source,
        title: previous.conversation_title,
        kind: previous.conversation_kind,
        coverage,
        messages: messages
            .into_iter()
            .zip(deletions)
            .map(|(message, deletion_state)| {
                let identity_quality = message
                    .metadata
                    .as_ref()
                    .map_or(IdentityQuality::Native, |m| m.identity_quality.clone());
                MessageObservation {
                    message,
                    identity_quality,
                    deletion_state,
                }
            })
            .collect(),
    })
}

fn publish_or_reuse(
    store: &SnapshotStore,
    id: &str,
    observation: ConversationObservation,
) -> io::Result<()> {
    match store.load(id) {
        Ok(existing) => {
            if existing.source != observation.source
                || existing.conversation_title != observation.title
                || existing.conversation_kind != observation.kind
                || existing.coverage != observation.coverage
                || existing.messages.len() != observation.messages.len()
                || existing.metadata.as_ref().is_none_or(|m| {
                    m.connector_id != DESCRIPTOR.id || m.connector_revision != DESCRIPTOR.revision
                })
            {
                return Err(invalid(
                    "existing diagnostic snapshot conflicts with the replay",
                ));
            }
            for (mut stored, mut expected) in
                existing.messages.into_iter().zip(observation.messages)
            {
                let metadata = stored
                    .metadata
                    .take()
                    .ok_or_else(|| invalid("missing replay metadata"))?;
                expected.message.metadata = None;
                if stored != expected.message
                    || metadata.deletion_state != expected.deletion_state
                    || metadata.identity_quality != expected.identity_quality
                {
                    return Err(invalid(
                        "existing diagnostic snapshot content conflicts with the replay",
                    ));
                }
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let source = observation.source.clone();
            store
                .publish_observation(DESCRIPTOR, id, &source, observation)
                .map(|_| ())
        }
        Err(error) => Err(error),
    }
}

fn capture_error(error: CaptureError) -> io::Error {
    io::Error::new(
        if error == CaptureError::Busy {
            io::ErrorKind::WouldBlock
        } else {
            io::ErrorKind::Other
        },
        error,
    )
}

fn utc_text(timestamp: i64) -> Option<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0)
        .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn invalid(message: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
mod tests;
