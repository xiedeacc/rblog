//! Admin endpoints for posts.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rblog_content::content::Visible;
use rblog_core::{
    DraftPost, PostDetail, PostListItem, PostListQuery, PostSettingsUpdate, PostStatusFilter,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::routes::admin::AuthedUser;
use crate::search_sync;
use crate::{AppState, HttpError};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/posts", get(list).post(create))
        .route(
            "/api/admin/posts/:name",
            get(detail).put(update_content).delete(soft_delete),
        )
        .route("/api/admin/posts/:name/publish", post(publish))
        .route("/api/admin/posts/:name/unpublish", post(unpublish))
        .route("/api/admin/posts/:name/restore", post(restore))
        .route("/api/admin/posts/:name/purge", delete(purge))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ListQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub include_deleted: bool,
    #[serde(default)]
    pub deleted_only: bool,
    #[serde(default)]
    pub visible: Option<String>,
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
    pub items: Vec<PostSummary>,
    pub total: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PostSummary {
    pub name: String,
    pub title: String,
    pub slug: String,
    pub permalink: String,
    pub publish_time: Option<chrono::DateTime<chrono::Utc>>,
    pub published: bool,
    #[schema(value_type = String, example = "PUBLIC")]
    pub visible: Visible,
    pub deleted: bool,
    pub deletion_time: Option<chrono::DateTime<chrono::Utc>>,
    pub creation_time: Option<chrono::DateTime<chrono::Utc>>,
    pub last_modify_time: Option<chrono::DateTime<chrono::Utc>>,
    pub comments_count: i32,
    pub visits: i32,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
}

impl From<PostListItem> for PostSummary {
    fn from(p: PostListItem) -> Self {
        Self {
            name: p.name,
            title: p.title,
            slug: p.slug,
            permalink: p.permalink,
            publish_time: p.publish_time,
            published: p.published,
            visible: p.visible,
            deleted: p.deleted,
            deletion_time: p.deletion_time,
            creation_time: p.creation_time,
            last_modify_time: p.last_modify_time,
            comments_count: p.comments_count,
            visits: p.visits,
            tags: p.tags,
            categories: p.categories,
        }
    }
}

/// Paginated list of posts. Admins see drafts by default.
#[utoipa::path(
    get,
    path = "/api/admin/posts",
    tag = "posts",
    params(ListQuery),
    responses((status = 200, description = "Post page", body = ListPage)),
)]
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<ListPage>, HttpError> {
    state.services.posts.purge_expired_deleted().await?;
    let status = match q.status.as_deref() {
        Some("published") => PostStatusFilter::Published,
        Some("draft") => PostStatusFilter::Draft,
        Some("any") | None => PostStatusFilter::Any,
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
    let query = PostListQuery {
        status,
        include_deleted: q.include_deleted,
        deleted_only: q.deleted_only,
        visible,
        tag: q.tag,
        category: q.category,
        offset: q.offset,
        limit: q.limit.min(200),
        public_only: false,
    };
    let page = state.services.posts.list(query)?;
    Ok(Json(ListPage {
        items: page.items.into_iter().map(PostSummary::from).collect(),
        total: page.total,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateRequest {
    pub name: String,
    pub title: String,
    pub slug: String,
    pub markdown: String,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub categories: Option<Vec<String>>,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub excerpt: Option<String>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub allow_comment: Option<bool>,
    #[serde(default)]
    #[schema(value_type = String, example = "PUBLIC")]
    pub visible: Visible,
}

/// Create a draft post (with its base snapshot).
#[utoipa::path(
    post,
    path = "/api/admin/posts",
    tag = "posts",
    request_body = CreateRequest,
    responses(
        (status = 201, description = "Draft created", ),
        (status = 409, description = "Name already exists"),
        (status = 422, description = "Validation error"),
    ),
)]
pub async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<AuthedUser>,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<PostDetail>), HttpError> {
    let draft = DraftPost {
        name: body.name,
        title: body.title,
        slug: body.slug,
        markdown: body.markdown,
        owner: user.name,
        template: body.template,
        cover: body.cover,
        categories: body.categories,
        tags: body.tags,
        excerpt: body.excerpt,
        priority: body.priority,
        pinned: body.pinned,
        allow_comment: body.allow_comment,
        visible: body.visible,
    };
    let detail = state.services.posts.draft(draft).await?;
    search_sync::index_if_published(&state.search, &detail);
    Ok((StatusCode::CREATED, Json(detail)))
}

/// Admin view of a single post.
#[utoipa::path(
    get,
    path = "/api/admin/posts/{name}",
    tag = "posts",
    params(("name" = String, Path, description = "Post name")),
    responses(
        (status = 200, description = "Post detail", ),
        (status = 404, description = "Not found"),
    ),
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<PostDetail>, HttpError> {
    let detail = state.services.posts.admin_detail(&name).await?;
    Ok(Json(detail))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateContent {
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

/// Replace post content. Updates the base snapshot in place; bumps
/// `headSnapshot`.
#[utoipa::path(
    put,
    path = "/api/admin/posts/{name}",
    tag = "posts",
    params(("name" = String, Path, description = "Post name")),
    request_body = UpdateContent,
    responses((status = 200, description = "Updated", )),
)]
pub async fn update_content(
    State(state): State<AppState>,
    Extension(user): Extension<AuthedUser>,
    Path(name): Path<String>,
    Json(body): Json<UpdateContent>,
) -> Result<Json<PostDetail>, HttpError> {
    state
        .services
        .posts
        .update_content(&name, &body.markdown, &user.name)
        .await?;
    let detail = state
        .services
        .posts
        .update_settings(
            &name,
            PostSettingsUpdate {
                title: body.title,
                slug: body.slug,
                excerpt: body.excerpt,
                visible: body.visible,
                cover: body.cover,
                template: body.template,
                priority: body.priority,
                pinned: body.pinned,
                allow_comment: body.allow_comment,
                publish_time: body.publish_time,
            },
        )
        .await?;
    search_sync::index_if_published(&state.search, &detail);
    Ok(Json(detail))
}

#[derive(Debug, Deserialize, Default, ToSchema)]
pub struct PublishBody {
    #[serde(default)]
    pub publish_time: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    #[schema(value_type = Option<String>, example = "PUBLIC")]
    pub visible: Option<Visible>,
}

/// Publish a post. Sets `release_snapshot` to the head, flips the published
/// label, and stamps `publish_time`.
#[utoipa::path(
    post,
    path = "/api/admin/posts/{name}/publish",
    tag = "posts",
    params(("name" = String, Path, description = "Post name")),
    request_body = PublishBody,
    responses((status = 200, description = "Published", )),
)]
pub async fn publish(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<PublishBody>,
) -> Result<Json<PostDetail>, HttpError> {
    let opts = rblog_core::PublishOptions {
        publish_time: body.publish_time,
        visible: body.visible,
    };
    let detail = state.services.posts.publish(&name, opts).await?;
    search_sync::index_if_published(&state.search, &detail);
    Ok(Json(detail))
}

