//! Offline connector inventory and code-owned technical capabilities.
//!
//! Inventory claims cannot register code, change compatibility, authorize AI
//! use, delete data or disable an existing local importer on a policy deadline.

mod model;
mod validation;

use std::io;

use chrono::NaiveDate;
use serde::Serialize;

use crate::connector::{ArchiveImporter, ConnectorDescriptor, TelegramJson};

pub use model::*;
pub use validation::{Finding, Severity, ValidationMode, ValidationReport};

/// Runtime observations are supplied by a host, never read from the inventory.
#[derive(Default)]
pub struct ObservedVersion<'a> {
    pub format_id: Option<&'a str>,
    pub client_version: Option<&'a str>,
    pub os: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Compatibility {
    Compatible,
    Unknown,
    Unsupported,
}

/// A driver may pin exact client/OS pairs. An empty list makes no claim about
/// installed clients and only permits the independently identified file format.
pub struct VersionRequirement {
    pub format_id: &'static str,
    pub clients: &'static [(&'static str, &'static str)],
}

impl VersionRequirement {
    pub fn check(&self, observed: &ObservedVersion<'_>) -> Compatibility {
        if observed.format_id.is_some_and(|id| id != self.format_id) {
            return Compatibility::Unsupported;
        }
        if observed.format_id.is_none() {
            return Compatibility::Unknown;
        }
        if self.clients.is_empty() {
            return Compatibility::Compatible;
        }
        match (observed.client_version, observed.os) {
            (Some(version), Some(os)) if self.clients.contains(&(version, os)) => {
                Compatibility::Compatible
            }
            (Some(_), Some(_)) => Compatibility::Unsupported,
            _ => Compatibility::Unknown,
        }
    }
}

/// An explicit source-code registration. JSON cannot construct this factory.
pub struct CompiledConnector {
    pub profile_id: &'static str,
    pub importer: fn() -> Box<dyn ArchiveImporter>,
    pub versions: VersionRequirement,
}

impl CompiledConnector {
    pub fn descriptor(&self) -> ConnectorDescriptor {
        (self.importer)().descriptor()
    }

    pub fn load(&self, observed: &ObservedVersion<'_>) -> io::Result<Box<dyn ArchiveImporter>> {
        if self.versions.check(observed) != Compatibility::Compatible {
            return Err(invalid(
                "unknown or unsupported connector format/client version",
            ));
        }
        Ok((self.importer)())
    }

    pub fn operations(&self) -> &'static [Operation] {
        &[Operation::LocalImport]
    }
}

/// This list must change with actual implementation code, not with policy JSON.
pub fn compiled_connectors() -> Vec<CompiledConnector> {
    ["telegram_full_export", "telegram_single_export"]
        .into_iter()
        .map(|profile_id| CompiledConnector {
            profile_id,
            importer: || Box::new(TelegramJson),
            versions: VersionRequirement {
                format_id: "telegram_desktop_json",
                clients: &[],
            },
        })
        .collect()
}

