//! Home page (paginated post list) and archive list.

use axum::extract::{Path, State};
use axum::response::Response;
use axum_extra::extract::cookie::CookieJar;
use rblog_content::content::Visible;
use rblog_core::{PostListQuery, PostStatusFilter};
use serde_json::json;

use crate::routes::public::context::{base_context, current_user, pagination};
use crate::{AppState, HttpError};

const PAGE_SIZE: usize = 10;

pub async fn index(state: State<AppState>, jar: CookieJar) -> Result<Response, HttpError> {
    render_index(&state, &jar, 1).await
}

pub async fn index_paged(
    state: State<AppState>,
    jar: CookieJar,
    Path(page): Path<usize>,
) -> Result<Response, HttpError> {
    let page = page.max(1);
    render_index(&state, &jar, page).await
}

async fn render_index(
    state: &AppState,
    jar: &CookieJar,
    page: usize,
) -> Result<Response, HttpError> {
    let user = current_user(state, jar).await;
    let public_only = user.is_none();
    let offset = (page - 1) * PAGE_SIZE;
    let all_posts = state
        .services
        .posts
        .list(PostListQuery {
            status: PostStatusFilter::Published,
            offset: 0,
            limit: 10_000,
            public_only,
            ..PostListQuery::default()
        })
        .await?;
    let user_name = user
        .as_ref()
        .and_then(|value| value.get("name"))
        .and_then(serde_json::Value::as_str);
    let all_items = filter_posts_for_user(all_posts.items, user_name);
    let total = all_items.len();
    let items = all_items
        .iter()
        .skip(offset)
        .take(PAGE_SIZE)
        .cloned()
        .collect::<Vec<_>>();
    let mut ctx = base_context(state);
    ctx["current_user"] = user.unwrap_or(serde_json::Value::Null);
    ctx["posts"] = serde_json::to_value(&items).unwrap_or(json!([]));
    ctx["pagination"] = pagination("/", page, PAGE_SIZE, total);
    ctx["home"] = homepage_context(state, &all_items).await?;
    let renderer = active_renderer(state)?;
    Ok(super::no_store_html(renderer.render("index.html", &ctx)?))
}

pub async fn archive(state: State<AppState>, jar: CookieJar) -> Result<Response, HttpError> {
    let user = current_user(&state, &jar).await;
    // Archive shows every published post on a single page. Large blogs can
    // override this template themselves; v1 keeps the simple shape.
    let list = state
        .services
        .posts
        .list(PostListQuery {
            status: PostStatusFilter::Published,
            offset: 0,
            limit: 10_000,
            public_only: user.is_none(),
            ..PostListQuery::default()
        })
        .await?;
    let user_name = user
        .as_ref()
        .and_then(|value| value.get("name"))
        .and_then(serde_json::Value::as_str);
    let items = filter_posts_for_user(list.items, user_name);
    let mut ctx = base_context(&state);
    ctx["current_user"] = user.unwrap_or(serde_json::Value::Null);
    ctx["posts"] = serde_json::to_value(&items).unwrap_or(json!([]));
    let renderer = active_renderer(&state)?;
    Ok(super::no_store_html(renderer.render("archive.html", &ctx)?))
}

fn filter_posts_for_user(
    posts: Vec<rblog_core::PostListItem>,
    user_name: Option<&str>,
) -> Vec<rblog_core::PostListItem> {
    posts
        .into_iter()
        .filter(|post| post.visible != Visible::Private || post.owner.as_deref() == user_name)
        .collect()
}

async fn homepage_context(
    state: &AppState,
    posts: &[rblog_core::PostListItem],
) -> Result<serde_json::Value, HttpError> {
    let categories = state.services.categories.stats().await?;
    let tags = state.services.tags.stats().await?;
    let comments = state.services.comments.public_comment_count()?;
    let public_categories = categories
        .into_iter()
        .filter(|category| category.post_count > 0)
        .collect::<Vec<_>>();
    let public_tags = tags
        .into_iter()
        .filter(|tag| tag.post_count > 0)
        .collect::<Vec<_>>();
    let total_post_visits = posts
        .iter()
        .map(|post| u64::try_from(post.visits).unwrap_or_default())
        .sum::<u64>();

    Ok(json!({
        "stats": {
            "posts": posts.len(),
            "categories": public_categories.len(),
            "comments": comments,
            "visits": total_post_visits,
        },
        "categories": public_categories,
        "tags": public_tags,
    }))
}

fn active_renderer(
    state: &AppState,
) -> Result<std::sync::Arc<rblog_theme::ThemeRenderer>, HttpError> {
    let theme = state
        .themes
        .active()
        .map_err(|e| HttpError::Internal(anyhow::Error::new(e)))?;
    Ok(theme.renderer)
}
