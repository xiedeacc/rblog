//! Admin endpoints for categories.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use rblog_content::content::{Category, CategorySpec};
use rblog_core::NewCategory;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{AppState, HttpError};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/categories", get(list).post(create))
        .route("/api/admin/categories/:name", delete(remove).put(update))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CategoryItem {
    pub name: String,
    pub display_name: String,
    pub slug: String,
    pub permalink: String,
    pub priority: i32,
    pub post_count: usize,
}

/// List categories with post-count stats, ordered by priority then name.
#[utoipa::path(
    get,
    path = "/api/admin/categories",
    tag = "categories",
    responses((status = 200, body = Vec<CategoryItem>)),
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<CategoryItem>>, HttpError> {
    let rows = state.services.categories.stats().await?;
    let items = rows
        .into_iter()
        .map(|c| CategoryItem {
            name: c.name,
            display_name: c.display_name,
            slug: c.slug,
            permalink: c.permalink,
            priority: c.priority,
            post_count: c.post_count,
        })
        .collect();
    Ok(Json(items))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateCategory {
    pub name: String,
    pub display_name: String,
    pub slug: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub post_template: Option<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub children: Option<Vec<String>>,
}

/// Create a category.
#[utoipa::path(
    post,
    path = "/api/admin/categories",
    tag = "categories",
    request_body = CreateCategory,
    responses(
        (status = 201, description = "Created"),
        (status = 409, description = "Category with that name already exists"),
    ),
)]
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateCategory>,
) -> Result<(StatusCode, Json<serde_json::Value>), HttpError> {
    let saved = state
        .services
        .categories
        .create(NewCategory {
            name: body.name,
            display_name: body.display_name,
            slug: body.slug,
            description: body.description,
            cover: body.cover,
            template: body.template,
            post_template: body.post_template,
            priority: body.priority,
            children: body.children,
        })
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(saved).map_err(|e| HttpError::Internal(e.into()))?),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateCategory {
    pub display_name: String,
    pub slug: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub post_template: Option<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub children: Option<Vec<String>>,
}

/// Update a category.
#[utoipa::path(
    put,
    path = "/api/admin/categories/{name}",
    tag = "categories",
    params(("name" = String, Path, description = "Category name")),
    request_body = UpdateCategory,
    responses((status = 200, description = "Updated")),
)]
pub async fn update(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<UpdateCategory>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let mut cat: Category = state.services.categories.get(&name).await?;
    cat.spec = Some(CategorySpec {
        display_name: body.display_name,
        slug: body.slug,
        description: body.description,
        cover: body.cover,
        template: body.template,
        post_template: body.post_template,
        priority: body.priority,
        children: body.children,
        prevent_parent_post_cascade_query: false,
        hide_from_list: false,
    });
    let saved = state.services.categories.update(&cat).await?;
    Ok(Json(
        serde_json::to_value(saved).map_err(|e| HttpError::Internal(e.into()))?,
    ))
}

/// Delete a category.
#[utoipa::path(
    delete,
    path = "/api/admin/categories/{name}",
    tag = "categories",
    params(("name" = String, Path, description = "Category name")),
    responses((status = 204, description = "Deleted")),
)]
pub async fn remove(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.services.categories.delete(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}
