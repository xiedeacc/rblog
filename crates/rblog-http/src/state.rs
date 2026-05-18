//! Shared application state for Axum handlers.

use std::sync::Arc;

use rblog_attachments::AttachmentService;
use rblog_auth::PasswordHasher;
use rblog_content::render::MarkdownPipeline;
use rblog_core::Services;
use rblog_plugins::PluginRuntime;
use rblog_search::SearchIndex;
use rblog_store::AnyPool;
use rblog_theme::ThemeRegistry;

use crate::config::AppConfig;
use crate::ratelimit::RateLimiter;
use crate::session::SessionStore;

/// All long-lived application context. `Clone` is cheap because every field is
/// `Arc` / `AnyPool` (which is itself just an Arc wrapper around the SQLx
/// pool).
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub pool: AnyPool,
    pub services: Services,
    pub themes: Arc<ThemeRegistry>,
    pub pipeline: Arc<MarkdownPipeline>,
    pub hasher: Arc<PasswordHasher>,
    pub sessions: Arc<SessionStore>,
    pub attachments: Arc<AttachmentService>,
    pub comment_rate_limit: RateLimiter,
    pub search: SearchIndex,
    pub plugins: PluginRuntime,
}

impl AppState {
    #[allow(clippy::too_many_arguments, clippy::similar_names)]
    pub fn new(
        config: AppConfig,
        pool: AnyPool,
        services: Services,
        themes: ThemeRegistry,
        pipeline: Arc<MarkdownPipeline>,
        hasher: Arc<PasswordHasher>,
        attachments: AttachmentService,
        search: SearchIndex,
        plugins: PluginRuntime,
    ) -> Self {
        let sessions = Arc::new(SessionStore::new());
        Self {
            config: Arc::new(config),
            pool,
            services,
            themes: Arc::new(themes),
            pipeline,
            hasher,
            sessions,
            attachments: Arc::new(attachments),
            comment_rate_limit: RateLimiter::comments_default(),
            search,
            plugins,
        }
    }
}
