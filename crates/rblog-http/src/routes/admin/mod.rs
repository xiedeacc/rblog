//! Admin REST API.
//!
//! Mounted under `/api/admin`. Every endpoint except the bootstrap and the
//! login/logout pair is gated behind [`require_session`]: it looks up the
//! `rblog_session` cookie in the in-memory [`SessionStore`] and the resolved
//! user record, then attaches a [`AuthedUser`] extension before the handler
//! runs.
//!
//! ## OpenAPI
//!
//! Each handler is annotated with `#[utoipa::path]`. The full spec lives at
//! `/api/admin/openapi.json`, ready to drop into a generator (`openapi-ts`,
//! `oazapfts`, `rapidoc`, …). Schemas come from `serde::Serialize` /
//! `Deserialize` types so we don't ship a parallel description.

pub mod attachments;
pub mod auth;
pub mod categories;
pub mod comments;
pub mod openapi;
pub mod plugins;
pub mod posts;
pub mod settings;
pub mod system;
pub mod tags;
pub mod users;

use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::Router;
use axum_extra::extract::cookie::CookieJar;

use crate::{AppState, HttpError};

/// User identity attached to the request by [`require_session`]. Handlers
/// downstream extract it via `Extension<AuthedUser>`.
#[derive(Debug, Clone)]
pub struct AuthedUser {
    pub name: String,
    pub display_name: String,
    pub email: String,
}

pub fn router(state: AppState) -> Router<AppState> {
    let public = Router::new()
        .merge(system::public_router())
        .merge(auth::public_router());

    let private = Router::new()
        .merge(auth::private_router())
        .merge(posts::router())
        .merge(tags::router())
        .merge(categories::router())
        .merge(comments::router())
        .merge(users::router())
        .merge(settings::router())
        .merge(attachments::router())
        .merge(plugins::router())
        .merge(system::private_router())
        .route_layer(middleware::from_fn_with_state(state, require_session));

    Router::new()
        .merge(public)
        .merge(private)
        .merge(openapi::router())
}

/// Authentication middleware. Reads `rblog_session` from the cookie jar,
/// looks it up in the in-memory session store, fetches the user record,
/// and inserts an [`AuthedUser`] extension. Returns 401 on any failure.
pub async fn require_session(
    State(state): State<AppState>,
    jar: CookieJar,
    mut req: Request,
    next: Next,
) -> Result<Response, HttpError> {
    let cookie = jar
        .get(&state.config.session.cookie_name)
        .ok_or_else(|| HttpError::unauthorized("missing session cookie"))?;
    let token = cookie.value().to_owned();
    let record = state
        .sessions
        .lookup(&token)
        .ok_or_else(|| HttpError::unauthorized("invalid or expired session"))?;
    let user = state
        .services
        .users
        .get(&record.user)
        .await
        .map_err(|_| HttpError::unauthorized("session references an unknown user"))?;
    let spec = user.spec.clone().unwrap_or_default();
    req.extensions_mut().insert(AuthedUser {
        name: user.metadata.name.clone(),
        display_name: spec.display_name,
        email: spec.email,
    });
    Ok(next.run(req).await)
}
