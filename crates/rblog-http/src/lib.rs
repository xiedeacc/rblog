//! HTTP scaffold for rblog.
//!
//! This crate is intentionally route-light: the public SSR routes land in
//! step 9, the admin REST API in step 10. Step 8 ships the plumbing
//! everything else relies on:
//!
//! - [`AppConfig`]: layered configuration loader (`rblog.toml` + env).
//! - [`AppState`]: cheap-to-clone bag of services / pool / themes / sessions.
//! - [`HttpError`]: the unified error envelope every handler returns.
//! - [`SessionStore`]: in-memory cookie session store.
//! - [`middleware::with_common_layers`]: tracing, request ID, security
//!   headers, gzip, body limit, timeout.
//! - [`routes::build_router`]: top-level Axum router (currently health-only).
//! - [`server::serve`]: bind + serve + graceful shutdown.
//!
//! Wiring example (production-shaped):
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use rblog_http::{AppConfig, AppState, routes::build_router, server::serve};
//! # use rblog_store::{AnyPool, run_migrations};
//! # use rblog_core::build_services;
//! # use rblog_content::render::MarkdownPipeline;
//! # use rblog_auth::PasswordHasher;
//! # use rblog_theme::ThemeRegistry;
//! # use rblog_attachments::{AttachmentService, Storage, ThumbnailEngine};
//! # async fn run() -> anyhow::Result<()> {
//! let config = AppConfig::load().unwrap_or_default();
//! let pool = AnyPool::connect(&config.database.url).await?;
//! run_migrations(&pool).await?;
//! let pipeline = Arc::new(MarkdownPipeline::new());
//! let hasher = Arc::new(PasswordHasher::new());
//! let services = build_services(pool.clone(), pipeline.clone(), hasher.clone()).await?;
//! let themes = ThemeRegistry::new(config.paths.themes_root.clone(), pipeline.clone());
//! themes.reload()?;
//! let backend = Storage::Local {
//!     root: config.paths.uploads_root.clone(),
//!     public_prefix: "/uploads".into(),
//! }.build()?;
//! let attachments = AttachmentService::new(backend, ThumbnailEngine::default());
//! let search = rblog_search::SearchIndex::open(&config.paths.search_root)?;
//! let plugins = rblog_plugins::PluginRuntime::new()?;
//! plugins.reload(&config.paths.plugins_root)?;
//! let state = AppState::new(config.clone(), pool, services, themes, pipeline, hasher, attachments, search, plugins);
//! serve(build_router(state), config.server.bind).await
//! # }
//! ```

pub mod config;
pub mod error;
pub mod middleware;
pub mod plugin_host;
pub mod ratelimit;
pub mod routes;
pub mod search_sync;
pub mod server;
pub mod session;
pub mod state;

pub use config::{AppConfig, ConfigError, DatabaseConfig, PathConfig, ServerConfig, SiteConfig};
pub use error::HttpError;
pub use ratelimit::{RateLimiter, RateVerdict};
pub use session::{SessionRecord, SessionStore};
pub use state::AppState;
