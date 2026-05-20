//! Single-post permalink rendering.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum_extra::extract::cookie::CookieJar;
use rblog_core::ServiceError;
use serde_json::{json, Value};

use crate::routes::public::context::{base_context, current_user};
use crate::{AppState, HttpError};

pub async fn detail(
    state: State<AppState>,
    jar: CookieJar,
    Path(slug): Path<String>,
) -> Result<Response, HttpError> {
    let user = current_user(&state, &jar).await;
    let mut detail = match state.services.posts.by_slug(&slug, user.is_some()).await {
        Ok(d) => d,
        Err(ServiceError::NotFound { .. }) => return Ok(render_404(&state)),
        Err(e) => return Err(e.into()),
    };
    detail.visits = state.services.posts.increment_visit(&detail.name).await?;
    let mut ctx = base_context(&state);
    let post = json!({
        "name": detail.name,
        "title": detail.title,
        "slug": detail.slug,
        "permalink": detail.permalink,
        "publish_time": detail.publish_time,
        "excerpt": detail.excerpt,
        "content": detail.content_html,
        "visits": detail.visits,
        "cover": detail.cover,
        "tags": tag_list(&state, &detail.tags),
        "categories": category_list(&state, &detail.categories),
    });
    let mut comments = Vec::new();
    for comment in state
        .services
        .comments
        .public_thread("Post", post["name"].as_str().unwrap_or_default())?
    {
        let replies = state
            .services
            .comments
            .replies(comment.metadata.name())?
            .into_iter()
            .map(|reply| {
                let spec = reply.spec.unwrap_or_default();
                json!({
                    "name": reply.metadata.name,
                    "content_html": spec.base.content,
                    "owner_display_name": spec
                        .base
                        .owner
                        .display_name
                        .unwrap_or(spec.base.owner.name),
                    "created_at": spec.base.creation_time,
                    "quote_reply": spec.quote_reply,
                })
            })
            .collect::<Vec<_>>();
        let spec = comment.spec.unwrap_or_default();
        comments.push(json!({
            "name": comment.metadata.name,
            "content_html": spec.base.content,
            "owner_display_name": spec
                .base
                .owner
                .display_name
                .unwrap_or(spec.base.owner.name),
            "created_at": spec.base.creation_time,
            "replies": replies,
        }));
    }
    ctx["post"] = post;
    ctx["current_user"] = user.unwrap_or(serde_json::Value::Null);
    ctx["comments"] = serde_json::to_value(comments).unwrap_or(json!([]));
    let theme = state
        .themes
        .active()
        .map_err(|e| HttpError::Internal(anyhow::Error::new(e)))?;
    Ok(super::no_store_html(
        theme.renderer.render("post.html", &ctx)?,
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

/// Resolve each `tag` name to a `{display_name, slug, permalink}` triple so
/// templates can render real links instead of bare metadata.name values.
fn tag_list(state: &AppState, names: &[String]) -> Vec<Value> {
    use rblog_content::content::Tag;
    use rblog_scheme::Extension;
    names
        .iter()
        .filter_map(|n| {
            state
                .services
                .index
                .get(&Tag::gvk(), n)
                .and_then(|entry| serde_json::from_value::<Tag>(entry.raw).ok())
                .map(|t| {
                    let spec = t.spec.unwrap_or_default();
                    json!({
                        "name": n,
                        "display_name": spec.display_name,
                        "slug": spec.slug.clone(),
                        "permalink": format!("/tags/{}", spec.slug),
                    })
                })
        })
        .collect()
}

fn category_list(state: &AppState, names: &[String]) -> Vec<Value> {
    use rblog_content::content::Category;
    use rblog_scheme::Extension;
    names
        .iter()
        .filter_map(|n| {
            state
                .services
                .index
                .get(&Category::gvk(), n)
                .and_then(|entry| serde_json::from_value::<Category>(entry.raw).ok())
                .map(|c| {
                    let spec = c.spec.unwrap_or_default();
                    json!({
                        "name": n,
                        "display_name": spec.display_name,
                        "slug": spec.slug.clone(),
                        "permalink": format!("/categories/{}", spec.slug),
                    })
                })
        })
        .collect()
}
