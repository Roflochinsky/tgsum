//! Durable selected-peer observations for the Linux continuous source.
//! Source/client control and Project publication are separate callers. All
//! log cursors and event indexes commit together; no history is pruned.

use std::fs::{File, Metadata};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, DirBuilder, DirBuilderExt, OpenOptions, OpenOptionsExt};
use fs4::FileExt;
use redb::{Database, Durability, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use super::capture::{
    digest, event_hash, follow_logs, identity, private_permissions, valid_id, validate_config,
    validate_state, CaptureBinding, CaptureConfig, CaptureError, CaptureGap, CaptureState,
    GapCount, PollReport, VERSION,
};
use super::parser::{MessageKind, ParsedEvent};
use crate::attachments::ArchiveFiles;

const DATABASE_FILE: &str = "observations.redb";
const OWNER_FILE: &str = "owner.json";
const READY_FILE: &str = "initialized";
const CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 128 * 1024;
const MAX_EVENT_BYTES: usize = 32 * 1024 * 1024;
const MAX_PAGE_EVENTS: usize = 512;
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const EVENTS: TableDefinition<u64, &[u8]> = TableDefinition::new("events");
const SEEN: TableDefinition<&str, u64> = TableDefinition::new("seen");
const LATEST: TableDefinition<&str, &[u8]> = TableDefinition::new("latest");

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    schema_version: u32,
    binding: CaptureBinding,
    database_identity: (u64, u64),
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    follower: CaptureState,
    last_sequence: u64,
    observation_revision: u64,
    applied: Option<AppliedCheckpoint>,
}

/// Set only after the caller has durably published the Project snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedCheckpoint {
    pub sequence: u64,
    pub observation_revision: u64,
    pub snapshot_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalMessage {
    /// None means a tombstone observed before any message body.
    pub event: Option<ParsedEvent>,
    pub versions: u64,
    pub deleted: bool,
}

pub struct JournalEvent {
    pub sequence: u64,
    pub event: ParsedEvent,
}

#[derive(Clone, Serialize)]
pub struct JournalStatus {
    pub events: u64,
    pub observation_revision: u64,
    pub applied: Option<AppliedCheckpoint>,
    pub gaps: Vec<GapCount>,
}

/// One writer, one confirmed account and one selected native typed peer.
/// Poll buffers and read pages are bounded; redb cache is fixed at 8 MiB.
/// redb allocator metadata can grow with disk size. Disk history has no TTL.
pub struct ContinuousCapture {
    config: CaptureConfig,
    input: ArchiveFiles,
    input_identity: (u64, u64),
    directory: Dir,
    directory_identity: (u64, u64),
    owner: Owner,
    owner_identity: (u64, u64),
    owner_digest: String,
    ready_identity: Option<(u64, u64)>,
    database_file: File,
    database: Database,
    // An I/O failure during commit has an unknown outcome. Stop until reopen
    // reads the database's recovered atomic checkpoint, rather than guessing.
    poisoned: bool,
    // Keep ownership locked until redb Drop finishes its allocator writes.
    _owner_lock: File,
}

impl ContinuousCapture {
    pub fn binding(&self) -> &CaptureBinding {
        &self.owner.binding
    }

    pub fn open(config: CaptureConfig) -> Result<Self, CaptureError> {
        Self::open_with(config, |file| {
            Database::builder()
                .set_cache_size(CACHE_BYTES)
                .create_file(file)
                .map_err(|error| match error {
                    redb::DatabaseError::DatabaseAlreadyOpen => CaptureError::Busy,
                    _ => CaptureError::InvalidState,
                })
        })
    }

