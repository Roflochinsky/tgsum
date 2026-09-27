//! Project-local pseudonym allocation. Originals never belong in an agent bundle.
//!
//! Callers provide canonical identity (including source/account namespace where
//! appropriate) separately from its display alias. No implicit normalization is
//! performed here. Allocation is append-only within an epoch, bounded in memory,
//! and published by the Project revision CAS after an immutable private file.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::project::{Project, ProjectStore};

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_ALIASES: usize = 100_000;
const MAX_ALIASES_PER_IDENTITY: usize = 256;
const MAX_IDENTITY_BYTES: usize = 4096;
const MAX_ORIGINAL_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PseudonymCategory {
    Person,
    Username,
    Email,
    Phone,
    Ip,
    Host,
    Domain,
    Url,
    Path,
    CloudResource,
    CustomTerm,
}

impl PseudonymCategory {
    fn prefix(self) -> &'static str {
        match self {
            Self::Person => "PERSON",
            Self::Username => "USER",
            Self::Email => "EMAIL",
            Self::Phone => "PHONE",
            Self::Ip => "IP",
            Self::Host => "HOST",
            Self::Domain => "DOMAIN",
            Self::Url => "URL",
            Self::Path => "PATH",
            Self::CloudResource => "RESOURCE",
            Self::CustomTerm => "TERM",
        }
    }
}

/// Sensitive input: deliberately has no Debug or serialization implementation.
pub struct PseudonymInput<'a> {
    pub category: PseudonymCategory,
    pub identity: &'a str,
    pub original: &'a str,
}

/// Private metadata, including a digest of plaintext. Only `id()` may be exported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingRef {
    id: String,
    epoch: String,
    generation: u64,
    sha256: String,
}

impl MappingRef {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn epoch(&self) -> &str {
        &self.epoch
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        if !opaque_id(&self.id, "map-")
            || !opaque_id(&self.epoch, "epoch-")
            || self.sha256.len() != 64
            || !hexadecimal(&self.sha256)
        {
            return Err(invalid("invalid private mapping reference"));
        }
        Ok(())
    }
}

/// Labels are returned in the caller's input order; Project contains the new
/// revision to use for subsequent Review. Identical imports do not bump it.
pub struct PseudonymAssignment {
    pub project: Project,
    pub labels: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    category: PseudonymCategory,
    identity: String,
    originals: BTreeSet<String>,
    pseudonym: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredMapping {
    schema_version: u32,
    project_id: String,
    id: String,
    epoch: String,
    generation: u64,
    entries: Vec<Entry>,
}

/// Sensitive local lookup, deliberately neither Debug nor Serialize. Historical
/// reads use an explicit MappingRef; they never fall back to the current epoch.
pub struct PseudonymMapping {
    data: StoredMapping,
    identities: BTreeMap<(PseudonymCategory, String), usize>,
    labels: BTreeMap<String, usize>,
}

impl PseudonymMapping {
    pub(crate) fn category_entries(
        &self,
        category: PseudonymCategory,
    ) -> impl Iterator<Item = (&str, &str, &BTreeSet<String>)> {
        self.data
            .entries
            .iter()
            .filter(move |e| e.category == category)
            .map(|e| (e.identity.as_str(), e.pseudonym.as_str(), &e.originals))
    }

    pub fn lookup(&self, category: PseudonymCategory, identity: &str) -> Option<&str> {
        let index = self.identities.get(&(category, identity.to_owned()))?;
        Some(&self.data.entries[*index].pseudonym)
    }

    pub fn originals(&self, pseudonym: &str) -> Option<Vec<&str>> {
        let index = self.labels.get(pseudonym)?;
        Some(
            self.data.entries[*index]
                .originals
                .iter()
                .map(String::as_str)
                .collect(),
        )
    }

