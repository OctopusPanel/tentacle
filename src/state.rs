use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::config::Config;
use crate::core::container::ContainerEngine;
use crate::core::metrics::ContainerMetrics;
use crate::core::server::{Server, ServerConfig};
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

        // 3. Remove from registry
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
}
