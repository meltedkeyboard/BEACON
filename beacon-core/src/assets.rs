use std::path::Path;

use crate::downloader::{ensure_files, DownloadTask, ProgressCallback};
use crate::error::Result;
use crate::manifest::{fetch_asset_index, AssetIndexRef};

const RESOURCES_BASE_URL: &str = "https://resources.download.minecraft.net";

/// Downloads the asset index itself plus every object it references into
/// `<assets_dir>/indexes` and `<assets_dir>/objects`.
pub async fn install_assets(
    client: &reqwest::Client,
    assets_dir: &Path,
    asset_index_ref: &AssetIndexRef,
    on_progress: Option<ProgressCallback>,
) -> Result<()> {
    let indexes_dir = assets_dir.join("indexes");
    let index_dest = indexes_dir.join(format!("{}.json", asset_index_ref.id));
    ensure_files(
        client,
        vec![DownloadTask {
            url: asset_index_ref.url.clone(),
            dest: index_dest.clone(),
            sha1: Some(asset_index_ref.sha1.clone()),
            size: Some(asset_index_ref.size),
        }],
        None,
    )
    .await?;

    let index = fetch_asset_index(client, asset_index_ref).await?;
    let objects_dir = assets_dir.join("objects");

    let tasks = index
        .objects
        .values()
        .map(|object| {
            let prefix = &object.hash[0..2];
            DownloadTask {
                url: format!("{RESOURCES_BASE_URL}/{prefix}/{}", object.hash),
                dest: objects_dir.join(prefix).join(&object.hash),
                sha1: Some(object.hash.clone()),
                size: Some(object.size),
            }
        })
        .collect();

    ensure_files(client, tasks, on_progress).await
}
