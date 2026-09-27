//! Local-only before/after review. Original attachment bytes are returned only
//! on this explicit operation and only if they still match the captured digest.

use serde::Serialize;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

use super::{check_revision, files, EvidenceEntry, EvidenceRef, ProjectStore, INDEX_LINE_BYTES};
use crate::attachments::{ArchiveFiles, TextReference, MAX_FILE_BYTES};
use files::{check_cancel, invalid, read_regular};

const PAGE_ITEMS: usize = 50;
const TEXT_BYTES: usize = 24 * 1024;

#[derive(Serialize)]
pub struct ReviewItem {
    pub reference: EvidenceRef,
    pub kind: &'static str,
    pub label: String,
    pub source_id: String,
}

#[derive(Serialize)]
pub struct ReviewItems {
    pub offset: usize,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub items: Vec<ReviewItem>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BeforeState {
    Snapshot,
    VerifiedFile,
    FileChanged,
    FileUnavailable,
}

#[derive(Serialize)]
pub struct EvidencePreview {
    pub reference: EvidenceRef,
    pub before_state: BeforeState,
    pub before: Option<String>,
    pub before_truncated: bool,
    pub after: String,
    pub after_truncated: bool,
}

impl ProjectStore {
    pub fn review_items(
        &self,
        project_id: &str,
        bundle_id: &str,
        expected_revision: u64,
        offset: usize,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<ReviewItems> {
        let (directory, private, manifest) =
            self.checked_bundle(project_id, bundle_id, &cancelled)?;
        check_revision(private.project_revision, expected_revision)?;
        check_revision(self.open(project_id)?.revision, expected_revision)?;
        let total = manifest
            .messages
            .checked_add(manifest.included_attachments)
            .ok_or_else(|| invalid("review count overflow"))?;
        if offset > total {
            return Err(invalid("review page is outside this bundle"));
        }
        let key = files::EvidenceKey::load(&self.directory(project_id)?)?;
        let mut page = ReviewItems {
            offset,
            total,
            next_offset: None,
            items: Vec::new(),
        };
        let mut ordinal = 0;
        visit_index(&directory, &cancelled, |entry| {
            let skip = ordinal < offset;
            ordinal += 1;
            if skip {
                return Ok(true);
            }
            let (kind, label) = if let Some(attachment) = &entry.attachment {
                (
                    "attachment",
                    clip(
                        attachment
                            .expected
                            .original_name
                            .as_deref()
                            .or(attachment.expected.relative_path.as_deref())
                            .unwrap_or("Attachment"),
                        256,
                    )
                    .0,
                )
            } else {
                ("message", clip(&entry.message_key.message_id, 256).0)
            };
            page.items.push(ReviewItem {
                reference: entry.reference,
                kind,
                label,
                source_id: key.opaque("source", &entry.message_key.source)?,
            });
            Ok(page.items.len() < PAGE_ITEMS)
        })?;
        if offset + page.items.len() < total {
            if page.items.len() != PAGE_ITEMS {
                return Err(invalid("review index is incomplete"));
            }
            page.next_offset = Some(offset + page.items.len());
        }
        check_revision(self.open(project_id)?.revision, expected_revision)?;
        check_cancel(&cancelled)?;
        Ok(page)
    }

    /// Local Review only: raw `before` data must never join an agent request.
    /// `resolve_evidence` itself continues to resolve retained data without
    /// reopening mutable source files.
    pub fn preview_evidence(
        &self,
        project_id: &str,
        bundle_id: &str,
        expected_revision: u64,
        reference: &EvidenceRef,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<EvidencePreview> {
        if reference.id.len() > 128 || reference.revision.len() > 128 {
            return Err(invalid("invalid preview reference"));
        }
        let (directory, private, manifest) =
            self.checked_bundle(project_id, bundle_id, &cancelled)?;
        check_revision(private.project_revision, expected_revision)?;
        check_revision(self.open(project_id)?.revision, expected_revision)?;
        let mut found = None;
        visit_index(&directory, &cancelled, |entry| {
            if entry.reference == *reference {
                found = Some(entry);
                Ok(false)
            } else {
                Ok(true)
            }
        })?;
        let entry = found.ok_or_else(|| invalid("preview evidence is outside this bundle"))?;
        let resolved = self.resolve_evidence(project_id, bundle_id, reference)?;
        let name = entry
            .document
            .as_ref()
            .or_else(|| entry.attachment.as_ref().map(|a| &a.file.name))
            .ok_or_else(|| invalid("prepare a new bundle for message comparison"))?;
        let file = manifest
            .files
            .iter()
            .find(|file| &file.name == name)
            .ok_or_else(|| invalid("preview document is outside the manifest"))?;
        let path = directory.join("context").join(name);
        if read_regular(&path)?.metadata()?.len() != file.bytes
            || files::digest_file(&path, &cancelled)? != file.sha256
        {
            return Err(invalid("reviewed preview file changed"));
        }
        let (after, after_truncated) = quoted_preview(
            BufReader::new(read_regular(&path)?.take(file.bytes + 1)),
            reference,
            &cancelled,
        )?;
        let (before_state, before, before_truncated) = if let Some(attachment) = entry.attachment {
            let selected = private
                .inputs
                .iter()
                .filter(|i| i.source == entry.message_key.source)
                .filter_map(|i| i.selection.attachments.as_ref())
                .find(|s| {
                    s.files.iter().any(|c| {
                        c.message_id == entry.message_key.message_id
                            && c.position == attachment.position
                            && c.expected == attachment.expected
                    })
                })
                .ok_or_else(|| invalid("reviewed attachment selection is missing"))?;
            let original = TextReference::new(&attachment.expected).and_then(|reference| {
                ArchiveFiles::open(&selected.root)?.read(reference, MAX_FILE_BYTES, &cancelled)
            });
            match original {
                Ok(text)
                    if text.source_bytes == attachment.source_bytes
                        && text.sha256 == attachment.source_sha256 =>
                {
                    let (value, truncated) = clip(&text.text, TEXT_BYTES);
                    (BeforeState::VerifiedFile, Some(value), truncated)
                }
                Ok(_) => (BeforeState::FileChanged, None, false),
                Err(error) if crate::is_cancelled(&error) => return Err(error),
                Err(_) => (BeforeState::FileUnavailable, None, false),
            }
        } else {
            let (value, truncated) = clip(&resolved.message.text, TEXT_BYTES);
            (BeforeState::Snapshot, Some(value), truncated)
        };
        check_revision(self.open(project_id)?.revision, expected_revision)?;
        check_cancel(&cancelled)?;
        Ok(EvidencePreview {
            reference: reference.clone(),
            before_state,
            before,
            before_truncated,
            after,
            after_truncated,
        })
    }
}

fn visit_index(
    directory: &Path,
    cancelled: &impl Fn() -> bool,
    mut visit: impl FnMut(EvidenceEntry) -> io::Result<bool>,
) -> io::Result<()> {
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
            return Ok(());
        }
        if line.len() as u64 > INDEX_LINE_BYTES {
            return Err(invalid("evidence entry is too large"));
        }
        let entry = serde_json::from_str(&line).map_err(|_| invalid("invalid evidence entry"))?;
        if !visit(entry)? {
            return Ok(());
        }
    }
}

fn clip(value: &str, limit: usize) -> (String, bool) {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].into(), end < value.len())
}

