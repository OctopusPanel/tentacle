use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use bollard::container::StatsOptions;
use bollard::Docker;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};
use tracing::warn;

use crate::core::server::{Server, ServerStatus};

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
    pub timestamp: u64,
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
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;

                let server_list: Vec<(String, Arc<Server>)> = {
                    let map = servers.read().await;
                    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                };

                for (server_id, server) in server_list {
                    let status = server.get_status().await;
                    if status != ServerStatus::Running && status != ServerStatus::Starting {
                        continue;
                    }

                    let cid_opt = {
                        let lock = server.container_id.read().await;
                        lock.clone()
                    };

                    if let Some(cid) = cid_opt {
                        let stats_opts = StatsOptions {
                            stream: false,
                            one_shot: true,
                        };

                        let mut stats_stream = docker.stats(&cid, Some(stats_opts));
                        if let Some(Ok(stats)) = stats_stream.next().await {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();

                            let cpu_usage_pct = Self::calculate_cpu_percent(&stats);
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

                            let metric = ContainerMetrics {
                                server_id: server_id.clone(),
                                cpu_usage_pct,
                                memory_bytes,
                                memory_limit_bytes,
                                network_rx_bytes,
                                network_tx_bytes,
                                disk_read_bytes,
                                disk_write_bytes,
                                timestamp: now,
                            };

                            let _ = broadcast_tx.send(metric);
                        } else {
                            warn!("Failed to fetch stats for container {}", cid);
                        }
                    }
                }
            }
        });
    }

    fn calculate_cpu_percent(stats: &bollard::container::Stats) -> f64 {
        let cpu_stats = &stats.cpu_stats;
        let precpu_stats = &stats.precpu_stats;

        let cpu_delta = cpu_stats.cpu_usage.total_usage as f64
            - precpu_stats.cpu_usage.total_usage as f64;
        let system_delta = cpu_stats.system_cpu_usage.unwrap_or(0) as f64
            - precpu_stats.system_cpu_usage.unwrap_or(0) as f64;

        if system_delta > 0.0 && cpu_delta > 0.0 {
            let online_cpus = cpu_stats.online_cpus.unwrap_or(1) as f64;
            (cpu_delta / system_delta) * online_cpus * 100.0
        } else {
            0.0
        }
    }
}
