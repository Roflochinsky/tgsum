//! Opt-in, bounded follower of an explicitly selected diagnostic-log directory.
//! No discovery, client process control, session storage or outbound packets.
//! The controlled experiment assumes no concurrent same-user replacement of
//! the output directory: path checks detect changes, but the final temporary
//! file publication is path-based, not an adversarial race boundary.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use fs4::FileExt;
use regex::bytes::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::parser::{
    parse_packet, CoverageGap, MessageKind, ParsedEvent, ParserLimits, PeerKind, TypedPeer,
};
use crate::attachments::ArchiveFiles;

const STATE_FILE: &str = "capture-state.json";
const LOCK_FILE: &str = ".capture-lock";
const VERSION: &str = "7.2.5";
static RECORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3} [0-9]{2,}-[0-9]{7,}\] \(dc:[A-Za-z0-9_]+\) Recv: ").expect("static record pattern")
});
static SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    // AbstractConnection::ProtocolDcDebugId renders test/media roles as
    // prefixes/suffixes, not as the signed internal protocol DC integer.
    Regex::new(r"^ \(dc:(?:test_)?[0-9]+(?:_media)?,key:[0-9]+,session:[0-9]+\)\r?$")
        .expect("static suffix pattern")
});
static HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3} [0-9]{2,}-[0-9]{7,}\]")
        .expect("static header pattern")
});

#[derive(Clone, Copy, Debug)]
pub struct CaptureLimits {
    pub max_files: usize,
    pub max_events: usize,
    pub max_packet_bytes: usize,
    pub max_poll_bytes: usize,
    pub max_state_bytes: usize,
}
impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            max_files: 96,
            max_events: 10_000,
            max_packet_bytes: 2 * 1024 * 1024,
            max_poll_bytes: 16 * 1024 * 1024,
            max_state_bytes: 64 * 1024 * 1024,
        }
    }
}

