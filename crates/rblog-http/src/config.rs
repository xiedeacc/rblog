//! Runtime configuration.
//!
//! Configuration sources, in increasing precedence:
//!
//! 1. Built-in defaults (see [`AppConfig::default`]).
//! 2. `rblog.toml` in the current working directory.
//! 3. The file pointed to by `RBLOG_CONFIG`.
//! 4. Environment variables prefixed `RBLOG__`, double underscore = nesting
//!    (e.g. `RBLOG__SERVER__BIND=0.0.0.0:8080`).
//!
//! Layered loading is implemented by the `config` crate. We deliberately
//! avoid an `Option<T>` everywhere style: settings have sensible defaults so
//! `rblog --no-config` still boots a usable server.

use std::env;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Top-level runtime config. Cloneable so it can live inside `AppState`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub paths: PathConfig,
    #[serde(default)]
    pub session: SessionConfig,
    #[serde(default)]
    pub site: SiteConfig,
    #[serde(default)]
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Bind address. Accepts both `host:port` and IPv6 forms.
    #[serde(default = "default_bind")]
    pub bind: SocketAddr,
    /// Request body limit (MB).
    #[serde(default = "default_body_mb")]
    pub max_body_mb: usize,
    /// Per-request timeout (seconds). The server applies it as a tower layer.
    #[serde(default = "default_timeout")]
    pub request_timeout_seconds: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            max_body_mb: default_body_mb(),
            request_timeout_seconds: default_timeout(),
        }
    }
}

fn default_bind() -> SocketAddr {
    "127.0.0.1:8080"
        .parse()
        .expect("static bind address must parse")
}

fn default_body_mb() -> usize {
    16
}
fn default_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// SQLx-compatible URL. Empty → in-memory SQLite for ephemeral / dev use.
    #[serde(default = "default_database_url")]
    pub url: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: default_database_url(),
        }
    }
}

fn default_database_url() -> String {
    "sqlite::memory:".to_owned()
}

#[derive(Debug, Clone, Deserialize)]
pub struct PathConfig {
    /// Root for themes; defaults to `themes/` next to the binary.
    #[serde(default = "default_themes_root")]
    pub themes_root: PathBuf,
    /// Root for uploaded attachments.
    #[serde(default = "default_uploads_root")]
    pub uploads_root: PathBuf,
    /// Root for the tantivy search index.
    #[serde(default = "default_search_root")]
    pub search_root: PathBuf,
    /// Optional override for the admin SPA dist directory.
    ///
    /// When `embed-admin` is enabled, this is ignored (the SPA is baked
    /// into the binary). Otherwise this path is served at `/admin/*` if
    /// it exists, which is handy for `pnpm dev`-style local iteration.
    #[serde(default)]
    pub admin_dist: Option<PathBuf>,
    /// Root directory for WASM plugins. Each plugin lives in a subfolder
    /// with a `plugin.toml` manifest + `plugin.wasm` binary.
    #[serde(default = "default_plugins_root")]
    pub plugins_root: PathBuf,
}

impl Default for PathConfig {
    fn default() -> Self {
        Self {
            themes_root: default_themes_root(),
            uploads_root: default_uploads_root(),
            search_root: default_search_root(),
            admin_dist: None,
            plugins_root: default_plugins_root(),
        }
    }
}

fn default_plugins_root() -> PathBuf {
    PathBuf::from("./plugins")
}

fn default_themes_root() -> PathBuf {
    PathBuf::from("./themes")
}

fn default_uploads_root() -> PathBuf {
    PathBuf::from("./uploads")
}

fn default_search_root() -> PathBuf {
    PathBuf::from("./search-index")
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionConfig {
    /// Cookie name for the admin session.
    #[serde(default = "default_cookie")]
    pub cookie_name: String,
    /// `Secure` flag — turn on in production.
    #[serde(default)]
    pub secure: bool,
    /// Max age in days.
    #[serde(default = "default_session_days")]
    pub max_age_days: i64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            cookie_name: default_cookie(),
            secure: false,
            max_age_days: default_session_days(),
        }
    }
}

fn default_cookie() -> String {
    "rblog_session".to_owned()
}

fn default_session_days() -> i64 {
    14
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SiteConfig {
    /// Canonical base URL (e.g. `https://blog.example.com`). Falls back to
    /// the value stored in the `system` ConfigMap.
    #[serde(default)]
    pub base_url: Option<String>,
}

/// Attachment storage configuration.
///
/// The `backend` discriminator selects which fields are honored. Anything
/// missing fall back to the local filesystem at `<paths.uploads_root>`,
/// served under `/uploads/`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase", tag = "backend")]
pub enum StorageConfig {
    Local {
        #[serde(default = "default_public_prefix")]
        public_prefix: String,
    },
    S3 {
        bucket: String,
        #[serde(default)]
        endpoint: Option<String>,
        #[serde(default)]
        region: Option<String>,
        #[serde(default)]
        access_key: Option<String>,
        #[serde(default)]
        secret_key: Option<String>,
        public_base_url: String,
    },
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self::Local {
            public_prefix: default_public_prefix(),
        }
    }
}

fn default_public_prefix() -> String {
    "/uploads".to_owned()
}

impl AppConfig {
    /// Load configuration from disk + environment using the precedence
    /// described in the crate-level docs.
    pub fn load() -> Result<Self, ConfigError> {
        let mut builder = config::Config::builder();

        // Default sources: rblog.toml in cwd if present.
        let cwd_config = Path::new("rblog.toml");
        if cwd_config.exists() {
            builder = builder.add_source(config::File::from(cwd_config));
        }
        if let Ok(path) = env::var("RBLOG_CONFIG") {
            builder = builder.add_source(config::File::with_name(&path));
        }
        builder = builder.add_source(
            config::Environment::with_prefix("RBLOG")
                .separator("__")
                .try_parsing(true)
                .list_separator(","),
        );

        let raw = builder
            .build()
            .map_err(|e| ConfigError::Load(e.to_string()))?;
        let parsed: Self = raw
            .try_deserialize()
            .map_err(|e| ConfigError::Parse(e.to_string()))?;
        Ok(parsed)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to load configuration: {0}")]
    Load(String),
    #[error("failed to parse configuration: {0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.server.bind.to_string(), "127.0.0.1:8080");
        assert_eq!(cfg.database.url, "sqlite::memory:");
        assert_eq!(cfg.session.cookie_name, "rblog_session");
        assert_eq!(cfg.session.max_age_days, 14);
    }
}
