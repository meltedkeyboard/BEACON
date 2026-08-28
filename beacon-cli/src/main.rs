use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use beacon_core::config::{default_config_path, LauncherConfig};
use beacon_core::downloader::DownloadProgress;
use beacon_core::launcher::LaunchOptions;
use beacon_core::{account, forget_account, install_version, launch, login_with_device_code, refresh_session};

#[derive(Parser)]
#[command(name = "beacon", about = "Minimal Minecraft launcher CLI")]
struct Cli {
    /// Override the config file location (default: OS config dir/beacon/config.json)
    #[arg(long, global = true)]
    config: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
struct AccountSelector {
    /// Offline account nickname
    #[arg(long)]
    offline: Option<String>,
    /// Saved account id, as printed by `accounts` (e.g. `microsoft:<xuid>`)
    #[arg(long)]
    account: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// List release and snapshot versions available upstream
    ListVersions {
        /// Also show snapshots (releases only by default)
        #[arg(long)]
        snapshots: bool,
    },
    /// Download client jar, libraries, natives and assets for a version
    Install { version: String },
    /// Install (if needed) and launch a version
    Launch {
        version: String,
        #[command(flatten)]
        account: AccountSelector,
        #[arg(long)]
        java: Option<String>,
    },
    /// Install a mod loader on top of a vanilla version (test harness for the loader pipeline --
    /// downloads/runs everything the GUI would, prints the resulting effective version id).
    InstallLoader {
        version: String,
        /// fabric | forge | neoforge | quilt
        kind: String,
        loader_version: String,
    },
    /// Search Modrinth for mods compatible with a given (version, loader) pair (test harness).
    SearchMods {
        query: String,
        version: String,
        /// fabric | forge | neoforge | quilt
        loader: String,
    },
    /// List every Modrinth version compatible with an instance's version/loader (test harness).
    ListModVersions { instance_id: String, project_id: String },
    /// Read a mod jar's own version/icon metadata (test harness for the Mods table's data).
    ReadModMetadata { jar_path: String },
    /// Install a mod (and its required Modrinth dependencies) into an instance (test harness).
    InstallMod {
        instance_id: String,
        project_id: String,
        /// Explicit version id instead of auto-picking the newest compatible one.
        #[arg(long)]
        version: Option<String>,
    },
    /// Sign in with a Microsoft account (device code flow) and save it
    LoginMicrosoft,
    /// List saved accounts
    Accounts,
    /// Remove a saved account and its stored credentials
    Logout { account_id: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(default_config_path);
    let http = beacon_core::http_client();

    match cli.command {
        Command::ListVersions { snapshots } => {
            let manifest = beacon_core::fetch_version_manifest(&http).await?;
            for entry in &manifest.versions {
                if !snapshots && entry.version_type != "release" {
                    continue;
                }
                println!("{:<20} {:<10} {}", entry.id, entry.version_type, entry.release_time);
            }
        }
        Command::Install { version } => {
            let config = LauncherConfig::load_or_default(&config_path).await?;
            install_version(&http, &config, &version, Some(progress_callback())).await?;
            println!("\n{version} installed.");
        }
        Command::Launch { version, account, java } => {
            let mut config = LauncherConfig::load_or_default(&config_path).await?;
            if let Some(java) = java {
                config.java_path = java;
            }

            if account.offline.is_some() && !config.has_verified_microsoft_account() {
                anyhow::bail!(
                    "Offline accounts require signing in with a Microsoft account first (run \
                     `beacon login-microsoft`) to confirm you own the game."
                );
            }

            let version_data =
                install_version(&http, &config, &version, Some(progress_callback())).await?;
            println!();

            let (account, ms_session) = if let Some(nick) = account.offline {
                (account::offline_account(&config, nick)?, None)
            } else {
                let account_id = account.account.expect("clap requires one of offline/account");
                let account = config
                    .find_account(&account_id)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("no saved account '{account_id}'"))?;
                // `--account` can now point at a saved offline account too (the GUI lets you
                // save one) -- only Microsoft accounts have a session to refresh.
                let session = match &account {
                    beacon_core::Account::Microsoft { .. } => {
                        Some(refresh_session(&http, &config.azure_client_id, &account).await?)
                    }
                    beacon_core::Account::Offline { .. } => None,
                };
                (account, session)
            };

            let options = LaunchOptions {
                game_dir: config.game_dir.clone(),
                java_path: config.java_path.clone(),
                extra_jvm_args: Vec::new(),
            };

            let mut child = launch(&config, &version_data, &account, ms_session.as_ref(), options).await?;
            if let Some(stdout) = child.stdout.take() {
                forward_lines(stdout, false);
            }
            if let Some(stderr) = child.stderr.take() {
                forward_lines(stderr, true);
            }
            let status = child.wait().await?;
            std::process::exit(status.code().unwrap_or(0));
        }
        Command::InstallLoader { version, kind, loader_version } => {
            let config = LauncherConfig::load_or_default(&config_path).await?;
            let kind = parse_loader_kind(&kind)?;
            let merged = beacon_core::modloader::install(
                &http,
                &config,
                kind,
                &version,
                &loader_version,
                Some(progress_callback()),
            )
            .await?;
            println!(
                "\n{kind:?} {loader_version} installed on {version} -- effective version id: {}",
                merged.id
            );
        }
        Command::SearchMods { query, version, loader } => {
            let kind = parse_loader_kind(&loader)?;
            let results =
                beacon_core::modsource::modrinth::search(&http, beacon_core::modsource::ContentKind::Mod, &query, Some(kind), &version, 0)
                    .await?;
            for r in &results {
                println!("{:<24} {:<40} downloads={}", r.id, r.title, r.downloads);
            }
        }
        Command::ListModVersions { instance_id, project_id } => {
            let config = LauncherConfig::load_or_default(&config_path).await?;
            let instance = config
                .find_instance(&instance_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no instance '{instance_id}'"))?;
            let kind = instance
                .mod_loader
                .as_ref()
                .map(|l| l.kind)
                .ok_or_else(|| anyhow::anyhow!("instance '{instance_id}' has no mod loader installed"))?;
            let versions = beacon_core::modsource::modrinth::list_versions(&http, &project_id, Some(kind), &instance.version_id).await?;
            for v in &versions {
                println!("{:<12} {:<24} {}", v.id, v.version_number, v.filename);
            }
        }
        Command::ReadModMetadata { jar_path } => {
            let metadata = beacon_core::mod_metadata::read_mod_metadata(std::path::Path::new(&jar_path));
            println!("version: {:?}", metadata.version);
            println!("icon_data_url: {} bytes", metadata.icon_data_url.map(|s| s.len()).unwrap_or(0));
        }
        Command::InstallMod { instance_id, project_id, version } => {
            let config = LauncherConfig::load_or_default(&config_path).await?;
            let instance = config
                .find_instance(&instance_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no instance '{instance_id}'"))?;
            let kind = instance
                .mod_loader
                .as_ref()
                .map(|l| l.kind)
                .ok_or_else(|| anyhow::anyhow!("instance '{instance_id}' has no mod loader installed"))?;
            let filename = beacon_core::modsource::modrinth::install(
                &http,
                &config,
                &instance,
                beacon_core::modsource::ContentKind::Mod,
                &project_id,
                version.as_deref(),
                Some(kind),
                Some(progress_callback()),
            )
            .await?;
            beacon_core::instance::record_content_provenance(
                &instance.mods_dir(&config),
                &filename,
                beacon_core::modsource::ModSource::Modrinth,
                &project_id,
            )?;
            println!("\ninstalled {filename} into {}", instance.mods_dir(&config).display());
        }
        Command::LoginMicrosoft => {
            let mut config = LauncherConfig::load_or_default(&config_path).await?;
            let (account, session) = login_with_device_code(&http, &config.azure_client_id, |auth| {
                println!("{}", auth.message);
                println!("Open {} and enter code: {}", auth.verification_uri, auth.user_code);
            })
            .await?;

            println!("Signed in as {} ({})", session.username, account.id());
            config.upsert_account(account.clone());
            config.selected_account_id = Some(account.id());
            config.save(&config_path).await?;
        }
        Command::Accounts => {
            let config = LauncherConfig::load_or_default(&config_path).await?;
            for saved in &config.accounts {
                let selected = if config.selected_account_id.as_deref() == Some(&saved.id()) {
                    "*"
                } else {
                    " "
                };
                println!("{selected} {:<40} {}", saved.id(), saved.username());
            }
        }
        Command::Logout { account_id } => {
            let mut config = LauncherConfig::load_or_default(&config_path).await?;
            let Some(account) = config.find_account(&account_id).cloned() else {
                anyhow::bail!("no saved account '{account_id}'");
            };
            forget_account(&account).await?;
            config.accounts.retain(|a| a.id() != account_id);
            if config.selected_account_id.as_deref() == Some(account_id.as_str()) {
                config.selected_account_id = None;
            }
            config.save(&config_path).await?;
            println!("Removed {account_id}");
        }
    }

    Ok(())
}

/// `launch()` now pipes the child's output instead of inheriting the console (a GUI frontend has
/// none to inherit into), so the CLI has to forward it itself to keep the old behavior.
fn forward_lines(reader: impl tokio::io::AsyncRead + Unpin + Send + 'static, to_stderr: bool) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if to_stderr {
                eprintln!("{line}");
            } else {
                println!("{line}");
            }
        }
    });
}

fn parse_loader_kind(kind: &str) -> anyhow::Result<beacon_core::ModLoaderKind> {
    match kind.to_ascii_lowercase().as_str() {
        "fabric" => Ok(beacon_core::ModLoaderKind::Fabric),
        "forge" => Ok(beacon_core::ModLoaderKind::Forge),
        "neoforge" => Ok(beacon_core::ModLoaderKind::NeoForge),
        "quilt" => Ok(beacon_core::ModLoaderKind::Quilt),
        other => anyhow::bail!("unknown loader kind '{other}' (expected fabric/forge/neoforge/quilt)"),
    }
}

fn progress_callback() -> Arc<dyn Fn(DownloadProgress) + Send + Sync> {
    Arc::new(|progress: DownloadProgress| {
        print!(
            "\rDownloading {}: {}/{} files ({} bytes){:<30}",
            if progress.phase.is_empty() { "files" } else { &progress.phase },
            progress.files_done,
            progress.files_total,
            progress.bytes_done,
            progress.current_file.unwrap_or_default(),
        );
        use std::io::Write;
        let _ = std::io::stdout().flush();
    })
}
