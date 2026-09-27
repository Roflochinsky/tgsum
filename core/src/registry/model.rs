use serde::{Deserialize, Serialize};

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

macro_rules! values {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
    };
}

values!(SupportStatus {
    Supported,
    Candidate,
    Research,
    Excluded,
    Deferred
});
values!(Operation {
    LocalImport,
    Acquisition,
    LocalInference,
    CloudInference
});
values!(ReviewLevel {
    Initial,
    Verified,
    NotReviewed
});
values!(History {
    Available,
    Limited,
    None,
    Unknown
});
values!(Delta {
    None,
    Events,
    Reimport,
    HistoryPolling,
    Unknown
});
values!(Attachments {
    LocalFiles,
    References,
    Mixed,
    None,
    Unknown
});
values!(Coverage {
    Complete,
    Partial,
    OwnMessagesOnly,
    FutureOnly,
    Unknown
});
values!(BanRisk {
    NoPlatformCalls,
    Conditional,
    ProhibitedPath,
    Unknown
});
values!(AiPolicy {
    ReviewRequired,
    Restricted,
    Unknown,
    AllowedWithConditions
});
values!(QualificationScope {
    FormatContract,
    SyntheticClient,
    RealClient
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectorType {
    ArchiveImporter,
    ApiConnector,
    LocalDataConnector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    #[serde(rename = "current")]
    Current,
    P0,
    P1,
    P2,
    #[serde(rename = "defer")]
    Defer,
    #[serde(rename = "excluded")]
    Excluded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredCapabilities {
    pub history: History,
    pub delta: Delta,
    pub attachments: Attachments,
    pub coverage: Coverage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiPolicies {
    pub local_inference: AiPolicy,
    pub cloud_inference: AiPolicy,
    pub training: AiPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImplementationRef {
    pub id: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientVersion {
    pub version: String,
    pub os: String,
}

/// Qualification is scoped evidence, not a Boolean "safe/supported" label.
/// A format fixture says nothing about a GUI, installer, real account or API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Qualification {
    pub scope: QualificationScope,
    pub implementation_revision: String,
    pub format_id: String,
    #[serde(deserialize_with = "required_option")]
    pub client: Option<ClientVersion>,
    pub verified_at: String,
    /// Repository-relative-to-inventory document recording outcome and limits.
    pub evidence: String,
    /// Same base as evidence; tests/fixtures providing the recorded evidence.
    pub checks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorEntry {
    pub id: String,
    pub platform: String,
    pub connector_type: ConnectorType,
    pub access_method: String,
    pub support_status: SupportStatus,
    pub priority: Priority,
    pub credential_type: String,
    pub authorization_scope: String,
    pub selected_scope: String,
    /// Platform/research expectations; runtime uses the compiled descriptor.
    pub capabilities: DeclaredCapabilities,
    pub ban_risk: BanRisk,
    pub ai_policy: AiPolicies,
    /// Maintainer release claims, never executable feature flags.
    pub enabled_operations: Vec<Operation>,
    pub review_level: ReviewLevel,
    #[serde(deserialize_with = "required_option")]
    pub last_policy_reviewed_at: Option<String>,
    pub next_review_due_at: String,
    pub owner: String,
    pub evidence: String,
    pub sources: Vec<String>,
    pub open_questions: Vec<String>,
    #[serde(deserialize_with = "required_option")]
    pub implementation: Option<ImplementationRef>,
    pub qualifications: Vec<Qualification>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub schema_version: u32,
    pub enforcement: String,
    pub policy_use: String,
    pub connectors: Vec<ConnectorEntry>,
}
