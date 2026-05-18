//! Admin endpoints for system + namespaced settings.
//!
//! Two things live here:
//!
//! - `/api/admin/configmaps/:name` — generic key/value `ConfigMap` store.
//!   The well-known `system` ConfigMap holds blog-wide values like
//!   `site.title`, `site.baseUrl`, …
//! - `/api/admin/settings/:name` — `Setting` form schemas (à la Halo).

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use rblog_content::core::{Setting, SettingSpec};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{AppState, HttpError};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/configmaps/:name",
            get(get_configmap).put(upsert_configmap),
        )
        .route("/api/admin/system/settings", get(system).put(upsert_system))
        .route(
            "/api/admin/settings/:name",
            get(get_setting).put(upsert_setting),
        )
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ConfigMapView {
    pub name: String,
    pub data: BTreeMap<String, String>,
    pub version: Option<i64>,
}

/// Read a `ConfigMap`.
#[utoipa::path(
    get,
    path = "/api/admin/configmaps/{name}",
    tag = "settings",
    params(("name" = String, Path, description = "ConfigMap name")),
    responses(
        (status = 200, body = ConfigMapView),
        (status = 404, description = "Unknown ConfigMap"),
    ),
)]
pub async fn get_configmap(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<ConfigMapView>, HttpError> {
    let cm = state.services.configmaps.get(&name).await?;
    Ok(Json(ConfigMapView {
        name: cm.metadata.name.clone(),
        data: cm.data.unwrap_or_default(),
        version: cm.metadata.version,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ConfigMapUpsert {
    pub data: BTreeMap<String, String>,
}

/// Create or replace a `ConfigMap`.
#[utoipa::path(
    put,
    path = "/api/admin/configmaps/{name}",
    tag = "settings",
    params(("name" = String, Path, description = "ConfigMap name")),
    request_body = ConfigMapUpsert,
    responses((status = 200, body = ConfigMapView)),
)]
pub async fn upsert_configmap(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<ConfigMapUpsert>,
) -> Result<Json<ConfigMapView>, HttpError> {
    let cm = state.services.configmaps.upsert(&name, body.data).await?;
    Ok(Json(ConfigMapView {
        name: cm.metadata.name.clone(),
        data: cm.data.unwrap_or_default(),
        version: cm.metadata.version,
    }))
}

/// Shortcut for the well-known `system` ConfigMap (`site.title`, etc.).
#[utoipa::path(
    get,
    path = "/api/admin/system/settings",
    tag = "settings",
    responses((status = 200, body = ConfigMapView)),
)]
pub async fn system(State(state): State<AppState>) -> Result<Json<ConfigMapView>, HttpError> {
    let cm = state.services.configmaps.system().await?;
    Ok(Json(ConfigMapView {
        name: cm.metadata.name.clone(),
        data: cm.data.unwrap_or_default(),
        version: cm.metadata.version,
    }))
}

#[utoipa::path(
    put,
    path = "/api/admin/system/settings",
    tag = "settings",
    request_body = ConfigMapUpsert,
    responses((status = 200, body = ConfigMapView)),
)]
pub async fn upsert_system(
    State(state): State<AppState>,
    Json(body): Json<ConfigMapUpsert>,
) -> Result<Json<ConfigMapView>, HttpError> {
    let cm = state
        .services
        .configmaps
        .upsert(rblog_core::settings::SYSTEM_CONFIGMAP, body.data)
        .await?;
    Ok(Json(ConfigMapView {
        name: cm.metadata.name.clone(),
        data: cm.data.unwrap_or_default(),
        version: cm.metadata.version,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SettingView {
    pub name: String,
    pub form_schema: serde_json::Value,
}

/// Read a Setting form schema.
#[utoipa::path(
    get,
    path = "/api/admin/settings/{name}",
    tag = "settings",
    params(("name" = String, Path, description = "Setting name")),
    responses((status = 200, body = SettingView)),
)]
pub async fn get_setting(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<SettingView>, HttpError> {
    let s = state.services.settings.get(&name).await?;
    let form_schema = serde_json::to_value(s.spec.unwrap_or_default())
        .map_err(|e| HttpError::Internal(e.into()))?;
    Ok(Json(SettingView {
        name: s.metadata.name,
        form_schema,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SettingUpsert {
    /// Free-form `SettingSpec` body. The JSON is decoded with serde_json into
    /// the in-tree [`rblog_content::core::SettingSpec`] before storage.
    #[schema(value_type = Object)]
    pub spec: serde_json::Value,
}

/// Create or update a Setting.
#[utoipa::path(
    put,
    path = "/api/admin/settings/{name}",
    tag = "settings",
    params(("name" = String, Path, description = "Setting name")),
    request_body = SettingUpsert,
    responses((status = 200, body = SettingView)),
)]
pub async fn upsert_setting(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<SettingUpsert>,
) -> Result<(StatusCode, Json<SettingView>), HttpError> {
    let spec: SettingSpec = serde_json::from_value(body.spec)
        .map_err(|e| HttpError::validation(format!("invalid SettingSpec: {e}")))?;
    let setting = match state.services.settings.get(&name).await {
        Ok(mut existing) => {
            existing.spec = Some(spec);
            state.services.settings.upsert(&existing).await?
        }
        Err(rblog_core::ServiceError::NotFound { .. }) => {
            let new = Setting::new(&name).with_spec(spec);
            state.services.settings.upsert(&new).await?
        }
        Err(e) => return Err(e.into()),
    };
    let form_schema = serde_json::to_value(setting.spec.unwrap_or_default())
        .map_err(|e| HttpError::Internal(e.into()))?;
    Ok((
        StatusCode::OK,
        Json(SettingView {
            name: setting.metadata.name,
            form_schema,
        }),
    ))
}
