//! Bounded observation of a user's selected Telegram export directory.
//!
//! A stable candidate is only a hint for the UI. Neither a quiet file nor
//! valid JSON proves that Telegram finished exporting media or this run.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::project::Project;

const MAX_DIRECTORY_ENTRIES: usize = 4096;
const MAX_CANDIDATES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportCandidate {
    pub path: PathBuf,
    pub bytes: u64,
    pub modified_unix_ms: Option<u64>,
    /// String avoids rounding a nanosecond timestamp through JavaScript's
    /// 53-bit number limit when the host deduplicates notifications.
    pub modified_unix_ns: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    bytes: u64,
    modified: Option<SystemTime>,
}

#[derive(Debug)]
struct Seen {
    fingerprint: Fingerprint,
    since: Instant,
}

/// Polling state for a single desktop process. The host decides when to poll;
/// no messenger, account or source file is opened by this module.
#[derive(Debug)]
pub struct ExportInbox {
    debounce: Duration,
    seen: HashMap<(String, String, PathBuf), Seen>,
}

impl Default for ExportInbox {
    fn default() -> Self {
        Self::new(Duration::from_secs(2))
    }
}

impl ExportInbox {
    pub fn new(debounce: Duration) -> Self {
        Self {
            debounce,
            seen: HashMap::new(),
        }
    }

    /// Return paths whose size and mtime have stayed unchanged for `debounce`.
    /// The caller must still require an independent completion signal or human
    /// confirmation before opening or importing any candidate.
    pub fn poll(
        &mut self,
        project: &Project,
        source_id: &str,
        now: Instant,
    ) -> io::Result<Vec<ExportCandidate>> {
        let source = project
            .sources
            .iter()
            .find(|source| source.source_id == source_id)
            .ok_or_else(|| invalid("source is not connected to the Project"))?;
        if source.connector_id != "telegram_json" || source.scope.platform != "telegram" {
            return Err(invalid(
                "only a connected Telegram JSON source can be watched",
            ));
        }
        let directory = &project
            .assisted_exports
            .get(source_id)
            .ok_or_else(|| invalid("configure an assisted export directory first"))?
            .directory;
        let candidates = scan(directory)?;
        let prefix = (project.project_id.clone(), source_id.to_owned());
        let active: HashSet<PathBuf> = candidates
            .iter()
            .map(|(candidate, _)| candidate.path.clone())
            .collect();
        self.seen.retain(|(project_id, id, path), _| {
            project_id != &prefix.0 || id != &prefix.1 || active.contains(path)
        });
        let mut stable = Vec::new();
        for (candidate, fingerprint) in candidates {
            let key = (prefix.0.clone(), prefix.1.clone(), candidate.path.clone());
            let seen = self.seen.entry(key).or_insert_with(|| Seen {
                fingerprint: fingerprint.clone(),
                since: now,
            });
            if seen.fingerprint != fingerprint {
                *seen = Seen {
                    fingerprint,
                    since: now,
                };
            } else if now.saturating_duration_since(seen.since) >= self.debounce {
                stable.push(candidate);
            }
        }
        stable.sort_by(|a, b| {
            b.modified_unix_ms
                .cmp(&a.modified_unix_ms)
                .then_with(|| a.path.cmp(&b.path))
        });
        Ok(stable)
    }
}

fn scan(directory: &Path) -> io::Result<Vec<(ExportCandidate, Fingerprint)>> {
    if !directory.is_absolute() {
        return Err(invalid("export directory must be absolute"));
    }
    let root = match fs::symlink_metadata(directory) {
        Ok(root) => root,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    if !root.is_dir() || root.file_type().is_symlink() {
        return Err(invalid("export directory must be a real directory"));
    }
    let mut candidates = Vec::new();
    add_regular_json(directory.join("result.json"), &mut candidates)?;
    for (count, entry) in fs::read_dir(directory)?.enumerate() {
        if count >= MAX_DIRECTORY_ENTRIES {
            return Err(invalid(
                "too many export directory entries; select a smaller folder",
            ));
        }
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("ChatExport_")
            || !entry.file_type()?.is_dir()
        {
            continue;
        }
        add_regular_json(entry.path().join("result.json"), &mut candidates)?;
        if candidates.len() > MAX_CANDIDATES {
            return Err(invalid(
                "too many Telegram exports; select a smaller folder",
            ));
        }
    }
    Ok(candidates)
}

fn add_regular_json(
    path: PathBuf,
    candidates: &mut Vec<(ExportCandidate, Fingerprint)>,
) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(());
    }
    let modified = metadata.modified().ok();
    let candidate = ExportCandidate {
        path,
        bytes: metadata.len(),
        modified_unix_ms: modified
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .and_then(|since| u64::try_from(since.as_millis()).ok()),
        modified_unix_ns: modified
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|since| since.as_nanos().to_string()),
    };
    candidates.push((
        candidate,
        Fingerprint {
            bytes: metadata.len(),
            modified,
        },
    ));
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
