//! Standalone public page rendering.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum_extra::extract::cookie::CookieJar;
use rblog_core::ServiceError;
use serde_json::json;

use crate::routes::public::context::{base_context, current_user};
use crate::{AppState, HttpError};

pub async fn detail(
    state: State<AppState>,
    jar: CookieJar,
    Path(slug): Path<String>,
) -> Result<Response, HttpError> {
    let user = current_user(&state, &jar).await;
    let mut detail = match state.services.pages.by_slug(&slug, user.is_some()).await {
        Ok(detail) => detail,
        Err(ServiceError::NotFound { .. }) => return Ok(render_404(&state)),
        Err(e) => return Err(e.into()),
    };
    detail.visits = state.services.pages.increment_visit(&detail.name).await?;
    let mut ctx = base_context(&state);
    ctx["current_user"] = user.unwrap_or(serde_json::Value::Null);
    ctx["page"] = json!({
        "name": detail.name,
        "title": detail.title,
        "slug": detail.slug,
        "permalink": detail.permalink,
        "publish_time": detail.publish_time,
        "excerpt": detail.excerpt,
        "content": detail.content_html,
        "visits": detail.visits,
        "cover": detail.cover,
    });
    let theme = state
        .themes
        .active()
        .map_err(|e| HttpError::Internal(anyhow::Error::new(e)))?;
    Ok(super::no_store_html(
        theme.renderer.render("page.html", &ctx)?,
    ))
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