pub struct CaptureConfig {
    pub input_directory: PathBuf,
    pub output_directory: PathBuf,
    pub account_namespace: String,
    pub peer: TypedPeer,
    pub self_user_id: Option<String>,
    pub confirmed_single_account: bool,
    pub limits: CaptureLimits,
}
impl CaptureConfig {
    pub fn new(
        input_directory: PathBuf,
        output_directory: PathBuf,
        account_namespace: String,
        peer: TypedPeer,
    ) -> Self {
        Self {
            input_directory,
            output_directory,
            account_namespace,
            peer,
            self_user_id: None,
            confirmed_single_account: false,
            limits: CaptureLimits::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureError {
    InvalidConfiguration,
    UnsafePath,
    UnownedOutput,
    BindingMismatch,
    Busy,
    Capacity,
    Io,
    InvalidState,
}
impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidConfiguration => "invalid or unconfirmed capture configuration",
            Self::UnsafePath => "capture path must be an explicit safe local directory",
            Self::UnownedOutput => "capture refuses an existing unowned output directory",
            Self::BindingMismatch => {
                "capture state belongs to a different source or account binding"
            }
            Self::Busy => "another capture owns this output directory",
            Self::Capacity => "capture capacity reached; no partial checkpoint was saved",
            Self::Io => "capture filesystem operation failed",
            Self::InvalidState => "capture state failed validation",
        })
    }
}
impl std::error::Error for CaptureError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureBinding {
    pub account_namespace: String,
    pub peer: TypedPeer,
    pub self_user_id: Option<String>,
    pub client_version: String,
    pub input_directory_sha256: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reason", content = "detail", rename_all = "snake_case")]
pub enum CaptureGap {
    StartedWithoutHistory,
    FileRotated,
    FileTruncated,
    LoggingRestarted,
    MalformedPacket,
    Parser(CoverageGap),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapCount {
    pub gap: CaptureGap,
    pub count: u64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileCursor {
    name: String,
    offset: u64,
    device: u64,
    inode: u64,
    observed_len: u64,
    modified: Option<(u64, u32)>,
    head_len: usize,
    head_sha256: String,
    tail_sha256: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureState {
    pub schema_version: u32,
    pub binding: CaptureBinding,
    pub events: Vec<ParsedEvent>,
    pub gaps: Vec<GapCount>,
    cursors: Vec<FileCursor>,
}

/// A view into the stored event history; duplicate/older packets cannot undo
/// the latest observed edit. Deletion means an observed channel delete only.
pub struct LatestMessage<'a> {
    pub message_id: &'a str,
    pub event: &'a ParsedEvent,
    pub versions: usize,
    pub deleted: bool,
}
impl CaptureState {
    pub fn latest_messages(&self) -> Vec<LatestMessage<'_>> {
        let mut latest: BTreeMap<&str, LatestMessage<'_>> = BTreeMap::new();
        let mut deleted = BTreeSet::new();
        for event in &self.events {
            match event {
                ParsedEvent::Message {
                    message_id,
                    kind,
                    timestamp,
                    ..
                } => {
                    if let Some(current) = latest.get_mut(message_id.as_str()) {
                        current.versions += 1;
                        if let ParsedEvent::Message {
                            timestamp: old_time,
                            ..
                        } = current.event
                        {
                            // Catch-up/history snapshots may carry a newer
                            // edit_date despite their New/Observed envelope.
                            // At equal seconds only an explicit edit advances
                            // arrival order; old snapshots cannot undo it.
                            if timestamp > old_time
                                || (*kind == MessageKind::Edit && timestamp == old_time)
                            {
                                current.event = event;
                            }
                        }
                    } else {
                        latest.insert(
                            message_id,
                            LatestMessage {
                                message_id,
                                event,
                                versions: 1,
                                deleted: false,
                            },
                        );
                    }
                }
                ParsedEvent::Delete { message_ids, .. } => {
                    deleted.extend(message_ids.iter().map(String::as_str));
                }
            }
        }
        for (id, message) in &mut latest {
            message.deleted = deleted.contains(id);
        }
        latest.into_values().collect()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct PollReport {
    pub bytes_read: usize,
    pub packets: usize,
    pub added_events: usize,
    pub duplicates: usize,
    pub gaps: usize,
    pub pending_tail: bool,
}

pub struct CaptureSession {
    config: CaptureConfig,
    input: ArchiveFiles,
    output_identity: (u64, u64),
    state: CaptureState,
    persisted_digest: Option<String>,
    _lock: File,
}
impl CaptureSession {
    pub fn open(mut config: CaptureConfig) -> Result<Self, CaptureError> {
        validate_config(&config)?;
        let input =
            ArchiveFiles::open(&config.input_directory).map_err(|_| CaptureError::UnsafePath)?;
        config.input_directory =
            fs::canonicalize(&config.input_directory).map_err(|_| CaptureError::UnsafePath)?;
        let parent = config
            .output_directory
            .parent()
            .ok_or(CaptureError::UnsafePath)?;
        ArchiveFiles::open(parent).map_err(|_| CaptureError::UnsafePath)?;
        let leaf = config
            .output_directory
            .file_name()
            .ok_or(CaptureError::UnsafePath)?;
        config.output_directory = fs::canonicalize(parent)
            .map_err(|_| CaptureError::UnsafePath)?
            .join(leaf);
        if config.input_directory.starts_with(&config.output_directory)
            || config.output_directory.starts_with(&config.input_directory)
        {
            return Err(CaptureError::UnsafePath);
        }
        let binding = CaptureBinding {
            account_namespace: config.account_namespace.clone(),
            peer: config.peer.clone(),
            self_user_id: config.self_user_id.clone(),
            client_version: VERSION.into(),
            input_directory_sha256: digest(config.input_directory.as_os_str().as_encoded_bytes()),
        };
        let created = match fs::symlink_metadata(&config.output_directory) {
            Ok(metadata) => {
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(CaptureError::UnsafePath);
                }
                private_permissions(&metadata, true)?;
                false
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder
                    .create(&config.output_directory)
                    .map_err(|_| CaptureError::Io)?;
                true
            }
            Err(_) => return Err(CaptureError::Io),
        };
        let output =
            ArchiveFiles::open(&config.output_directory).map_err(|_| CaptureError::UnsafePath)?;
        let lock = if created {
            let mut options = OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options
                .open(config.output_directory.join(LOCK_FILE))
                .map_err(|_| CaptureError::Io)?
        } else {
            output
                .open_regular(Path::new(LOCK_FILE))
                .map_err(|_| CaptureError::UnownedOutput)?
        };
        private_permissions(&lock.metadata().map_err(|_| CaptureError::Io)?, false)?;
        FileExt::try_lock(&lock).map_err(|_| CaptureError::Busy)?;
        let (state, persisted_digest) = if created {
            (
                CaptureState {
                    schema_version: 1,
                    binding,
                    events: Vec::new(),
                    gaps: vec![GapCount {
                        gap: CaptureGap::StartedWithoutHistory,
                        count: 1,
                    }],
                    cursors: Vec::new(),
                },
                None,
            )
        } else {
            for entry in fs::read_dir(&config.output_directory).map_err(|_| CaptureError::Io)? {
                let entry = entry.map_err(|_| CaptureError::Io)?;
                if entry.file_name() != STATE_FILE && entry.file_name() != LOCK_FILE {
                    return Err(CaptureError::UnownedOutput);
                }
            }
            let mut file = output
                .open_regular(Path::new(STATE_FILE))
                .map_err(|_| CaptureError::UnownedOutput)?;
            private_permissions(&file.metadata().map_err(|_| CaptureError::Io)?, false)?;
            let mut bytes = Vec::new();
            Read::by_ref(&mut file)
                .take(config.limits.max_state_bytes as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| CaptureError::Io)?;
            if bytes.len() > config.limits.max_state_bytes {
                return Err(CaptureError::Capacity);
            }
            let state: CaptureState =
                serde_json::from_slice(&bytes).map_err(|_| CaptureError::InvalidState)?;
            if state.binding != binding {
                return Err(CaptureError::BindingMismatch);
            }
            validate_state(&state, config.limits)?;
            (state, Some(digest(&bytes)))
        };
        let output_identity =
            identity(&fs::metadata(&config.output_directory).map_err(|_| CaptureError::Io)?);
        let mut session = Self {
            config,
            input,
            output_identity,
            state,
            persisted_digest,
            _lock: lock,
        };
        if created {
            session.persisted_digest = Some(session.save(&session.state)?);
        }
        Ok(session)
    }
    pub fn state(&self) -> &CaptureState {
        &self.state
    }
    pub fn state_path(&self) -> PathBuf {
        self.config.output_directory.join(STATE_FILE)
    }

    pub fn poll(&mut self) -> Result<PollReport, CaptureError> {
        self.check_output()?;
        let mut candidate = self.state.clone();
        let mut report = PollReport::default();
        let mut seen: BTreeSet<String> = candidate
            .events
            .iter()
            .map(event_hash)
            .collect::<Result<_, _>>()?;
        let mut names = Vec::new();
        let mut inspected = 0;
        for entry in fs::read_dir(&self.config.input_directory).map_err(|_| CaptureError::Io)? {
            inspected += 1;
            if inspected > 4096 {
                return Err(CaptureError::Capacity);
            }
            let entry = entry.map_err(|_| CaptureError::Io)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !log_name(&name) {
                continue;
            }
            if !entry.file_type().map_err(|_| CaptureError::Io)?.is_file() {
                return Err(CaptureError::UnsafePath);
            }
            names.push(name);
            if names.len() > self.config.limits.max_files {
                return Err(CaptureError::Capacity);
            }
        }
        names.sort();
        let removed = candidate
            .cursors
            .iter()
            .filter(|cursor| !names.contains(&cursor.name))
            .count();
        for _ in 0..removed {
            gap(&mut candidate, &mut report, CaptureGap::FileRotated);
        }
        candidate
            .cursors
            .retain(|cursor| names.contains(&cursor.name));
        for name in names {
            if report.bytes_read >= self.config.limits.max_poll_bytes {
                report.pending_tail = true;
                break;
            }
            let mut file = self
                .input
                .open_regular(Path::new(&name))
                .map_err(|_| CaptureError::UnsafePath)?;
            let metadata = file.metadata().map_err(|_| CaptureError::Io)?;
            let (device, inode) = identity(&metadata);
            let previous = candidate
                .cursors
                .iter()
                .position(|cursor| cursor.name == name);
            let mut cursor = previous
                .map(|i| candidate.cursors[i].clone())
                .unwrap_or(FileCursor {
                    name,
                    offset: 0,
                    device,
                    inode,
                    observed_len: metadata.len(),
                    modified: modified(&metadata),
                    head_len: 0,
                    head_sha256: digest(&[]),
                    tail_sha256: digest(&[]),
                });
            if previous.is_none() && !self.state.cursors.is_empty() {
                gap(&mut candidate, &mut report, CaptureGap::FileRotated);
            }
            if (cursor.device, cursor.inode) != (device, inode) {
                gap(&mut candidate, &mut report, CaptureGap::FileRotated);
                cursor.offset = 0;
            } else if metadata.len() < cursor.offset
                || (previous.is_some()
                    && metadata.len() == cursor.observed_len
                    && modified(&metadata) != cursor.modified)
                || (cursor.offset > 0
                    && (read_hash(&mut file, 0, cursor.head_len)? != cursor.head_sha256
                        || read_hash(
                            &mut file,
                            cursor.offset.saturating_sub(256),
                            cursor.offset.min(256) as usize,
                        )? != cursor.tail_sha256))
            {
                gap(&mut candidate, &mut report, CaptureGap::FileTruncated);
                cursor.offset = 0;
            }
            cursor.device = device;
            cursor.inode = inode;
            cursor.observed_len = metadata.len();
            cursor.modified = modified(&metadata);
            file.seek(SeekFrom::Start(cursor.offset))
                .map_err(|_| CaptureError::Io)?;
            let allowance = self.config.limits.max_poll_bytes - report.bytes_read;
            let mut bytes = Vec::new();
            Read::by_ref(&mut file)
                .take(allowance as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| CaptureError::Io)?;
            report.bytes_read += bytes.len();
            let consumed = self.consume(&bytes, &mut candidate, &mut seen, &mut report)?;
            cursor.offset += consumed as u64;
            report.pending_tail |= consumed < bytes.len() || cursor.offset < metadata.len();
            cursor.head_len = (metadata.len().min(64)) as usize;
            cursor.head_sha256 = read_hash(&mut file, 0, cursor.head_len)?;
            cursor.tail_sha256 = read_hash(
                &mut file,
                cursor.offset.saturating_sub(256),
                cursor.offset.min(256) as usize,
            )?;
            if let Some(index) = previous {
                candidate.cursors[index] = cursor;
            } else {
                candidate.cursors.push(cursor);
            }
        }
        if candidate != self.state {
            self.persisted_digest = Some(self.save(&candidate)?);
            self.state = candidate;
        }
        Ok(report)
    }

    fn consume(
        &self,
        bytes: &[u8],
        state: &mut CaptureState,
        seen: &mut BTreeSet<String>,
        report: &mut PollReport,
    ) -> Result<usize, CaptureError> {
        let mut offset = 0;
        while offset < bytes.len() {
            let rest = &bytes[offset..];
            let Some(line_end) = rest.iter().position(|&byte| byte == b'\n') else {
                if rest.len() > self.config.limits.max_packet_bytes {
                    return Err(CaptureError::Capacity);
                }
                break;
            };
            let line = &rest[..line_end];
            if line.strip_suffix(b"\r").unwrap_or(line) == b"NEW LOGGING INSTANCE STARTED!!!" {
                gap(state, report, CaptureGap::LoggingRestarted);
            }
            let Some(header) = RECORD.find(line) else {
                offset += line_end + 1;
                continue;
            };
            let dump_start = header.end();
            let packet = &rest[dump_start..];
            let end = match complete_dump(packet, self.config.limits.max_packet_bytes)? {
                Frame::Incomplete => break,
                Frame::Malformed => {
                    gap(state, report, CaptureGap::MalformedPacket);
                    offset += line_end + 1;
                    continue;
                }
                Frame::Complete(end) => end,
            };
            let Some(suffix_end) = packet[end..].iter().position(|&byte| byte == b'\n') else {
                if packet.len() > self.config.limits.max_packet_bytes {
                    return Err(CaptureError::Capacity);
                }
                break;
            };
            let advance = dump_start + end + suffix_end + 1;
            if !SUFFIX.is_match(&packet[end..end + suffix_end]) {
                gap(state, report, CaptureGap::MalformedPacket);
                offset += advance;
                continue;
            }
            report.packets += 1;
            let parsed = parse_packet(
                &packet[..end],
                self.config.self_user_id.as_deref(),
                ParserLimits {
                    max_bytes: self.config.limits.max_packet_bytes,
                    ..ParserLimits::default()
                },
            );
            match parsed {
                Ok(parsed) => {
                    for reason in parsed.gaps {
                        gap(state, report, CaptureGap::Parser(reason));
                    }
                    for event in parsed.events {
                        let peer = match &event {
                            ParsedEvent::Message { peer, .. }
                            | ParsedEvent::Delete { peer, .. } => peer,
                        };
                        if peer != &self.config.peer {
                            continue;
                        }
                        if matches!(&event, ParsedEvent::Delete { peer, .. } if peer.kind != PeerKind::Channel)
                        {
                            continue;
                        }
                        if !seen.insert(event_hash(&event)?) {
                            report.duplicates += 1;
                            continue;
                        }
                        if state.events.len() >= self.config.limits.max_events {
                            return Err(CaptureError::Capacity);
                        }
                        state.events.push(event);
                        report.added_events += 1;
                    }
                }
                Err(super::parser::ParseError::LimitExceeded) => {
                    return Err(CaptureError::Capacity)
                }
                Err(_) => gap(state, report, CaptureGap::MalformedPacket),
            }
            offset += advance;
        }
        Ok(offset)
    }

    fn check_output(&self) -> Result<(), CaptureError> {
        ArchiveFiles::open(&self.config.output_directory).map_err(|_| CaptureError::UnsafePath)?;
        let metadata = fs::metadata(&self.config.output_directory).map_err(|_| CaptureError::Io)?;
        private_permissions(&metadata, true)?;
        if identity(&metadata) != self.output_identity {
            return Err(CaptureError::UnsafePath);
        }
        Ok(())
    }
    fn save(&self, state: &CaptureState) -> Result<String, CaptureError> {
        self.check_output()?;
        // Refuse an externally changed state file detected before publication.
        if let Some(expected) = &self.persisted_digest {
            let files = ArchiveFiles::open(&self.config.output_directory)
                .map_err(|_| CaptureError::UnsafePath)?;
            let mut existing = files
                .open_regular(Path::new(STATE_FILE))
                .map_err(|_| CaptureError::InvalidState)?;
            let mut bytes = Vec::new();
            Read::by_ref(&mut existing)
                .take(self.config.limits.max_state_bytes as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| CaptureError::Io)?;
            if bytes.len() > self.config.limits.max_state_bytes || digest(&bytes) != *expected {
                return Err(CaptureError::InvalidState);
            }
        } else if fs::symlink_metadata(self.state_path()).is_ok() {
            return Err(CaptureError::UnownedOutput);
        }
        let mut bytes = BoundedBytes {
            bytes: Vec::new(),
            limit: self.config.limits.max_state_bytes,
        };
        serde_json::to_writer(&mut bytes, state).map_err(|_| CaptureError::Capacity)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.config.output_directory)
            .map_err(|_| CaptureError::Io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| CaptureError::Io)?;
        }
        temporary
            .write_all(&bytes.bytes)
            .map_err(|_| CaptureError::Io)?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| CaptureError::Io)?;
        temporary
            .persist(self.state_path())
            .map_err(|_| CaptureError::Io)?;
        #[cfg(unix)]
        File::open(&self.config.output_directory)
            .and_then(|file| file.sync_all())
            .map_err(|_| CaptureError::Io)?;
        Ok(digest(&bytes.bytes))
    }
}

