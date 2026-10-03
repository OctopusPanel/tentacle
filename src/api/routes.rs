use axum::extract::{Request, State};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::api::auth::auth_middleware;
use crate::api::handlers::{files, servers, system};
use crate::api::websocket::ws_handler;
use crate::error::TentacleError;
use crate::state::AppState;

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let auth_filter = from_fn_with_state(state.clone(), verify_auth);

    let protected_routes = Router::new()
        // System
        .route("/api/system", get(system::get_system_status))
        .route("/api/system/metrics", get(system::get_system_metrics))
        // Servers
        .route(
            "/api/servers",
            get(servers::list_servers).post(servers::create_server),
        )
        .route(
            "/api/servers/{id}",
            get(servers::get_server).delete(servers::delete_server),
        )
        .route("/api/servers/{id}/power", post(servers::power_action))
        .route("/api/servers/{id}/install", post(servers::install_server))
        // Files
        .route(
            "/api/servers/{id}/files",
            get(files::list_files).delete(files::delete_file),
        )
        .route(
            "/api/servers/{id}/files/contents",
            get(files::read_file).post(files::write_file),
        )
        .route(
            "/api/servers/{id}/files/directory",
            post(files::create_directory),
        )
        .route("/api/servers/{id}/files/rename", post(files::rename_file))
        .route("/api/servers/{id}/files/chmod", post(files::chmod_file))
        .route(
            "/api/servers/{id}/files/compress",
            post(files::compress_files),
        )
        .route(
            "/api/servers/{id}/files/decompress",
            post(files::decompress_file),
        )
        .route_layer(auth_filter);

    let websocket_route = Router::new().route("/api/servers/{id}/ws", get(ws_handler));

    let public_routes = Router::new()
        .route("/health", get(health_check))
        .route("/api/system/health", get(health_check));

    Router::new()
        .merge(public_routes)
        .merge(websocket_route)
        .merge(protected_routes)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn verify_auth(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, TentacleError> {
    auth_middleware(&state.config, req, next).await
}

async fn health_check() -> impl IntoResponse {
    Json(json!({
        "status": "healthy",
        "service": "tentacle",
        "version": env!("CARGO_PKG_VERSION")
    }))
}
