//! Acquisition and normalization contracts. This module contains no network
//! client or credential storage. Capabilities describe implemented behavior;
//! they never upgrade the coverage of an individual observation.

use std::io::{self, Read};

use serde::Serialize;

use crate::bridge::{ClientUpdate, ExportRequest};
use crate::snapshot::{CanonicalMessage, Coverage, DeletionState, IdentityQuality, SourceScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorKind {
    Archive,
    LocalData,
    UserOAuth,
    Bot,
    OfficialClient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshMethod {
    None,
    Reimport,
    Cursor,
    Events,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentAccess {
    None,
    ReferencesOnly,
    LocalFiles,
    RemoteReferences,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    None,
    UserOAuth,
    BotToken,
    RecoveryKey,
    OfficialClientManaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ConnectorCapabilities {
    pub history: bool,
    pub refresh: RefreshMethod,
    pub stable_message_ids: bool,
    pub attachments: AttachmentAccess,
    pub credentials: CredentialKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ConnectorDescriptor {
    /// Implementation ID; multiple registry acquisition profiles may use it.
    pub id: &'static str,
    pub revision: &'static str,
    pub platform: &'static str,
    pub kind: ConnectorKind,
    pub format_id: &'static str,
    pub capabilities: ConnectorCapabilities,
}

/// Source facts, separated from the metadata computed by the store.
pub struct MessageObservation {
    pub message: CanonicalMessage,
    pub identity_quality: IdentityQuality,
    pub deletion_state: DeletionState,
}

impl MessageObservation {
    pub fn native_present(message: CanonicalMessage) -> Self {
        Self {
            message,
            identity_quality: IdentityQuality::Native,
            deletion_state: DeletionState::Present,
        }
    }
}

pub struct ConversationObservation {
    pub source: SourceScope,
    pub title: Option<String>,
    pub kind: String,
    pub coverage: Coverage,
    pub messages: Vec<MessageObservation>,
}

/// Normalize one selected conversation. Emit it once, then finish validating
/// the input. The store stages the observation and publishes only after this
/// method succeeds. An error after emission must still abort publication.
/// Implementations must consume and validate the entire source representation.
pub trait ArchiveImporter {
    fn descriptor(&self) -> ConnectorDescriptor;
    fn normalize(
        &self,
        reader: &mut dyn Read,
        source: &SourceScope,
        emit: &mut dyn FnMut(ConversationObservation) -> io::Result<()>,
    ) -> io::Result<()>;
}

/// Optional acquisition driver for an explicitly configured export request.
/// It owns client/API interaction; the host owns polling cadence and feeds
/// returned events into ExportJob. No default implementation contacts clients.
/// A local-data importer can be used directly without this interface.
pub trait SourceAcquisition {
    fn descriptor(&self) -> ConnectorDescriptor;
    fn start(&mut self, request: &ExportRequest) -> io::Result<ClientUpdate>;
    fn poll(&mut self, request: &ExportRequest) -> io::Result<Option<ClientUpdate>>;
    fn cancel(&mut self, request: &ExportRequest) -> io::Result<ClientUpdate>;
}

#[derive(Debug, Clone, Copy)]
pub struct TelegramJson;

impl ArchiveImporter for TelegramJson {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            id: "telegram_json",
            revision: concat!(env!("CARGO_PKG_VERSION"), ":canonical-2"),
            platform: "telegram",
            kind: ConnectorKind::Archive,
            format_id: "telegram_desktop_json",
            capabilities: ConnectorCapabilities {
                history: true,
                refresh: RefreshMethod::Reimport,
                stable_message_ids: true,
                attachments: AttachmentAccess::ReferencesOnly,
                credentials: CredentialKind::None,
            },
        }
    }

    fn normalize(
        &self,
        reader: &mut dyn Read,
        source: &SourceScope,
        emit: &mut dyn FnMut(ConversationObservation) -> io::Result<()>,
    ) -> io::Result<()> {
        crate::snapshot::normalize_telegram(reader, source, emit)
    }
}