    fn checked(data: StoredMapping) -> io::Result<Self> {
        if data.schema_version != 1 || data.entries.len() > MAX_ENTRIES {
            return Err(invalid("unsupported or oversized private mapping"));
        }
        let mut identities = BTreeMap::new();
        let mut labels = BTreeMap::new();
        let mut counters = BTreeMap::<PseudonymCategory, usize>::new();
        let mut aliases = 0usize;
        for (index, entry) in data.entries.iter().enumerate() {
            valid_value(&entry.identity, MAX_IDENTITY_BYTES)?;
            aliases += entry.originals.len();
            if entry.originals.is_empty()
                || entry.originals.len() > MAX_ALIASES_PER_IDENTITY
                || aliases > MAX_ALIASES
            {
                return Err(invalid(
                    "private mapping exceeds alias limits or has no original value",
                ));
            }
            for original in &entry.originals {
                valid_value(original, MAX_ORIGINAL_BYTES)?;
            }
            let counter = counters.entry(entry.category).or_default();
            *counter += 1;
            if entry.pseudonym != label(entry.category, *counter)
                || identities
                    .insert((entry.category, entry.identity.clone()), index)
                    .is_some()
                || labels.insert(entry.pseudonym.clone(), index).is_some()
            {
                return Err(invalid("invalid or duplicate private mapping entry"));
            }
        }
        Ok(Self {
            data,
            identities,
            labels,
        })
    }
}

/// Private transaction state shared by multi-field bundle preparation. A failed
/// allocation poisons the draft; callers cannot accidentally stage partial data.
pub(crate) struct MappingDraft {
    mapping: PseudonymMapping,
    previous: Option<MappingRef>,
    counters: BTreeMap<PseudonymCategory, usize>,
    aliases: usize,
    bytes: usize,
    changed: bool,
    poisoned: bool,
}

impl MappingDraft {
    pub(crate) fn mapping(&self) -> &PseudonymMapping {
        &self.mapping
    }

