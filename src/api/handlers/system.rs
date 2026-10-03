use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::error::TentacleError;
use crate::state::AppState;
use crate::utils::system::HostSystemInfo;

pub async fn get_system_status(
    State(state): State<AppState>,
) -> Result<Json<Value>, TentacleError> {
    let node = &state.config.node;
    let mut monitor = state.system_monitor.lock().await;
    let sys_info = monitor.snapshot();

    Ok(Json(json!({
        "node_id": node.id,
        "node_name": node.name,
        "system": sys_info,
    })))
}

pub async fn get_system_metrics(
    State(state): State<AppState>,
) -> Result<Json<HostSystemInfo>, TentacleError> {
    let mut monitor = state.system_monitor.lock().await;
    let snapshot = monitor.snapshot();
    Ok(Json(snapshot))
}
