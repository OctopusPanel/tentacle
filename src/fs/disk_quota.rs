use std::path::Path;
use crate::error::TentacleError;

pub struct DiskQuota;

impl DiskQuota {
    pub async fn calculate_usage<P: AsRef<Path>>(path: P) -> Result<u64, TentacleError> {
        let p = path.as_ref().to_path_buf();
        if !p.exists() {
            return Ok(0);
        }

        tokio::task::spawn_blocking(move || {
            let mut total_size = 0u64;
            let mut stack = vec![p];

            while let Some(current) = stack.pop() {
                if let Ok(entries) = std::fs::read_dir(&current) {
                    for entry in entries.flatten() {
                        if let Ok(metadata) = entry.metadata() {
                            if metadata.is_dir() {
                                stack.push(entry.path());
                            } else {
                                total_size += metadata.len();
                            }
                        }
                    }
                }
            }
            Ok(total_size)
        })
        .await
        .map_err(|e| TentacleError::Internal(format!("Failed to calculate directory size: {}", e)))?
    }

    pub async fn check_quota<P: AsRef<Path>>(
        path: P,
        quota_bytes: Option<u64>,
        additional_bytes: u64,
    ) -> Result<(), TentacleError> {
        if let Some(quota) = quota_bytes {
            let current_usage = Self::calculate_usage(path).await?;
            if current_usage + additional_bytes > quota {
                return Err(TentacleError::QuotaExceeded(format!(
                    "Disk quota of {} bytes exceeded (current: {}, incoming: {})",
                    quota, current_usage, additional_bytes
                )));
            }
        }
        Ok(())
    }
}
