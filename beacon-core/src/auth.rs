use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{CoreError, Result};

const MAX_AUTH_ATTEMPTS: u32 = 3;

/// Retries a transient send failure (a dropped connection, a DNS/TLS hiccup, a timeout -- routine
/// noise on any of these five hops, same reasoning as `downloader.rs`'s `fetch_with_retries`) up
/// to `MAX_AUTH_ATTEMPTS` times with a short backoff, instead of failing sign-in/session-refresh
/// outright on the first blip. A non-2xx *response* isn't retried here -- that's a real answer
/// from the server, not a network failure, and each caller's own `json_or_error`/status check
/// handles it. `try_clone()` only fails for a streaming body, which none of these requests use
/// (all are `.form(...)`/`.json(...)`, buffered up front) -- the `None` branch exists only as a
/// safety net, not a path any real call here should hit.
async fn send_with_retries(builder: reqwest::RequestBuilder) -> Result<reqwest::Response> {
    let mut last_err = None;
    for attempt in 1..=MAX_AUTH_ATTEMPTS {
        let Some(attempt_builder) = builder.try_clone() else {
            return Ok(builder.send().await?);
        };
        match attempt_builder.send().await {
            Ok(response) => return Ok(response),
            Err(e) => {
                eprintln!("[beacon] auth request failed (attempt {attempt}/{MAX_AUTH_ATTEMPTS}): {e}");
                last_err = Some(e);
                if attempt < MAX_AUTH_ATTEMPTS {
                    tokio::time::sleep(Duration::from_millis(400 * attempt as u64)).await;
                }
            }
        }
    }
    Err(last_err.expect("loop always sets last_err before exiting on failure").into())
}

/// Azure App Registration client id (public client, "Allow public client flows" enabled,
/// `XboxLive.signin` delegated permission granted). Override via
/// [`crate::config::LauncherConfig::azure_client_id`] to use a different registration without
/// recompiling.
pub const AZURE_CLIENT_ID: &str = "2b08fb6d-bb51-4ad2-a6f1-9ef426ba15db";

const OAUTH_SCOPE: &str = "XboxLive.signin offline_access";
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBOX_LIVE_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_ENTITLEMENT_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

/// What the user needs to see and act on to complete the device code flow: a code to enter and
/// the URL to enter it at. Kept serde-serializable so a GUI can render it directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthorization {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    pub message: String,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
    message: String,
}

pub async fn request_device_code(
    client: &reqwest::Client,
    client_id: &str,
) -> Result<DeviceAuthorization> {
    let response: DeviceCodeResponse = send_with_retries(client.post(DEVICE_CODE_URL).form(&[("client_id", client_id), ("scope", OAUTH_SCOPE)]))
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(DeviceAuthorization {
        device_code: response.device_code,
        user_code: response.user_code,
        verification_uri: response.verification_uri,
        expires_in: response.expires_in,
        interval: response.interval,
        message: response.message,
    })
}

#[derive(Debug, Clone)]
pub struct MicrosoftTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    error: String,
}

/// Polls the token endpoint at the interval Microsoft told us to, until the user finishes
/// signing in, the device code expires, or they deny consent.
pub async fn poll_device_code(
    client: &reqwest::Client,
    client_id: &str,
    authorization: &DeviceAuthorization,
) -> Result<MicrosoftTokens> {
    let mut interval = Duration::from_secs(authorization.interval.max(1));
    let deadline = std::time::Instant::now() + Duration::from_secs(authorization.expires_in);

    loop {
        if std::time::Instant::now() >= deadline {
            return Err(CoreError::Auth("device code expired before sign-in completed".into()));
        }
        tokio::time::sleep(interval).await;

        // A network blip here shouldn't abort a multi-minute device-code wait the user already
        // scanned a code for -- treat it as just another "not done yet" and try again next
        // interval, the same as `authorization_pending` below, rather than failing the whole
        // sign-in over one dropped connection.
        let response = match client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", &authorization.device_code),
            ])
            .send()
            .await
        {
            Ok(response) => response,
            Err(e) => {
                eprintln!("[beacon] device code poll request failed, will retry next interval: {e}");
                continue;
            }
        };

        if response.status().is_success() {
            let tokens: TokenResponse = response.json().await?;
            return Ok(MicrosoftTokens {
                access_token: tokens.access_token,
                refresh_token: tokens.refresh_token,
                expires_in: tokens.expires_in,
            });
        }

        let error: TokenErrorResponse = response.json().await?;
        match error.error.as_str() {
            "authorization_pending" => continue,
            "slow_down" => {
                interval += Duration::from_secs(5);
                continue;
            }
            "expired_token" => {
                return Err(CoreError::Auth("device code expired before sign-in completed".into()))
            }
            "access_denied" => return Err(CoreError::Auth("sign-in was denied".into())),
            other => return Err(CoreError::Auth(format!("device code sign-in failed: {other}"))),
        }
    }
}