fn validate_config(config: &CaptureConfig) -> Result<(), CaptureError> {
    let limits = config.limits;
    if !config.confirmed_single_account
        || config.account_namespace.is_empty()
        || config.account_namespace.len() > 80
        || !config
            .account_namespace
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || !valid_id(&config.peer.id)
        || config
            .self_user_id
            .as_deref()
            .is_some_and(|id| !valid_id(id))
        || limits.max_files == 0
        || limits.max_files > 96
        || limits.max_events == 0
        || limits.max_events > 100_000
        || limits.max_packet_bytes == 0
        || limits.max_packet_bytes > 4 * 1024 * 1024
        || limits.max_poll_bytes < limits.max_packet_bytes
        || limits.max_poll_bytes > 64 * 1024 * 1024
        || limits.max_state_bytes == 0
        || limits.max_state_bytes > 128 * 1024 * 1024
    {
        return Err(CaptureError::InvalidConfiguration);
    }
    if config
        .input_directory
        .file_name()
        .is_none_or(|name| name != "DebugLogs")
        || [&config.input_directory, &config.output_directory]
            .iter()
            .any(|path| {
                !path.is_absolute()
                    || path.components().any(|part| match part {
                        Component::Normal(name) => name.eq_ignore_ascii_case("tdata"),
                        Component::CurDir | Component::ParentDir => true,
                        _ => false,
                    })
            })
    {
        return Err(CaptureError::UnsafePath);
    }
    Ok(())
}
fn validate_state(state: &CaptureState, limits: CaptureLimits) -> Result<(), CaptureError> {
    if state.schema_version != 1
        || state.events.len() > limits.max_events
        || state.cursors.len() > limits.max_files
        || state.gaps.len() > 16
        || state.events.iter().any(|event| match event {
            ParsedEvent::Message {
                peer, message_id, ..
            } => peer != &state.binding.peer || !valid_id(message_id),
            ParsedEvent::Delete { peer, message_ids } => {
                peer != &state.binding.peer
                    || peer.kind != PeerKind::Channel
                    || message_ids.iter().any(|id| !valid_id(id))
            }
        })
        || state
            .cursors
            .iter()
            .any(|cursor| !log_name(&cursor.name) || cursor.head_len > 64)
    {
        return Err(CaptureError::InvalidState);
    }
    Ok(())
}
fn valid_id(value: &str) -> bool {
    !value.starts_with('0')
        && value
            .parse::<i64>()
            .is_ok_and(|v| v > 0 && v.to_string() == value)
}
fn log_name(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 13
        && &b[..4] == b"mtp_"
        && b[6] == b'_'
        && &b[9..] == b".txt"
        && b[4..6].iter().chain(b[7..9].iter()).all(u8::is_ascii_digit)
        && &b[4..6] < b"24"
        && &b[7..9] < b"60"
}
fn gap(state: &mut CaptureState, report: &mut PollReport, reason: CaptureGap) {
    report.gaps += 1;
    if let Some(entry) = state.gaps.iter_mut().find(|entry| entry.gap == reason) {
        entry.count = entry.count.saturating_add(1);
    } else {
        state.gaps.push(GapCount {
            gap: reason,
            count: 1,
        });
    }
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn event_hash(event: &ParsedEvent) -> Result<String, CaptureError> {
    serde_json::to_vec(event)
        .map(|bytes| digest(&bytes))
        .map_err(|_| CaptureError::InvalidState)
}
fn read_hash(file: &mut File, offset: u64, count: usize) -> Result<String, CaptureError> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| CaptureError::Io)?;
    let mut bytes = vec![0; count];
    file.read_exact(&mut bytes).map_err(|_| CaptureError::Io)?;
    Ok(digest(&bytes))
}
fn identity(metadata: &fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (metadata.dev(), metadata.ino())
    }
    #[cfg(not(unix))]
    {
        // The live capture profile is Arch Linux. Portable synthetic tests
        // detect replacement through length/time/fingerprints, conservatively
        // reporting FileTruncated when no inode identity is available.
        let _ = metadata;
        (0, 0)
    }
}
fn modified(metadata: &fs::Metadata) -> Option<(u64, u32)> {
    let time = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some((time.as_secs(), time.subsec_nanos()))
}
fn private_permissions(metadata: &fs::Metadata, directory: bool) -> Result<(), CaptureError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let expected = if directory { 0o700 } else { 0o600 };
        if metadata.permissions().mode() & 0o777 != expected {
            return Err(CaptureError::UnsafePath);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (metadata, directory);
    }
    Ok(())
}

