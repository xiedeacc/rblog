//! Admin endpoints for user management.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use rblog_content::core::User;
use rblog_core::CreateUser;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{AppState, HttpError};

const USER_SETTINGS_KEY: &str = "user";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/users", get(list).post(create))
        .route("/api/admin/users/:name", get(detail).delete(remove))
        .route("/api/admin/users/:name/password", put(set_password))
        .route("/api/admin/users/:name/disable", post(disable))
        .route("/api/admin/users/:name/enable", post(enable))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserItem {
    pub name: String,
    pub display_name: String,
    pub email: String,
    pub disabled: bool,
    pub registered_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn item_from(user: &User) -> UserItem {
    let spec = user.spec.clone().unwrap_or_default();
    UserItem {
        name: user.metadata.name.clone(),
        display_name: spec.display_name,
        email: spec.email,
        disabled: spec.disabled.unwrap_or(false),
        registered_at: spec.registered_at,
    }
}

/// List every user account.
#[utoipa::path(
    get,
    path = "/api/admin/users",
    tag = "users",
    responses((status = 200, body = Vec<UserItem>)),
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<UserItem>>, HttpError> {
    let users = state.services.users.list().await?;
    Ok(Json(users.iter().map(item_from).collect()))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateUserRequest {
    pub name: String,
    pub display_name: String,
    pub email: String,
    pub password: String,
}

/// Create a new user.
#[utoipa::path(
    post,
    path = "/api/admin/users",
    tag = "users",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "Created", body = UserItem),
        (status = 409, description = "Name already taken"),
    ),
)]
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserItem>), HttpError> {
    if !registration_enabled(&state).await? {
        return Err(HttpError::forbidden("new user registration is disabled"));
    }

    let saved = state
        .services
        .users
        .create(CreateUser {
            name: body.name,
            display_name: body.display_name,
            email: body.email,
            password: body.password,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(item_from(&saved))))
}

async fn registration_enabled(state: &AppState) -> Result<bool, HttpError> {
    let enabled = state
        .services
        .configmaps
        .system_value(USER_SETTINGS_KEY)
        .await?
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|value| value.get("allowRegistration").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    Ok(enabled)
}

/// Get a single user by name.
#[utoipa::path(
    get,
    path = "/api/admin/users/{name}",
    tag = "users",
    params(("name" = String, Path, description = "User name")),
    responses(
        (status = 200, body = UserItem),
        (status = 404, description = "Unknown user"),
    ),
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<UserItem>, HttpError> {
    let user = state.services.users.get(&name).await?;
    Ok(Json(item_from(&user)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangePassword {
    pub password: String,
}

/// Change a user's password.
#[utoipa::path(
    put,
    path = "/api/admin/users/{name}/password",
    tag = "users",
    params(("name" = String, Path, description = "User name")),
    request_body = ChangePassword,
    responses((status = 204, description = "Password updated")),
)]
pub async fn set_password(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<ChangePassword>,
) -> Result<StatusCode, HttpError> {
    state
        .services
        .users
        .set_password(&name, &body.password)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Disable an account (`spec.disabled = true`). Disabled users cannot log in.
#[utoipa::path(
    post,
    path = "/api/admin/users/{name}/disable",
    tag = "users",
    params(("name" = String, Path, description = "User name")),
    responses((status = 200, body = UserItem)),
)]
pub async fn disable(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<UserItem>, HttpError> {
    set_disabled(&state, &name, true).await
}

/// Re-enable a previously disabled account.
#[utoipa::path(
    post,
    path = "/api/admin/users/{name}/enable",
    tag = "users",
    params(("name" = String, Path, description = "User name")),
    responses((status = 200, body = UserItem)),
)]
pub async fn enable(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<UserItem>, HttpError> {
    set_disabled(&state, &name, false).await
}

async fn set_disabled(
    state: &AppState,
    name: &str,
    disabled: bool,
) -> Result<Json<UserItem>, HttpError> {
    let saved = state.services.users.set_disabled(name, disabled).await?;
    Ok(Json(item_from(&saved)))
}

/// Permanently delete a user account.
#[utoipa::path(
    delete,
    path = "/api/admin/users/{name}",
    tag = "users",
    params(("name" = String, Path, description = "User name")),
    responses((status = 204, description = "Deleted")),
)]
pub async fn remove(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.services.users.delete(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}