    pub(crate) fn allocate(&mut self, inputs: &[PseudonymInput<'_>]) -> io::Result<Vec<String>> {
        if self.poisoned {
            return Err(invalid("private mapping draft already failed"));
        }
        self.poisoned = true;
        let result = self.allocate_inner(inputs);
        if result.is_ok() {
            self.poisoned = false;
        }
        result
    }

    fn allocate_inner(&mut self, inputs: &[PseudonymInput<'_>]) -> io::Result<Vec<String>> {
        validate_inputs(inputs)?;
        let sorted: BTreeSet<_> = inputs
            .iter()
            .map(|i| (i.category, i.identity, i.original))
            .collect();
        for (category, identity, original) in sorted {
            let key = (category, identity.to_owned());
            if let Some(index) = self.mapping.identities.get(&key).copied() {
                let originals = &self.mapping.data.entries[index].originals;
                if originals.contains(original) {
                    continue;
                }
                if originals.len() == MAX_ALIASES_PER_IDENTITY || self.aliases == MAX_ALIASES {
                    return Err(invalid("private mapping exceeds alias limits"));
                }
                self.mark_changed()?;
                self.reserve(encoded_size(original)? + 1)?; // Existing alias array needs a comma.
                self.mapping.data.entries[index]
                    .originals
                    .insert(original.to_owned());
            } else {
                if self.mapping.data.entries.len() == MAX_ENTRIES || self.aliases == MAX_ALIASES {
                    return Err(invalid("private mapping exceeds entry limit"));
                }
                let counter = self.counters.get(&category).copied().unwrap_or(0) + 1;
                let entry = Entry {
                    category,
                    identity: identity.into(),
                    originals: BTreeSet::from([original.to_owned()]),
                    pseudonym: label(category, counter),
                };
                let bytes =
                    encoded_size(&entry)? + usize::from(!self.mapping.data.entries.is_empty());
                self.mark_changed()?;
                self.reserve(bytes)?;
                let index = self.mapping.data.entries.len();
                self.mapping.identities.insert(key, index);
                self.mapping.labels.insert(entry.pseudonym.clone(), index);
                self.mapping.data.entries.push(entry);
                self.counters.insert(category, counter);
            }
            self.aliases += 1;
        }
        Ok(inputs
            .iter()
            .map(|i| {
                self.mapping
                    .lookup(i.category, i.identity)
                    .expect("allocated above")
                    .to_owned()
            })
            .collect())
    }

    fn mark_changed(&mut self) -> io::Result<()> {
        if !self.changed {
            if let Some(previous) = &self.previous {
                let generation = previous
                    .generation
                    .checked_add(1)
                    .ok_or_else(|| invalid("private mapping generation exhausted"))?;
                self.reserve(generation.to_string().len() - previous.generation.to_string().len())?;
                self.mapping.data.generation = generation;
                self.mapping.data.id = random_id("map-")?;
            }
            self.changed = true;
        }
        Ok(())
    }

    fn reserve(&mut self, extra: usize) -> io::Result<()> {
        self.bytes = self
            .bytes
            .checked_add(extra)
            .filter(|size| *size <= MAX_BYTES)
            .ok_or_else(|| invalid("private mapping exceeds storage limit"))?;
        Ok(())
    }
}

impl ProjectStore {
    pub(crate) fn draft_pseudonyms(&self, project: &Project) -> io::Result<MappingDraft> {
        let mapping = match &project.pseudonyms {
            Some(reference) => self.load_pseudonyms(&project.project_id, reference)?,
            None => PseudonymMapping::checked(empty_mapping(&project.project_id)?)?,
        };
        let mut counters = BTreeMap::new();
        for entry in &mapping.data.entries {
            *counters.entry(entry.category).or_default() += 1;
        }
        Ok(MappingDraft {
            aliases: mapping.data.entries.iter().map(|e| e.originals.len()).sum(),
            bytes: encoded_size(&mapping.data)?,
            mapping,
            counters,
            previous: project.pseudonyms.clone(),
            changed: false,
            poisoned: false,
        })
    }

    /// Stage at most one immutable map. Project CAS remains the caller's final
    /// commit, after all dependent bundle files/checks have succeeded.
    pub(crate) fn stage_pseudonyms(&self, draft: MappingDraft) -> io::Result<Option<MappingRef>> {
        if draft.poisoned {
            return Err(invalid("private mapping draft already failed"));
        }
        if draft.changed {
            self.store_pseudonyms(&draft.mapping.data).map(Some)
        } else {
            Ok(draft.previous)
        }
    }

    /// Explicit reset: publish an empty new epoch and invalidate old Review via
    /// the Project revision. Historical mappings and analysis references remain
    /// readable. This is not erasure; deleting retained private data is separate.
    pub fn reset_pseudonyms(
        &self,
        project_id: &str,
        expected_revision: u64,
    ) -> io::Result<Project> {
        let project = self.open(project_id)?;
        check_revision(project.revision, expected_revision)?;
        // Never silently regenerate lost/corrupt state, even during reset.
        if let Some(reference) = &project.pseudonyms {
            self.load_pseudonyms(project_id, reference)?;
        }
        let reference = self.store_pseudonyms(&empty_mapping(project_id)?)?;
        self.publish_pseudonyms(project_id, expected_revision, reference)
    }

    /// Allocate stable labels in a bounded batch, retaining aliases. Newly seen
    /// identities are sorted within the batch; existing labels never renumber.
    /// Concurrent writers return WouldBlock and must reload/retry. A failed CAS
    /// may leave an unreferenced immutable file, never a replacement current map.
    pub fn assign_pseudonyms(
        &self,
        project_id: &str,
        expected_revision: u64,
        inputs: &[PseudonymInput<'_>],
    ) -> io::Result<PseudonymAssignment> {
        validate_inputs(inputs)?;
        let project = self.open(project_id)?;
        check_revision(project.revision, expected_revision)?;
        let mut draft = self.draft_pseudonyms(&project)?;
        let labels = draft.allocate(inputs)?;
        if !draft.changed {
            check_revision(self.open(project_id)?.revision, expected_revision)?;
            return Ok(PseudonymAssignment { project, labels });
        }
        let reference = self
            .stage_pseudonyms(draft)?
            .expect("changed draft has a mapping");
        let project = self.publish_pseudonyms(project_id, expected_revision, reference)?;
        Ok(PseudonymAssignment { project, labels })
    }

    pub fn load_pseudonyms(
        &self,
        project_id: &str,
        reference: &MappingRef,
    ) -> io::Result<PseudonymMapping> {
        reference.validate()?;
        let directory = self.directory(project_id)?.join("pseudonyms");
        require_directory(&directory)?;
        let path = directory.join(format!("{}.json", reference.id));
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.len() > MAX_BYTES as u64 {
            return Err(invalid("private mapping must be a bounded regular file"));
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_BYTES || digest(&bytes) != reference.sha256 {
            return Err(invalid("private mapping integrity mismatch"));
        }
        let data: StoredMapping =
            serde_json::from_slice(&bytes).map_err(|_| invalid("invalid private mapping data"))?;
        if data.project_id != project_id
            || data.id != reference.id
            || data.epoch != reference.epoch
            || data.generation != reference.generation
        {
            return Err(invalid("private mapping identity mismatch"));
        }
        PseudonymMapping::checked(data)
    }

    fn store_pseudonyms(&self, data: &StoredMapping) -> io::Result<MappingRef> {
        let directory = self.directory(&data.project_id)?.join("pseudonyms");
        if !directory.exists() {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(false);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&directory) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        require_directory(&directory)?;
        let mut bytes = BoundedBytes(Vec::new());
        serde_json::to_writer(&mut bytes, data)
            .map_err(|_| invalid("private mapping exceeds storage limit"))?;
        let reference = MappingRef {
            id: data.id.clone(),
            epoch: data.epoch.clone(),
            generation: data.generation,
            sha256: digest(&bytes.0),
        };
        let mut staged = tempfile::NamedTempFile::new_in(&directory)?;
        staged.write_all(&bytes.0)?;
        staged.as_file().sync_all()?;
        staged
            .persist_noclobber(directory.join(format!("{}.json", data.id)))
            .map_err(|e| e.error)?;
        #[cfg(unix)]
        File::open(&directory)?.sync_all()?;
        Ok(reference)
    }
}

fn empty_mapping(project_id: &str) -> io::Result<StoredMapping> {
    Ok(StoredMapping {
        schema_version: 1,
        project_id: project_id.into(),
        id: random_id("map-")?,
        epoch: random_id("epoch-")?,
        generation: 0,
        entries: Vec::new(),
    })
}

fn validate_inputs(inputs: &[PseudonymInput<'_>]) -> io::Result<()> {
    if inputs.len() > MAX_ENTRIES {
        return Err(invalid("private mapping batch exceeds entry limit"));
    }
    let mut bytes = 0usize;
    for input in inputs {
        valid_value(input.identity, MAX_IDENTITY_BYTES)?;
        valid_value(input.original, MAX_ORIGINAL_BYTES)?;
        bytes = bytes
            .saturating_add(input.identity.len())
            .saturating_add(input.original.len());
        if bytes > MAX_BYTES {
            return Err(invalid("private mapping batch exceeds byte limit"));
        }
    }
    Ok(())
}

fn valid_value(value: &str, limit: usize) -> io::Result<()> {
    if value.is_empty() || value.len() > limit {
        Err(invalid("invalid private mapping value length"))
    } else {
        Ok(())
    }
}

fn label(category: PseudonymCategory, number: usize) -> String {
    format!("{}_{number:04}", category.prefix())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hexadecimal(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn opaque_id(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|s| s.len() == 32 && hexadecimal(s))
}
fn random_id(prefix: &str) -> io::Result<String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| io::Error::other("private mapping randomness unavailable"))?;
    Ok(format!(
        "{prefix}{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}
fn require_directory(path: &Path) -> io::Result<()> {
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(invalid("private mapping directory is not a directory"));
    }
    Ok(())
}
fn check_revision(actual: u64, expected: u64) -> io::Result<()> {
    if actual != expected {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "project changed; reload private mapping",
        ));
    }
    Ok(())
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn encoded_size(value: &(impl Serialize + ?Sized)) -> io::Result<usize> {
    let mut counter = ByteCount(0);
    serde_json::to_writer(&mut counter, value)
        .map_err(|_| invalid("private mapping exceeds storage limit"))?;
    Ok(counter.0)
}

struct ByteCount(usize);
impl Write for ByteCount {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|size| *size <= MAX_BYTES)
            .ok_or_else(|| invalid("private mapping exceeds storage limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct BoundedBytes(Vec<u8>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_BYTES {
            return Err(invalid("private mapping exceeds storage limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