pub async fn refresh_tokens(
    client: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
) -> Result<MicrosoftTokens> {
    let tokens: TokenResponse = send_with_retries(client.post(TOKEN_URL).form(&[
        ("grant_type", "refresh_token"),
        ("client_id", client_id),
        ("refresh_token", refresh_token),
        ("scope", OAUTH_SCOPE),
    ]))
    .await?
    .error_for_status()?
    .json()
    .await?;

    Ok(MicrosoftTokens {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_in: tokens.expires_in,
    })
}

#[derive(Debug, Serialize)]
struct XblAuthRequest<'a> {
    #[serde(rename = "Properties")]
    properties: XblAuthProperties<'a>,
    #[serde(rename = "RelyingParty")]
    relying_party: &'a str,
    #[serde(rename = "TokenType")]
    token_type: &'a str,
}

#[derive(Debug, Serialize)]
struct XblAuthProperties<'a> {
    #[serde(rename = "AuthMethod")]
    auth_method: &'a str,
    #[serde(rename = "SiteName")]
    site_name: &'a str,
    #[serde(rename = "RpsTicket")]
    rps_ticket: String,
}

#[derive(Debug, Serialize)]
struct XstsAuthRequest<'a> {
    #[serde(rename = "Properties")]
    properties: XstsAuthProperties<'a>,
    #[serde(rename = "RelyingParty")]
    relying_party: &'a str,
    #[serde(rename = "TokenType")]
    token_type: &'a str,
}

#[derive(Debug, Serialize)]
struct XstsAuthProperties<'a> {
    #[serde(rename = "SandboxId")]
    sandbox_id: &'a str,
    #[serde(rename = "UserTokens")]
    user_tokens: [&'a str; 1],
}

#[derive(Debug, Deserialize)]
struct XboxAuthResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: DisplayClaims,
}

#[derive(Debug, Deserialize)]
struct DisplayClaims {
    xui: Vec<XuiClaim>,
}

#[derive(Debug, Deserialize)]
struct XuiClaim {
    uhs: String,
    #[serde(default)]
    xid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XstsErrorResponse {
    #[serde(rename = "XErr")]
    x_err: u64,
}

fn xsts_error_message(x_err: u64) -> String {
    match x_err {
        2148916233 => {
            "this Microsoft account has no Xbox Live profile; sign in at xbox.com once to create one"
                .to_string()
        }
        2148916235 => "Xbox Live is not available in this account's country".to_string(),
        2148916236 | 2148916237 => {
            "adult verification is required on this Xbox Live account".to_string()
        }
        2148916238 => {
            "this is a child account; it must be added to a Family group before it can sign in"
                .to_string()
        }
        other => format!("Xbox Live sign-in rejected the account (XErr {other})"),
    }
}

/// Reads the response as JSON on success, or as an `Auth` error carrying the status and response
/// body on failure -- `error_for_status()` alone discards the body, which is where Xbox
/// Live/Minecraft Services APIs put the actually useful error detail.
pub(crate) async fn json_or_error<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    step: &str,
) -> Result<T> {
    let status = response.status();
    let bytes = response.bytes().await?;
    if !status.is_success() {
        let body = String::from_utf8_lossy(&bytes);
        return Err(CoreError::Auth(format!("{step} failed ({status}): {body}")));
    }
    serde_json::from_slice(&bytes).map_err(|e| {
        CoreError::Auth(format!(
            "{step}: unexpected response shape: {e} (body: {})",
            String::from_utf8_lossy(&bytes)
        ))
    })
}

