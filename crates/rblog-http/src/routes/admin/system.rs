//! System endpoints.
//!
//! Bootstrap is intentionally unauthenticated: a fresh install has no users
//! yet. To keep that safe, the handler refuses to run if an admin user
//! already exists. Once bootstrapped, the SPA hits `whoami` to identify the
//! logged-in user and check role membership.

use axum::extract::{Extension, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use rblog_core::{bootstrap_system, resync_all, BootstrapOptions};
use rblog_index::ListOptions;
use rblog_scheme::Extension as _;
use rblog_store::ExtensionRow;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::routes::admin::AuthedUser;
use crate::search_sync;
use crate::{AppState, HttpError};

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct BootstrapRequest {
    pub admin_username: String,
    #[serde(default)]
    pub admin_email: Option<String>,
    pub admin_password: String,
    #[serde(default)]
    pub site_title: Option<String>,
    #[serde(default)]
    pub site_subtitle: Option<String>,
    #[serde(default)]
    pub site_base_url: Option<String>,
}

pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/bootstrap", post(bootstrap))
        .route("/api/admin/bootstrap/status", get(bootstrap_status))
}

pub fn private_router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/whoami", get(whoami))
        .route("/api/admin/system/info", get(system_info))
        .route(
            "/api/admin/system/search/rebuild",
            post(rebuild_search_index),
        )
        .route("/api/admin/system/restore/halo", post(restore_halo_dump))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BootstrapResponse {
    pub bootstrapped: bool,
    pub created_role: bool,
    pub created_role_binding: bool,
    pub created_admin: bool,
    pub created_system_configmap: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BootstrapStatusResponse {
    pub bootstrapped: bool,
}

pub async fn is_bootstrapped(state: &AppState) -> Result<bool, HttpError> {
    let users = state
        .services
        .index
        .list(
            &rblog_content::core::User::gvk(),
            &ListOptions::default().paged(0, 1),
        )
        .map_err(HttpError::from)?;
    Ok(users.total > 0)
}

/// Lightweight public status check for the admin SPA.
#[utoipa::path(
    get,
    path = "/api/admin/bootstrap/status",
    tag = "system",
    responses((status = 200, description = "Bootstrap status", body = BootstrapStatusResponse)),
)]
pub async fn bootstrap_status(
    State(state): State<AppState>,
) -> Result<Json<BootstrapStatusResponse>, HttpError> {
    Ok(Json(BootstrapStatusResponse {
        bootstrapped: is_bootstrapped(&state).await?,
    }))
}

