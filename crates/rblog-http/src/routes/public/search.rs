//! Public search endpoint.
//!
//! `GET /api/search?q=…&limit=…` returns a JSON array of [`SearchHit`]s.
//! The themed search page lives at `/search?q=…` and renders `search.html`.

use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rblog_search::SearchHit;
use serde::Deserialize;

use crate::routes::public::context::base_context;
use crate::{AppState, HttpError};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/search", get(json))
        .route("/search", get(themed))
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    20
}

/// JSON variant — used by the SPA and by AJAX-style theme widgets.
pub async fn json(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<SearchHit>>, HttpError> {
    let limit = q.limit.clamp(1, 100);
    Ok(Json(state.search.search(&q.q, limit)?))
}

/// Themed full-page variant — renders `search.html` if present, otherwise
/// falls back to a minimal inline HTML page so the route is always usable.
pub async fn themed(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Response, HttpError> {
    let hits = if q.q.trim().is_empty() {
        Vec::new()
    } else {
        state.search.search(&q.q, 50)?
    };
    let mut ctx = base_context(&state);
    if let Some(obj) = ctx.as_object_mut() {
        obj.insert("query".to_owned(), serde_json::Value::String(q.q.clone()));
        obj.insert(
            "hits".to_owned(),
            serde_json::to_value(&hits).map_err(|e| HttpError::Internal(e.into()))?,
        );
        obj.insert("total".to_owned(), serde_json::Value::from(hits.len()));
    }
    let body = state
        .themes
        .active()
        .ok()
        .and_then(|t| t.renderer.render("search.html", &ctx).ok())
        .unwrap_or_else(|| fallback_html(&q.q, &hits));
    Ok(Html(body).into_response())
}

fn fallback_html(query: &str, hits: &[SearchHit]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("<!doctype html><html><body>");
    let _ = write!(
        out,
        "<h1>Search results for \"{}\"</h1>",
        escape_html(query)
    );
    if hits.is_empty() {
        out.push_str("<p>No results.</p>");
    } else {
        out.push_str("<ul>");
        for hit in hits {
            let _ = write!(
                out,
                "<li><a href=\"{}\">{}</a></li>",
                escape_html(&hit.permalink),
                escape_html(&hit.title)
            );
        }
        out.push_str("</ul>");
    }
    out.push_str("</body></html>");
    out
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