struct BoundedBytes {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("capture state capacity"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
enum Frame {
    Incomplete,
    Malformed,
    Complete(usize),
}
fn complete_dump(bytes: &[u8], limit: usize) -> Result<Frame, CaptureError> {
    let start = if bytes.starts_with(b"[GZIPPED] ") {
        10
    } else {
        0
    };
    if bytes.get(start) != Some(&b'{') {
        return Ok(Frame::Malformed);
    }
    let mut brackets = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    for (index, &byte) in bytes.iter().enumerate().skip(start) {
        if index >= limit {
            return Err(CaptureError::Capacity);
        }
        // Dump strings escape LF. A new timestamp at a physical line start
        // therefore ends a damaged/incomplete record, even inside its quote.
        if index > start && bytes[index - 1] == b'\n' && HEADER.is_match(&bytes[index..]) {
            return Ok(Frame::Malformed);
        }
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'{' | b'[' => {
                if brackets.len() >= 64 {
                    return Err(CaptureError::Capacity);
                }
                brackets.push(byte);
            }
            b'}' | b']' => {
                if brackets.pop() != Some(if byte == b'}' { b'{' } else { b'[' }) {
                    return Ok(Frame::Malformed);
                }
                if brackets.is_empty() {
                    return Ok(Frame::Complete(index + 1));
                }
            }
            _ => (),
        }
    }
    Ok(Frame::Incomplete)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _root: tempfile::TempDir,
        input: PathBuf,
        output: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let canonical = fs::canonicalize(root.path()).unwrap();
            let input = canonical.join("DebugLogs");
            fs::create_dir(&input).unwrap();
            let output = canonical.join("capture");
            Self {
                _root: root,
                input,
                output,
            }
        }
        fn config(&self) -> CaptureConfig {
            let mut config = CaptureConfig::new(
                self.input.clone(),
                self.output.clone(),
                "synthetic-account".into(),
                TypedPeer {
                    kind: PeerKind::Channel,
                    id: "9007199254740995".into(),
                },
            );
            config.confirmed_single_account = true;
            config
        }
        fn append(&self, value: &[u8]) {
            OpenOptions::new()
                .append(true)
                .create(true)
                .open(self.input.join("mtp_12_00.txt"))
                .unwrap()
                .write_all(value)
                .unwrap();
        }
    }
    fn object(name: &str, fields: &[(&str, String)]) -> String {
        format!(
            "{{ {name}\n{}}}",
            fields
                .iter()
                .map(|(name, value)| format!("  {name}: {value},\n"))
                .collect::<String>()
        )
    }
    fn text(value: &str) -> String {
        format!(
            "\"{}\" [STRING]",
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        )
    }
    fn packet(id: &str, value: &str, edit: Option<i64>, foreign: bool) -> String {
        let peer = object(
            if foreign { "peerUser" } else { "peerChannel" },
            &[(
                if foreign { "user_id" } else { "channel_id" },
                "9007199254740995 [LONG]".into(),
            )],
        );
        let mut fields = vec![
            ("flags", "256 [LONG]".into()),
            ("id", format!("{id} [INT]")),
            ("peer_id", peer),
            (
                "from_id",
                object("peerUser", &[("user_id", "9007199254740993 [LONG]".into())]),
            ),
            ("date", "1700000000 [INT]".into()),
            ("message", text(value)),
        ];
        if let Some(timestamp) = edit {
            fields.push(("edit_date", format!("{timestamp} [INT]")));
        }
        let message = object("message", &fields);
        let update = object(
            if edit.is_some() {
                "updateEditChannelMessage"
            } else {
                "updateNewChannelMessage"
            },
            &[
                ("message", message),
                ("pts", "1 [INT]".into()),
                ("pts_count", "1 [INT]".into()),
            ],
        );
        framed(&object(
            "updateShort",
            &[("update", update), ("date", "1700000000 [INT]".into())],
        ))
    }
    fn framed(body: &str) -> String {
        let core = object(
            "core_message",
            &[
                ("msg_id", "7352359257580183524 [LONG]".into()),
                ("seq_no", "1 [INT]".into()),
                ("bytes", "400 [INT]".into()),
                ("body", body.into()),
            ],
        );
        format!(
            "[12:00:00.123 00-0000001] (dc:2_main) Recv: {core} (dc:2,key:123456,session:987654)\n"
        )
    }
    fn latest_text(state: &CaptureState) -> &str {
        match state.latest_messages()[0].event {
            ParsedEvent::Message { text, .. } => text,
            _ => panic!("message expected"),
        }
    }

    #[test]
    fn append_requires_complete_frame_and_filters_direction_typed_peer_and_metadata() {
        let fixture = Fixture::new();
        let mut capture = CaptureSession::open(fixture.config()).unwrap();
        let selected = packet(
            "2147483647",
            "Привет 😀 {quoted} \\\" [Recv:]\nnext",
            None,
            false,
        );
        fixture.append(b"20261008\n");
        fixture.append(&selected.as_bytes()[..83]);
        let first = capture.poll().unwrap();
        assert!(first.pending_tail);
        assert!(capture.state().events.is_empty());
        fixture.append(&selected.as_bytes()[83..selected.len() - 1]);
        assert!(capture.poll().unwrap().pending_tail);
        assert!(capture.state().events.is_empty());
        fixture.append(b"\n");
        fixture.append(packet("2", "FOREIGN_PRIVATE_TEXT", None, true).as_bytes());
        fixture.append(
            packet("3", "OUTBOUND_PRIVATE_TEXT", None, false)
                .replace(" Recv: ", " Send: ")
                .as_bytes(),
        );
        fs::create_dir(fixture.input.join("nested")).unwrap();
        fs::write(
            fixture.input.join("nested/mtp_12_15.txt"),
            packet("4", "NESTED_PRIVATE_TEXT", None, false),
        )
        .unwrap();
        fs::write(fixture.input.join("auth.txt"), b"AUTH_PRIVATE_TEXT").unwrap();
        let report = capture.poll().unwrap();
        assert_eq!(report.added_events, 1);
        assert!(!report.pending_tail);
        assert_eq!(capture.state().events.len(), 1);
        assert_eq!(
            capture.state().latest_messages()[0].message_id,
            "2147483647"
        );
        assert!(latest_text(capture.state()).contains("Привет 😀"));
        let saved = fs::read_to_string(capture.state_path()).unwrap();
        for excluded in [
            "FOREIGN_PRIVATE_TEXT",
            "OUTBOUND_PRIVATE_TEXT",
            "AUTH_PRIVATE_TEXT",
            "NESTED_PRIVATE_TEXT",
            "key:123456",
            "session:987654",
            "core_message",
        ] {
            assert!(!saved.contains(excluded), "saved forbidden material");
        }
    }

    #[test]
    fn protocol_dc_debug_suffix_accepts_normal_test_and_media_forms() {
        let fixture = Fixture::new();
        let mut capture = CaptureSession::open(fixture.config()).unwrap();
        for (index, dc) in ["2", "test_2", "2_media", "test_2_media"]
            .iter()
            .enumerate()
        {
            let record = packet(&(index + 1).to_string(), "synthetic text", None, false)
                .replace("(dc:2,key:", &format!("(dc:{dc},key:"));
            fixture.append(record.as_bytes());
        }
        let report = capture.poll().unwrap();
        assert_eq!(report.added_events, 4);
        assert_eq!(report.gaps, 0);
        assert!(!report.pending_tail);
        for dc in ["-2", "test_2_media_extra", "2_other"] {
            let record = packet("9", "unsupported suffix", None, false)
                .replace("(dc:2,key:", &format!("(dc:{dc},key:"));
            fixture.append(record.as_bytes());
        }
        let report = capture.poll().unwrap();
        assert_eq!(report.added_events, 0);
        assert_eq!(report.gaps, 3);
    }

    #[test]
    fn restart_deduplicates_and_preserves_edit_versions_without_reverting_latest() {
        let fixture = Fixture::new();
        let new = packet("10", "first", None, false);
        fixture.append(new.as_bytes());
        let mut capture = CaptureSession::open(fixture.config()).unwrap();
        capture.poll().unwrap();
        drop(capture);
        let mut capture = CaptureSession::open(fixture.config()).unwrap();
        assert_eq!(capture.poll().unwrap().added_events, 0);
        let edit = packet("10", "latest", Some(1700000020), false);
        fixture.append(edit.as_bytes());
        fixture.append(new.as_bytes());
        fixture.append(edit.as_bytes());
        fixture.append(packet("10", "older edit", Some(1700000010), false).as_bytes());
        let report = capture.poll().unwrap();
        assert_eq!(report.added_events, 2);
        assert_eq!(report.duplicates, 2);
        assert_eq!(latest_text(capture.state()), "latest");
        assert_eq!(capture.state().latest_messages()[0].versions, 3);
        drop(capture);
        let capture = CaptureSession::open(fixture.config()).unwrap();
        assert_eq!(latest_text(capture.state()), "latest");
        assert_eq!(
            capture.state().binding.account_namespace,
            "synthetic-account"
        );
    }

    #[test]
    fn truncation_same_size_rewrite_rotation_and_logging_restart_are_gaps() {
        let fixture = Fixture::new();
        fixture.append(packet("1", "one", None, false).as_bytes());
        let mut capture = CaptureSession::open(fixture.config()).unwrap();
        capture.poll().unwrap();
        fs::write(
            fixture.input.join("mtp_12_00.txt"),
            packet("2", "two", None, false),
        )
        .unwrap();
        capture.poll().unwrap();
        assert!(capture
            .state()
            .gaps
            .iter()
            .any(|gap| gap.gap == CaptureGap::FileTruncated));
        let replacement_gap = if cfg!(unix) {
            CaptureGap::FileRotated
        } else {
            CaptureGap::FileTruncated
        };
        let replacement_gaps_before = capture
            .state()
            .gaps
            .iter()
            .find(|gap| gap.gap == replacement_gap)
            .map_or(0, |gap| gap.count);
        fs::rename(
            fixture.input.join("mtp_12_00.txt"),
            fixture.input.join("old-log.txt"),
        )
        .unwrap();
        fixture.append(packet("3", "new", None, false).as_bytes());
        fixture.append(b"NEW LOGGING INSTANCE STARTED!!!\n");
        let replacement = capture.poll().unwrap();
        assert_eq!(replacement.added_events, 1);
        assert_eq!(capture.state().events.len(), 3);
        assert_eq!(
            capture
                .state()
                .gaps
                .iter()
                .find(|gap| gap.gap == replacement_gap)
                .map_or(0, |gap| gap.count),
            replacement_gaps_before + 1,
            "replacement must add a fresh gap even without native file identity"
        );
        assert!(capture
            .state()
            .gaps
            .iter()
            .any(|gap| gap.gap == CaptureGap::LoggingRestarted));
        fs::rename(
            fixture.input.join("mtp_12_00.txt"),
            fixture.input.join("mtp_12_15.txt"),
        )
        .unwrap();
        let result = capture.poll().unwrap();
        assert_eq!(result.added_events, 0);
        assert_eq!(result.duplicates, 1);
        assert!(capture
            .state()
            .gaps
            .iter()
            .any(|gap| gap.gap == CaptureGap::FileRotated));
    }

    #[test]
    fn capacity_failure_keeps_events_and_checkpoint_transactional() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config.limits.max_events = 1;
        let mut capture = CaptureSession::open(config).unwrap();
        let before = fs::read(capture.state_path()).unwrap();
        fixture.append(packet("1", "one", None, false).as_bytes());
        fixture.append(packet("2", "two", None, false).as_bytes());
        assert_eq!(capture.poll(), Err(CaptureError::Capacity));
        assert!(capture.state().events.is_empty());
        assert_eq!(fs::read(capture.state_path()).unwrap(), before);
        drop(capture);
        let mut capture = CaptureSession::open(fixture.config()).unwrap();
        assert_eq!(capture.poll().unwrap().added_events, 2);
    }

    #[test]
    fn output_is_private_bound_owned_and_exclusively_locked() {
        let fixture = Fixture::new();
        let capture = CaptureSession::open(fixture.config()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&fixture.output).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(capture.state_path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert!(matches!(
            CaptureSession::open(fixture.config()),
            Err(CaptureError::Busy)
        ));
        drop(capture);
        let mut other = fixture.config();
        other.account_namespace = "different-account".into();
        assert!(matches!(
            CaptureSession::open(other),
            Err(CaptureError::BindingMismatch)
        ));
        fs::write(fixture.output.join("user-notes.txt"), b"USER_DATA").unwrap();
        assert!(matches!(
            CaptureSession::open(fixture.config()),
            Err(CaptureError::UnownedOutput)
        ));
        assert_eq!(
            fs::read(fixture.output.join("user-notes.txt")).unwrap(),
            b"USER_DATA"
        );
    }

    #[test]
    fn requires_confirmed_explicit_directory_and_refuses_overlap() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config.confirmed_single_account = false;
        assert!(matches!(
            CaptureSession::open(config),
            Err(CaptureError::InvalidConfiguration)
        ));
        let mut config = fixture.config();
        config.output_directory = fixture.input.join("capture");
        assert!(matches!(
            CaptureSession::open(config),
            Err(CaptureError::UnsafePath)
        ));
        let mut config = fixture.config();
        config.input_directory = fixture._root.path().join("tdata/DebugLogs");
        assert!(matches!(
            CaptureSession::open(config),
            Err(CaptureError::UnsafePath)
        ));
        assert!(!fixture.output.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_input_files_and_directory_components_are_never_followed() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let secret = fixture._root.path().join("secret");
        fs::write(&secret, b"PRIVATE_UNRELATED_FILE").unwrap();
        symlink(&secret, fixture.input.join("mtp_12_00.txt")).unwrap();
        let mut capture = CaptureSession::open(fixture.config()).unwrap();
        assert_eq!(capture.poll(), Err(CaptureError::UnsafePath));
        assert!(capture.state().events.is_empty());
        let link_parent = fixture._root.path().join("link");
        symlink(&fixture._root, &link_parent).unwrap();
        let mut config = fixture.config();
        config.input_directory = link_parent.join("DebugLogs");
        assert!(matches!(
            CaptureSession::open(config),
            Err(CaptureError::UnsafePath)
        ));
    }

    #[test]
    fn observed_channel_delete_is_stored_and_unresolved_delete_cannot_remove_messages() {
        let fixture = Fixture::new();
        fixture.append(packet("10", "kept as history", None, false).as_bytes());
        let unresolved = object(
            "updateDeleteMessages",
            &[
                ("messages", "[ vector<0x0> (1)\n10 [INT],\n]".into()),
                ("pts", "1 [INT]".into()),
                ("pts_count", "1 [INT]".into()),
            ],
        );
        fixture.append(
            framed(&object(
                "updateShort",
                &[("update", unresolved), ("date", "1700000000 [INT]".into())],
            ))
            .as_bytes(),
        );
        let mut capture = CaptureSession::open(fixture.config()).unwrap();
        capture.poll().unwrap();
        assert!(!capture.state().latest_messages()[0].deleted);
        let delete = object(
            "updateDeleteChannelMessages",
            &[
                ("channel_id", "9007199254740995 [LONG]".into()),
                ("messages", "[ vector<0x0> (1)\n10 [INT],\n]".into()),
                ("pts", "1 [INT]".into()),
                ("pts_count", "1 [INT]".into()),
            ],
        );
        fixture.append(
            framed(&object(
                "updateShort",
                &[("update", delete), ("date", "1700000000 [INT]".into())],
            ))
            .as_bytes(),
        );
        capture.poll().unwrap();
        assert!(capture.state().latest_messages()[0].deleted);
        assert_eq!(latest_text(capture.state()), "kept as history");
        assert!(capture
            .state()
            .gaps
            .iter()
            .any(|gap| gap.gap == CaptureGap::Parser(CoverageGap::UnresolvedDelete)));
    }

    #[test]
    fn oversized_packet_fails_bounded_and_does_not_save_raw_tail() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config.limits.max_packet_bytes = 1024;
        let mut capture = CaptureSession::open(config).unwrap();
        let before = fs::read(capture.state_path()).unwrap();
        fixture.append(packet("1", &"PRIVATE_REPEATED_TEXT".repeat(150), None, false).as_bytes());
        assert_eq!(capture.poll(), Err(CaptureError::Capacity));
        assert_eq!(fs::read(capture.state_path()).unwrap(), before);
    }

    #[test]
    fn restart_waits_for_tail_and_recovers_after_a_damaged_record() {
        let fixture = Fixture::new();
        let first = packet("1", "complete later", None, false);
        fixture.append(&first.as_bytes()[..100]);
        let mut capture = CaptureSession::open(fixture.config()).unwrap();
        assert!(capture.poll().unwrap().pending_tail);
        drop(capture);
        fixture.append(&first.as_bytes()[100..]);
        let mut capture = CaptureSession::open(fixture.config()).unwrap();
        assert_eq!(capture.poll().unwrap().added_events, 1);
        fixture.append(b"[12:00:01.123 00-0000002] (dc:2_main) Recv: { core_message\nbody: { broken\nmessage: \"unfinished [ERROR]\n");
        fixture.append(packet("2", "after damaged record", None, false).as_bytes());
        let report = capture.poll().unwrap();
        assert_eq!(report.added_events, 1);
        assert!(!report.pending_tail);
        assert!(capture
            .state()
            .gaps
            .iter()
            .any(|gap| gap.gap == CaptureGap::MalformedPacket));
        fixture.append(
            packet("3", "invalid footer", None, false)
                .replace("key:123456", "bad:123456")
                .as_bytes(),
        );
        let report = capture.poll().unwrap();
        assert_eq!(report.added_events, 0);
        assert_eq!(report.gaps, 1);
    }

    #[test]
    fn acquires_lock_before_reading_state_and_refuses_external_file_changes() {
        let fixture = Fixture::new();
        let mut capture = CaptureSession::open(fixture.config()).unwrap();
        fs::write(capture.state_path(), b"UNRELATED_USER_DATA").unwrap();
        assert!(matches!(
            CaptureSession::open(fixture.config()),
            Err(CaptureError::Busy)
        ));
        fixture.append(packet("1", "would be stored", None, false).as_bytes());
        assert_eq!(capture.poll(), Err(CaptureError::InvalidState));
        assert_eq!(
            fs::read(capture.state_path()).unwrap(),
            b"UNRELATED_USER_DATA"
        );
        assert!(capture.state().events.is_empty());
    }

    #[test]
    fn newer_catchup_snapshot_advances_revision_but_older_packets_do_not() {
        let fixture = Fixture::new();
        let first = packet("1", "original", None, false);
        fixture.append(first.as_bytes());
        fixture.append(packet("1", "live edit", Some(1700000010), false).as_bytes());
        let mut capture = CaptureSession::open(fixture.config()).unwrap();
        capture.poll().unwrap();
        assert_eq!(latest_text(capture.state()), "live edit");
        fixture.append(
            packet("1", "newer catchup", Some(1700000020), false)
                .replace("updateEditChannelMessage", "updateNewChannelMessage")
                .as_bytes(),
        );
        fixture.append(first.as_bytes());
        fixture.append(packet("1", "older edit", Some(1700000015), false).as_bytes());
        capture.poll().unwrap();
        assert_eq!(latest_text(capture.state()), "newer catchup");
        assert_eq!(capture.state().latest_messages()[0].versions, 4);
    }
}
