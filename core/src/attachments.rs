//! Explicit local attachment choices and bounded, handle-relative text reads.
//! No directory discovery, client profile access, URL fetching or extraction.

mod catalog;
pub use catalog::{AttachmentCandidate, AttachmentCatalog};

use std::collections::BTreeSet;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, Metadata, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::snapshot::{Attachment, AttachmentAvailability};

pub const MAX_FILES: usize = 100;
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;

/// Stored privately with the Project. Metadata binding prevents a refresh from
/// silently retargeting a selected position to a different attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentChoice {
    pub message_id: String,
    pub position: usize,
    pub expected: Attachment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentSelection {
    /// User-selected export root. Store its canonical target at selection time;
    /// reading reopens this path component by component without following links.
    pub root: PathBuf,
    pub files: Vec<AttachmentChoice>,
}

impl AttachmentSelection {
    pub(crate) fn validate(&self) -> io::Result<()> {
        root_anchor(&self.root)?;
        if self.files.is_empty() || self.files.len() > MAX_FILES {
            return Err(invalid("select between 1 and 100 attachments per source"));
        }
        let mut identities = BTreeSet::new();
        for choice in &self.files {
            if choice.message_id.is_empty()
                || choice.message_id.len() > 1024
                || choice.message_id.chars().any(char::is_control)
                || !identities.insert((&choice.message_id, choice.position))
            {
                return Err(invalid("invalid or duplicate attachment choice"));
            }
        }
        Ok(())
    }
}

pub(crate) struct TextAttachment {
    pub text: String,
    pub source_bytes: u64,
    pub sha256: String,
}

pub(crate) struct TextReference<'a>(Vec<&'a str>);

impl<'a> TextReference<'a> {
    pub(crate) fn new(attachment: &'a Attachment) -> io::Result<Self> {
        if attachment.availability != AttachmentAvailability::UnverifiedReference {
            return Err(invalid("attachment has no available local reference"));
        }
        let path = attachment
            .relative_path
            .as_deref()
            .ok_or_else(|| invalid("attachment has no local reference"))?;
        let parts = relative_parts(path)?;
        if !text_extension(parts.last().expect("nonempty validated path")) {
            return Err(invalid("attachment type is outside the text allowlist"));
        }
        Ok(Self(parts))
    }
}

pub(crate) struct ArchiveFiles {
    root: Dir,
}

impl ArchiveFiles {
    pub(crate) fn open(root: &Path) -> io::Result<Self> {
        let (anchor, components) = root_anchor(root)?;
        let mut directory =
            Dir::open_ambient_dir(anchor, cap_std::ambient_authority()).map_err(access_error)?;
        reject_reparse(&directory.dir_metadata().map_err(access_error)?)?;
        for component in components {
            directory = directory
                .open_dir_nofollow(component)
                .map_err(access_error)?;
            reject_reparse(&directory.dir_metadata().map_err(access_error)?)?;
        }
        Ok(Self { root: directory })
    }

    pub(crate) fn read(
        &self,
        reference: TextReference<'_>,
        remaining: u64,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<TextAttachment> {
        let parts = reference.0;
        let leaf = parts.last().expect("nonempty validated path");
        let mut parent = self.root.try_clone().map_err(access_error)?;
        for part in &parts[..parts.len() - 1] {
            check_cancel(cancelled)?;
            parent = parent.open_dir_nofollow(part).map_err(access_error)?;
            reject_reparse(&parent.dir_metadata().map_err(access_error)?)?;
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).nonblock(true);
        let mut file = parent.open_with(leaf, &options).map_err(access_error)?;
        let before = file.metadata().map_err(access_error)?;
        reject_reparse(&before)?;
        if !before.is_file() {
            return Err(invalid("attachment must be a regular file"));
        }
        let limit = remaining.min(MAX_FILE_BYTES);
        if before.len() > limit {
            return Err(invalid("attachment exceeds the file or total byte budget"));
        }
        // Actual reads, not the archive's advertised size, enforce the limit.
        let mut bytes = Vec::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            check_cancel(cancelled)?;
            let available = (limit + 1 - bytes.len() as u64).min(buffer.len() as u64) as usize;
            let count = file.read(&mut buffer[..available]).map_err(access_error)?;
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            if bytes.len() as u64 > limit {
                return Err(invalid("attachment exceeds the file or total byte budget"));
            }
        }
        let after = file.metadata().map_err(access_error)?;
        if before.len() != after.len()
            || after.len() != bytes.len() as u64
            || before.modified().ok() != after.modified().ok()
        {
            return Err(invalid("attachment changed during reading; prepare again"));
        }
        let source_bytes = bytes.len() as u64;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let text = String::from_utf8(bytes).map_err(|_| invalid("attachment is not UTF-8 text"))?;
        // Preserve a UTF-8 BOM in the content/digest. Only ordinary text controls
        // are accepted; shell escapes, NUL and other binary controls are rejected.
        if text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(invalid("attachment contains non-text control characters"));
        }
        Ok(TextAttachment {
            text,
            source_bytes,
            sha256,
        })
    }
}

fn relative_parts(path: &str) -> io::Result<Vec<&str>> {
    if path.len() > 4096
        || path.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        || path.chars().any(char::is_control)
    {
        return Err(invalid("unsafe attachment path"));
    }
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() > 64
        || parts.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || part.ends_with(['.', ' '])
                || reserved_name(part)
        })
    {
        return Err(invalid("unsafe attachment path"));
    }
    Ok(parts)
}

fn reserved_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).to_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|n| {
            matches!(
                n,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

fn text_extension(name: &str) -> bool {
    matches!(
        name.rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .as_deref(),
        Some(
            "txt"
                | "md"
                | "log"
                | "json"
                | "jsonl"
                | "yaml"
                | "yml"
                | "toml"
                | "ini"
                | "csv"
                | "rs"
                | "py"
                | "js"
                | "jsx"
                | "ts"
                | "tsx"
                | "go"
                | "java"
                | "kt"
                | "kts"
                | "c"
                | "h"
                | "cc"
                | "cpp"
                | "hpp"
                | "cs"
                | "rb"
                | "php"
                | "swift"
                | "scala"
                | "sql"
                | "sh"
                | "bash"
                | "zsh"
                | "ps1"
                | "css"
                | "scss"
                | "html"
                | "xml"
                | "conf"
                | "cfg"
        )
    )
}

fn root_anchor(root: &Path) -> io::Result<(PathBuf, Vec<&std::ffi::OsStr>)> {
    if !root.is_absolute() {
        return Err(invalid("attachment root must be absolute"));
    }
    let mut anchor = PathBuf::new();
    let mut names = Vec::new();
    for component in root.components() {
        match component {
            #[cfg(windows)]
            Component::Prefix(prefix) => {
                if !matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                ) {
                    return Err(invalid("attachment root requires a local drive"));
                }
                anchor.push(prefix.as_os_str());
            }
            Component::RootDir => anchor.push(component.as_os_str()),
            Component::Normal(name) => names.push(name),
            _ => {
                return Err(invalid(
                    "attachment root must contain only normal components",
                ))
            }
        }
    }
    Ok((anchor, names))
}

fn reject_reparse(metadata: &Metadata) -> io::Result<()> {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(invalid("attachment reparse points are not allowed"));
        }
    }
    if metadata.file_type().is_symlink() {
        return Err(invalid("attachment symlinks are not allowed"));
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

fn access_error(error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        "cannot access selected attachment without following links",
    )
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
