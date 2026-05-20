//! Admin endpoints for tags.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use rblog_content::content::{Tag, TagSpec};
use rblog_core::NewTag;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{AppState, HttpError};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/tags", get(list).post(create))
        .route("/api/admin/tags/:name", delete(remove).put(update))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TagItem {
    pub name: String,
    pub display_name: String,
    pub slug: String,
    pub permalink: String,
    pub color: Option<String>,
    pub post_count: usize,
}

/// List tags with post-count stats, biggest first.
#[utoipa::path(
    get,
    path = "/api/admin/tags",
    tag = "tags",
    responses((status = 200, body = Vec<TagItem>)),
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<TagItem>>, HttpError> {
    let rows = state.services.tags.stats().await?;
    let items = rows
        .into_iter()
        .map(|t| TagItem {
            name: t.name,
            display_name: t.display_name,
            slug: t.slug,
            permalink: t.permalink,
            color: t.color,
            post_count: t.post_count,
        })
        .collect();
    Ok(Json(items))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTag {
    pub name: String,
    pub display_name: String,
    pub slug: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub cover: Option<String>,
}

/// Create a tag.
#[utoipa::path(
    post,
    path = "/api/admin/tags",
    tag = "tags",
    request_body = CreateTag,
    responses(
        (status = 201, description = "Created"),
        (status = 409, description = "Tag with that name already exists"),
        (status = 422, description = "Invalid slug"),
    ),
)]
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateTag>,
) -> Result<(StatusCode, Json<serde_json::Value>), HttpError> {
    let saved = state
        .services
        .tags
        .create(NewTag {
            name: body.name,
            display_name: body.display_name,
            slug: body.slug,
            description: body.description,
            color: body.color,
            cover: body.cover,
        })
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(saved).map_err(|e| HttpError::Internal(e.into()))?),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateTag {
    pub display_name: String,
    pub slug: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub cover: Option<String>,
}

/// Update a tag in place.
#[utoipa::path(
    put,
    path = "/api/admin/tags/{name}",
    tag = "tags",
    params(("name" = String, Path, description = "Tag name")),
    request_body = UpdateTag,
    responses((status = 200, description = "Updated")),
)]
pub async fn update(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<UpdateTag>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let mut tag: Tag = state.services.tags.get(&name).await?;
    tag.spec = Some(TagSpec {
        display_name: body.display_name,
        slug: body.slug,
        description: body.description,
        color: body.color,
        cover: body.cover,
    });
    let saved = state.services.tags.update(&tag).await?;
    Ok(Json(
        serde_json::to_value(saved).map_err(|e| HttpError::Internal(e.into()))?,
    ))
}

/// Delete a tag. Posts that reference it keep the tag name in their spec
/// until they are next edited.
#[utoipa::path(
    delete,
    path = "/api/admin/tags/{name}",
    tag = "tags",
    params(("name" = String, Path, description = "Tag name")),
    responses((status = 204, description = "Deleted")),
)]
pub async fn remove(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.services.tags.delete(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}
