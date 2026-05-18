//! Admin endpoints for comment moderation.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{AppState, HttpError};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/comments/queue", get(queue))
        .route("/api/admin/comments", get(list))
        .route("/api/admin/comments/:name/approve", post(approve))
        .route("/api/admin/comments/:name/hide", post(hide))
        .route("/api/admin/comments/:name/show", post(show))
        .route("/api/admin/comments/:name", axum::routing::delete(delete))
        .route("/api/admin/replies/:name/approve", post(approve_reply))
        .route("/api/admin/replies/:name/hide", post(hide_reply))
        .route("/api/admin/replies/:name/show", post(show_reply))
        .route("/api/admin/replies/:name", axum::routing::delete(delete_reply))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CommentItem {
    pub name: String,
    pub kind: String,
    pub raw: String,
    pub content: String,
    pub owner_name: String,
    pub owner_kind: String,
    pub owner_display: String,
    pub subject_kind: String,
    pub subject_name: String,
    pub parent_name: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub approved: bool,
    pub hidden: bool,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

/// All comments and replies for admin management.
#[utoipa::path(
    get,
    path = "/api/admin/comments",
    tag = "comments",
    params(ListQuery),
    responses((status = 200, body = Vec<CommentItem>)),
)]
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<CommentItem>>, HttpError> {
    let mut items = Vec::new();
    if q.kind.as_deref().map_or(true, |kind| kind == "Comment") {
        items.extend(
            state
                .services
                .comments
                .admin_comments()?
                .into_iter()
                .map(comment_item),
        );
    }
    if q.kind.as_deref().map_or(true, |kind| kind == "Reply") {
        items.extend(
            state
                .services
                .comments
                .admin_replies()?
                .into_iter()
                .map(reply_item),
        );
    }
    items.retain(|item| match q.status.as_deref() {
        Some("pending") => !item.approved && !item.hidden,
        Some("approved") => item.approved && !item.hidden,
        Some("hidden") => item.hidden,
        Some("all") | None => true,
        Some(_) => true,
    });
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(Json(items))
}

/// Unapproved + non-hidden comments awaiting moderation.
#[utoipa::path(
    get,
    path = "/api/admin/comments/queue",
    tag = "comments",
    responses((status = 200, body = Vec<CommentItem>)),
)]
pub async fn queue(State(state): State<AppState>) -> Result<Json<Vec<CommentItem>>, HttpError> {
    let mut items: Vec<CommentItem> = state
        .services
        .comments
        .moderation_queue()?
        .into_iter()
        .map(comment_item)
        .collect();
    items.extend(
        state
            .services
            .comments
            .reply_moderation_queue()?
            .into_iter()
            .map(reply_item),
    );
    Ok(Json(items))
}

fn comment_item(c: rblog_content::content::Comment) -> CommentItem {
    let spec = c.spec.unwrap_or_default();
    CommentItem {
        name: c.metadata.name,
        kind: "Comment".to_owned(),
        raw: spec.base.raw,
        content: spec.base.content,
        owner_name: spec.base.owner.name.clone(),
        owner_kind: spec.base.owner.kind.clone(),
        owner_display: spec.base.owner.display_name.unwrap_or(spec.base.owner.name),
        subject_kind: spec.subject_ref.kind,
        subject_name: spec.subject_ref.name,
        parent_name: None,
        created_at: spec.base.creation_time,
        approved: spec.base.approved,
        hidden: spec.base.hidden,
    }
}

fn reply_item(r: rblog_content::content::Reply) -> CommentItem {
    let spec = r.spec.unwrap_or_default();
    CommentItem {
        name: r.metadata.name,
        kind: "Reply".to_owned(),
        raw: spec.base.raw,
        content: spec.base.content,
        owner_name: spec.base.owner.name.clone(),
        owner_kind: spec.base.owner.kind.clone(),
        owner_display: spec.base.owner.display_name.unwrap_or(spec.base.owner.name),
        subject_kind: "Comment".to_owned(),
        subject_name: spec.comment_name.clone(),
        parent_name: Some(spec.comment_name),
        created_at: spec.base.creation_time,
        approved: spec.base.approved,
        hidden: spec.base.hidden,
    }
}

/// Approve a comment so it shows on the public thread.
#[utoipa::path(
    post,
    path = "/api/admin/comments/{name}/approve",
    tag = "comments",
    params(("name" = String, Path, description = "Comment metadata.name")),
    responses((status = 200, description = "Approved")),
)]
pub async fn approve(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let saved = state.services.comments.approve(&name).await?;
    Ok(Json(
        serde_json::to_value(saved).map_err(|e| HttpError::Internal(e.into()))?,
    ))
}

/// Hide a comment from the public thread without deleting it.
#[utoipa::path(
    post,
    path = "/api/admin/comments/{name}/hide",
    tag = "comments",
    params(("name" = String, Path, description = "Comment metadata.name")),
    responses((status = 200, description = "Hidden")),
)]
pub async fn hide(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let saved = state.services.comments.hide(&name).await?;
    Ok(Json(
        serde_json::to_value(saved).map_err(|e| HttpError::Internal(e.into()))?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/admin/comments/{name}/show",
    tag = "comments",
    params(("name" = String, Path, description = "Comment metadata.name")),
    responses((status = 200, description = "Shown")),
)]
pub async fn show(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let saved = state.services.comments.show(&name).await?;
    Ok(Json(
        serde_json::to_value(saved).map_err(|e| HttpError::Internal(e.into()))?,
    ))
}

/// Permanently delete a comment.
#[utoipa::path(
    delete,
    path = "/api/admin/comments/{name}",
    tag = "comments",
    params(("name" = String, Path, description = "Comment metadata.name")),
    responses((status = 204, description = "Deleted")),
)]
pub async fn delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.services.comments.delete(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/admin/replies/{name}/approve",
    tag = "comments",
    params(("name" = String, Path, description = "Reply metadata.name")),
    responses((status = 200, description = "Approved")),
)]
pub async fn approve_reply(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let saved = state.services.comments.approve_reply(&name).await?;
    Ok(Json(
        serde_json::to_value(saved).map_err(|e| HttpError::Internal(e.into()))?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/admin/replies/{name}/hide",
    tag = "comments",
    params(("name" = String, Path, description = "Reply metadata.name")),
    responses((status = 200, description = "Hidden")),
)]
pub async fn hide_reply(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let saved = state.services.comments.hide_reply(&name).await?;
    Ok(Json(
        serde_json::to_value(saved).map_err(|e| HttpError::Internal(e.into()))?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/admin/replies/{name}/show",
    tag = "comments",
    params(("name" = String, Path, description = "Reply metadata.name")),
    responses((status = 200, description = "Shown")),
)]
pub async fn show_reply(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let saved = state.services.comments.show_reply(&name).await?;
    Ok(Json(
        serde_json::to_value(saved).map_err(|e| HttpError::Internal(e.into()))?,
    ))
}

#[utoipa::path(
    delete,
    path = "/api/admin/replies/{name}",
    tag = "comments",
    params(("name" = String, Path, description = "Reply metadata.name")),
    responses((status = 204, description = "Deleted")),
)]
pub async fn delete_reply(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.services.comments.delete_reply(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}
