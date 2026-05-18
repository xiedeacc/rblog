//! Cookie-based login / logout.
//!
//! On a successful POST `/api/admin/auth/login` the handler verifies the
//! password with argon2, mints an opaque session token, stashes it in the
//! in-memory [`SessionStore`], and sets a `HttpOnly` cookie via
//! [`CookieJar`]. Logout invalidates the cookie + session.

use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::routes::admin::AuthedUser;
use crate::{AppState, HttpError};

pub fn public_router() -> Router<AppState> {
    Router::new().route("/api/admin/auth/login", post(login))
}

pub fn private_router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/auth/logout", post(logout))
        .route("/api/admin/auth/session", get(current_session))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    pub name: String,
    pub display_name: String,
    pub email: String,
}

/// Verify credentials, set a session cookie, return the resolved user.
#[utoipa::path(
    post,
    path = "/api/admin/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Authenticated", body = LoginResponse),
        (status = 401, description = "Bad credentials"),
    ),
)]
pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<LoginRequest>,
) -> Result<(CookieJar, Json<LoginResponse>), HttpError> {
    let user = state
        .services
        .users
        .authenticate(&body.username, &body.password)
        .await?;
    let ttl = Duration::from_secs(
        u64::try_from(state.config.session.max_age_days.max(1)).unwrap_or(14) * 24 * 60 * 60,
    );
    let record = state.sessions.create(&user.name, ttl);
    let cookie = Cookie::build((
        state.config.session.cookie_name.clone(),
        record.token.as_str().to_owned(),
    ))
    .path("/")
    .http_only(true)
    .same_site(SameSite::Lax)
    .secure(state.config.session.secure)
    .max_age(time::Duration::seconds(
        i64::try_from(ttl.as_secs()).unwrap_or(60 * 60 * 24 * 14),
    ))
    .build();
    let jar = jar.add(cookie);
    Ok((
        jar,
        Json(LoginResponse {
            name: user.name,
            display_name: user.display_name,
            email: user.email,
        }),
    ))
}

/// Invalidate the current session and clear the cookie.
#[utoipa::path(
    post,
    path = "/api/admin/auth/logout",
    tag = "auth",
    responses((status = 204, description = "Signed out")),
)]
pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    let cookie_name = state.config.session.cookie_name.clone();
    if let Some(c) = jar.get(&cookie_name) {
        state.sessions.revoke(c.value());
    }
    let removal = Cookie::build((cookie_name, String::new()))
        .path("/")
        .http_only(true)
        .max_age(time::Duration::seconds(0))
        .build();
    (StatusCode::NO_CONTENT, jar.add(removal))
}

/// Return the AuthedUser for the current session, or 401 (handled by the
/// middleware) if missing.
#[utoipa::path(
    get,
    path = "/api/admin/auth/session",
    tag = "auth",
    responses((status = 200, description = "Active session", body = LoginResponse)),
)]
pub async fn current_session(
    axum::Extension(user): axum::Extension<AuthedUser>,
) -> Json<LoginResponse> {
    Json(LoginResponse {
        name: user.name,
        display_name: user.display_name,
        email: user.email,
    })
}