/// Unpublish a post. Snapshots are retained, just hidden from public views.
#[utoipa::path(
    post,
    path = "/api/admin/posts/{name}/unpublish",
    tag = "posts",
    params(("name" = String, Path, description = "Post name")),
    responses((status = 200, description = "Unpublished", )),
)]
pub async fn unpublish(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<PostDetail>, HttpError> {
    let detail = state.services.posts.unpublish(&name).await?;
    search_sync::index_if_published(&state.search, &detail);
    Ok(Json(detail))
}

/// Soft delete: marks the post hidden, retains everything for recovery.
#[utoipa::path(
    delete,
    path = "/api/admin/posts/{name}",
    tag = "posts",
    params(("name" = String, Path, description = "Post name")),
    responses((status = 204, description = "Soft-deleted")),
)]
pub async fn soft_delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.services.posts.soft_delete(&name).await?;
    search_sync::delete(&state.search, &name);
    Ok(StatusCode::NO_CONTENT)
}

/// Restore a soft-deleted post from the recycle bin.
#[utoipa::path(
    post,
    path = "/api/admin/posts/{name}/restore",
    tag = "posts",
    params(("name" = String, Path, description = "Post name")),
    responses((status = 200, description = "Restored", )),
)]
pub async fn restore(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<PostDetail>, HttpError> {
    let detail = state.services.posts.restore(&name).await?;
    search_sync::index_if_published(&state.search, &detail);
    Ok(Json(detail))
}

/// Hard delete: removes the post + every snapshot referencing it.
#[utoipa::path(
    delete,
    path = "/api/admin/posts/{name}/purge",
    tag = "posts",
    params(("name" = String, Path, description = "Post name")),
    responses((status = 204, description = "Purged")),
)]
pub async fn purge(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    state.services.posts.purge(&name).await?;
    search_sync::delete(&state.search, &name);
    Ok(StatusCode::NO_CONTENT)
}
