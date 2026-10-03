use axum::extract::{Path, Query, State};
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::TentacleError;
use crate::fs::{ArchiveEngine, FileEntry};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct DirectoryQuery {
    pub directory: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FileQuery {
    pub file: String,
}

#[derive(Debug, Deserialize)]
pub struct DirectoryCreatePayload {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct FileWritePayload {
    pub file: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct RenamePayload {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize)]
pub struct ChmodPayload {
    pub file: String,
    pub mode: u32,
}

#[derive(Debug, Deserialize)]
pub struct CompressPayload {
    pub format: String, // "tar.gz", "tar.zst", "zip"
    pub dest: String,
}

#[derive(Debug, Deserialize)]
pub struct DecompressPayload {
    pub archive: String,
}

pub async fn list_files(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<Vec<FileEntry>>, TentacleError> {
    let server = state.get_server(&id).await?;
    let dir = query.directory.unwrap_or_else(|| ".".to_string());
    let entries = server.fs.list_dir(&dir).await?;
    Ok(Json(entries))
}

pub async fn read_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Response, TentacleError> {
    let server = state.get_server(&id).await?;
    let content = server.fs.read_file(&query.file).await?;

    let response = (
        [(CONTENT_TYPE, "application/octet-stream")],
        content,
    )
        .into_response();

    Ok(response)
}

pub async fn write_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<FileWritePayload>,
) -> Result<Json<Value>, TentacleError> {
    let server = state.get_server(&id).await?;
    server
        .fs
        .write_file(&payload.file, payload.content.as_bytes())
        .await?;

    Ok(Json(json!({
        "success": true,
        "file": payload.file
    })))
}

pub async fn create_directory(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<DirectoryCreatePayload>,
) -> Result<Json<Value>, TentacleError> {
    let server = state.get_server(&id).await?;
    server.fs.create_dir(&payload.path).await?;

    Ok(Json(json!({
        "success": true,
        "directory": payload.path
    })))
}

pub async fn delete_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Json<Value>, TentacleError> {
    let server = state.get_server(&id).await?;
    server.fs.delete_file(&query.file).await?;

    Ok(Json(json!({
        "success": true,
        "deleted": query.file
    })))
}

pub async fn rename_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<RenamePayload>,
) -> Result<Json<Value>, TentacleError> {
    let server = state.get_server(&id).await?;
    server.fs.rename(&payload.from, &payload.to).await?;

    Ok(Json(json!({
        "success": true,
        "from": payload.from,
        "to": payload.to
    })))
}

pub async fn chmod_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<ChmodPayload>,
) -> Result<Json<Value>, TentacleError> {
    let server = state.get_server(&id).await?;
    server.fs.chmod(&payload.file, payload.mode).await?;

    Ok(Json(json!({
        "success": true,
        "file": payload.file,
        "mode": payload.mode
    })))
}

pub async fn compress_files(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<CompressPayload>,
) -> Result<Json<Value>, TentacleError> {
    let server = state.get_server(&id).await?;
    let dest_path = server.fs.resolve_safe_path(&payload.dest)?;

    match payload.format.to_lowercase().as_str() {
        "tar.gz" | "tgz" => {
            ArchiveEngine::create_tar_gz(server.fs.root(), &dest_path).await?;
        }
        "tar.zst" | "tzst" => {
            ArchiveEngine::create_tar_zst(server.fs.root(), &dest_path).await?;
        }
        "zip" => {
            ArchiveEngine::create_zip(server.fs.root(), &dest_path).await?;
        }
        _ => {
            return Err(TentacleError::Archive(format!(
                "Unsupported format: {}",
                payload.format
            )))
        }
    }

    Ok(Json(json!({
        "success": true,
        "destination": payload.dest
    })))
}

pub async fn decompress_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<DecompressPayload>,
) -> Result<Json<Value>, TentacleError> {
    let server = state.get_server(&id).await?;
    let archive_path = server.fs.resolve_safe_path(&payload.archive)?;

    ArchiveEngine::extract_archive(&archive_path, &server.fs).await?;

    Ok(Json(json!({
        "success": true,
        "archive": payload.archive
    })))
}
