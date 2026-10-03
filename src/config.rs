use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

use crate::error::TentacleError;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub node: NodeConfig,

    #[serde(default)]
    pub auth: AuthConfig,

    #[serde(default)]
    pub docker: DockerConfig,

    #[serde(default)]
    pub storage: StorageConfig,

    #[serde(default)]
    pub sftp: SftpConfig,

    #[serde(default)]
    pub resources: ResourceConfig,

    #[serde(default)]
    pub system: SystemConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub id: String,
    pub name: String,
    pub listen_host: String,
    pub listen_port: u16,
    pub base_url: Option<String>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            id: "node-local-01".to_string(),
            name: "Local Tentacle Node".to_string(),
            listen_host: "0.0.0.0".to_string(),
            listen_port: 8080,
            base_url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub panel_secret: String,
    pub token_expiry_secs: u64,
    pub jwt_audience: Option<String>,
    pub jwt_issuer: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            panel_secret: "change-this-secret-in-production-tentacle-panel-key".to_string(),
            token_expiry_secs: 3600,
            jwt_audience: Some("tentacle-node".to_string()),
            jwt_issuer: Some("octopus-panel".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerConfig {
    pub socket_path: String,
    pub network: String,
    pub connection_timeout_secs: u64,
}

impl Default for DockerConfig {
    fn default() -> Self {
        #[cfg(unix)]
        let socket_path = "/var/run/docker.sock".to_string();
        #[cfg(windows)]
        let socket_path = "//./pipe/docker_engine".to_string();
        #[cfg(not(any(unix, windows)))]
        let socket_path = "/var/run/docker.sock".to_string();

        Self {
            socket_path,
            network: "bridge".to_string(),
            connection_timeout_secs: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub volumes_path: PathBuf,
    pub backups_path: PathBuf,
    pub temp_path: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        #[cfg(unix)]
        let (volumes_path, backups_path, temp_path) = (
            PathBuf::from("/var/lib/octopus/volumes"),
            PathBuf::from("/var/lib/octopus/backups"),
            PathBuf::from("/var/lib/octopus/tmp"),
        );

        #[cfg(windows)]
        let (volumes_path, backups_path, temp_path) = (
            PathBuf::from("./data/volumes"),
            PathBuf::from("./data/backups"),
            PathBuf::from("./data/tmp"),
        );

        #[cfg(not(any(unix, windows)))]
        let (volumes_path, backups_path, temp_path) = (
            PathBuf::from("./data/volumes"),
            PathBuf::from("./data/backups"),
            PathBuf::from("./data/tmp"),
        );

        Self {
            volumes_path,
            backups_path,
            temp_path,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpConfig {
    pub enabled: bool,
    pub listen_host: String,
    pub listen_port: u16,
    pub host_key_path: PathBuf,
}

impl Default for SftpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen_host: "0.0.0.0".to_string(),
            listen_port: 2022,
            host_key_path: PathBuf::from("./host_key"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    pub default_cpu_limit: Option<f64>,
    pub default_memory_limit_mb: Option<u64>,
    pub metrics_poll_interval_secs: u64,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            default_cpu_limit: None,
            default_memory_limit_mb: None,
            metrics_poll_interval_secs: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    pub log_level: String,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
        }
    }
}


impl Config {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, TentacleError> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(|e| {
            TentacleError::Config(format!(
                "Failed to read config file {}: {}",
                path.display(),
                e
            ))
        })?;

        let config: Config = serde_yaml::from_str(&content).map_err(|e| {
            TentacleError::Config(format!(
                "Failed to parse config YAML {}: {}",
                path.display(),
                e
            ))
        })?;

        config.ensure_directories()?;
        Ok(config)
    }

    pub fn load_or_default<P: AsRef<Path>>(path: Option<P>) -> Result<Self, TentacleError> {
        if let Some(p) = path {
            if p.as_ref().exists() {
                return Self::load_from_file(p);
            }
        }

        let default_config = Self::default();
        default_config.ensure_directories()?;
        Ok(default_config)
    }

    pub fn ensure_directories(&self) -> Result<(), TentacleError> {
        std::fs::create_dir_all(&self.storage.volumes_path)?;
        std::fs::create_dir_all(&self.storage.backups_path)?;
        std::fs::create_dir_all(&self.storage.temp_path)?;
        Ok(())
    }
}
