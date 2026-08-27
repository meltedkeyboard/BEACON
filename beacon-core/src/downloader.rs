use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;

use crate::error::{io_err, CoreError, Result};

const DEFAULT_CONCURRENCY: usize = 12;

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub url: String,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

/// Progress snapshot suitable for streaming to a UI (CLI progress bar today, Tauri events later).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    /// Human-readable label for what's being downloaded right now (e.g. `"Libraries"`,
    /// `"Assets"`) -- `ensure_files` itself has no idea what a batch of tasks represents, so this
    /// is always empty here and gets stamped on by the caller (see `install_version`'s
    /// `with_phase` wrapper) before the UI ever sees it.
    #[serde(default)]
    pub phase: String,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current_file: Option<String>,
}

pub type ProgressCallback = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

async fn file_matches_sha1(path: &Path, expected: &str) -> Result<bool> {
    let bytes = match tokio::fs::read(path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(io_err(path)(e)),
    };
    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    let actual = hex::encode(hasher.finalize());
    Ok(actual.eq_ignore_ascii_case(expected))
}

const MAX_FETCH_ATTEMPTS: u32 = 3;

/// GETs `url` and reads the full response body, retrying transient network failures (a dropped
/// connection, a truncated body -- both routine noise on Mojang's CDN under load, not something
/// that should force the user to click Play repeatedly by hand) up to `MAX_FETCH_ATTEMPTS` times
/// with a short backoff between attempts. A non-2xx status is treated the same as any other
/// network failure here; a bad response body (checksum mismatch) is not retried by this function
/// at all -- that's the caller's job, since retrying a response that decoded fine but hashed wrong
/// wouldn't be a network retry.
async fn fetch_with_retries(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let mut last_err = None;
    for attempt in 1..=MAX_FETCH_ATTEMPTS {
        eprintln!("[beacon] GET {url} (attempt {attempt}/{MAX_FETCH_ATTEMPTS})");
        let result = async {
            let response = client.get(url).send().await?.error_for_status()?;
            response.bytes().await.map(|b| b.to_vec())
        }
        .await;

        match result {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                eprintln!("[beacon] GET {url} failed on attempt {attempt}/{MAX_FETCH_ATTEMPTS}: {e}");
                last_err = Some(e);
                if attempt < MAX_FETCH_ATTEMPTS {
                    tokio::time::sleep(std::time::Duration::from_millis(300 * attempt as u64)).await;
                }
            }
        }
    }
    Err(last_err.expect("loop always sets last_err before exiting on failure").into())
}

/// Downloads `task.url` to `task.dest` unless a file already there matches the expected SHA1
/// (when known). Returns `true` if a network download actually happened.
pub async fn ensure_file(client: &reqwest::Client, task: &DownloadTask) -> Result<bool> {
    if let Some(expected) = &task.sha1 {
        if file_matches_sha1(&task.dest, expected).await? {
            return Ok(false);
        }
    } else if tokio::fs::try_exists(&task.dest)
        .await
        .map_err(io_err(&task.dest))?
    {
        return Ok(false);
    }

    if let Some(parent) = task.dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(io_err(parent))?;
    }

    let bytes = fetch_with_retries(client, &task.url).await?;

    if let Some(expected) = &task.sha1 {
        let mut hasher = Sha1::new();
        hasher.update(&bytes);
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            eprintln!(
                "[beacon] checksum mismatch: {} (expected {expected}, got {actual})",
                task.dest.display()
            );
            return Err(CoreError::ChecksumMismatch {
                path: task.dest.clone(),
                expected: expected.clone(),
                actual,
            });
        }
    }

    let tmp_path = task.dest.with_extension("part");
    {
        let mut file = tokio::fs::File::create(&tmp_path)
            .await
            .map_err(io_err(&tmp_path))?;
        file.write_all(&bytes).await.map_err(io_err(&tmp_path))?;
    }
    if let Err(e) = tokio::fs::rename(&tmp_path, &task.dest).await {
        eprintln!("[beacon] rename failed: {} -> {} ({e})", tmp_path.display(), task.dest.display());
        return Err(io_err(&task.dest)(e));
    }

    Ok(true)
}

/// Downloads a batch of files with bounded concurrency, verifying/skipping already-valid files
/// and reporting aggregate progress after each completed task.
pub async fn ensure_files(
    client: &reqwest::Client,
    tasks: Vec<DownloadTask>,
    on_progress: Option<ProgressCallback>,
) -> Result<()> {
    // The asset index maps many different names to the same content hash (duplicate sounds,
    // textures re-used under another name, etc.), which can produce multiple tasks with the
    // identical `dest` -- old/legacy asset indexes especially. Downloading the same destination
    // concurrently races two tasks over the same `.part` temp file: whichever renames it into
    // place second finds it already gone (the first task moved it away) and fails with a "file
    // not found" I/O error, even though the file itself ends up correct. Deduplicating by `dest`
    // removes the race instead of letting `ensure_file`'s rename lose it.
    let mut seen_dest = std::collections::HashSet::new();
    let tasks: Vec<DownloadTask> = tasks.into_iter().filter(|t| seen_dest.insert(t.dest.clone())).collect();

    let files_total = tasks.len() as u64;
    let bytes_total = tasks.iter().filter_map(|t| t.size).sum::<u64>();
    eprintln!("[beacon] ensure_files: {files_total} files queued ({bytes_total} bytes total)");
    let files_done = Arc::new(AtomicU64::new(0));
    let bytes_done = Arc::new(AtomicU64::new(0));
    let downloaded = Arc::new(AtomicU64::new(0));

    let results: Vec<Result<()>> = stream::iter(tasks.into_iter().map(|task| {
        let client = client.clone();
        let files_done = files_done.clone();
        let bytes_done = bytes_done.clone();
        let downloaded = downloaded.clone();
        let on_progress = on_progress.clone();
        async move {
            if ensure_file(&client, &task).await? {
                downloaded.fetch_add(1, Ordering::SeqCst);
            }
            let done = files_done.fetch_add(1, Ordering::SeqCst) + 1;
            let bytes = bytes_done.fetch_add(task.size.unwrap_or(0), Ordering::SeqCst)
                + task.size.unwrap_or(0);
            if let Some(cb) = on_progress {
                cb(DownloadProgress {
                    phase: String::new(),
                    files_done: done,
                    files_total,
                    bytes_done: bytes,
                    bytes_total,
                    current_file: task.dest.file_name().map(|n| n.to_string_lossy().into_owned()),
                });
            }
            Ok(())
        }
    }))
    .buffer_unordered(DEFAULT_CONCURRENCY)
    .collect()
    .await;

    let failed = results.iter().filter(|r| r.is_err()).count();
    let succeeded = files_done.load(Ordering::SeqCst);
    eprintln!(
        "[beacon] ensure_files: done -- {} downloaded, {} already valid, {failed} failed",
        downloaded.load(Ordering::SeqCst),
        succeeded - downloaded.load(Ordering::SeqCst),
    );

    for result in results {
        result?;
    }
    Ok(())
}

mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        use std::fmt::Write;
        let mut out = String::with_capacity(bytes.as_ref().len() * 2);
        for byte in bytes.as_ref() {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }
}
