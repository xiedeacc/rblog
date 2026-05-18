//! OpenAPI 3.1 spec for the admin REST surface.
//!
//! Mounted at `/api/admin/openapi.json` and rendered by [`utoipa`] from the
//! `#[utoipa::path]` annotations on every admin handler. The spec is generated
//! at process start-up and cached; serving it is a simple `Json` clone.
//!
//! We deliberately do not bundle Swagger UI; the SPA can point a third-party
//! viewer at this URL during development if needed.

use std::sync::OnceLock;

use axum::routing::get;
use axum::{Json, Router};
use serde_json::Value;
use utoipa::OpenApi;

use crate::AppState;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "rblog admin API",
        description = "REST API powering the rblog admin SPA. Sessions are issued via /api/admin/auth/login and carried in the `rblog_session` cookie.",
    ),
    paths(
        super::auth::login,
        super::auth::logout,
        super::auth::current_session,
        super::system::bootstrap,
        super::system::bootstrap_status,
        super::system::whoami,
        super::system::system_info,
        super::system::rebuild_search_index,
        super::system::restore_halo_dump,
        super::posts::list,
        super::posts::create,
        super::posts::detail,
        super::posts::update_content,
        super::posts::publish,
        super::posts::unpublish,
        super::posts::soft_delete,
        super::posts::restore,
        super::posts::purge,
        super::tags::list,
        super::tags::create,
        super::tags::update,
        super::tags::remove,
        super::categories::list,
        super::categories::create,
        super::categories::update,
        super::categories::remove,
        super::comments::list,
        super::comments::queue,
        super::comments::approve,
        super::comments::hide,
        super::comments::show,
        super::comments::delete,
        super::comments::approve_reply,
        super::comments::hide_reply,
        super::comments::show_reply,
        super::comments::delete_reply,
        super::users::list,
        super::users::create,
        super::users::detail,
        super::users::set_password,
        super::users::disable,
        super::users::enable,
        super::users::remove,
        super::settings::get_configmap,
        super::settings::upsert_configmap,
        super::settings::system,
        super::settings::upsert_system,
        super::settings::get_setting,
        super::settings::upsert_setting,
        super::attachments::list,
        super::attachments::upload,
        super::attachments::remove,
        super::plugins::list,
        super::plugins::detail,
        super::plugins::enable,
        super::plugins::disable,
        super::plugins::reload,
    ),
    components(schemas(
        super::auth::LoginRequest,
        super::auth::LoginResponse,
        super::system::BootstrapRequest,
        super::system::BootstrapResponse,
        super::system::BootstrapStatusResponse,
        super::system::WhoAmI,
        super::system::SystemInfo,
        super::system::SearchRebuildResponse,
        super::system::RestoreHaloDumpRequest,
        super::system::RestoreHaloDumpResponse,
        super::posts::CreateRequest,
        super::posts::UpdateContent,
        super::posts::PublishBody,
        super::posts::ListPage,
        super::posts::PostSummary,
        super::tags::TagItem,
        super::tags::CreateTag,
        super::tags::UpdateTag,
        super::categories::CategoryItem,
        super::categories::CreateCategory,
        super::categories::UpdateCategory,
        super::comments::CommentItem,
        super::users::UserItem,
        super::users::CreateUserRequest,
        super::users::ChangePassword,
        super::settings::ConfigMapView,
        super::settings::ConfigMapUpsert,
        super::settings::SettingView,
        super::settings::SettingUpsert,
        super::attachments::AttachmentListItem,
        super::attachments::UploadResponse,
        super::attachments::ThumbnailItem,
        super::plugins::PluginInfoView,
        super::plugins::PluginListResponse,
        super::plugins::PluginRoute,
        super::plugins::ReloadResponse,
    )),
    tags(
        (name = "auth", description = "Session lifecycle"),
        (name = "system", description = "Bootstrap and runtime info"),
        (name = "posts", description = "Posts + snapshots"),
        (name = "tags", description = "Tag management"),
        (name = "categories", description = "Category management"),
        (name = "comments", description = "Comment moderation"),
        (name = "users", description = "User accounts"),
        (name = "settings", description = "ConfigMaps + Settings"),
        (name = "attachments", description = "File uploads + thumbnails"),
        (name = "plugins", description = "WASM plugin lifecycle"),
    ),
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/admin/openapi.json", get(spec))
}

static CACHED: OnceLock<Value> = OnceLock::new();

/// Cache the generated spec on first request and serve a clone thereafter.
pub async fn spec() -> Json<Value> {
    let value = CACHED
        .get_or_init(|| serde_json::to_value(ApiDoc::openapi()).unwrap_or(Value::Null))
        .clone();
    Json(value)
}
