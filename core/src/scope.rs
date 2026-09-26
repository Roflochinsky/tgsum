//! Local selection over observed snapshots. A successful analysis baseline is
//! independent of the latest import and includes the scope actually analyzed.

use std::collections::BTreeMap;
use std::io;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::snapshot::{CanonicalMessage, DeletionState, IdentityQuality, Snapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DateBasis {
    #[default]
    SourceDate,
    Utc,
}

/// Inclusive calendar dates. UTC mode excludes values without a known instant;
/// source-date mode uses the date written in the archive without converting it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateRange {
    pub from: Option<String>,
    pub through: Option<String>,
    pub basis: DateBasis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MessageFilter {
    /// None selects all threads; an empty list selects no threads.
    pub topic_ids: Option<Vec<String>>,
    pub dates: Option<DateRange>,
    pub include_unknown_dates: bool,
    pub include_service: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceSelection {
    pub enabled: bool,
    pub filter: MessageFilter,
    pub only_changes: bool,
}

impl Default for SourceSelection {
    fn default() -> Self {
        Self {
            enabled: true,
            filter: MessageFilter::default(),
            only_changes: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeStats {
    pub selected: usize,
    pub created: usize,
    pub edited: usize,
    pub deleted: usize,
    pub unchanged: usize,
    pub missing: usize,
    pub excluded_unknown_dates: usize,
    pub included_unknown_dates: usize,
    pub has_baseline: bool,
}

pub struct ScopedMessages<'a> {
    pub messages: Vec<&'a CanonicalMessage>,
    pub stats: ScopeStats,
}

impl MessageFilter {
    pub fn validate(&self) -> io::Result<()> {
        if self.topic_ids.as_ref().is_some_and(|ids| {
            ids.iter()
                .any(|id| id.trim().is_empty() || id.chars().any(char::is_control))
        }) {
            return Err(invalid("invalid topic identity"));
        }
        if let Some(range) = &self.dates {
            let from = range.from.as_deref().map(parse_date).transpose()?;
            let through = range.through.as_deref().map(parse_date).transpose()?;
            if from.zip(through).is_some_and(|(a, b)| a > b) {
                return Err(invalid("date range is reversed"));
            }
        }
        Ok(())
    }

    fn membership(&self, message: &CanonicalMessage) -> Membership {
        if !self.include_service && message.is_service {
            return Membership::Outside;
        }
        if let Some(topics) = &self.topic_ids {
            let topic = if message.service_action.as_deref() == Some("topic_created") {
                Some(&message.key.message_id)
            } else {
                message.thread_id.as_ref()
            };
            if !topic.is_some_and(|id| topics.contains(id)) {
                return Membership::Outside;
            }
        }
        let Some(range) = &self.dates else {
            return Membership::Inside;
        };
        if range.from.is_none() && range.through.is_none() {
            return Membership::Inside;
        }
        let date = match range.basis {
            DateBasis::SourceDate => message.timestamp.as_deref().and_then(|raw| {
                DateTime::parse_from_rfc3339(raw)
                    .map(|d| d.date_naive())
                    .ok()
                    .or_else(|| {
                        NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")
                            .map(|d| d.date())
                            .ok()
                    })
            }),
            DateBasis::Utc => message
                .metadata
                .as_ref()
                .and_then(|meta| meta.timestamp.utc)
                .and_then(|t| DateTime::<Utc>::from_timestamp(t.seconds, t.nanoseconds))
                .map(|d| d.date_naive()),
        };
        let Some(date) = date else {
            return Membership::Unknown;
        };
        // validate() already checked the dates before scanning any messages.
        if range
            .from
            .as_deref()
            .and_then(|s| parse_date(s).ok())
            .is_some_and(|d| date < d)
            || range
                .through
                .as_deref()
                .and_then(|s| parse_date(s).ok())
                .is_some_and(|d| date > d)
        {
            Membership::Outside
        } else {
            Membership::Inside
        }
    }

    fn includes(&self, message: &CanonicalMessage) -> bool {
        match self.membership(message) {
            Membership::Inside => true,
            Membership::Unknown => self.include_unknown_dates,
            Membership::Outside => false,
        }
    }
}

enum Membership {
    Inside,
    Outside,
    Unknown,
}

/// Baseline filtering uses the old run's scope. Widening scope includes records
/// never analyzed before, even if they already existed in the prior snapshot.
pub fn select_messages<'a>(
    snapshot: &'a Snapshot,
    selection: &SourceSelection,
    baseline: Option<(&Snapshot, &MessageFilter)>,
) -> io::Result<ScopedMessages<'a>> {
    snapshot.validate()?;
    selection.filter.validate()?;
    let mut stats = ScopeStats {
        has_baseline: baseline.is_some(),
        ..Default::default()
    };
    let mut old = BTreeMap::new();
    if let Some((previous, filter)) = baseline {
        previous.validate()?;
        filter.validate()?;
        if snapshot.source != previous.source {
            return Err(invalid("analysis baseline belongs to a different source"));
        }
        if snapshot.snapshot_id != previous.snapshot_id
            && snapshot.messages.iter().chain(&previous.messages).any(|m| {
                m.metadata
                    .as_ref()
                    .is_some_and(|meta| meta.identity_quality == IdentityQuality::SnapshotLocal)
            })
        {
            return Err(invalid(
                "snapshot-local identities require cross-snapshot matching",
            ));
        }
        old.extend(
            previous
                .messages
                .iter()
                .filter(|m| filter.includes(m))
                .map(|m| (&m.key, m)),
        );
    }
    let mut messages = Vec::new();
    if !selection.enabled {
        return Ok(ScopedMessages { messages, stats });
    }
    let current: BTreeMap<_, _> = snapshot.messages.iter().map(|m| (&m.key, m)).collect();
    stats.missing = old
        .values()
        .filter(|m| selection.filter.includes(m) && !current.contains_key(&m.key))
        .count();
    for message in &snapshot.messages {
        let unknown = match selection.filter.membership(message) {
            Membership::Outside => continue,
            Membership::Unknown if !selection.filter.include_unknown_dates => {
                stats.excluded_unknown_dates += 1;
                continue;
            }
            Membership::Unknown => true,
            Membership::Inside => false,
        };
        let meta = message
            .metadata
            .as_ref()
            .ok_or_else(|| invalid("message metadata missing"))?;
        let previous = old.get(&message.key);
        let unchanged = previous.is_some_and(|m| {
            m.metadata
                .as_ref()
                .is_some_and(|old| old.revision_id == meta.revision_id)
        });
        if unchanged {
            stats.unchanged += 1;
        } else if meta.deletion_state == DeletionState::Deleted {
            stats.deleted += 1;
        } else if previous.is_some() {
            stats.edited += 1;
        } else {
            stats.created += 1;
        }
        if selection.only_changes && unchanged {
            continue;
        }
        if unknown {
            stats.included_unknown_dates += 1;
        }
        messages.push(message);
    }
    stats.selected = messages.len();
    Ok(ScopedMessages { messages, stats })
}

fn parse_date(value: &str) -> io::Result<NaiveDate> {
    if value.len() != 10 {
        return Err(invalid("date must use YYYY-MM-DD"));
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| invalid("invalid calendar date"))
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
