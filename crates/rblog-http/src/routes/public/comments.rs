//! Public comment endpoints.
//!
//! Two routes live here:
//!
//! - `GET  /api/comments?subject_kind=Post&subject_name=…` — list approved
//!   comments under a post (or single page). Returns a JSON array of
//!   [`PublicComment`].
//! - `POST /api/comments` — submit a new comment. Subject is identified by
//!   either `subject_slug` (for posts) or `subject_name`. The handler runs
//!   three gates before persisting:
//!     1. A per-IP fixed-window rate limit (default 5/min).
//!     2. A honeypot field (`website`); if it's set the request is
//!        accepted silently and dropped on the floor.
//!     3. The [`SpamHeuristic`] in `rblog-core`.

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use rblog_content::content::{Comment, CommentOwner, Reply};
use rblog_core::NewComment;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::ratelimit::RateVerdict;
use crate::{AppState, HttpError};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/comments", get(list).post(submit))
        .route("/api/comments/:comment_name/replies", post(submit_reply))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub subject_kind: Option<String>,
    #[serde(default)]
    pub subject_name: Option<String>,
    #[serde(default)]
    pub subject_slug: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PublicComment {
    pub name: String,
    pub content_html: String,
    pub owner_display_name: String,
    pub owner_kind: String,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub priority: i32,
    pub top: bool,
    pub replies: Vec<PublicReply>,
}

#[derive(Debug, Serialize)]
pub struct PublicReply {
    pub name: String,
    pub content_html: String,
    pub owner_display_name: String,
    pub owner_kind: String,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub quote_reply: Option<String>,
}

impl PublicComment {
    fn from_comment(c: Comment, replies: Vec<PublicReply>) -> Self {
        let spec = c.spec.unwrap_or_default();
        Self {
            name: c.metadata.name,
            content_html: spec.base.content,
            owner_display_name: spec
                .base
                .owner
                .display_name
                .clone()
                .unwrap_or_else(|| spec.base.owner.name.clone()),
            owner_kind: spec.base.owner.kind,
            created_at: spec.base.creation_time,
            priority: spec.base.priority,
            top: spec.base.top,
            replies,
        }
    }
}

impl From<Reply> for PublicReply {
    fn from(reply: Reply) -> Self {
        let spec = reply.spec.unwrap_or_default();
        Self {
            name: reply.metadata.name,
            content_html: spec.base.content,
            owner_display_name: spec
                .base
                .owner
                .display_name
                .clone()
                .unwrap_or_else(|| spec.base.owner.name.clone()),
            owner_kind: spec.base.owner.kind,
            created_at: spec.base.creation_time,
            quote_reply: spec.quote_reply,
        }
    }
}