async fn authenticate_xbox_live(client: &reqwest::Client, ms_access_token: &str) -> Result<String> {
    let body = XblAuthRequest {
        properties: XblAuthProperties {
            auth_method: "RPS",
            site_name: "user.auth.xboxlive.com",
            rps_ticket: format!("d={ms_access_token}"),
        },
        relying_party: "http://auth.xboxlive.com",
        token_type: "JWT",
    };
    let response = send_with_retries(client.post(XBOX_LIVE_AUTH_URL).json(&body)).await?;
    let response: XboxAuthResponse = json_or_error(response, "Xbox Live sign-in").await?;
    Ok(response.token)
}

struct XstsResult {
    token: String,
    uhs: String,
    xuid: String,
}

async fn authenticate_xsts(client: &reqwest::Client, xbl_token: &str) -> Result<XstsResult> {
    let body = XstsAuthRequest {
        properties: XstsAuthProperties {
            sandbox_id: "RETAIL",
            user_tokens: [xbl_token],
        },
        relying_party: "rp://api.minecraftservices.com/",
        token_type: "JWT",
    };
    let response = send_with_retries(client.post(XSTS_AUTH_URL).json(&body)).await?;

    if response.status().as_u16() == 401 {
        let error: XstsErrorResponse = response.json().await?;
        return Err(CoreError::Auth(xsts_error_message(error.x_err)));
    }

    let response: XboxAuthResponse = json_or_error(response, "XSTS authorization").await?;
    let claim = response
        .display_claims
        .xui
        .into_iter()
        .next()
        .ok_or_else(|| CoreError::Auth("XSTS response had no user claims".into()))?;

    Ok(XstsResult {
        token: response.token,
        xuid: claim.xid.unwrap_or_default(),
        uhs: claim.uhs,
    })
}

#[derive(Debug, Serialize)]
struct McLoginRequest {
    #[serde(rename = "identityToken")]
    identity_token: String,
}

#[derive(Debug, Deserialize)]
struct McLoginResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct McEntitlementResponse {
    items: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct McProfileResponse {
    id: String,
    name: String,
}

/// The full result of a Minecraft sign-in: a usable game access token plus the profile it
/// belongs to. `access_token` is short-lived (~24h) and is never persisted; only the Microsoft
/// refresh token is (see [`crate::secret_store`]), so this needs to be re-derived on every launch.
#[derive(Debug, Clone)]
pub struct MinecraftSession {
    pub access_token: String,
    pub xuid: String,
    pub uuid: Uuid,
    pub username: String,
    pub expires_at: SystemTime,
}

/// Chains Xbox Live -> XSTS -> Minecraft sign-in -> entitlement check -> profile fetch, starting
/// from a Microsoft OAuth2 access token (from [`request_device_code`]/[`poll_device_code`] or
/// [`refresh_tokens`]).
pub async fn authenticate_minecraft(
    client: &reqwest::Client,
    ms_access_token: &str,
) -> Result<MinecraftSession> {
    let xbl_token = authenticate_xbox_live(client, ms_access_token).await?;
    let xsts = authenticate_xsts(client, &xbl_token).await?;

    let identity_token = format!("XBL3.0 x={};{}", xsts.uhs, xsts.token);
    let response = send_with_retries(client.post(MC_LOGIN_URL).json(&McLoginRequest { identity_token })).await?;
    let login: McLoginResponse = json_or_error(response, "Minecraft sign-in").await?;

    let response = send_with_retries(client.get(MC_ENTITLEMENT_URL).bearer_auth(&login.access_token)).await?;
    let entitlement: McEntitlementResponse = json_or_error(response, "entitlement check").await?;
    if entitlement.items.is_empty() {
        return Err(CoreError::Auth("this Microsoft account does not own Minecraft".into()));
    }

    let profile_response = send_with_retries(client.get(MC_PROFILE_URL).bearer_auth(&login.access_token)).await?;
    if profile_response.status().as_u16() == 404 {
        return Err(CoreError::Auth(
            "this Microsoft account has no Minecraft profile (name) set up yet".into(),
        ));
    }
    let profile: McProfileResponse = json_or_error(profile_response, "profile fetch").await?;
    let uuid = Uuid::parse_str(&profile.id)
        .map_err(|e| CoreError::Auth(format!("invalid profile uuid from Mojang: {e}")))?;

    Ok(MinecraftSession {
        access_token: login.access_token,
        xuid: xsts.xuid,
        uuid,
        username: profile.name,
        expires_at: SystemTime::now() + Duration::from_secs(login.expires_in),
    })
}
