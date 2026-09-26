//! Versioned observation metadata; never infer UTC from the machine timezone.

use std::io::{self, Write};

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{invalid, CanonicalMessage, Coverage, CoverageLevel, Snapshot, SCHEMA_VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UtcInstant {
    pub seconds: i64,
    pub nanoseconds: u32,
}

impl UtcInstant {
    pub fn validate(&self) -> io::Result<()> {
        if self.nanoseconds >= 1_000_000_000
            || DateTime::<Utc>::from_timestamp(self.seconds, self.nanoseconds).is_none()
        {
            return Err(invalid("invalid UTC instant"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimezoneStatus {
    UnixTime,
    ExplicitOffset,
    UnknownTimezone,
    Missing,
    Invalid,
    Conflicting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimestampInfo {
    pub utc: Option<UtcInstant>,
    pub timezone_status: TimezoneStatus,
}

impl TimestampInfo {
    pub fn from_export(raw: Option<&str>, unix: Option<&str>) -> Self {
        let offset = raw.and_then(|s| DateTime::parse_from_rfc3339(s).ok());
        if let Some(unix) = unix {
            let digits = unix.strip_prefix('-').unwrap_or(unix);
            let seconds = (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
                .then(|| unix.parse::<i64>().ok())
                .flatten();
            let Some(seconds) =
                seconds.filter(|s| DateTime::<Utc>::from_timestamp(*s, 0).is_some())
            else {
                return Self {
                    utc: None,
                    timezone_status: TimezoneStatus::Invalid,
                };
            };
            if offset.as_ref().is_some_and(|t| t.timestamp() != seconds) {
                return Self {
                    utc: None,
                    timezone_status: TimezoneStatus::Conflicting,
                };
            }
            let instant = UtcInstant {
                seconds,
                nanoseconds: offset.map_or(0, |t| t.timestamp_subsec_nanos()),
            };
            if instant.validate().is_err() {
                return Self {
                    utc: None,
                    timezone_status: TimezoneStatus::Invalid,
                };
            }
            return Self {
                utc: Some(instant),
                timezone_status: TimezoneStatus::UnixTime,
            };
        }
        if let Some(time) = offset {
            let instant = UtcInstant {
                seconds: time.timestamp(),
                nanoseconds: time.timestamp_subsec_nanos(),
            };
            if instant.validate().is_ok() {
                return Self {
                    utc: Some(instant),
                    timezone_status: TimezoneStatus::ExplicitOffset,
                };
            }
        }
        let timezone_status = match raw {
            None => TimezoneStatus::Missing,
            Some(s) if NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").is_ok() => {
                TimezoneStatus::UnknownTimezone
            }
            Some(_) => TimezoneStatus::Invalid,
        };
        Self {
            utc: None,
            timezone_status,
        }
    }
}

/// Inclusive bounds. This is a source's claim, not min/max observed messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: UtcInstant,
    pub end: UtcInstant,
}

impl TimeRange {
    fn validate(&self) -> io::Result<()> {
        self.start.validate()?;
        self.end.validate()?;
        if self.start > self.end {
            return Err(invalid("coverage range is reversed"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageGap {
    pub range: Option<TimeRange>,
    pub reason: String,
}

impl Coverage {
    pub(super) fn validate(&self) -> io::Result<()> {
        if let Some(range) = &self.range {
            range.validate()?;
        }
        for gap in &self.known_gaps {
            if let Some(range) = &gap.range {
                range.validate()?;
            }
            if gap.reason.trim().is_empty() {
                return Err(invalid("coverage gap needs a reason"));
            }
        }
        if self.level == CoverageLevel::Complete
            && (self.range.is_none()
                || self.evidence.iter().all(|s| s.trim().is_empty())
                || !self.known_gaps.is_empty())
        {
            return Err(invalid(
                "complete coverage needs a bounded range, evidence and no known gaps",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityQuality {
    Native,
    SnapshotLocal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionState {
    Present,
    Deleted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordLocatorKind {
    SelectedConversationMessages,
    MigratedCanonicalMessages,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageProvenance {
    pub snapshot_id: String,
    pub record_ordinal: u64,
    pub locator_kind: RecordLocatorKind,
    /// None unless the source record's bytes were actually retained/hashed.
    pub raw_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMetadata {
    pub identity_quality: IdentityQuality,
    pub timestamp: TimestampInfo,
    pub edited_timestamp: TimestampInfo,
    pub deletion_state: DeletionState,
    /// SHA-256 of the canonical content projection, independent of observation.
    pub revision_id: String,
    pub provenance: MessageProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    pub connector_id: String,
    pub connector_revision: String,
    pub acquisition_method: String,
    pub format_id: String,
    /// Import time; it does not assert when the client exported the archive.
    pub imported_at: Option<UtcInstant>,
    pub migrated_from_schema: Option<u32>,
    pub content_digest: String,
}

impl CanonicalMessage {
    pub(super) fn revision(&self, deletion: &DeletionState) -> io::Result<String> {
        // Explicit projection keeps new observation metadata out of content
        // revisions. Changes to this projection require a schema migration.
        digest(&(
            &self.key,
            &self.timestamp,
            &self.timestamp_unix,
            &self.sender_id,
            &self.sender_name,
            &self.text,
            &self.reply_to,
            &self.thread_id,
            &self.edited_at,
            &self.edited_unix,
            self.is_service,
            &self.service_action,
            &self.service_title,
            &self.attachments,
            deletion,
        ))
    }
}

impl Snapshot {
    pub(super) fn add_metadata(&mut self, migrated: bool) -> io::Result<()> {
        for (index, message) in self.messages.iter_mut().enumerate() {
            message.metadata = Some(MessageMetadata {
                identity_quality: IdentityQuality::Native,
                timestamp: TimestampInfo::from_export(
                    message.timestamp.as_deref(),
                    message.timestamp_unix.as_deref(),
                ),
                edited_timestamp: TimestampInfo::from_export(
                    message.edited_at.as_deref(),
                    message.edited_unix.as_deref(),
                ),
                deletion_state: DeletionState::Present,
                revision_id: message.revision(&DeletionState::Present)?,
                provenance: MessageProvenance {
                    snapshot_id: self.snapshot_id.clone(),
                    record_ordinal: index as u64,
                    locator_kind: if migrated {
                        RecordLocatorKind::MigratedCanonicalMessages
                    } else {
                        RecordLocatorKind::SelectedConversationMessages
                    },
                    raw_digest: None,
                },
            });
        }
        let imported_at = if migrated {
            None
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|d| {
                    i64::try_from(d.as_secs()).ok().map(|seconds| UtcInstant {
                        seconds,
                        nanoseconds: d.subsec_nanos(),
                    })
                })
        };
        self.metadata = Some(SnapshotMetadata {
            connector_id: if migrated {
                "legacy_telegram_json"
            } else {
                "telegram_json"
            }
            .into(),
            connector_revision: if migrated {
                "schema-1".into()
            } else {
                format!("{}:canonical-2", env!("CARGO_PKG_VERSION"))
            },
            acquisition_method: "archive".into(),
            format_id: "telegram_desktop_json".into(),
            imported_at,
            migrated_from_schema: migrated.then_some(1),
            content_digest: self.content_digest()?,
        });
        self.schema_version = SCHEMA_VERSION;
        Ok(())
    }

    pub(super) fn content_digest(&self) -> io::Result<String> {
        let mut revisions: Vec<_> = self
            .messages
            .iter()
            .map(|m| {
                m.metadata
                    .as_ref()
                    .map(|meta| (&m.key, &meta.revision_id))
                    .ok_or_else(|| invalid("message metadata missing"))
            })
            .collect::<io::Result<_>>()?;
        revisions.sort_by_key(|(key, _)| *key);
        digest(&(
            &self.source,
            &self.conversation_title,
            &self.conversation_kind,
            revisions,
        ))
    }

    pub(super) fn validate_metadata(&self) -> io::Result<()> {
        self.coverage.validate()?;
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| invalid("snapshot metadata missing"))?;
        if let Some(time) = metadata.imported_at {
            time.validate()?;
        }
        if metadata.content_digest != self.content_digest()? {
            return Err(invalid("snapshot content digest mismatch"));
        }
        for (index, message) in self.messages.iter().enumerate() {
            let meta = message
                .metadata
                .as_ref()
                .ok_or_else(|| invalid("message metadata missing"))?;
            if meta.provenance.snapshot_id != self.snapshot_id
                || meta.provenance.record_ordinal != index as u64
            {
                return Err(invalid(
                    "message provenance does not match snapshot location",
                ));
            }
            if meta.timestamp
                != TimestampInfo::from_export(
                    message.timestamp.as_deref(),
                    message.timestamp_unix.as_deref(),
                )
                || meta.edited_timestamp
                    != TimestampInfo::from_export(
                        message.edited_at.as_deref(),
                        message.edited_unix.as_deref(),
                    )
            {
                return Err(invalid("timestamp metadata does not match exported value"));
            }
            if meta.revision_id != message.revision(&meta.deletion_state)? {
                return Err(invalid("message revision digest mismatch"));
            }
        }
        Ok(())
    }
}

fn digest(value: &impl Serialize) -> io::Result<String> {
    struct HashWriter(Sha256);
    impl Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, value).map_err(invalid)?;
    Ok(format!("sha256:{:x}", writer.0.finalize()))
}
