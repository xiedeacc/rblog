//! Liveness / readiness probes. Always-on; never authenticated.

use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::json;

use crate::AppState;

use rblog_scheme::Extension as _;

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
        .index
        .entry_count(&rblog_content::content::Post::gvk());
    let users = state
        .services
        .index
        .entry_count(&rblog_content::core::User::gvk());
    Json(ReadinessBody {
        state: "ready",
        backend: state.pool.backend(),
        posts,
        users,
    })
}
