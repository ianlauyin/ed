use std::{path::PathBuf, pin::Pin, sync::Arc};

use fs::Fs;
use futures::{AsyncRead, AsyncReadExt, io::BufReader};
use http::Request;
use http_client::{AsyncBody, HttpClient};
use serde::{Deserialize, Serialize};

use crate::devcontainer_api::DevContainerError;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TokenResponse {
    pub(crate) token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DockerManifestsResponse {
    pub(crate) layers: Vec<ManifestLayer>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestLayer {
    pub(crate) digest: String,
}

/// Gets a bearer token for pulling from a container registry repository.
///
/// This uses the registry's `/token` endpoint directly, which works for
/// `ghcr.io` and other registries that follow the same convention.  For
/// registries that require a full `WWW-Authenticate` negotiation flow this
/// would need to be extended.
pub(crate) async fn get_oci_token(
    registry: &str,
    repository_path: &str,
    client: &Arc<dyn HttpClient>,
) -> Result<TokenResponse, String> {
    let url = format!(
        "https://{registry}/token?service={registry}&scope=repository:{repository_path}:pull",
    );
    log::debug!("Fetching OCI token from: {}", url);
    get_deserialized_response("", &url, client)
        .await
        .map_err(|e| {
            log::error!("OCI token request failed for {}: {e}", url);
            e
        })
}

pub(crate) async fn get_latest_oci_manifest(
    token: &str,
    registry: &str,
    repository_path: &str,
    client: &Arc<dyn HttpClient>,
    id: Option<&str>,
) -> Result<DockerManifestsResponse, String> {
    get_oci_manifest(registry, repository_path, token, client, "latest", id).await
}

pub(crate) async fn get_oci_manifest(
    registry: &str,
    repository_path: &str,
    token: &str,
    client: &Arc<dyn HttpClient>,
    version: &str,
    id: Option<&str>,
) -> Result<DockerManifestsResponse, String> {
    let url = match id {
        Some(id) => format!("https://{registry}/v2/{repository_path}/{id}/manifests/{version}"),
        None => format!("https://{registry}/v2/{repository_path}/manifests/{version}"),
    };

    get_deserialized_response(token, &url, client).await
}

pub(crate) async fn get_deserializable_oci_blob<T>(
    token: &str,
    registry: &str,
    repository_path: &str,
    blob_digest: &str,
    client: &Arc<dyn HttpClient>,
) -> Result<T, String>
where
    T: for<'a> Deserialize<'a>,
{
    let url = format!("https://{registry}/v2/{repository_path}/blobs/{blob_digest}");
    get_deserialized_response(token, &url, client).await
}

pub(crate) async fn download_oci_tarball(
    token: &str,
    registry: &str,
    repository_path: &str,
    blob_digest: &str,
    accept_header: &str,
    dest_dir: &PathBuf,
    client: &Arc<dyn HttpClient>,
    fs: &Arc<dyn Fs>,
    id: Option<&str>,
) -> Result<(), DevContainerError> {
    let url = match id {
        Some(id) => format!("https://{registry}/v2/{repository_path}/{id}/blobs/{blob_digest}"),
        None => format!("https://{registry}/v2/{repository_path}/blobs/{blob_digest}"),
    };

    let request = Request::get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", accept_header)
        .body(AsyncBody::default())
        .map_err(|e| {
            log::error!("Failed to create blob request: {e}");
            DevContainerError::ResourceFetchFailed
        })?;

    let mut response = client.send(request).await.map_err(|e| {
        log::error!("Failed to download feature blob: {e}");
        DevContainerError::ResourceFetchFailed
    })?;
    let status = response.status();

    let body = BufReader::new(response.body_mut());

    if !status.is_success() {
        let body_text = String::from_utf8_lossy(body.buffer());
        log::error!(
            "Feature blob download returned HTTP {}: {}",
            status.as_u16(),
            body_text,
        );
        return Err(DevContainerError::ResourceFetchFailed);
    }

    futures::pin_mut!(body);
    let body: Pin<&mut (dyn AsyncRead + Send)> = body;
    let archive = async_tar::Archive::new(body);
    fs.extract_tar_file(dest_dir, archive).await.map_err(|e| {
        log::error!("Failed to extract feature tarball: {e}");
        DevContainerError::FilesystemError
    })?;

    Ok(())
}

pub(crate) async fn get_deserialized_response<T>(
    token: &str,
    url: &str,
    client: &Arc<dyn HttpClient>,
) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let request = match Request::get(url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.oci.image.manifest.v1+json")
        .body(AsyncBody::default())
    {
        Ok(request) => request,
        Err(e) => return Err(format!("Failed to create request: {}", e)),
    };
    let response = match client.send(request).await {
        Ok(response) => response,
        Err(e) => {
            return Err(format!("Failed to send request to {}: {}", url, e));
        }
    };

    let status = response.status();
    let mut output = String::new();

    if let Err(e) = response.into_body().read_to_string(&mut output).await {
        return Err(format!("Failed to read response body from {}: {}", url, e));
    };

    if !status.is_success() {
        return Err(format!(
            "OCI request to {} returned HTTP {}: {}",
            url,
            status.as_u16(),
            &output[..output.len().min(500)],
        ));
    }

    match serde_json_lenient::from_str(&output) {
        Ok(response) => Ok(response),
        Err(e) => Err(format!(
            "Failed to deserialize response from {}: {} (body: {})",
            url,
            e,
            &output[..output.len().min(500)],
        )),
    }
}
