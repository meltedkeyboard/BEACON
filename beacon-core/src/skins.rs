use serde::{Deserialize, Serialize};

use crate::auth::json_or_error;
use crate::error::{CoreError, Result};

const PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const SKINS_URL: &str = "https://api.minecraftservices.com/minecraft/profile/skins";
const CAPES_ACTIVE_URL: &str = "https://api.minecraftservices.com/minecraft/profile/capes/active";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinInfo {
    pub id: String,
    pub state: String,
    pub url: String,
    pub variant: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapeInfo {
    pub id: String,
    pub state: String,
    pub url: String,
    pub alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub skins: Vec<SkinInfo>,
    #[serde(default)]
    pub capes: Vec<CapeInfo>,
}

pub async fn fetch_profile(client: &reqwest::Client, access_token: &str) -> Result<MinecraftProfile> {
    let response = client.get(PROFILE_URL).bearer_auth(access_token).send().await?;
    json_or_error(response, "profile fetch").await
}

/// Checks for a 2xx status and discards the body -- every mutating skin/cape endpoint below is
/// followed by a fresh [`fetch_profile`] call rather than trusting its own response shape, so all
/// that matters here is whether it succeeded.
async fn ensure_success(response: reqwest::Response, step: &str) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.text().await.unwrap_or_default();
    Err(CoreError::Auth(format!("{step} failed ({status}): {body}")))
}

/// Uploads a new skin from raw PNG bytes, replacing whatever skin is currently active.
/// `variant` is `"classic"` (wide/Steve arms) or `"slim"` (narrow/Alex arms).
pub async fn upload_skin(
    client: &reqwest::Client,
    access_token: &str,
    file_bytes: Vec<u8>,
    file_name: &str,
    variant: &str,
) -> Result<()> {
    let part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(file_name.to_string())
        .mime_str("image/png")
        .map_err(|e| CoreError::Other(format!("invalid skin file: {e}")))?;
    let form = reqwest::multipart::Form::new()
        .text("variant", variant.to_string())
        .part("file", part);

    let response = client.post(SKINS_URL).bearer_auth(access_token).multipart(form).send().await?;
    ensure_success(response, "skin upload").await
}

/// Resets to the account's default skin (Steve or Alex, whichever Mojang assigns by UUID).
pub async fn reset_skin(client: &reqwest::Client, access_token: &str) -> Result<()> {
    let response = client
        .delete(format!("{SKINS_URL}/active"))
        .bearer_auth(access_token)
        .send()
        .await?;
    ensure_success(response, "skin reset").await
}

/// Equips a cape the account already owns -- this can't add a new cape, only pick one already in
/// `MinecraftProfile::capes`.
pub async fn set_cape(client: &reqwest::Client, access_token: &str, cape_id: &str) -> Result<()> {
    let response = client
        .put(CAPES_ACTIVE_URL)
        .bearer_auth(access_token)
        .json(&serde_json::json!({ "capeId": cape_id }))
        .send()
        .await?;
    ensure_success(response, "set cape").await
}

/// Removes whichever cape is currently equipped, leaving none active.
pub async fn clear_cape(client: &reqwest::Client, access_token: &str) -> Result<()> {
    let response = client.delete(CAPES_ACTIVE_URL).bearer_auth(access_token).send().await?;
    ensure_success(response, "clear cape").await
}
