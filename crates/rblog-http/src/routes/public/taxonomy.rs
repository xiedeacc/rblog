//! Tag and category index + archive pages.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum_extra::extract::cookie::CookieJar;
use rblog_content::content::{Category, Tag};
use rblog_core::{PostListQuery, PostStatusFilter};
use rblog_scheme::Extension as _;
use serde_json::{json, Value};

use crate::routes::public::context::{base_context, current_user, pagination};
use crate::{AppState, HttpError};

const PAGE_SIZE: usize = 10;

pub async fn tags(state: State<AppState>, jar: CookieJar) -> Result<Response, HttpError> {
    let user = current_user(&state, &jar).await;
    let tag_stats = state.services.tags.stats()?;
    let mut ctx = base_context(&state);
    ctx["current_user"] = user.unwrap_or(serde_json::Value::Null);
    ctx["tags"] = serde_json::to_value(&tag_stats).unwrap_or(json!([]));
    let theme = active(&state)?;
    Ok(super::no_store_html(
        theme.render("tags.html", &ctx).or_else(|_| {
            // Fall back to per-tag template style: themes that don't ship a
            // bulk list use the category-list template (most themes overlap).
            theme.render("tag.html", &ctx)
        })?,
    ))
}

pub async fn tag_posts(
    state: State<AppState>,
    jar: CookieJar,
    Path(slug): Path<String>,
) -> Result<Response, HttpError> {
    let user = current_user(&state, &jar).await;
    let Some(tag) = resolve_tag(&state, &slug)? else {
        return Ok(render_404(&state));
    };
    let list = state.services.posts.list(PostListQuery {
        status: PostStatusFilter::Published,
        tag: Some(tag.0.clone()),
        offset: 0,
        limit: PAGE_SIZE,
        public_only: user.is_none(),
        ..PostListQuery::default()
    })?;
    let mut ctx = base_context(&state);
    ctx["current_user"] = user.unwrap_or(serde_json::Value::Null);
    ctx["tag"] = tag.1;
    ctx["posts"] = serde_json::to_value(&list.items).unwrap_or(json!([]));
    ctx["pagination"] = pagination(&format!("/tags/{slug}"), 1, PAGE_SIZE, list.total);
    Ok(super::no_store_html(
        active(&state)?.render("tag.html", &ctx)?,
    ))
}

pub async fn categories(state: State<AppState>, jar: CookieJar) -> Result<Response, HttpError> {
    let user = current_user(&state, &jar).await;
    let cat_stats = state.services.categories.stats()?;
    let mut ctx = base_context(&state);
    ctx["current_user"] = user.unwrap_or(serde_json::Value::Null);
    ctx["categories"] = serde_json::to_value(&cat_stats).unwrap_or(json!([]));
    let theme = active(&state)?;
    Ok(super::no_store_html(
        theme
            .render("categories.html", &ctx)
            .or_else(|_| theme.render("category.html", &ctx))?,
    ))
}

pub async fn category_posts(
    state: State<AppState>,
    jar: CookieJar,
    Path(slug): Path<String>,
) -> Result<Response, HttpError> {
    let user = current_user(&state, &jar).await;
    let Some(cat) = resolve_category(&state, &slug)? else {
        return Ok(render_404(&state));
    };
    let list = state.services.posts.list(PostListQuery {
        status: PostStatusFilter::Published,
        category: Some(cat.0.clone()),
        offset: 0,
        limit: PAGE_SIZE,
        public_only: user.is_none(),
        ..PostListQuery::default()
    })?;
    let mut ctx = base_context(&state);
    ctx["current_user"] = user.unwrap_or(serde_json::Value::Null);
    ctx["category"] = cat.1;
    ctx["posts"] = serde_json::to_value(&list.items).unwrap_or(json!([]));
    ctx["pagination"] = pagination(&format!("/categories/{slug}"), 1, PAGE_SIZE, list.total);
    Ok(super::no_store_html(
        active(&state)?.render("category.html", &ctx)?,
    ))
}

fn active(state: &AppState) -> Result<std::sync::Arc<rblog_theme::ThemeRenderer>, HttpError> {
    state
        .themes
        .active()
        .map(|t| t.renderer)
        .map_err(|e| HttpError::Internal(anyhow::Error::new(e)))
}

/// Resolve a tag by slug — walks the index for an entry with matching
/// `spec.slug`. Returns the (name, view-shape) pair.
fn resolve_tag(state: &AppState, slug: &str) -> Result<Option<(String, Value)>, HttpError> {
    let opts = rblog_index::ListOptions::default().with_field(rblog_index::FieldSelector::Equals {
        path: "spec.slug".to_owned(),
        value: Value::String(slug.to_owned()),
    });
    let res = state.services.index.list(&Tag::gvk(), &opts)?;
    let Some(entry) = res.items.into_iter().next() else {
        return Ok(None);
    };
    let tag: Tag = serde_json::from_value(entry.raw.clone())
        .map_err(|e| HttpError::Internal(anyhow::Error::new(e)))?;
    let spec = tag.spec.unwrap_or_default();
    Ok(Some((
        tag.metadata.name.clone(),
        json!({
            "name": tag.metadata.name,
            "display_name": spec.display_name,
            "slug": spec.slug,
            "color": spec.color,
            "description": spec.description,
        }),
    )))
}

fn resolve_category(state: &AppState, slug: &str) -> Result<Option<(String, Value)>, HttpError> {
    let opts = rblog_index::ListOptions::default().with_field(rblog_index::FieldSelector::Equals {
        path: "spec.slug".to_owned(),
        value: Value::String(slug.to_owned()),
    });
    let res = state.services.index.list(&Category::gvk(), &opts)?;
    let Some(entry) = res.items.into_iter().next() else {
        return Ok(None);
    };
    let cat: Category = serde_json::from_value(entry.raw.clone())
        .map_err(|e| HttpError::Internal(anyhow::Error::new(e)))?;
    let spec = cat.spec.unwrap_or_default();
    Ok(Some((
        cat.metadata.name.clone(),
        json!({
            "name": cat.metadata.name,
            "display_name": spec.display_name,
            "slug": spec.slug,
            "description": spec.description,
        }),
    )))
}

fn render_404(state: &AppState) -> Response {
    let ctx = base_context(state);
    let body = state
        .themes
        .active()
        .ok()
        .and_then(|t| t.renderer.render("404.html", &ctx).ok())
        .unwrap_or_else(|| "<h1>404 Not Found</h1>".to_owned());
    super::no_store_status_html(StatusCode::NOT_FOUND, body)
}
