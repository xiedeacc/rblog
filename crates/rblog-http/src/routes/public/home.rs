//! Home page (paginated post list) and archive list.

use axum::extract::{Path, State};
use axum::response::Html;
use rblog_content::content::{Comment, Post};
use rblog_core::{PostListQuery, PostStatusFilter};
use rblog_index::ListOptions;
use rblog_scheme::Extension as _;
use serde_json::json;

use crate::routes::public::context::{base_context, pagination};
use crate::{AppState, HttpError};

const PAGE_SIZE: usize = 10;

pub async fn index(state: State<AppState>) -> Result<Html<String>, HttpError> {
    render_index(&state, 1)
}

pub async fn index_paged(
    state: State<AppState>,
    Path(page): Path<usize>,
) -> Result<Html<String>, HttpError> {
    let page = page.max(1);
    render_index(&state, page)
}

fn render_index(state: &AppState, page: usize) -> Result<Html<String>, HttpError> {
    let offset = (page - 1) * PAGE_SIZE;
    let list = state.services.posts.list(PostListQuery {
        status: PostStatusFilter::Published,
        offset,
        limit: PAGE_SIZE,
        public_only: true,
        ..PostListQuery::default()
    })?;
    let all_public = state.services.posts.list(PostListQuery {
        status: PostStatusFilter::Published,
        offset: 0,
        limit: 10_000,
        public_only: true,
        ..PostListQuery::default()
    })?;
    let mut ctx = base_context(state);
    ctx["posts"] = serde_json::to_value(&list.items).unwrap_or(json!([]));
    ctx["pagination"] = pagination("/", page, PAGE_SIZE, list.total);
    ctx["home"] = homepage_context(state, &all_public.items)?;
    let renderer = active_renderer(state)?;
    Ok(Html(renderer.render("index.html", &ctx)?))
}

pub async fn archive(state: State<AppState>) -> Result<Html<String>, HttpError> {
    // Archive shows every published post on a single page. Large blogs can
    // override this template themselves; v1 keeps the simple shape.
    let list = state.services.posts.list(PostListQuery {
        status: PostStatusFilter::Published,
        offset: 0,
        limit: 10_000,
        public_only: true,
        ..PostListQuery::default()
    })?;
    let mut ctx = base_context(&state);
    ctx["posts"] = serde_json::to_value(&list.items).unwrap_or(json!([]));
    let renderer = active_renderer(&state)?;
    Ok(Html(renderer.render("archive.html", &ctx)?))
}

fn homepage_context(
    state: &AppState,
    posts: &[rblog_core::PostListItem],
) -> Result<serde_json::Value, HttpError> {
    let categories = state.services.categories.stats()?;
    let tags = state.services.tags.stats()?;
    let comments = state
        .services
        .index
        .list(&Comment::gvk(), &ListOptions::default())?;
    let visits = posts
        .iter()
        .filter_map(|post| state.services.index.get(&Post::gvk(), &post.name))
        .filter_map(|entry| {
            entry
                .raw
                .get("metadata")?
                .get("annotations")?
                .get("content.halo.run/stats")?
                .as_str()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .and_then(|stats| stats.get("visit").and_then(serde_json::Value::as_u64))
        })
        .sum::<u64>();
    let public_categories = categories
        .into_iter()
        .filter(|category| category.post_count > 0)
        .collect::<Vec<_>>();
    let public_tags = tags
        .into_iter()
        .filter(|tag| tag.post_count > 0)
        .collect::<Vec<_>>();

    Ok(json!({
        "stats": {
            "posts": posts.len(),
            "categories": public_categories.len(),
            "comments": comments.total,
            "visits": visits,
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
