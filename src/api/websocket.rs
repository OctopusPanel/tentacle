use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use crate::api::auth::{AuthQuery, TokenValidator};
use crate::core::server::ServerStatus;
use crate::error::TentacleError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct WsIncomingMessage {
    pub event: String,
    pub command: Option<String>,
    pub action: Option<String>,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(query): Query<AuthQuery>,
    State(state): State<AppState>,
) -> Result<Response, TentacleError> {
    // Validate authentication token
    let token = query
        .token
        .ok_or_else(|| TentacleError::Auth("Missing WebSocket authentication token".to_string()))?;

    if token != state.config.auth.panel_secret {
        let claims = TokenValidator::verify_jwt(&token, &state.config.auth.panel_secret)?;
        if let Some(scoped_id) = claims.server_id {
            if scoped_id != id {
                return Err(TentacleError::Auth(
                    "Token is not authorized for this server".to_string(),
                ));
            }
        }
    }

    // Verify server exists
    let server = state.get_server(&id).await?;
    let stream_session = state.get_or_create_stream(&id).await;

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, id, server, stream_session, state)))
}

async fn handle_socket(
    socket: WebSocket,
    server_id: String,
    server: std::sync::Arc<crate::core::server::Server>,
    stream: std::sync::Arc<crate::core::stream::StreamSession>,
    state: AppState,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // 1. Send initial status
    let initial_status = server.get_status().await;
    let status_msg = json!({
        "event": "status",
        "args": [initial_status],
        "data": initial_status
    });
    let _ = ws_sender.send(Message::Text(status_msg.to_string().into())).await;

    // 2. Send initial buffer snapshot
    let snapshot = {
        let buf = stream.buffer.read().await;
        buf.snapshot()
    };

    for line in snapshot {
        let console_msg = json!({
            "event": "console_output",
            "args": [line.clone()],
            "data": line
        });
        if ws_sender.send(Message::Text(console_msg.to_string().into())).await.is_err() {
            return;
        }
    }

    // Outgoing broadcast loop (console logs + stats)
    let mut log_rx = stream.broadcast_tx.subscribe();
    let mut metrics_rx = state.metrics_tx.subscribe();
    let sid_for_metrics = server_id.clone();

    let mut send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                Ok(log_msg) = log_rx.recv() => {
                    let payload = json!({
                        "event": "console_output",
                        "args": [log_msg.data.clone()],
                        "data": log_msg.data,
                        "stream": log_msg.stream,
                        "timestamp": log_msg.timestamp
                    });
                    if ws_sender.send(Message::Text(payload.to_string().into())).await.is_err() {
                        break;
                    }
                }
                Ok(metric) = metrics_rx.recv() => {
                    if metric.server_id == sid_for_metrics {
                        let payload = json!({
                            "event": "stats",
                            "args": [metric.clone()],
                            "data": metric
                        });
                        if ws_sender.send(Message::Text(payload.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    // Incoming receive loop (stdin + power actions)
    let stdin_tx = stream.stdin_tx.clone();
    let docker = state.docker.clone();
    let server_clone = server.clone();

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            if let Message::Text(text) = msg {
                if let Ok(incoming) = serde_json::from_str::<WsIncomingMessage>(&text) {
                    match incoming.event.as_str() {
                        "send_command" => {
                            if let Some(cmd) = incoming.command {
                                let _ = stdin_tx.send(cmd).await;
                            }
                        }
                        "power" => {
                            if let Some(action) = incoming.action {
                                let cid = { server_clone.container_id.read().await.clone() };
                                let timeout = { server_clone.config.read().await.stop_timeout_secs };

                                if let Some(cid) = cid {
                                    match action.to_lowercase().as_str() {
                                        "start" => {
                                            server_clone.set_status(ServerStatus::Starting).await;
                                            let _ = docker.start_container(&cid).await;
                                        }
                                        "stop" => {
                                            server_clone.set_status(ServerStatus::Stopping).await;
                                            let _ = docker.stop_container_graceful(&cid, timeout).await;
                                            server_clone.set_status(ServerStatus::Offline).await;
                                        }
                                        "restart" => {
                                            server_clone.set_status(ServerStatus::Starting).await;
                                            let _ = docker.restart_container(&cid, timeout).await;
                                        }
                                        "kill" => {
                                            let _ = docker.kill_container(&cid).await;
                                            server_clone.set_status(ServerStatus::Offline).await;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    }

    info!("WebSocket disconnected for server {}", server_id);
}
