//! Portable, private configuration of a selected diagnostic source. Persisting
//! this plan does not start a client or qualify the source on another OS.

use std::io;
use std::path::{Component, PathBuf};

use serde::{Deserialize, Serialize};

use super::parser::{PeerKind, TypedPeer};
use crate::project::ProjectSource;
use crate::snapshot::{validate_snapshot_id, Snapshot};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuousSettings {
    pub enabled: bool,
    /// A fresh generation has its own journal; stopped journals are retained.
    pub generation: String,
    pub input_directory: PathBuf,
    pub peer: TypedPeer,
    pub self_user_id: Option<String>,
    pub confirmed_single_account: bool,
    pub bootstrap_snapshot_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionCheckpoint {
    pub sequence: u64,
    pub observation_revision: u64,
    pub snapshot_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuousPlan {
    pub settings: ContinuousSettings,
    pub checkpoint: Option<ProjectionCheckpoint>,
    /// Keep the explicitly selected media root even when package-generated
    /// text choices become empty. The bootstrap archive may have moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_directory: Option<PathBuf>,
}

impl ContinuousSettings {
    pub(crate) fn validate(&self, source: &ProjectSource) -> io::Result<()> {
        validate_snapshot_id(&self.generation)?;
        validate_snapshot_id(&self.bootstrap_snapshot_id)?;
        if !self.input_directory.is_absolute()
            || self
                .input_directory
                .components()
                .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
            || !self.confirmed_single_account
            || !native_id(&self.peer.id)
            || self.self_user_id.as_ref().is_some_and(|id| !native_id(id))
            || source.scope.platform != "telegram"
            || source.connector_id != "telegram_json"
            || source.scope.conversation_id != self.peer.id
            || source.scope.account_local_id.len() > 1024
        {
            return Err(invalid(
                "invalid or unconfirmed continuous Telegram binding",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_bootstrap(&self, snapshot: &Snapshot) -> io::Result<()> {
        let kind = match snapshot.conversation_kind.as_str() {
            "private_supergroup" | "public_supergroup" | "private_channel" | "public_channel" => {
                PeerKind::Channel
            }
            "private_group" | "public_group" => PeerKind::Chat,
            "personal_chat" => PeerKind::User,
            _ => return Err(invalid("unknown Telegram conversation kind")),
        };
        if self.peer.kind != kind || snapshot.snapshot_id != self.bootstrap_snapshot_id {
            return Err(invalid(
                "diagnostic peer does not match the bootstrap chat kind",
            ));
        }
        Ok(())
    }

    pub(crate) fn same_binding(&self, other: &Self) -> bool {
        let mut left = self.clone();
        left.enabled = other.enabled;
        left == *other
    }
}

impl ContinuousPlan {
    pub(crate) fn validate(&self, source: &ProjectSource) -> io::Result<()> {
        self.settings.validate(source)?;
        if self.media_directory.as_ref().is_some_and(|path| {
            !path.is_absolute()
                || path
                    .components()
                    .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
        }) {
            return Err(invalid(
                "continuous media root must be an explicit absolute directory",
            ));
        }
        let expected = if let Some(checkpoint) = &self.checkpoint {
            validate_snapshot_id(&checkpoint.snapshot_id)?;
            &checkpoint.snapshot_id
        } else {
            &self.settings.bootstrap_snapshot_id
        };
        if source.latest_snapshot_id.as_ref() != Some(expected) {
            return Err(invalid(
                "continuous checkpoint does not match the source snapshot",
            ));
        }
        Ok(())
    }
}

fn native_id(value: &str) -> bool {
    !value.starts_with('0')
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.parse::<i64>().is_ok_and(|id| id > 0)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
