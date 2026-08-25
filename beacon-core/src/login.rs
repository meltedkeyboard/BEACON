use crate::account::Account;
use crate::auth::{self, DeviceAuthorization, MinecraftSession};
use crate::error::{CoreError, Result};
use crate::secret_store;

/// Runs the full device code sign-in: requests a code, hands it to `on_prompt` so the caller can
/// show it to the user (CLI prints it, a GUI would render it), then blocks until the user
/// finishes signing in on their own device. On success, saves the Microsoft refresh token to the
/// OS credential store and returns the resulting account plus a ready-to-use game session.
pub async fn login_with_device_code(
    client: &reqwest::Client,
    client_id: &str,
    on_prompt: impl FnOnce(&DeviceAuthorization),
) -> Result<(Account, MinecraftSession)> {
    let authorization = auth::request_device_code(client, client_id).await?;
    on_prompt(&authorization);
    let tokens = auth::poll_device_code(client, client_id, &authorization).await?;
    let session = auth::authenticate_minecraft(client, &tokens.access_token).await?;

    let account = Account::Microsoft {
        id: session.xuid.clone(),
        username: session.username.clone(),
        uuid: session.uuid,
    };
    secret_store::save_refresh_token(&account.id(), &tokens.refresh_token).await?;

    Ok((account, session))
}

/// Re-derives a fresh game session for an already-saved Microsoft account, using its stored
/// refresh token. Call this before every launch -- the Minecraft access token this returns is
/// short-lived and intentionally never persisted. Rotates and re-saves the refresh token, since
/// Microsoft's token endpoint may issue a new one on each use.
pub async fn refresh_session(
    client: &reqwest::Client,
    client_id: &str,
    account: &Account,
) -> Result<MinecraftSession> {
    let Account::Microsoft { .. } = account else {
        return Err(CoreError::Other("refresh_session called on a non-Microsoft account".into()));
    };

    let refresh_token = secret_store::load_refresh_token(&account.id())
        .await?
        .ok_or_else(|| CoreError::Auth("no saved sign-in for this account; log in again".into()))?;

    let tokens = auth::refresh_tokens(client, client_id, &refresh_token).await?;
    let session = auth::authenticate_minecraft(client, &tokens.access_token).await?;
    secret_store::save_refresh_token(&account.id(), &tokens.refresh_token).await?;

    Ok(session)
}

/// Removes a Microsoft account's saved refresh token from the OS credential store. The caller is
/// still responsible for removing the account from [`crate::config::LauncherConfig`].
pub async fn forget_account(account: &Account) -> Result<()> {
    secret_store::delete_refresh_token(&account.id()).await
}