    fn open_with(
        mut config: CaptureConfig,
        make_database: impl FnOnce(File) -> Result<Database, CaptureError>,
    ) -> Result<Self, CaptureError> {
        validate_config(&config)?;
        let input =
            ArchiveFiles::open(&config.input_directory).map_err(|_| CaptureError::UnsafePath)?;
        let input_identity = identity(&input.directory_metadata().map_err(|_| CaptureError::Io)?);
        config.input_directory =
            std::fs::canonicalize(&config.input_directory).map_err(|_| CaptureError::UnsafePath)?;
        if identity(
            &open_directory(&config.input_directory)?
                .into_std_file()
                .metadata()
                .map_err(|_| CaptureError::Io)?,
        ) != input_identity
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
        let parent_path = config
            .output_directory
            .parent()
            .ok_or(CaptureError::UnsafePath)?;
        let parent = open_directory(parent_path)?;
        let leaf = config
            .output_directory
            .file_name()
            .ok_or(CaptureError::UnsafePath)?
            .to_owned();
        let parent_path =
            std::fs::canonicalize(parent_path).map_err(|_| CaptureError::UnsafePath)?;
        if identity(
            &parent
                .try_clone()
                .map_err(|_| CaptureError::Io)?
                .into_std_file()
                .metadata()
                .map_err(|_| CaptureError::Io)?,
        ) != identity(
            &open_directory(&parent_path)?
                .into_std_file()
                .metadata()
                .map_err(|_| CaptureError::Io)?,
        ) {
            return Err(CaptureError::UnsafePath);
        }
        config.output_directory = parent_path.join(&leaf);
        if config.output_directory.starts_with(&config.input_directory)
            || config.input_directory.starts_with(&config.output_directory)
        {
            return Err(CaptureError::UnsafePath);
        }
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        let created = match parent.create_dir_with(&leaf, &builder) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(_) => return Err(CaptureError::Io),
        };
        if created {
            sync_directory(&parent)?;
        }
        let directory = parent
            .open_dir_nofollow(&leaf)
            .map_err(|_| CaptureError::UnsafePath)?;
        let directory_metadata = directory
            .try_clone()
            .map_err(|_| CaptureError::Io)?
            .into_std_file()
            .metadata()
            .map_err(|_| CaptureError::Io)?;
        validate_directory(&directory_metadata)?;
        let directory_identity = identity(&directory_metadata);
        let (owner_file, owner, database_file) = if created {
            let database_file = open_file(&directory, DATABASE_FILE, true, true)?;
            database_file.sync_all().map_err(|_| CaptureError::Io)?;
            let owner = Owner {
                schema_version: 1,
                binding,
                database_identity: identity(
                    &database_file.metadata().map_err(|_| CaptureError::Io)?,
                ),
            };
            let mut owner_file = open_file(&directory, OWNER_FILE, true, true)?;
            owner_file
                .write_all(&encode(&owner, MAX_METADATA_BYTES)?)
                .map_err(|_| CaptureError::Io)?;
            owner_file.sync_all().map_err(|_| CaptureError::Io)?;
            sync_directory(&directory)?;
            (owner_file, owner, database_file)
        } else {
            // Never call redb create/recovery until ownership is verified:
            // create_file would otherwise initialize an arbitrary empty FD.
            for entry in directory.entries().map_err(|_| CaptureError::Io)? {
                let entry = entry.map_err(|_| CaptureError::Io)?;
                if ![DATABASE_FILE, OWNER_FILE, READY_FILE]
                    .iter()
                    .any(|name| entry.file_name() == *name)
                {
                    return Err(CaptureError::UnownedOutput);
                }
            }
            let mut owner_file = open_file(&directory, OWNER_FILE, false, false)
                .map_err(|_| CaptureError::UnownedOutput)?;
            let owner: Owner = decode(&read_bounded(&mut owner_file, MAX_METADATA_BYTES)?)?;
            if owner.schema_version != 1 {
                return Err(CaptureError::InvalidState);
            }
            if owner.binding != binding {
                return Err(CaptureError::BindingMismatch);
            }
            let database_file = open_file(&directory, DATABASE_FILE, true, false)
                .map_err(|_| CaptureError::UnownedOutput)?;
            if identity(&database_file.metadata().map_err(|_| CaptureError::Io)?)
                != owner.database_identity
            {
                return Err(CaptureError::UnownedOutput);
            }
            (owner_file, owner, database_file)
        };
        let owner_identity = identity(&owner_file.metadata().map_err(|_| CaptureError::Io)?);
        let owner_digest = digest(&encode(&owner, MAX_METADATA_BYTES)?);
        FileExt::try_lock(&owner_file).map_err(|_| CaptureError::Busy)?;
        let seal = match open_file(&directory, READY_FILE, false, false) {
            Ok(mut file) => {
                let bytes = read_bounded(&mut file, MAX_METADATA_BYTES)?;
                // A crash while writing the seal may leave a prefix. Complete
                // it only after validating this exact owned DB's checkpoint.
                if !owner_digest.as_bytes().starts_with(&bytes) {
                    return Err(CaptureError::InvalidState);
                }
                Some((
                    identity(&file.metadata().map_err(|_| CaptureError::Io)?),
                    bytes,
                ))
            }
            Err(CaptureError::Io) => {
                if directory.symlink_metadata(READY_FILE).is_ok() {
                    return Err(CaptureError::UnsafePath);
                }
                None
            }
            Err(error) => return Err(error),
        };
        let initialized = seal.is_some();
        if initialized
            && database_file
                .metadata()
                .map_err(|_| CaptureError::Io)?
                .len()
                == 0
        {
            return Err(CaptureError::InvalidState);
        }
        let retained_file = database_file.try_clone().map_err(|_| CaptureError::Io)?;
        let database = make_database(database_file)?;
        let mut session = Self {
            config,
            input,
            input_identity,
            directory,
            directory_identity,
            owner,
            owner_identity,
            owner_digest,
            ready_identity: None,
            _owner_lock: owner_file,
            database_file: retained_file,
            database,
            poisoned: false,
        };
        session.check_paths()?;
        let initial = Checkpoint {
            follower: CaptureState {
                schema_version: 1,
                binding: session.owner.binding.clone(),
                events: Vec::new(),
                gaps: vec![GapCount {
                    gap: CaptureGap::StartedWithoutHistory,
                    count: 1,
                }],
                cursors: Vec::new(),
                next_file: None,
            },
            last_sequence: 0,
            observation_revision: 0,
            applied: None,
        };
        let transaction = session
            .database
            .begin_read()
            .map_err(|_| CaptureError::Io)?;
        let checkpoint = match transaction.open_table(META) {
            Ok(table) => {
                let value = table
                    .get("checkpoint")
                    .map_err(|_| CaptureError::Io)?
                    .ok_or(CaptureError::InvalidState)?;
                Some(decode_metadata::<Checkpoint>(value.value())?)
            }
            Err(redb::TableError::TableDoesNotExist(_)) if !initialized => None,
            Err(_) => return Err(CaptureError::InvalidState),
        };
        drop(transaction);
        if let Some(checkpoint) = checkpoint {
            session.validate_checkpoint(&checkpoint)?;
        } else {
            let mut transaction = session
                .database
                .begin_write()
                .map_err(|_| CaptureError::Io)?;
            transaction.set_durability(Durability::Immediate);
            {
                transaction
                    .open_table(EVENTS)
                    .map_err(|_| CaptureError::Io)?;
                transaction.open_table(SEEN).map_err(|_| CaptureError::Io)?;
                transaction
                    .open_table(LATEST)
                    .map_err(|_| CaptureError::Io)?;
                let mut table = transaction.open_table(META).map_err(|_| CaptureError::Io)?;
                table
                    .insert(
                        "checkpoint",
                        encode(&initial, MAX_METADATA_BYTES)?.as_slice(),
                    )
                    .map_err(|_| CaptureError::Io)?;
            }
            transaction.commit().map_err(|_| CaptureError::Io)?;
        }
        if seal
            .as_ref()
            .is_none_or(|(_, bytes)| bytes != session.owner_digest.as_bytes())
        {
            let mut ready = open_file(&session.directory, READY_FILE, true, seal.is_none())?;
            let previous = read_bounded(&mut ready, MAX_METADATA_BYTES)?;
            if let Some((expected, bytes)) = &seal {
                if identity(&ready.metadata().map_err(|_| CaptureError::Io)?) != *expected
                    || previous != *bytes
                {
                    return Err(CaptureError::UnownedOutput);
                }
            } else if !previous.is_empty() {
                return Err(CaptureError::UnownedOutput);
            }
            ready
                .write_all(&session.owner_digest.as_bytes()[previous.len()..])
                .map_err(|_| CaptureError::Io)?;
            ready.sync_all().map_err(|_| CaptureError::Io)?;
            sync_directory(&session.directory)?;
        }
        // No cached checkpoint is authoritative: always load it from the DB.
        session.ready_identity = Some(identity(
            &open_file(&session.directory, READY_FILE, false, false)?
                .metadata()
                .map_err(|_| CaptureError::Io)?,
        ));
        session.poisoned = false;
        session.check_paths()?;
        Ok(session)
    }

    pub fn database_path(&self) -> PathBuf {
        self.config.output_directory.join(DATABASE_FILE)
    }

    pub fn status(&self) -> Result<JournalStatus, CaptureError> {
        self.check_paths()?;
        let checkpoint = self.checkpoint()?;
        Ok(JournalStatus {
            events: checkpoint.last_sequence,
            observation_revision: checkpoint.observation_revision,
            applied: checkpoint.applied,
            gaps: checkpoint.follower.gaps,
        })
    }

    pub fn poll(&mut self) -> Result<PollReport, CaptureError> {
        let result = self.poll_inner();
        if result == Err(CaptureError::Io) {
            self.poisoned = true;
        }
        result
    }

    fn poll_inner(&mut self) -> Result<PollReport, CaptureError> {
        let result = self.poll_transaction(|| Ok(()), || Ok(()));
        if result == Err(CaptureError::SourceChanged) {
            let mut checkpoint = self.checkpoint()?;
            let reason = CaptureGap::SourceRewrittenDuringPoll;
            if let Some(gap) = checkpoint
                .follower
                .gaps
                .iter_mut()
                .find(|gap| gap.gap == reason)
            {
                gap.count = gap.count.saturating_add(1);
            } else {
                checkpoint.follower.gaps.push(GapCount {
                    gap: reason,
                    count: 1,
                });
            }
            checkpoint.observation_revision = checkpoint
                .observation_revision
                .checked_add(1)
                .ok_or(CaptureError::Capacity)?;
            self.write_checkpoint(&checkpoint)?;
        }
        result
    }

    fn poll_transaction(
        &mut self,
        mut after_event: impl FnMut() -> Result<(), CaptureError>,
        before_commit: impl FnOnce() -> Result<(), CaptureError>,
    ) -> Result<PollReport, CaptureError> {
        self.check_paths()?;
        let mut checkpoint = self.checkpoint()?;
        let previous = checkpoint.clone();
        let mut transaction = self.database.begin_write().map_err(|_| CaptureError::Io)?;
        transaction.set_durability(Durability::Immediate);
        let report = {
            let mut events = transaction
                .open_table(EVENTS)
                .map_err(|_| CaptureError::Io)?;
            let mut seen = transaction.open_table(SEEN).map_err(|_| CaptureError::Io)?;
            let mut latest = transaction
                .open_table(LATEST)
                .map_err(|_| CaptureError::Io)?;
            follow_logs(
                &self.config,
                &self.input,
                &mut checkpoint.follower,
                |event| {
                    let hash = event_hash(&event)?;
                    if seen
                        .get(hash.as_str())
                        .map_err(|_| CaptureError::Io)?
                        .is_some()
                    {
                        return Ok(false);
                    }
                    checkpoint.last_sequence = checkpoint
                        .last_sequence
                        .checked_add(1)
                        .ok_or(CaptureError::Capacity)?;
                    let sequence = checkpoint.last_sequence;
                    events
                        .insert(sequence, encode(&event, MAX_EVENT_BYTES)?.as_slice())
                        .map_err(|_| CaptureError::Io)?;
                    seen.insert(hash.as_str(), sequence)
                        .map_err(|_| CaptureError::Io)?;
                    match &event {
                        ParsedEvent::Message {
                            message_id,
                            timestamp,
                            kind,
                            ..
                        } => {
                            let previous = latest
                                .get(message_id.as_str())
                                .map_err(|_| CaptureError::Io)?;
                            let mut current: JournalMessage = previous
                                .as_ref()
                                .map(|value| decode(value.value()))
                                .transpose()?
                                .unwrap_or(JournalMessage {
                                    event: None,
                                    versions: 0,
                                    deleted: false,
                                });
                            drop(previous);
                            current.versions = current
                                .versions
                                .checked_add(1)
                                .ok_or(CaptureError::Capacity)?;
                            let replace = match &current.event {
                                Some(ParsedEvent::Message { timestamp: old, .. }) => {
                                    timestamp > old
                                        || (*kind == MessageKind::Edit && timestamp == old)
                                }
                                None => true,
                                _ => return Err(CaptureError::InvalidState),
                            };
                            if replace {
                                current.event = Some(event.clone());
                            }
                            latest
                                .insert(
                                    message_id.as_str(),
                                    encode(&current, MAX_EVENT_BYTES)?.as_slice(),
                                )
                                .map_err(|_| CaptureError::Io)?;
                        }
                        ParsedEvent::Delete { message_ids, .. } => {
                            for id in message_ids {
                                let previous =
                                    latest.get(id.as_str()).map_err(|_| CaptureError::Io)?;
                                let mut current: JournalMessage = previous
                                    .as_ref()
                                    .map(|value| decode(value.value()))
                                    .transpose()?
                                    .unwrap_or(JournalMessage {
                                        event: None,
                                        versions: 0,
                                        deleted: false,
                                    });
                                drop(previous);
                                current.deleted = true;
                                latest
                                    .insert(
                                        id.as_str(),
                                        encode(&current, MAX_EVENT_BYTES)?.as_slice(),
                                    )
                                    .map_err(|_| CaptureError::Io)?;
                            }
                        }
                    }
                    after_event()?;
                    Ok(true)
                },
            )?
        };
        if checkpoint == previous {
            return Ok(report);
        }
        if checkpoint.last_sequence != previous.last_sequence
            || checkpoint.follower.gaps != previous.follower.gaps
        {
            checkpoint.observation_revision = checkpoint
                .observation_revision
                .checked_add(1)
                .ok_or(CaptureError::Capacity)?;
        }
        {
            let mut meta = transaction.open_table(META).map_err(|_| CaptureError::Io)?;
            meta.insert(
                "checkpoint",
                encode(&checkpoint, MAX_METADATA_BYTES)?.as_slice(),
            )
            .map_err(|_| CaptureError::Io)?;
        }
        self.check_paths()?;
        before_commit()?;
        if transaction.commit().is_err() {
            self.poisoned = true;
            return Err(CaptureError::Io);
        }
        Ok(report)
    }

    /// Bounded count AND byte size; each page drops its read transaction.
    pub fn events_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<JournalEvent>, CaptureError> {
        self.check_paths()?;
        if limit == 0 || limit > MAX_PAGE_EVENTS || sequence > self.checkpoint()?.last_sequence {
            return Err(CaptureError::InvalidConfiguration);
        }
        let transaction = self.database.begin_read().map_err(|_| CaptureError::Io)?;
        let table = transaction
            .open_table(EVENTS)
            .map_err(|_| CaptureError::InvalidState)?;
        let mut result = Vec::new();
        let mut bytes = 0usize;
        if sequence == u64::MAX {
            return Ok(result);
        }
        for entry in table
            .range((sequence + 1)..)
            .map_err(|_| CaptureError::Io)?
        {
            let (key, value) = entry.map_err(|_| CaptureError::Io)?;
            if value.value().len() > MAX_EVENT_BYTES {
                return Err(CaptureError::Capacity);
            }
            if bytes + value.value().len() > MAX_EVENT_BYTES || result.len() == limit {
                break;
            }
            bytes += value.value().len();
            result.push(JournalEvent {
                sequence: key.value(),
                event: decode(value.value())?,
            });
        }
        Ok(result)
    }

    pub fn latest_message(&self, id: &str) -> Result<Option<JournalMessage>, CaptureError> {
        self.check_paths()?;
        if !valid_id(id) {
            return Err(CaptureError::InvalidConfiguration);
        }
        let transaction = self.database.begin_read().map_err(|_| CaptureError::Io)?;
        let table = transaction
            .open_table(LATEST)
            .map_err(|_| CaptureError::InvalidState)?;
        table
            .get(id)
            .map_err(|_| CaptureError::Io)?
            .map(|value| decode(value.value()))
            .transpose()
    }

    pub fn acknowledge(&mut self, applied: AppliedCheckpoint) -> Result<(), CaptureError> {
        let result = self.acknowledge_inner(applied);
        if result == Err(CaptureError::Io) {
            self.poisoned = true;
        }
        result
    }

    fn acknowledge_inner(&mut self, applied: AppliedCheckpoint) -> Result<(), CaptureError> {
        self.check_paths()?;
        let mut checkpoint = self.checkpoint()?;
        if applied.sequence > checkpoint.last_sequence
            || applied.observation_revision > checkpoint.observation_revision
            || applied.snapshot_id.is_empty()
            || applied.snapshot_id.len() > 1024
            || applied.snapshot_id.chars().any(char::is_control)
        {
            return Err(CaptureError::InvalidConfiguration);
        }
        if let Some(previous) = &checkpoint.applied {
            if applied == *previous {
                return Ok(());
            }
            if applied.sequence < previous.sequence
                || applied.observation_revision < previous.observation_revision
                || (applied.sequence == previous.sequence
                    && applied.observation_revision == previous.observation_revision)
            {
                return Err(CaptureError::InvalidConfiguration);
            }
        }
        checkpoint.applied = Some(applied);
        self.write_checkpoint(&checkpoint)
    }

    fn write_checkpoint(&mut self, checkpoint: &Checkpoint) -> Result<(), CaptureError> {
        let mut transaction = self.database.begin_write().map_err(|_| CaptureError::Io)?;
        transaction.set_durability(Durability::Immediate);
        {
            let mut meta = transaction.open_table(META).map_err(|_| CaptureError::Io)?;
            meta.insert(
                "checkpoint",
                encode(checkpoint, MAX_METADATA_BYTES)?.as_slice(),
            )
            .map_err(|_| CaptureError::Io)?;
        }
        self.check_paths()?;
        if transaction.commit().is_err() {
            self.poisoned = true;
            return Err(CaptureError::Io);
        }
        Ok(())
    }

    fn checkpoint(&self) -> Result<Checkpoint, CaptureError> {
        let transaction = self.database.begin_read().map_err(|_| CaptureError::Io)?;
        let table = transaction
            .open_table(META)
            .map_err(|_| CaptureError::InvalidState)?;
        let value = table
            .get("checkpoint")
            .map_err(|_| CaptureError::Io)?
            .ok_or(CaptureError::InvalidState)?;
        let checkpoint = decode_metadata(value.value())?;
        self.validate_checkpoint(&checkpoint)?;
        Ok(checkpoint)
    }

    fn validate_checkpoint(&self, checkpoint: &Checkpoint) -> Result<(), CaptureError> {
        validate_state(&checkpoint.follower, self.config.limits)?;
        if !checkpoint.follower.events.is_empty()
            || checkpoint.follower.binding != self.owner.binding
            || checkpoint.applied.as_ref().is_some_and(|applied| {
                applied.sequence > checkpoint.last_sequence
                    || applied.observation_revision > checkpoint.observation_revision
                    || applied.snapshot_id.is_empty()
                    || applied.snapshot_id.len() > 1024
                    || applied.snapshot_id.chars().any(char::is_control)
            })
        {
            return Err(CaptureError::InvalidState);
        }
        Ok(())
    }

    fn check_paths(&self) -> Result<(), CaptureError> {
        if self.poisoned {
            return Err(CaptureError::Io);
        }
        let current = open_directory(&self.config.output_directory)?;
        let metadata = current
            .try_clone()
            .map_err(|_| CaptureError::Io)?
            .into_std_file()
            .metadata()
            .map_err(|_| CaptureError::Io)?;
        validate_directory(&metadata)?;
        if identity(&metadata) != self.directory_identity {
            return Err(CaptureError::UnsafePath);
        }
        let mut marker = open_file(&current, OWNER_FILE, false, false)?;
        if identity(&marker.metadata().map_err(|_| CaptureError::Io)?) != self.owner_identity
            || digest(&read_bounded(&mut marker, MAX_METADATA_BYTES)?) != self.owner_digest
        {
            return Err(CaptureError::UnownedOutput);
        }
        let file = open_file(&current, DATABASE_FILE, false, false)?;
        if identity(&file.metadata().map_err(|_| CaptureError::Io)?) != self.owner.database_identity
        {
            return Err(CaptureError::UnownedOutput);
        }
        validate_file(
            &self
                .database_file
                .metadata()
                .map_err(|_| CaptureError::Io)?,
        )?;
        if let Some(expected) = self.ready_identity {
            let mut ready = open_file(&current, READY_FILE, false, false)?;
            if identity(&ready.metadata().map_err(|_| CaptureError::Io)?) != expected
                || read_bounded(&mut ready, MAX_METADATA_BYTES)? != self.owner_digest.as_bytes()
            {
                return Err(CaptureError::UnownedOutput);
            }
        }
        let input = open_directory(&self.config.input_directory)?;
        if identity(
            &input
                .into_std_file()
                .metadata()
                .map_err(|_| CaptureError::Io)?,
        ) != self.input_identity
        {
            return Err(CaptureError::UnsafePath);
        }
        Ok(())
    }
}