/// Initial-install endpoint. Idempotent.
#[utoipa::path(
    post,
    path = "/api/admin/bootstrap",
    tag = "system",
    request_body = BootstrapRequest,
    responses(
        (status = 200, description = "Bootstrap completed", body = BootstrapResponse),
        (status = 409, description = "Already bootstrapped"),
    ),
)]
pub async fn bootstrap(
    State(state): State<AppState>,
    Json(req): Json<BootstrapRequest>,
) -> Result<Json<BootstrapResponse>, HttpError> {
    if is_bootstrapped(&state).await? {
        return Err(HttpError::conflict(
            "system is already bootstrapped — sign in instead",
        ));
    }
    let admin_email = req
        .admin_email
        .filter(|email| !email.trim().is_empty())
        .unwrap_or_else(|| "admin@localhost.invalid".to_owned());
    let opts = BootstrapOptions {
        admin_username: req.admin_username,
        admin_email,
        admin_password: req.admin_password,
        site_title: req.site_title.unwrap_or_else(|| "rblog".to_owned()),
        site_subtitle: req.site_subtitle,
        site_base_url: req.site_base_url,
    };
    let report = bootstrap_system(&state.pool, &state.services.index, &state.hasher, &opts).await?;
    Ok(Json(BootstrapResponse {
        bootstrapped: true,
        created_role: report.super_admin_role_created,
        created_role_binding: report.super_admin_binding_created,
        created_admin: report.admin_user_created,
        created_system_configmap: report.system_configmap_created,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WhoAmI {
    pub name: String,
    pub display_name: String,
    pub email: String,
}

/// Identify the user backing the session cookie.
#[utoipa::path(
    get,
    path = "/api/admin/whoami",
    tag = "system",
    responses((status = 200, description = "Authenticated user", body = WhoAmI)),
)]
pub async fn whoami(Extension(user): Extension<AuthedUser>) -> Json<WhoAmI> {
    Json(WhoAmI {
        name: user.name,
        display_name: user.display_name,
        email: user.email,
    })
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SystemInfo {
    pub version: &'static str,
    pub active_theme: String,
    pub active_theme_directory: String,
    pub themes: Vec<String>,
}

/// Lightweight runtime summary, used by the admin sidebar.
#[utoipa::path(
    get,
    path = "/api/admin/system/info",
    tag = "system",
    responses((status = 200, description = "Runtime summary", body = SystemInfo)),
)]
pub async fn system_info(State(state): State<AppState>) -> Result<Json<SystemInfo>, HttpError> {
    let active = state.themes.active()?;
    Ok(Json(SystemInfo {
        version: env!("CARGO_PKG_VERSION"),
        active_theme: active.manifest.name.clone(),
        active_theme_directory: active.dir.display().to_string(),
        themes: state.themes.names(),
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SearchRebuildResponse {
    pub indexed: usize,
}

/// Wipe and re-build the search index from current posts.
///
/// Idempotent — safe to invoke at any time. Returns the number of documents
/// committed to tantivy.
#[utoipa::path(
    post,
    path = "/api/admin/system/search/rebuild",
    tag = "system",
    responses(
        (status = 200, description = "Index rebuilt", body = SearchRebuildResponse),
    ),
)]
pub async fn rebuild_search_index(
    State(state): State<AppState>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<SearchRebuildResponse>, HttpError> {
    let indexed =
        search_sync::rebuild_from_store(&state.search, &state.services, &state.pool).await?;
    Ok(Json(SearchRebuildResponse { indexed }))
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct RestoreHaloDumpRequest {
    /// Server-local path to a MySQL/MariaDB Halo dump.
    ///
    /// The local development default matches the repository backup path.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RestoreHaloDumpResponse {
    pub restored_rows: usize,
    pub posts: usize,
    pub snapshots: usize,
    pub tags: usize,
    pub categories: usize,
    pub comments: usize,
    pub users: usize,
    pub search_indexed: usize,
}

/// Restore a Halo MySQL dump into the current `extensions` table.
///
/// This is a full restore: current extension rows are replaced by the dump,
/// then the in-memory secondary index and the public search index are rebuilt.
#[utoipa::path(
    post,
    path = "/api/admin/system/restore/halo",
    tag = "system",
    request_body = RestoreHaloDumpRequest,
    responses(
        (status = 200, description = "Halo dump restored", body = RestoreHaloDumpResponse),
    ),
)]
pub async fn restore_halo_dump(
    State(state): State<AppState>,
    Extension(_user): Extension<AuthedUser>,
    Json(req): Json<RestoreHaloDumpRequest>,
) -> Result<Json<RestoreHaloDumpResponse>, HttpError> {
    let path = req
        .path
        .unwrap_or_else(|| "/root/src/blog/db/halodb_backup.sql".to_owned());
    let sql = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| HttpError::bad_request(format!("failed to read `{path}`: {e}")))?;
    let rows = parse_halo_extensions_dump(&sql).map_err(HttpError::bad_request)?;
    let counts = RestoreCounts::from_rows(&rows);

    state.pool.replace_all(&rows).await?;
    resync_all(&state.services.index, &state.pool).await?;
    let search_indexed =
        search_sync::rebuild_from_store(&state.search, &state.services, &state.pool).await?;

    Ok(Json(RestoreHaloDumpResponse {
        restored_rows: rows.len(),
        posts: counts.posts,
        snapshots: counts.snapshots,
        tags: counts.tags,
        categories: counts.categories,
        comments: counts.comments,
        users: counts.users,
        search_indexed,
    }))
}

#[derive(Default)]
struct RestoreCounts {
    posts: usize,
    snapshots: usize,
    tags: usize,
    categories: usize,
    comments: usize,
    users: usize,
}

impl RestoreCounts {
    fn from_rows(rows: &[ExtensionRow]) -> Self {
        let mut counts = Self::default();
        for row in rows {
            match row.name.as_str() {
                n if n.starts_with("/registry/content.halo.run/posts/") => counts.posts += 1,
                n if n.starts_with("/registry/content.halo.run/snapshots/") => {
                    counts.snapshots += 1;
                }
                n if n.starts_with("/registry/content.halo.run/tags/") => counts.tags += 1,
                n if n.starts_with("/registry/content.halo.run/categories/") => {
                    counts.categories += 1;
                }
                n if n.starts_with("/registry/content.halo.run/comments/") => counts.comments += 1,
                n if n.starts_with("/registry/users/") => counts.users += 1,
                _ => {}
            }
        }
        counts
    }
}

fn parse_halo_extensions_dump(sql: &str) -> Result<Vec<ExtensionRow>, String> {
    const INSERT: &str = "INSERT INTO `extensions` VALUES";
    let mut rows = Vec::new();
    let mut cursor = 0;

    while let Some(rel) = sql[cursor..].find(INSERT) {
        let values_start = cursor + rel + INSERT.len();
        let stmt_end = find_statement_end(sql, values_start)
            .ok_or_else(|| "unterminated INSERT INTO `extensions` statement".to_owned())?;
        parse_values(&sql[values_start..stmt_end], &mut rows)?;
        cursor = stmt_end + 1;
    }

    if rows.is_empty() {
        return Err("no INSERT rows for `extensions` found in dump".to_owned());
    }
    Ok(rows)
}

fn find_statement_end(sql: &str, from: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    let mut i = from;
    let mut in_quote = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_quote => {
                i += 2;
                continue;
            }
            b'\'' => in_quote = !in_quote,
            b';' if !in_quote => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_values(input: &str, out: &mut Vec<ExtensionRow>) -> Result<(), String> {
    let bytes = input.as_bytes();
    let mut pos = 0;
    loop {
        skip_ws_and_commas(bytes, &mut pos);
        if pos >= bytes.len() {
            return Ok(());
        }
        expect(bytes, &mut pos, b'(')?;
        skip_ws(bytes, &mut pos);

        let name = parse_quoted(bytes, &mut pos)?;
        let name = String::from_utf8(name).map_err(|e| format!("invalid row name utf8: {e}"))?;
        skip_ws(bytes, &mut pos);
        expect(bytes, &mut pos, b',')?;
        skip_ws(bytes, &mut pos);

        if bytes[pos..].starts_with(b"_binary") {
            pos += b"_binary".len();
            skip_ws(bytes, &mut pos);
        }
        let data = parse_quoted(bytes, &mut pos)?;
        skip_ws(bytes, &mut pos);
        expect(bytes, &mut pos, b',')?;
        skip_ws(bytes, &mut pos);

        let version = parse_version(bytes, &mut pos)?;
        skip_ws(bytes, &mut pos);
        expect(bytes, &mut pos, b')')?;

        out.push(ExtensionRow {
            name,
            data,
            version,
        });
    }
}

fn skip_ws_and_commas(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && (bytes[*pos].is_ascii_whitespace() || bytes[*pos] == b',') {
        *pos += 1;
    }
}

fn skip_ws(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
}

fn expect(bytes: &[u8], pos: &mut usize, expected: u8) -> Result<(), String> {
    if bytes.get(*pos) == Some(&expected) {
        *pos += 1;
        Ok(())
    } else {
        Err(format!(
            "expected `{}` near byte {}",
            expected as char, *pos
        ))
    }
}

fn parse_quoted(bytes: &[u8], pos: &mut usize) -> Result<Vec<u8>, String> {
    expect(bytes, pos, b'\'')?;
    let mut out = Vec::new();
    while *pos < bytes.len() {
        let b = bytes[*pos];
        *pos += 1;
        match b {
            b'\'' => return Ok(out),
            b'\\' => {
                let escaped = *bytes
                    .get(*pos)
                    .ok_or_else(|| "unterminated escape in quoted value".to_owned())?;
                *pos += 1;
                out.push(match escaped {
                    b'0' => 0,
                    b'b' => 8,
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'Z' => 26,
                    other => other,
                });
            }
            other => out.push(other),
        }
    }
    Err("unterminated quoted value".to_owned())
}

fn parse_version(bytes: &[u8], pos: &mut usize) -> Result<i64, String> {
    if bytes[*pos..].starts_with(b"NULL") {
        *pos += b"NULL".len();
        return Ok(0);
    }
    let start = *pos;
    while *pos < bytes.len() && (bytes[*pos].is_ascii_digit() || bytes[*pos] == b'-') {
        *pos += 1;
    }
    std::str::from_utf8(&bytes[start..*pos])
        .map_err(|e| format!("invalid version utf8: {e}"))?
        .parse()
        .map_err(|e| format!("invalid version near byte {start}: {e}"))
}

#[cfg(test)]
mod restore_tests {
    use super::parse_halo_extensions_dump;

    #[test]
    fn parses_mysql_extensions_insert() {
        let sql = r#"
CREATE TABLE `extensions` (`name` varchar(255), `data` longblob, `version` bigint);
INSERT INTO `extensions` VALUES ('/registry/content.halo.run/posts/p1',_binary '{\"kind\":\"Post\",\"spec\":{\"title\":\"can\'t\"}}',2),('/registry/users/admin',_binary '{\"kind\":\"User\"}',NULL);
"#;
        let rows = parse_halo_extensions_dump(sql).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "/registry/content.halo.run/posts/p1");
        assert_eq!(rows[0].version, 2);
        assert_eq!(rows[1].version, 0);
        assert!(String::from_utf8(rows[0].data.clone())
            .unwrap()
            .contains("can't"));
    }
}
