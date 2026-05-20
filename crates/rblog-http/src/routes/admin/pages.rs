//! Admin endpoints for standalone pages.

use axum::extract::{Extension, Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use rblog_content::content::Visible;
use rblog_core::{PageListQuery, PageSettingsUpdate, PageStatusFilter};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::routes::admin::AuthedUser;
use crate::{AppState, HttpError};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/pages", get(list))
        .route("/api/admin/pages/:name", get(detail).put(update_content))
        .route("/api/admin/pages/:name/publish", post(publish))
        .route("/api/admin/pages/:name/unpublish", post(unpublish))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub visible: Option<String>,
    #[serde(default)]
    pub include_deleted: bool,
    #[serde(default = "default_offset")]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_offset() -> usize {
    0
}

fn default_limit() -> usize {
    20
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ListPage {
    pub items: Vec<PageSummary>,
    pub total: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PageSummary {
    pub name: String,
    pub title: String,
    pub slug: String,
    pub permalink: String,
    pub publish_time: Option<chrono::DateTime<chrono::Utc>>,
    pub published: bool,
    #[schema(value_type = String, example = "PUBLIC")]
    pub visible: Visible,
    pub deleted: bool,
    pub creation_time: Option<chrono::DateTime<chrono::Utc>>,
    pub last_modify_time: Option<chrono::DateTime<chrono::Utc>>,
    pub comments_count: i32,
    pub visits: i32,
    pub image_count: usize,
    pub pinned: bool,
}

impl From<rblog_core::PageListItem> for PageSummary {
    fn from(page: rblog_core::PageListItem) -> Self {
        Self {
            name: page.name,
            title: page.title,
            slug: page.slug,
            permalink: page.permalink,
            publish_time: page.publish_time,
            published: page.published,
            visible: page.visible,
            deleted: page.deleted,
            creation_time: page.creation_time,
            last_modify_time: page.last_modify_time,
            comments_count: page.comments_count,
            visits: page.visits,
            image_count: page.image_count,
            pinned: page.pinned,
        }
    }
}

pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<ListPage>, HttpError> {
    let status = match q.status.as_deref() {
        Some("published") => PageStatusFilter::Published,
        Some("draft") => PageStatusFilter::Draft,
        Some("any") | None => PageStatusFilter::Any,
        Some(other) => {
            return Err(HttpError::validation(format!(
                "unknown status `{other}` (expected `published`, `draft`, `any`)"
            )))
        }
    };
    let visible = match q.visible.as_deref() {
        Some("PUBLIC") => Some(Visible::Public),
        Some("INTERNAL") => Some(Visible::Internal),
        Some("PRIVATE") => Some(Visible::Private),
        Some(other) => {
            return Err(HttpError::validation(format!(
                "unknown visibility `{other}` (expected `PUBLIC`, `INTERNAL`, `PRIVATE`)"
            )))
        }
        None => None,
    };
    let page = state
        .services
        .pages
        .list(PageListQuery {
            status,
            include_deleted: q.include_deleted,
            visible,
            offset: q.offset,
            limit: q.limit.min(200),
        })
        .await?;
    Ok(Json(ListPage {
        items: page.items.into_iter().map(PageSummary::from).collect(),
        total: page.total,
    }))
}

pub async fn detail(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<rblog_core::PageDetail>, HttpError> {
    Ok(Json(state.services.pages.admin_detail(&name).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateRequest {
    pub markdown: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub excerpt: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<String>, example = "PUBLIC")]
    pub visible: Option<Visible>,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub allow_comment: Option<bool>,
    #[serde(default)]
    pub publish_time: Option<Option<chrono::DateTime<chrono::Utc>>>,
}

pub async fn update_content(
    State(state): State<AppState>,
    Extension(user): Extension<AuthedUser>,
    Path(name): Path<String>,
    Json(req): Json<UpdateRequest>,
) -> Result<Json<rblog_core::PageDetail>, HttpError> {
    state
        .services
        .pages
        .update_content(&name, &req.markdown, &user.name)
        .await?;
    let detail = state
        .services
        .pages
        .update_settings(
            &name,
            PageSettingsUpdate {
                title: req.title,
                slug: req.slug,
                excerpt: req.excerpt,
                visible: req.visible,
                cover: req.cover,
                template: req.template,
                priority: req.priority,
                pinned: req.pinned,
                allow_comment: req.allow_comment,
                publish_time: req.publish_time,
            },
        )
        .await?;
    Ok(Json(detail))
}

pub async fn publish(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<rblog_core::PageDetail>, HttpError> {
    let detail = state.services.pages.publish(&name).await?;
    Ok(Json(detail))
}

pub async fn unpublish(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<rblog_core::PageDetail>, HttpError> {
    let detail = state.services.pages.unpublish(&name).await?;
    Ok(Json(detail))
}