fn encode<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, CaptureError> {
    let bytes = serde_json::to_vec(value).map_err(|_| CaptureError::InvalidState)?;
    if bytes.len() > limit {
        return Err(CaptureError::Capacity);
    }
    Ok(bytes)
}
fn decode<T: for<'a> Deserialize<'a>>(bytes: &[u8]) -> Result<T, CaptureError> {
    if bytes.len() > MAX_EVENT_BYTES {
        return Err(CaptureError::Capacity);
    }
    serde_json::from_slice(bytes).map_err(|_| CaptureError::InvalidState)
}
fn decode_metadata<T: for<'a> Deserialize<'a>>(bytes: &[u8]) -> Result<T, CaptureError> {
    if bytes.len() > MAX_METADATA_BYTES {
        return Err(CaptureError::Capacity);
    }
    decode(bytes)
}
fn read_bounded(file: &mut File, limit: usize) -> Result<Vec<u8>, CaptureError> {
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CaptureError::Io)?;
    if bytes.len() > limit {
        return Err(CaptureError::Capacity);
    }
    Ok(bytes)
}
fn open_directory(path: &Path) -> Result<Dir, CaptureError> {
    if !path.is_absolute() {
        return Err(CaptureError::UnsafePath);
    }
    let mut directory = Dir::open_ambient_dir("/", cap_std::ambient_authority())
        .map_err(|_| CaptureError::UnsafePath)?;
    for component in path.components() {
        match component {
            Component::RootDir => (),
            Component::Normal(name) => {
                directory = directory
                    .open_dir_nofollow(name)
                    .map_err(|_| CaptureError::UnsafePath)?;
            }
            _ => return Err(CaptureError::UnsafePath),
        }
    }
    Ok(directory)
}
fn open_file(directory: &Dir, name: &str, write: bool, create: bool) -> Result<File, CaptureError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(write)
        .create_new(create)
        .follow(FollowSymlinks::No)
        .nonblock(true)
        .mode(0o600);
    let file = directory
        .open_with(name, &options)
        .map_err(|_| CaptureError::Io)?
        .into_std();
    validate_file(&file.metadata().map_err(|_| CaptureError::Io)?)?;
    Ok(file)
}
fn validate_file(metadata: &Metadata) -> Result<(), CaptureError> {
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(CaptureError::UnsafePath);
    }
    private_permissions(metadata, false)
}
fn sync_directory(directory: &Dir) -> Result<(), CaptureError> {
    directory
        .open(".")
        .map_err(|_| CaptureError::Io)?
        .into_std()
        .sync_all()
        .map_err(|_| CaptureError::Io)
}
fn validate_directory(metadata: &Metadata) -> Result<(), CaptureError> {
    if !metadata.is_dir() || metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(CaptureError::UnsafePath);
    }
    private_permissions(metadata, true)
}

#[cfg(test)]
#[path = "continuous_tests.rs"]
mod tests;
