use serde::Serialize;
use std::io;

use super::{invalid, AttachmentChoice, TextReference};
use crate::{project::ProjectStore, scope::select_messages};

const PAGE_ITEMS: usize = 50;
const MAX_ITEM_BYTES: usize = 8 * 1024;

/// Private local metadata for the explicit selection UI; never agent context.
#[derive(Serialize)]
pub struct AttachmentCandidate {
    pub choice: AttachmentChoice,
    pub eligible: bool,
    pub reason: Option<String>,
    pub selected: bool,
    pub selection_changed: bool,
}

#[derive(Serialize)]
pub struct AttachmentCatalog {
    pub project_revision: u64,
    pub offset: usize,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub items: Vec<AttachmentCandidate>,
}

impl ProjectStore {
    /// Catalog only the current selected message scope. This operation never
    /// opens the selected root, source attachment, archive or remote URL.
    pub fn attachment_catalog(
        &self,
        project_id: &str,
        expected_revision: u64,
        source_id: &str,
        offset: usize,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<AttachmentCatalog> {
        let project = self.open(project_id)?;
        if project.revision != expected_revision {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Project changed; reload attachment choices",
            ));
        }
        super::check_cancel(&cancelled)?;
        let source = project
            .sources
            .iter()
            .find(|s| s.source_id == source_id)
            .ok_or_else(|| invalid("source is not connected to this Project"))?;
        let mut result = AttachmentCatalog {
            project_revision: project.revision,
            offset,
            total: 0,
            next_offset: None,
            items: Vec::new(),
        };
        if !source.selection.enabled {
            return Ok(result);
        }
        let snapshots = self.snapshots(project_id)?;
        let id = source
            .latest_snapshot_id
            .as_deref()
            .ok_or_else(|| invalid("refresh source before selecting attachments"))?;
        let snapshot = snapshots.load(id)?;
        if snapshot.source != source.scope {
            return Err(invalid("attachment catalog source mismatch"));
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
            for (position, attachment) in message.attachments.iter().enumerate() {
                super::check_cancel(&cancelled)?;
                let ordinal = result.total;
                result.total += 1;
                if ordinal < offset || result.items.len() == PAGE_ITEMS {
                    continue;
                }
                let size = [
                    attachment.relative_path.as_deref(),
                    attachment.original_name.as_deref(),
                    attachment.mime_type.as_deref(),
                    attachment.source_attachment_id.as_deref(),
                    attachment.content_digest.as_deref(),
                    Some(attachment.media_type.as_str()),
                    Some(message.key.message_id.as_str()),
                ]
                .into_iter()
                .flatten()
                .fold(0usize, |total, s| total.saturating_add(s.len()));
                if size > MAX_ITEM_BYTES {
                    return Err(invalid("attachment metadata exceeds local catalog limit"));
                }
                let choice = AttachmentChoice {
                    message_id: message.key.message_id.clone(),
                    position,
                    expected: attachment.clone(),
                };
                let old = source.selection.attachments.as_ref().and_then(|selection| {
                    selection.files.iter().find(|c| {
                        c.message_id == choice.message_id && c.position == choice.position
                    })
                });
                let reason = TextReference::new(attachment).err().map(|e| e.to_string());
                result.items.push(AttachmentCandidate {
                    selected: old == Some(&choice),
                    selection_changed: old.is_some_and(|c| c != &choice),
                    eligible: reason.is_none(),
                    reason,
                    choice,
                });
            }
        }
        if offset > result.total {
            return Err(invalid(
                "attachment catalog offset is outside the current scope",
            ));
        }
        if offset + result.items.len() < result.total {
            result.next_offset = Some(offset + result.items.len());
        }
        // The source can be reconfigured while the snapshot is being read.
        if self.open(project_id)?.revision != expected_revision {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Project changed; reload attachment choices",
            ));
        }
        super::check_cancel(&cancelled)?;
        Ok(result)
    }
}
