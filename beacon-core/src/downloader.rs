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

    let response = client.get(&task.url).send().await?.error_for_status()?;
    let bytes = response.bytes().await?;

    if let Some(expected) = &task.sha1 {
        let mut hasher = Sha1::new();
        hasher.update(&bytes);
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
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
    tokio::fs::rename(&tmp_path, &task.dest)
        .await
        .map_err(io_err(&task.dest))?;

    Ok(true)
}

/// Downloads a batch of files with bounded concurrency, verifying/skipping already-valid files
/// and reporting aggregate progress after each completed task.
pub async fn ensure_files(
    client: &reqwest::Client,
    tasks: Vec<DownloadTask>,
    on_progress: Option<ProgressCallback>,
) -> Result<()> {
    let files_total = tasks.len() as u64;
    let bytes_total = tasks.iter().filter_map(|t| t.size).sum::<u64>();
    let files_done = Arc::new(AtomicU64::new(0));
    let bytes_done = Arc::new(AtomicU64::new(0));

    let results: Vec<Result<()>> = stream::iter(tasks.into_iter().map(|task| {
        let client = client.clone();
        let files_done = files_done.clone();
        let bytes_done = bytes_done.clone();
        let on_progress = on_progress.clone();
        async move {
            ensure_file(&client, &task).await?;
            let done = files_done.fetch_add(1, Ordering::SeqCst) + 1;
            let bytes = bytes_done.fetch_add(task.size.unwrap_or(0), Ordering::SeqCst)
                + task.size.unwrap_or(0);
            if let Some(cb) = on_progress {
                cb(DownloadProgress {
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