/// Keep only a bounded prefix per line while skipping other messages. A very
/// large source line therefore cannot enlarge the local review response.
fn line_prefix(
    reader: &mut impl BufRead,
    cancelled: &impl Fn() -> bool,
) -> io::Result<Option<(Vec<u8>, bool)>> {
    let mut prefix = Vec::new();
    let mut truncated = false;
    loop {
        check_cancel(cancelled)?;
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            return Ok((!prefix.is_empty()).then_some((prefix, truncated)));
        }
        let newline = bytes.iter().position(|b| *b == b'\n');
        let count = newline.map_or(bytes.len(), |i| i + 1);
        let retain = count.min(TEXT_BYTES + 512 - prefix.len());
        prefix.extend_from_slice(&bytes[..retain]);
        truncated |= retain < count;
        reader.consume(count);
        if newline.is_some() {
            return Ok(Some((prefix, truncated)));
        }
    }
}

fn quoted_preview(
    mut reader: impl BufRead,
    reference: &EvidenceRef,
    cancelled: &impl Fn() -> bool,
) -> io::Result<(String, bool)> {
    let header = format!("## Evidence {}@{}\n", reference.id, reference.revision);
    let mut found = false;
    let mut text = Vec::new();
    let mut truncated = false;
    while let Some((line, long_line)) = line_prefix(&mut reader, cancelled)? {
        if line.starts_with(b"## Evidence ") {
            if found {
                break;
            }
            found = line == header.as_bytes();
            continue;
        }
        if found && line.starts_with(b"> ") {
            let body = &line[2..];
            let retain = body.len().min(TEXT_BYTES + 1 - text.len());
            text.extend_from_slice(&body[..retain]);
            if long_line || retain < body.len() || text.len() > TEXT_BYTES {
                truncated = true;
                break;
            }
        }
    }
    if !found {
        return Err(invalid("reviewed evidence block is missing"));
    }
    if text.len() > TEXT_BYTES {
        text.truncate(TEXT_BYTES);
    }
    let value = match String::from_utf8(text) {
        Ok(text) => text,
        Err(error) if error.utf8_error().error_len().is_none() => {
            let end = error.utf8_error().valid_up_to();
            String::from_utf8(error.into_bytes()[..end].to_vec())
                .map_err(|_| invalid("invalid preview text"))?
        }
        Err(_) => return Err(invalid("invalid preview text")),
    };
    Ok((value, truncated))
}
