use std::collections::HashMap;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::fs::SandboxedFs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerStatus {
    Installing,
    Offline,
    Starting,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortAllocation {
    pub host_ip: String,
    pub host_port: u16,
    pub container_port: u16,
    pub protocol: String, // "tcp" or "udp"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallConfig {
    pub image: String,
    pub script: String,
    pub entrypoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub docker_image: String,
    pub startup_command: String,
    pub stop_command: Option<String>,
    pub stop_timeout_secs: u64,
    pub environment: HashMap<String, String>,
    pub allocations: Vec<PortAllocation>,
    pub memory_limit_bytes: Option<i64>,
    pub swap_limit_bytes: Option<i64>,
    pub cpu_quota: Option<i64>,
    pub cpu_period: Option<i64>,
    pub disk_quota_bytes: Option<u64>,
    pub io_throttle_read_bps: Option<u64>,
    pub io_throttle_write_bps: Option<u64>,
    pub start_detection_regex: Option<String>,
    pub crash_detection_regex: Option<String>,
    pub install_config: Option<InstallConfig>,
}

pub struct Server {
    pub config: RwLock<ServerConfig>,
    pub status: RwLock<ServerStatus>,
    pub fs: SandboxedFs,
    pub container_id: RwLock<Option<String>>,
}

impl Server {
    pub fn new(config: ServerConfig, fs: SandboxedFs) -> Arc<Self> {
        Arc::new(Self {
            config: RwLock::new(config),
            status: RwLock::new(ServerStatus::Offline),
            fs,
            container_id: RwLock::new(None),
        })
    }

    pub async fn get_status(&self) -> ServerStatus {
        *self.status.read().await
    }

    pub async fn set_status(&self, new_status: ServerStatus) {
        *self.status.write().await = new_status;
    }
}
