use anyhow::{Context as _, Result};
use futures::{AsyncReadExt as _, StreamExt as _};
use gpui::AsyncApp;
use http_client::{HttpClient, Url};
use paths::remote_servers_dir;
use release_channel::ReleaseChannel;
use semver::Version;
use serde::Deserialize;
use smol::fs::File;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

#[derive(Deserialize)]
pub struct ReleaseAsset {
    pub version: String,
    pub url: String,
}

async fn release_asset(
    channel: ReleaseChannel,
    version: Option<Version>,
    os: &str,
    arch: &str,
    cx: &mut AsyncApp,
) -> Result<ReleaseAsset> {
    let version = version
        .map(|mut version| {
            version.pre = semver::Prerelease::EMPTY;
            version.build = semver::BuildMetadata::EMPTY;
            version.to_string()
        })
        .unwrap_or_else(|| "latest".to_string());
    let url = Url::parse_with_params(
        &format!(
            "https://api.zed.dev/releases/{}/{version}/asset",
            channel.dev_name()
        ),
        &[("os", os), ("arch", arch), ("asset", "zed-remote-server")],
    )?;
    let client = cx.update(|cx| cx.http_client());
    let mut response = client.get(url.as_str(), Default::default(), true).await?;
    anyhow::ensure!(
        response.status().is_success(),
        "failed to fetch remote server release: {}",
        response.status()
    );
    let mut body = Vec::new();
    response.body_mut().read_to_end(&mut body).await?;
    serde_json::from_slice(&body).context("invalid remote server release")
}

pub async fn get_remote_server_release_url(
    channel: ReleaseChannel,
    version: Option<Version>,
    os: &str,
    arch: &str,
    cx: &mut AsyncApp,
) -> Result<Option<String>> {
    Ok(Some(
        release_asset(channel, version, os, arch, cx).await?.url,
    ))
}

pub async fn download_remote_server_release(
    channel: ReleaseChannel,
    version: Option<Version>,
    os: &str,
    arch: &str,
    set_status: impl Fn(&str, &mut AsyncApp) + Send + 'static,
    cx: &mut AsyncApp,
) -> Result<PathBuf> {
    set_status("Fetching remote server release", cx);
    let release = release_asset(channel, version, os, arch, cx).await?;
    let platform_dir = remote_servers_dir()
        .join(channel.dev_name())
        .join(format!("{os}-{arch}"));
    smol::fs::create_dir_all(&platform_dir).await?;
    let version_path = platform_dir.join(format!("{}.gz", release.version));
    if smol::fs::metadata(&version_path).await.is_err() {
        set_status("Downloading remote server", cx);
        let client = cx.update(|cx| cx.http_client());
        download_remote_server_binary(&version_path, release, client).await?;
    }
    if let Err(error) = cleanup_remote_server_cache(&platform_dir, &version_path, 5).await {
        log::warn!("Failed to clean remote server cache: {error:#}");
    }
    Ok(version_path)
}

async fn download_remote_server_binary(
    target_path: &PathBuf,
    release: ReleaseAsset,
    client: Arc<dyn HttpClient>,
) -> Result<()> {
    let temp = tempfile::Builder::new().tempfile_in(remote_servers_dir())?;
    let mut temp_file = File::create(&temp).await?;

    let mut response = client.get(&release.url, Default::default(), true).await?;
    anyhow::ensure!(
        response.status().is_success(),
        "failed to download remote server release: {:?}",
        response.status()
    );
    smol::io::copy(response.body_mut(), &mut temp_file).await?;
    smol::fs::rename(&temp, &target_path).await?;

    Ok(())
}

async fn cleanup_remote_server_cache(
    platform_dir: &Path,
    keep_path: &Path,
    limit: usize,
) -> Result<()> {
    if limit == 0 {
        return Ok(());
    }

    let mut entries = smol::fs::read_dir(platform_dir).await?;
    let now = SystemTime::now();
    let mut candidates = Vec::new();

    while let Some(entry) = entries.next().await {
        let entry = entry?;
        let path = entry.path();
        if path.extension() != Some(OsStr::new("gz")) {
            continue;
        }

        let mtime = if path == keep_path {
            now
        } else {
            smol::fs::metadata(&path)
                .await
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        };

        candidates.push((path, mtime));
    }

    if candidates.len() <= limit {
        return Ok(());
    }

    candidates.sort_by(|(path_a, time_a), (path_b, time_b)| {
        time_b.cmp(time_a).then_with(|| path_a.cmp(path_b))
    });

    for (index, (path, _)) in candidates.into_iter().enumerate() {
        if index < limit || path == keep_path {
            continue;
        }

        if let Err(error) = smol::fs::remove_file(&path).await {
            log::warn!(
                "Failed to remove old remote server archive {:?}: {}",
                path,
                error
            );
        }
    }

    Ok(())
}