/// List approved comments for a subject. Either `subject_name` or
/// `subject_slug` (post slug) must be provided.
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<PublicComment>>, HttpError> {
    let kind = q.subject_kind.clone().unwrap_or_else(|| "Post".to_owned());
    let name = resolve_subject(&state, &kind, &q).await?;
    let items = state.services.comments.public_thread(&kind, &name)?;
    let mut out = Vec::with_capacity(items.len());
    for comment in items {
        let replies = state
            .services
            .comments
            .replies(comment.metadata.name())?
            .into_iter()
            .map(Into::into)
            .collect();
        out.push(PublicComment::from_comment(comment, replies));
    }
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
pub struct SubmitRequest {
    pub raw: String,
    pub display_name: String,
    pub email: String,
    #[serde(default)]
    pub subject_kind: Option<String>,
    #[serde(default)]
    pub subject_name: Option<String>,
    #[serde(default)]
    pub subject_slug: Option<String>,
    /// Honeypot field. If filled in by a bot, the handler reports success
    /// without writing anything.
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub quote_reply: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SubmitResponse {
    pub name: String,
    pub approved: bool,
    pub queued_for_moderation: bool,
}

/// Submit a comment. Returns 201 on success.
pub async fn submit(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<SubmitRequest>,
) -> Result<(StatusCode, Json<SubmitResponse>), HttpError> {
    // 1) Honeypot. Bots happily fill every input; humans don't see it.
    if body
        .website
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Ok((
            StatusCode::CREATED,
            Json(SubmitResponse {
                name: String::new(),
                approved: false,
                queued_for_moderation: true,
            }),
        ));
    }

    // 2) Rate limit.
    let client_ip = real_ip(&headers, addr);
    if let RateVerdict::Reject { retry_after } = state.comment_rate_limit.check(client_ip) {
        return Err(HttpError::rate_limited_retry_after(retry_after.as_secs()));
    }

    // 3) Validate basic fields up front.
    if body.display_name.trim().is_empty() {
        return Err(HttpError::validation("display_name must not be empty"));
    }
    if !body.email.contains('@') {
        return Err(HttpError::validation("email looks invalid"));
    }

    let kind = body
        .subject_kind
        .clone()
        .unwrap_or_else(|| "Post".to_owned());
    let subject_name = resolve_subject_for_submit(&state, &kind, &body).await?;

    let owner = CommentOwner {
        kind: "Email".to_owned(),
        name: body.email.clone(),
        display_name: Some(body.display_name.clone()),
        annotations: None,
    };
    let saved = state
        .services
        .comments
        .submit(NewComment {
            subject_kind: Some(kind),
            subject_name,
            raw: body.raw,
            owner,
            user_agent: headers
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .map(ToOwned::to_owned),
            ip_address: Some(client_ip.to_string()),
            quote_reply: None,
        })
        .await?;
    let approved = saved.spec.as_ref().is_some_and(|s| s.base.approved);
    Ok((
        StatusCode::CREATED,
        Json(SubmitResponse {
            name: saved.metadata.name,
            approved,
            queued_for_moderation: !approved,
        }),
    ))
}

pub async fn submit_reply(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(comment_name): Path<String>,
    Json(body): Json<SubmitRequest>,
) -> Result<(StatusCode, Json<SubmitResponse>), HttpError> {
    if body
        .website
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Ok((
            StatusCode::CREATED,
            Json(SubmitResponse {
                name: String::new(),
                approved: false,
                queued_for_moderation: true,
            }),
        ));
    }

    let client_ip = real_ip(&headers, addr);
    if let RateVerdict::Reject { retry_after } = state.comment_rate_limit.check(client_ip) {
        return Err(HttpError::rate_limited_retry_after(retry_after.as_secs()));
    }
    if body.display_name.trim().is_empty() {
        return Err(HttpError::validation("display_name must not be empty"));
    }
    if !body.email.contains('@') {
        return Err(HttpError::validation("email looks invalid"));
    }
    let owner = CommentOwner {
        kind: "Email".to_owned(),
        name: body.email.clone(),
        display_name: Some(body.display_name.clone()),
        annotations: None,
    };
    let saved = state
        .services
        .comments
        .reply(
            &comment_name,
            NewComment {
                subject_kind: Some("Post".to_owned()),
                subject_name: String::new(),
                raw: body.raw,
                owner,
                user_agent: headers
                    .get("user-agent")
                    .and_then(|v| v.to_str().ok())
                    .map(ToOwned::to_owned),
                ip_address: Some(client_ip.to_string()),
                quote_reply: body.quote_reply,
            },
        )
        .await?;
    let approved = saved.spec.as_ref().is_some_and(|s| s.base.approved);
    Ok((
        StatusCode::CREATED,
        Json(SubmitResponse {
            name: saved.metadata.name,
            approved,
            queued_for_moderation: !approved,
        }),
    ))
}

fn real_ip(headers: &HeaderMap, addr: SocketAddr) -> std::net::IpAddr {
    if let Some(forwarded) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse().ok())
    {
        return forwarded;
    }
    addr.ip()
}

async fn resolve_subject(state: &AppState, kind: &str, q: &ListQuery) -> Result<String, HttpError> {
    if let Some(name) = q.subject_name.as_ref() {
        return Ok(name.clone());
    }
    if let Some(slug) = q.subject_slug.as_ref() {
        return name_from_slug(state, kind, slug).await;
    }
    Err(HttpError::validation(
        "either subject_name or subject_slug is required",
    ))
}

async fn resolve_subject_for_submit(
    state: &AppState,
    kind: &str,
    body: &SubmitRequest,
) -> Result<String, HttpError> {
    if let Some(name) = body.subject_name.as_ref() {
        return Ok(name.clone());
    }
    if let Some(slug) = body.subject_slug.as_ref() {
        return name_from_slug(state, kind, slug).await;
    }
    Err(HttpError::validation(
        "either subject_name or subject_slug is required",
    ))
}

async fn name_from_slug(state: &AppState, kind: &str, slug: &str) -> Result<String, HttpError> {
    state
        .services
        .posts
        .public_by_slug(slug)
        .await
        .map(|p| p.name)
        .map_err(|_| HttpError::not_found(format!("{kind} `{slug}` not found")))
}
