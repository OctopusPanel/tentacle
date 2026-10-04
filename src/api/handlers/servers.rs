use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::server::{ServerConfig, ServerStatus};
use crate::error::TentacleError;
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerOverview {
    pub id: String,
    pub name: String,
    pub status: ServerStatus,
    pub container_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PowerActionPayload {
    pub action: String, // "start", "stop", "restart", "kill"
}

pub async fn list_servers(
    State(state): State<AppState>,
) -> Result<Json<Vec<ServerOverview>>, TentacleError> {
    let servers = state.servers.read().await;
    let mut result = Vec::new();

    for (id, server) in servers.iter() {
        let name = { server.config.read().await.name.clone() };
        let status = server.get_status().await;
        let cid = { server.container_id.read().await.clone() };

        result.push(ServerOverview {
            id: id.clone(),
            name,
            status,
            container_id: cid,
        });
    }

    Ok(Json(result))
}

pub async fn get_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, TentacleError> {
    let server = state.get_server(&id).await?;
    let config = { server.config.read().await.clone() };
    let status = server.get_status().await;
    let cid = { server.container_id.read().await.clone() };

    Ok(Json(json!({
        "config": config,
        "status": status,
        "container_id": cid,
    })))
}

pub async fn create_server(
    State(state): State<AppState>,
    Json(config): Json<ServerConfig>,
) -> Result<Json<Value>, TentacleError> {
    let server = state.create_server(config.clone()).await?;
    let cid = { server.container_id.read().await.clone() };

    Ok(Json(json!({
        "success": true,
        "id": config.id,
        "container_id": cid
    })))
}

pub async fn delete_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, TentacleError> {
    state.delete_server(&id).await?;
    Ok(Json(json!({
        "success": true,
        "message": format!("Server {} deleted successfully", id)
    })))
}

pub async fn power_action(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<PowerActionPayload>,
) -> Result<Json<Value>, TentacleError> {
    let server = state.get_server(&id).await?;
    let stop_timeout = { server.config.read().await.stop_timeout_secs };

    match payload.action.to_lowercase().as_str() {
        "start" => {
            let cid = state.ensure_container_and_stream(&server).await?;
            server.set_status(ServerStatus::Starting).await;
            state.docker.start_container(&cid).await?;
        }
        "stop" => {
            let cid = {
                let lock = server.container_id.read().await;
                lock.clone()
                    .ok_or_else(|| TentacleError::ContainerNotFound("No container associated with server".to_string()))?
            };
            server.set_status(ServerStatus::Stopping).await;
            state.docker.stop_container_graceful(&cid, stop_timeout).await?;
            server.set_status(ServerStatus::Offline).await;
        }
        "restart" => {
            let cid = state.ensure_container_and_stream(&server).await?;
            server.set_status(ServerStatus::Starting).await;
            state.docker.restart_container(&cid, stop_timeout).await?;
        }
        "kill" => {
            let cid = {
                let lock = server.container_id.read().await;
                lock.clone()
                    .ok_or_else(|| TentacleError::ContainerNotFound("No container associated with server".to_string()))?
            };
            state.docker.kill_container(&cid).await?;
            server.set_status(ServerStatus::Offline).await;
        }
        _ => {
            return Err(TentacleError::InvalidState(format!(
                "Invalid power action: {}",
                payload.action
            )))
        }
    }

    Ok(Json(json!({
        "success": true,
        "action": payload.action,
        "status": server.get_status().await
    })))
}

pub async fn install_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, TentacleError> {
    let server = state.get_server(&id).await?;
    let (install_config, env) = {
        let cfg = server.config.read().await;
        (
            cfg.install_config.clone().ok_or_else(|| {
                TentacleError::Config("Server has no install configuration defined".to_string())
            })?,
            cfg.environment.clone(),
        )
    };

    server.set_status(ServerStatus::Installing).await;
    let volume_path = server.fs.root().to_path_buf();

    let docker = state.docker.clone();
    let server_clone = server.clone();
    let server_id = id.clone();

    tokio::spawn(async move {
        match docker.run_install_pipeline(&server_id, &install_config, &volume_path, &env).await {
            Ok(_) => {
                server_clone.set_status(ServerStatus::Offline).await;
            }
            Err(_) => {
                server_clone.set_status(ServerStatus::Error).await;
            }
        }
    });

    Ok(Json(json!({
        "success": true,
        "message": format!("Installation pipeline started for server {}", id)
    })))
}
