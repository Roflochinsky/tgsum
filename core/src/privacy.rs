//! Private, persistent privacy options and named category presets. Secret
//! scanning always runs; literal exceptions affect optional PII/infrastructure.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use std::{fmt, io};

use crate::custom_terms::CustomTerms;
use crate::infrastructure::{InfrastructureCategory, InfrastructureDetector, InfrastructurePolicy};
use crate::pii::{PiiCategory, PiiPolicy};

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BundleOptions {
    pub redact_candidates: bool,
    pub infrastructure: InfrastructurePolicy,
    pub pii: PiiPolicy,
    /// Exact case-sensitive whole detected values. Private policy, never public
    /// bundle metadata. Secrets and explicit custom terms cannot be exempted.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keep_values: Vec<String>,
}

impl fmt::Debug for BundleOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BundleOptions")
            .field("redact_candidates", &self.redact_candidates)
            .field("pii_categories", &self.pii.categories)
            .field("infrastructure_categories", &self.infrastructure.categories)
            .field("keep_values_count", &self.keep_values.len())
            .finish_non_exhaustive()
    }
}

impl BundleOptions {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        InfrastructureDetector::new(self.infrastructure.clone())?;
        validate_keep_values(&self.keep_values)
    }
}

pub(crate) fn validate_keep_values(values: &[String]) -> io::Result<()> {
    let mut unique = BTreeSet::new();
    if values.len() > 256
        || values.iter().map(String::len).sum::<usize>() > 32 * 1024
        || values.iter().any(|v| {
            v.is_empty()
                || v.trim() != v
                || v.len() > 4096
                || v.chars().any(char::is_control)
                || !unique.insert(v)
        })
    {
        return Err(invalid("invalid or oversized privacy exception list"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyPreset {
    Secrets,
    People,
    Work,
    Custom,
}

impl PrivacyPreset {
    pub const ALL: [Self; 4] = [Self::Secrets, Self::People, Self::Work, Self::Custom];

    pub fn id(self) -> &'static str {
        match self {
            Self::Secrets => "secrets",
            Self::People => "people",
            Self::Work => "work",
            Self::Custom => "custom",
        }
    }

    pub fn options(self) -> BundleOptions {
        let mut options = BundleOptions::default();
        if matches!(self, Self::People | Self::Work) {
            options.pii.categories = [
                PiiCategory::Participants,
                PiiCategory::Emails,
                PiiCategory::Phones,
                PiiCategory::Usernames,
            ]
            .into();
        }
        if self == Self::Work {
            options.infrastructure.categories = [
                InfrastructureCategory::Ip,
                InfrastructureCategory::Host,
                InfrastructureCategory::Domain,
                InfrastructureCategory::Url,
                InfrastructureCategory::Username,
                InfrastructureCategory::Path,
                InfrastructureCategory::CloudResource,
            ]
            .into();
        }
        options
    }
}

/// One edit publishes settings, optional categories and company terms together.
#[derive(Debug, Clone, Serialize)]
pub struct PrivacyProfile {
    pub preset: PrivacyPreset,
    pub options: BundleOptions,
    pub custom_terms: CustomTerms,
}

impl PrivacyProfile {
    pub(crate) fn validate(&self) -> io::Result<()> {
        self.options.validate()?;
        self.custom_terms.validate()?;
        let base = self.preset.options();
        if self.preset != PrivacyPreset::Custom
            && (base.pii.categories != self.options.pii.categories
                || base.infrastructure.categories != self.options.infrastructure.categories)
        {
            return Err(invalid("privacy categories do not match the named preset"));
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for PrivacyProfile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            preset: PrivacyPreset,
            options: BundleOptions,
            #[serde(default)]
            custom_terms: CustomTerms,
        }
        let wire = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid privacy profile"))?;
        let profile = Self {
            preset: wire.preset,
            options: wire.options,
            custom_terms: wire.custom_terms,
        };
        profile
            .validate()
            .map_err(|_| serde::de::Error::custom("invalid privacy profile"))?;
        Ok(profile)
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