pub fn load_importer(
    profile_id: &str,
    observed: &ObservedVersion<'_>,
) -> io::Result<Box<dyn ArchiveImporter>> {
    compiled_connectors()
        .into_iter()
        .find(|c| c.profile_id == profile_id)
        .ok_or_else(|| invalid("connector has no compiled importer"))?
        .load(observed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Current,
    Due,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportState {
    Unknown,
    Implemented,
    /// Read `qualifications.scope`; format-only is not real-client support.
    Qualified,
}

#[derive(Debug, Serialize)]
pub struct TechnicalSupport {
    pub profile_id: String,
    pub state: SupportState,
    pub implementation: Option<ConnectorDescriptor>,
    pub implemented_operations: Vec<Operation>,
    pub compatibility: Compatibility,
    /// Only exact implementation/format/client matches appear here.
    pub qualifications: Vec<Qualification>,
    pub review: ReviewState,
}

impl Registry {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(invalid("connector inventory exceeds 4 MiB"));
        }
        let registry: Self = serde_json::from_slice(bytes).map_err(invalid)?;
        if registry.schema_version != 2
            || registry.enforcement != "maintainer_release_checks"
            || registry.policy_use != "maintainer_research_not_user_authorization"
        {
            return Err(invalid(
                "unsupported connector registry schema or responsibility boundary",
            ));
        }
        registry.validate_structure()?;
        Ok(registry)
    }

    pub fn entry(&self, profile_id: &str) -> Option<&ConnectorEntry> {
        self.connectors.iter().find(|c| c.id == profile_id)
    }

    /// Descriptive metadata joins implementation facts without granting access.
    /// An expired review or restrictive AI note does not alter operations.
    pub fn technical_support(
        &self,
        profile_id: &str,
        observed: &ObservedVersion<'_>,
        today: NaiveDate,
    ) -> TechnicalSupport {
        let compiled = compiled_connectors()
            .into_iter()
            .find(|c| c.profile_id == profile_id);
        let entry = self.entry(profile_id);
        let descriptor = compiled.as_ref().map(CompiledConnector::descriptor);
        let compatibility = compiled
            .as_ref()
            .map_or(Compatibility::Unknown, |c| c.versions.check(observed));
        let qualifications: Vec<Qualification> = entry
            .into_iter()
            .flat_map(|entry| {
                entry
                    .qualifications
                    .iter()
                    .filter(|q| {
                        compatibility == Compatibility::Compatible
                            && parse_date(&q.verified_at).is_ok_and(|date| date <= today)
                            && descriptor.is_some_and(|d| {
                                entry
                                    .implementation
                                    .as_ref()
                                    .is_some_and(|i| i.id == d.id && i.revision == d.revision)
                                    && entry.platform.eq_ignore_ascii_case(d.platform)
                                    && q.implementation_revision == d.revision
                                    && q.format_id == d.format_id
                                    && match (&q.scope, &q.client) {
                                        (QualificationScope::FormatContract, None) => true,
                                        (
                                            QualificationScope::SyntheticClient
                                            | QualificationScope::RealClient,
                                            Some(client),
                                        ) => {
                                            observed.client_version == Some(client.version.as_str())
                                                && observed.os == Some(client.os.as_str())
                                                && compiled.as_ref().is_some_and(|code| {
                                                    code.versions.clients.contains(&(
                                                        client.version.as_str(),
                                                        client.os.as_str(),
                                                    ))
                                                })
                                        }
                                        _ => false,
                                    }
                            })
                    })
                    .cloned()
            })
            .collect();
        TechnicalSupport {
            profile_id: profile_id.into(),
            state: if !qualifications.is_empty() {
                SupportState::Qualified
            } else if descriptor.is_some() {
                SupportState::Implemented
            } else {
                SupportState::Unknown
            },
            implementation: descriptor,
            implemented_operations: compiled
                .as_ref()
                .map_or_else(Vec::new, |c| c.operations().to_vec()),
            compatibility,
            qualifications,
            review: entry.map_or(ReviewState::Unknown, |e| e.review_state(today)),
        }
    }
}

impl ConnectorEntry {
    pub fn review_state(&self, today: NaiveDate) -> ReviewState {
        let Some(last) = self
            .last_policy_reviewed_at
            .as_deref()
            .and_then(|d| parse_date(d).ok())
        else {
            return ReviewState::Unknown;
        };
        match parse_date(&self.next_review_due_at) {
            Ok(due) if last <= today && due > today => ReviewState::Current,
            Ok(due) if last <= today && due <= today => ReviewState::Due,
            _ => ReviewState::Unknown,
        }
    }
}

fn parse_date(value: &str) -> io::Result<NaiveDate> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(invalid)?;
    if date.to_string() != value {
        return Err(invalid("date must use YYYY-MM-DD"));
    }
    Ok(date)
}

fn invalid(value: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, value.to_string())
}
