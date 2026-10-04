use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::config::Config;
use crate::core::container::ContainerEngine;
use crate::core::metrics::ContainerMetrics;
use crate::core::server::{Server, ServerConfig, ServerStatus};
use crate::core::stream::StreamSession;
use crate::error::TentacleError;
use crate::fs::SandboxedFs;
use crate::utils::system::SystemMonitor;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub docker: Arc<ContainerEngine>,
    pub servers: Arc<RwLock<HashMap<String, Arc<Server>>>>,
    pub streams: Arc<RwLock<HashMap<String, Arc<StreamSession>>>>,
    pub metrics_tx: broadcast::Sender<ContainerMetrics>,
    pub system_monitor: Arc<Mutex<SystemMonitor>>,
}

impl AppState {
    pub fn new(config: Config) -> Result<Self, TentacleError> {
        let docker = ContainerEngine::new(&config.docker)?;
        let (metrics_tx, _) = broadcast::channel(512);

        Ok(Self {
            config: Arc::new(config),
            docker: Arc::new(docker),
            servers: Arc::new(RwLock::new(HashMap::new())),
            streams: Arc::new(RwLock::new(HashMap::new())),
            metrics_tx,
            system_monitor: Arc::new(Mutex::new(SystemMonitor::new())),
        })
    }

    pub async fn get_server(&self, id: &str) -> Result<Arc<Server>, TentacleError> {
        let map = self.servers.read().await;
        map.get(id)
            .cloned()
            .ok_or_else(|| TentacleError::ServerNotFound(format!("Server {} not found", id)))
    }

    pub async fn get_or_create_stream(&self, id: &str) -> Arc<StreamSession> {
        let mut map = self.streams.write().await;
        if let Some(session) = map.get(id) {
            return session.clone();
        }

        let (session, _) = StreamSession::new(1000);
        let session_arc = Arc::new(session);
        map.insert(id.to_string(), session_arc.clone());
        session_arc
    }

    pub async fn create_server(&self, config: ServerConfig) -> Result<Arc<Server>, TentacleError> {
        let volume_path = self.config.storage.volumes_path.join(&config.id);
        let fs = SandboxedFs::new(&volume_path)?;

        let server = Server::new(config.clone(), fs);

        // Pre-create game container
        let container_id = self
            .docker
            .create_game_container(&config, &volume_path)
            .await?;

        {
            let mut cid_lock = server.container_id.write().await;
            *cid_lock = Some(container_id);
        }

        let mut map = self.servers.write().await;
        map.insert(config.id.clone(), server.clone());

        // Initialize stream session
        let (session, stdin_rx) = StreamSession::new(1000);
        let session = Arc::new(session);

        {
            let mut streams = self.streams.write().await;
            streams.insert(config.id.clone(), session.clone());
        }

        // Attach stream
        let cid = {
            let lock = server.container_id.read().await;
            lock.clone().unwrap()
        };

        let _ = StreamSession::attach_and_run(
            self.docker.client(),
            &cid,
            server.clone(),
            session,
            stdin_rx,
        )
        .await;

        // Persist server configuration to disk
        let servers_dir = self.config.storage.volumes_path.join("..").join("servers");
        let _ = tokio::fs::create_dir_all(&servers_dir).await;
        let config_file = servers_dir.join(format!("{}.json", config.id));
        if let Ok(json_bytes) = serde_json::to_vec_pretty(&config) {
            let _ = tokio::fs::write(&config_file, json_bytes).await;
        }

        Ok(server)
    }

    pub async fn delete_server(&self, id: &str) -> Result<(), TentacleError> {
        let server = self.get_server(id).await?;

        // 1. Remove container
        if let Some(cid) = server.container_id.read().await.clone() {
            let _ = self.docker.remove_container(&cid).await;
        }

        // 2. Remove volume directory
        let volume_path = self.config.storage.volumes_path.join(id);
        if volume_path.exists() {
            tokio::fs::remove_dir_all(&volume_path).await?;
        }

        // 3. Remove persistent config file
        let servers_dir = self.config.storage.volumes_path.join("..").join("servers");
        let config_file = servers_dir.join(format!("{}.json", id));
        let _ = tokio::fs::remove_file(&config_file).await;

        // 4. Remove from registry
        {
            let mut map = self.servers.write().await;
            map.remove(id);
        }
        {
            let mut streams = self.streams.write().await;
            streams.remove(id);
        }

        Ok(())
    }

    pub async fn restore_servers(&self) -> Result<(), TentacleError> {
        let servers_dir = self.config.storage.volumes_path.join("..").join("servers");
        if !servers_dir.exists() {
            return Ok(());
        }

        let mut read_dir = match tokio::fs::read_dir(&servers_dir).await {
            Ok(rd) => rd,
            Err(_) => return Ok(()),
        };
        let mut server_configs = Vec::new();

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    if let Ok(config) = serde_json::from_str::<ServerConfig>(&content) {
                        server_configs.push(config);
                    }
                }
            }
        }

        for config in server_configs {
            let server_id = config.id.clone();
            let volume_path = self.config.storage.volumes_path.join(&server_id);
            let fs = match SandboxedFs::new(&volume_path) {
                Ok(fs) => fs,
                Err(e) => {
                    tracing::warn!("Failed to initialize sandboxed FS for server {}: {}", server_id, e);
                    continue;
                }
            };

            let server = Server::new(config.clone(), fs);
            let container_name = format!("octopus-{}", server_id);

            if let Ok(inspect) = self.docker.client().inspect_container(&container_name, None).await {
                if let Some(cid) = inspect.id {
                    {
                        let mut cid_lock = server.container_id.write().await;
                        *cid_lock = Some(cid.clone());
                    }

                    let state = inspect.state.and_then(|s| s.status);
                    let status = match state {
                        Some(bollard::models::ContainerStateStatusEnum::RUNNING) => ServerStatus::Running,
                        Some(bollard::models::ContainerStateStatusEnum::RESTARTING) => ServerStatus::Starting,
                        _ => ServerStatus::Offline,
                    };
                    server.set_status(status).await;

                    // Initialize stream session
                    let (session, stdin_rx) = StreamSession::new(1000);
                    let session = Arc::new(session);
                    {
                        let mut streams = self.streams.write().await;
                        streams.insert(server_id.clone(), session.clone());
                    }

                    let _ = StreamSession::attach_and_run(
                        self.docker.client(),
                        &cid,
                        server.clone(),
                        session,
                        stdin_rx,
                    )
                    .await;
                }
            }

            {
                let mut map = self.servers.write().await;
                map.insert(server_id.clone(), server.clone());
            }
            tracing::info!("Restored server '{}' ({}) from persistent storage", config.name, server_id);
        }

        Ok(())
    }
}
