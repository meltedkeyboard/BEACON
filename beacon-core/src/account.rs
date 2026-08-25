use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::LauncherConfig;
use crate::error::{CoreError, Result};

/// Namespace used for offline-account UUIDs, matching the vanilla client's
/// `UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(UTF_8))`.
const UUID_NAMESPACE: Uuid = Uuid::nil();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Account {
    Offline {
        username: String,
        uuid: Uuid,
    },
    Microsoft {
        /// Xbox User ID (xuid). Stable per Microsoft account, used as the credential-store key
        /// for the refresh token (see [`crate::secret_store`]) -- the token itself never lives
        /// in this struct or in the config file.
        id: String,
        username: String,
        uuid: Uuid,
    },
}

impl Account {
    pub fn username(&self) -> &str {
        match self {
            Account::Offline { username, .. } => username,
            Account::Microsoft { username, .. } => username,
        }
    }

    pub fn uuid(&self) -> Uuid {
        match self {
            Account::Offline { uuid, .. } => *uuid,
            Account::Microsoft { uuid, .. } => *uuid,
        }
    }

    /// Stable key used to look this account up in the config file.
    pub fn id(&self) -> String {
        match self {
            Account::Offline { username, .. } => format!("offline:{username}"),
            Account::Microsoft { id, .. } => format!("microsoft:{id}"),
        }
    }
}

/// Builds an offline account. Gated behind [`LauncherConfig::has_verified_microsoft_account`] --
/// offline accounts are only meant as a convenience for players who already proved ownership of
/// the game via a Microsoft sign-in, not as a way to bypass Microsoft/Xbox authentication
/// entirely. This check lives here (not just in the CLI) so no frontend can construct an offline
/// account without going through it.
pub fn offline_account(config: &LauncherConfig, username: impl Into<String>) -> Result<Account> {
    if !config.has_verified_microsoft_account() {
        return Err(CoreError::Auth(
            "offline accounts are unavailable until a Microsoft account has been signed in at \
             least once, to confirm you own the game"
                .into(),
        ));
    }

    let username = username.into();
    let uuid = Uuid::new_v3(&UUID_NAMESPACE, format!("OfflinePlayer:{username}").as_bytes());
    Ok(Account::Offline { username, uuid })
}
