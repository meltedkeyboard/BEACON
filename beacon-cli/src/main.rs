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

fn progress_callback() -> Arc<dyn Fn(DownloadProgress) + Send + Sync> {
    Arc::new(|progress: DownloadProgress| {
        print!(
            "\rDownloading {}/{} files ({} bytes){:<30}",
            progress.files_done,
            progress.files_total,
            progress.bytes_done,
            progress.current_file.unwrap_or_default(),
        );
        use std::io::Write;
        let _ = std::io::stdout().flush();
    })
}
