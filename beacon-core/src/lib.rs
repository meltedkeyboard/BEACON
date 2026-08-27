pub mod account;
pub mod assets;
pub mod auth;
pub mod config;
pub mod downloader;
pub mod error;
pub mod instance;
pub mod launcher;
pub mod libraries;
pub mod login;
pub mod manifest;
pub mod rules;
pub mod secret_store;
pub mod skins;

pub use account::{offline_account, Account};
pub use config::LauncherConfig;
pub use error::{CoreError, Result};
pub use instance::{Instance, ModInfo, ScreenshotInfo, WorldInfo};
pub use launcher::{install_version, launch, LaunchOptions};
pub use login::{forget_account, login_with_device_code, refresh_session};
pub use manifest::{fetch_version_manifest, VersionEntry, VersionManifest};
pub use skins::{CapeInfo, MinecraftProfile, SkinInfo};

/// The HTTP client every caller (CLI and Tauri alike) should build once and reuse for every
/// Mojang/Xbox Live/Minecraft Services request. Plain `reqwest::Client::new()` has no timeout at
/// all, so a single stalled request (a slow asset host, a dropped connection mid-download) hangs
/// forever with no error and no feedback -- the install/launch pipeline just looks frozen. These
/// timeouts turn that into a normal, reportable `CoreError::Http`.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("building the shared HTTP client should never fail")
}
