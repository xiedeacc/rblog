//! Liveness / readiness probes. Always-on; never authenticated.

use axum::routing::get;
use axum::{Json, Router};
use rblog_core::{PostListQuery, PostStatusFilter};
use serde::Serialize;
use serde_json::json;

use crate::AppState;

#[derive(Serialize)]
struct ReadinessBody {
    state: &'static str,
    backend: &'static str,
    posts: usize,
    users: usize,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(liveness))
        .route("/ready", get(readiness))
}

async fn liveness() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

async fn readiness(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<ReadinessBody> {
    let posts = state
        .services
        .posts
        .list(PostListQuery {
            status: PostStatusFilter::Any,
            include_deleted: true,
            offset: 0,
            limit: 1,
            ..PostListQuery::default()
        })
        .await
        .map_or(0, |page| page.total);
    let users = state
        .services
        .users
        .list()
        .await
        .map_or(0, |items| items.len());
    Json(ReadinessBody {
        state: "ready",
        backend: state.pool.backend(),
        posts,
        users,
    })
}
