//! Admin endpoints for managing WASM plugins.
//!
//! - `GET /api/admin/plugins` — list every loaded plugin with manifest
//!   metadata, declared capabilities, and exposed HTTP routes.
//! - `GET /api/admin/plugins/{name}` — single plugin descriptor.
//! - `POST /api/admin/plugins/{name}/enable` /
//!   `POST /api/admin/plugins/{name}/disable` — toggle a plugin without
//!   a server restart.
//! - `POST /api/admin/plugins/reload` — rescan
//!   `paths.plugins_root` and rebuild the in-memory index.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use rblog_plugins::PluginInfo;
use serde::Serialize;
use utoipa::ToSchema;

use crate::{AppState, HttpError};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/plugins", get(list))
        .route("/api/admin/plugins/reload", post(reload))
        .route("/api/admin/plugins/:name", get(detail))
        .route("/api/admin/plugins/:name/enable", post(enable))
        .route("/api/admin/plugins/:name/disable", post(disable))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PluginListResponse {
    pub plugins: Vec<PluginInfoView>,
}

/// Mirror of [`rblog_plugins::PluginInfo`] with a `ToSchema` derive so
/// utoipa can pick it up. The fields match 1:1.
#[derive(Debug, Serialize, ToSchema)]
pub struct PluginInfoView {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub enabled: bool,
    pub capabilities: Vec<String>,
    pub routes: Vec<PluginRoute>,
    pub directory: String,
    pub entry: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PluginRoute {
    pub path: String,
    pub methods: Vec<String>,
}

impl From<PluginInfo> for PluginInfoView {
    fn from(p: PluginInfo) -> Self {
        Self {
            name: p.name,
            display_name: p.display_name,
            version: p.version,
            description: p.description,
            authors: p.authors,
            enabled: p.enabled,
            capabilities: p.capabilities,
            routes: p
                .routes
                .into_iter()
                .map(|r| PluginRoute {
                    path: r.normalized_path(),
                    methods: r.methods.iter().map(|m| m.to_ascii_uppercase()).collect(),
                })
                .collect(),
            directory: p.directory,
            entry: p.entry,
        }
    }
}

/// List every plugin known to the runtime.
#[utoipa::path(
    get,
    path = "/api/admin/plugins",
    tag = "plugins",
    responses((status = 200, body = PluginListResponse)),
)]
pub async fn list(State(state): State<AppState>) -> Json<PluginListResponse> {
    Json(PluginListResponse {
        plugins: state.plugins.list().into_iter().map(Into::into).collect(),
    })
}

/// Get a single plugin descriptor.
#[utoipa::path(
    get,
    path = "/api/admin/plugins/{name}",
    tag = "plugins",
    params(("name" = String, Path, description = "Plugin name")),
    responses(
        (status = 200, body = PluginInfoView),
        (status = 404, description = "Unknown plugin"),
    ),
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<PluginInfoView>, HttpError> {
    let info = state
        .plugins
        .get(&name)
        .ok_or_else(|| HttpError::not_found(format!("plugin `{name}` not found")))?;
    Ok(Json(info.into()))
}

/// Enable a plugin so it accepts HTTP traffic.
#[utoipa::path(
    post,
    path = "/api/admin/plugins/{name}/enable",
    tag = "plugins",
    params(("name" = String, Path, description = "Plugin name")),
    responses((status = 200, body = PluginInfoView)),
)]
pub async fn enable(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<PluginInfoView>, HttpError> {
    let info = state.plugins.set_enabled(&name, true)?;
    Ok(Json(info.into()))
}

/// Disable a plugin. Routes mounted under
/// `/api/plugins/<name>/*` return 409 Conflict until re-enabled.
#[utoipa::path(
    post,
    path = "/api/admin/plugins/{name}/disable",
    tag = "plugins",
    params(("name" = String, Path, description = "Plugin name")),
    responses((status = 200, body = PluginInfoView)),
)]
pub async fn disable(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<PluginInfoView>, HttpError> {
    let info = state.plugins.set_enabled(&name, false)?;
    Ok(Json(info.into()))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReloadResponse {
    pub loaded: usize,
}

/// Rescan the plugins directory and rebuild the runtime index.
#[utoipa::path(
    post,
    path = "/api/admin/plugins/reload",
    tag = "plugins",
    responses((status = 200, body = ReloadResponse)),
)]
pub async fn reload(State(state): State<AppState>) -> Result<Json<ReloadResponse>, HttpError> {
    let loaded = state
        .plugins
        .reload(&state.config.paths.plugins_root)
        .map_err(HttpError::from)?;
    Ok(Json(ReloadResponse { loaded }))
}
