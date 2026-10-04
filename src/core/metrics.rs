use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use bollard::container::StatsOptions;
use bollard::Docker;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};
use tracing::warn;

use crate::core::server::{Server, ServerStatus};

/// Walking a large volume (e.g. Minecraft worlds) is expensive, so disk usage is refreshed less often than CPU/RAM.
const DISK_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerMetrics {
    pub server_id: String,
    pub cpu_usage_pct: f64,
    pub memory_bytes: u64,
    pub memory_limit_bytes: u64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    pub disk_bytes: u64,
    pub uptime_secs: u64,
    pub timestamp: u64,
}

#[derive(Default)]
struct PollerCache {
    /// (container total cpu usage, system cpu usage) from the previous tick
    cpu_samples: HashMap<String, (u64, u64)>,
    /// (last computed volume size, when it was computed)
    disk_usage: HashMap<String, (u64, Instant)>,
}

pub struct MetricsPoller;

impl MetricsPoller {
    pub fn start_polling(
        docker: Docker,
        servers: Arc<RwLock<HashMap<String, Arc<Server>>>>,
        broadcast_tx: broadcast::Sender<ContainerMetrics>,
        interval_secs: u64,
    ) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs.max(1)));
            let mut cache = PollerCache::default();

            loop {
                interval.tick().await;

                let server_list: Vec<(String, Arc<Server>)> = {
                    let map = servers.read().await;
                    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                };

                for (server_id, server) in server_list {
                    let status = server.get_status().await;
                    if status != ServerStatus::Running && status != ServerStatus::Starting {
                        cache.cpu_samples.remove(&server_id);
                        continue;
                    }

                    let cid_opt = {
                        let lock = server.container_id.read().await;
                        lock.clone()
                    };
                    let Some(cid) = cid_opt else { continue };

                    let stats_opts = StatsOptions {
                        stream: false,
                        one_shot: true,
                    };
                    let mut stats_stream = docker.stats(&cid, Some(stats_opts));
                    let Some(Ok(stats)) = stats_stream.next().await else {
                        warn!("Failed to fetch stats for container {}", cid);
                        continue;
                    };

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    let cpu_usage_pct = Self::calculate_cpu_percent(&stats, cache.cpu_samples.get(&server_id).copied());
                    cache.cpu_samples.insert(
                        server_id.clone(),
                        (
                            stats.cpu_stats.cpu_usage.total_usage,
                            stats.cpu_stats.system_cpu_usage.unwrap_or(0),
                        ),
                    );

                    let memory_bytes = stats.memory_stats.usage.unwrap_or(0);
                    let memory_limit_bytes = stats.memory_stats.limit.unwrap_or(0);

                    let mut network_rx_bytes = 0u64;
                    let mut network_tx_bytes = 0u64;
                    if let Some(networks) = stats.networks {
                        for (_, net) in networks {
                            network_rx_bytes += net.rx_bytes;
                            network_tx_bytes += net.tx_bytes;
                        }
                    }

                    let mut disk_read_bytes = 0u64;
                    let mut disk_write_bytes = 0u64;
                    if let Some(io_service) = stats.blkio_stats.io_service_bytes_recursive {
                        for entry in io_service {
                            match entry.op.to_lowercase().as_str() {
                                "read" => disk_read_bytes += entry.value,
                                "write" => disk_write_bytes += entry.value,
                                _ => {}
                            }
                        }
                    }

                    let uptime_secs = Self::container_uptime_secs(&docker, &cid, now).await;
                    let disk_bytes = Self::volume_usage(&mut cache, &server_id, server.fs.root()).await;

                    let metric = ContainerMetrics {
                        server_id: server_id.clone(),
                        cpu_usage_pct,
                        memory_bytes,
                        memory_limit_bytes,
                        network_rx_bytes,
                        network_tx_bytes,
                        disk_read_bytes,
                        disk_write_bytes,
                        disk_bytes,
                        uptime_secs,
                        timestamp: now,
                    };

                    let _ = broadcast_tx.send(metric);
                }
            }
        });
    }

    /// `one_shot` stats do not populate `precpu_stats`, so the delta is taken against our previous sample.
    fn calculate_cpu_percent(stats: &bollard::container::Stats, previous: Option<(u64, u64)>) -> f64 {
        let cpu_stats = &stats.cpu_stats;
        let precpu = &stats.precpu_stats;

        let (prev_total, prev_system) = match precpu.system_cpu_usage {
            Some(sys) if sys > 0 => (precpu.cpu_usage.total_usage, sys),
            _ => match previous {
                Some(p) => p,
                None => return 0.0,
            },
        };

        let cpu_delta = cpu_stats.cpu_usage.total_usage as f64 - prev_total as f64;
        let system_delta = cpu_stats.system_cpu_usage.unwrap_or(0) as f64 - prev_system as f64;

        if system_delta > 0.0 && cpu_delta > 0.0 {
            let online_cpus = cpu_stats
                .online_cpus
                .filter(|c| *c > 0)
                .or_else(|| cpu_stats.cpu_usage.percpu_usage.as_ref().map(|v| v.len() as u64))
                .unwrap_or(1) as f64;
            (cpu_delta / system_delta) * online_cpus * 100.0
        } else {
            0.0
        }
    }

    async fn container_uptime_secs(docker: &Docker, cid: &str, now: u64) -> u64 {
        let Ok(inspect) = docker.inspect_container(cid, None).await else {
            return 0;
        };
        let started_at = inspect
            .state
            .as_ref()
            .filter(|s| s.running.unwrap_or(false))
            .and_then(|s| s.started_at.clone());

        match started_at.and_then(|ts| chrono::DateTime::parse_from_rfc3339(&ts).ok()) {
            Some(dt) => now.saturating_sub(dt.timestamp().max(0) as u64),
            None => 0,
        }
    }

    async fn volume_usage(cache: &mut PollerCache, server_id: &str, root: &Path) -> u64 {
        if let Some((size, at)) = cache.disk_usage.get(server_id) {
            if at.elapsed() < DISK_REFRESH_INTERVAL {
                return *size;
            }
        }

        let root: PathBuf = root.to_path_buf();
        let size = tokio::task::spawn_blocking(move || dir_size(&root))
            .await
            .unwrap_or(0);
        cache.disk_usage.insert(server_id.to_string(), (size, Instant::now()));
        size
    }
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            // symlink_metadata so links pointing outside the volume are not followed
            let Ok(meta) = entry.path().symlink_metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}
