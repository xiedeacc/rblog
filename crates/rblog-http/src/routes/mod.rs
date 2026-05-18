//! Top-level router composition.
//!
//! The router is split into three sub-routers:
//!
//! - [`health`]: liveness/readiness endpoints, always public.
//! - The public SSR router (added in step 9, lands as `crate::routes::public`).
//! - The admin REST router (added in step 10, lands as `crate::routes::admin`).
//!
//! For now `build_router` only mounts `health` so step 8 covers a real bind
//!  serve loop end-to-end. The public + admin routes plug in via further
//!  `.merge` / `.nest` calls in subsequent commits.

pub mod admin;
pub mod admin_spa;
pub mod health;
pub mod public;

use axum::Router;

use crate::middleware::with_common_layers;
use crate::AppState;

/// Build the top-level router and wrap it with the shared middleware stack.
pub fn build_router(state: AppState) -> Router {
    let cfg = state.config.clone();
    let public_router = public::router(&state);
    let admin_router = admin::router(state.clone());
    let admin_spa_router = admin_spa::router(&state);
    let app = Router::new()
        .nest("/api/health", health::router())
        .merge(admin_router)
        .merge(admin_spa_router)
        .merge(public_router)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::redirect_to_setup_until_bootstrapped,
        ))
        .with_state(state);
    with_common_layers(app, &cfg)
}
