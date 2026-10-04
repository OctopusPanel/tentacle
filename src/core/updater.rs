use std::path::{Path, PathBuf};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_tar::Archive as AsyncTarArchive;
use async_compression::tokio::bufread::GzipDecoder;
use tracing::info;
#[cfg(unix)]
use tracing::{error, warn};

use crate::error::TentacleError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePayload {
    #[serde(alias = "target_version", alias = "targetVersion")]
    pub target_version: String,
    pub sha256: String,
    #[serde(alias = "download_url", alias = "downloadUrl")]
    pub download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    pub success: bool,
    pub message: String,
    pub target_version: String,
}

pub struct TentacleUpdater;

impl TentacleUpdater {
    pub async fn apply_update(payload: &UpdatePayload) -> Result<UpdateResult, TentacleError> {
        info!(
            target_version = %payload.target_version,
            download_url = %payload.download_url,
            "Received Tentacle remote update request"
        );

        let target_bin = std::env::var("TENTACLE_BIN_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/usr/local/bin/tentacle"));

        let downloaded_bytes = Self::download_file(&payload.download_url).await?;

        Self::verify_sha256(&downloaded_bytes, &payload.sha256)?;

        let binary_bytes = Self::extract_or_resolve_binary(&downloaded_bytes, &payload.download_url).await?;

        Self::atomically_replace_binary(&target_bin, &binary_bytes).await?;

        info!(
            target_version = %payload.target_version,
            target_bin = %target_bin.display(),
            "Tentacle binary updated successfully. Scheduling daemon restart."
        );

        Self::schedule_daemon_restart();

        Ok(UpdateResult {
            success: true,
            message: format!("Successfully updated Tentacle to {}", payload.target_version),
            target_version: payload.target_version.clone(),
        })
    }

    pub fn verify_sha256(data: &[u8], expected_sha256: &str) -> Result<(), TentacleError> {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let actual_hash = hex::encode(hasher.finalize());

        let expected_trimmed = expected_sha256.trim();
        if !actual_hash.eq_ignore_ascii_case(expected_trimmed) {
            return Err(TentacleError::Update(format!(
                "SHA256 checksum mismatch: expected {}, calculated {}",
                expected_trimmed, actual_hash
            )));
        }

        Ok(())
    }

    async fn download_file(url: &str) -> Result<Vec<u8>, TentacleError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| TentacleError::Update(format!("Failed to build HTTP client: {}", e)))?;

        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| TentacleError::Update(format!("Failed to download update package: {}", e)))?;

        if !response.status().is_success() {
            return Err(TentacleError::Update(format!(
                "Failed to download update: HTTP status {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| TentacleError::Update(format!("Failed to read update stream: {}", e)))?;

        Ok(bytes.to_vec())
    }

    pub async fn extract_or_resolve_binary(
        data: &[u8],
        download_url: &str,
    ) -> Result<Vec<u8>, TentacleError> {
        let url_lower = download_url.to_lowercase();
        let is_tar_gz = url_lower.ends_with(".tar.gz") || url_lower.ends_with(".tgz") || (data.len() > 2 && data[0] == 0x1f && data[1] == 0x8b);

        if is_tar_gz {
            let cursor = std::io::Cursor::new(data);
            let decoder = GzipDecoder::new(cursor);
            let mut archive = AsyncTarArchive::new(decoder);
            let mut entries = archive
                .entries()
                .map_err(|e| TentacleError::Archive(format!("Failed to inspect tar.gz: {}", e)))?;

            while let Some(entry_result) = entries.next().await {
                let mut entry = entry_result
                    .map_err(|e| TentacleError::Archive(format!("Failed to read archive entry: {}", e)))?;

                let path = entry
                    .path()
                    .map_err(|e| TentacleError::Archive(format!("Invalid path in archive: {}", e)))?;

                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                if file_name == "tentacle" || file_name == "tentacle.exe" {
                    use tokio::io::AsyncReadExt;
                    let mut binary_buf = Vec::new();
                    entry
                        .read_to_end(&mut binary_buf)
                        .await
                        .map_err(|e| TentacleError::Archive(format!("Failed to read binary from archive: {}", e)))?;
                    return Ok(binary_buf);
                }
            }

            Err(TentacleError::Update(
                "Archive does not contain a 'tentacle' binary executable".to_string(),
            ))
        } else {
            Ok(data.to_vec())
        }
    }

    pub async fn atomically_replace_binary(
        target_path: &Path,
        binary_bytes: &[u8],
    ) -> Result<(), TentacleError> {
        let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
        tokio::fs::create_dir_all(parent).await?;

        let temp_filename = format!(
            ".tentacle-update-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        );
        let temp_path = parent.join(temp_filename);

        tokio::fs::write(&temp_path, binary_bytes).await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(0o755);
            if let Err(e) = tokio::fs::set_permissions(&temp_path, permissions).await {
                warn!("Could not set executable permissions on {}: {}", temp_path.display(), e);
            }
        }

        tokio::fs::rename(&temp_path, target_path).await.map_err(|e| {
            TentacleError::Update(format!(
                "Failed to replace binary at {}: {}",
                target_path.display(),
                e
            ))
        })?;

        Ok(())
    }

    fn schedule_daemon_restart() {
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            #[cfg(unix)]
            {
                info!("Executing systemctl restart tentacle");
                if let Err(e) = tokio::process::Command::new("systemctl")
                    .args(["restart", "tentacle"])
                    .spawn()
                {
                    error!("Failed to trigger systemctl restart tentacle: {}", e);
                }
            }

            #[cfg(not(unix))]
            {
                info!("Non-unix environment detected; skipping systemctl restart");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payload_deserialization() {
        let json_camel = r#"{
            "targetVersion": "v0.2.0",
            "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "downloadUrl": "https://example.com/tentacle"
        }"#;
        let payload: UpdatePayload = serde_json::from_str(json_camel).unwrap();
        assert_eq!(payload.target_version, "v0.2.0");
        assert_eq!(payload.download_url, "https://example.com/tentacle");

        let json_snake = r#"{
            "target_version": "v0.2.1",
            "sha256": "abcdef",
            "download_url": "https://example.com/tentacle-2"
        }"#;
        let payload2: UpdatePayload = serde_json::from_str(json_snake).unwrap();
        assert_eq!(payload2.target_version, "v0.2.1");
    }

    #[test]
    fn test_sha256_verification() {
        let data = b"hello octopus tentacle update";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let correct_hash = hex::encode(hasher.finalize());

        assert!(TentacleUpdater::verify_sha256(data, &correct_hash).is_ok());
        assert!(TentacleUpdater::verify_sha256(data, "000000000000").is_err());
    }

    #[tokio::test]
    async fn test_atomic_replace_binary() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target_file = temp_dir.path().join("tentacle_test_bin");

        let dummy_bytes = b"#!/bin/sh\necho test\n";
        TentacleUpdater::atomically_replace_binary(&target_file, dummy_bytes)
            .await
            .unwrap();

        assert!(target_file.exists());
        let read_bytes = tokio::fs::read(&target_file).await.unwrap();
        assert_eq!(read_bytes, dummy_bytes);
    }
}
