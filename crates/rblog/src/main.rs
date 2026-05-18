//! rblog — Rust port of Halo CMS.
//!
//! Boot order:
//!
//! 1. Initialize tracing.
//! 2. Load [`AppConfig`] (env + `rblog.toml` + defaults).
//! 3. Open the SQLx pool, run pending migrations.
//! 4. Build the [`MarkdownPipeline`] and [`PasswordHasher`] (cheap, but
//!    we share them via `Arc` so all services see the same instance).
//! 5. Build the [`Services`] bundle (also seeds the in-memory index from
//!    storage).
//! 6. Install the default theme into `paths.themes_root`, then load the
//!    [`ThemeRegistry`] from disk.
//! 7. Build the Axum router and serve until shutdown.

use std::sync::Arc;

use rblog_attachments::{AttachmentService, Storage, ThumbnailEngine};
use rblog_auth::PasswordHasher;
use rblog_content::render::MarkdownPipeline;
use rblog_core::build_services;
use rblog_http::config::StorageConfig;
use rblog_http::plugin_host::RblogHost;
use rblog_http::search_sync;
use rblog_http::{routes::build_router, server::serve, AppConfig, AppState};
use rblog_plugins::PluginRuntime;
use rblog_search::SearchIndex;
use rblog_store::{run_migrations, AnyPool};
use rblog_theme::default_theme::install_default_theme;
use rblog_theme::ThemeRegistry;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn")),
        )
        .with_target(false)
        .compact()
        .init();

    let config = AppConfig::load().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "falling back to default configuration");
        AppConfig::default()
    });

    let pool = AnyPool::connect(&config.database.url).await?;
    run_migrations(&pool).await?;
    tracing::info!(
        backend = pool.backend(),
        url = %config.database.url,
        "extension store ready"
    );

    let pipeline = Arc::new(MarkdownPipeline::new());
    let hasher = Arc::new(PasswordHasher::new());

    let services = build_services(pool.clone(), pipeline.clone(), hasher.clone()).await?;
    tracing::info!(kinds = services.index.kind_count(), "index synced");

    // Bundled default theme: installed on first boot, ignored otherwise.
    install_default_theme(&config.paths.themes_root, false)?;
    let themes = ThemeRegistry::new(config.paths.themes_root.clone(), pipeline.clone());
    themes.reload()?;
    tracing::info!(count = themes.len(), "themes loaded");

    let storage = match &config.storage {
        StorageConfig::Local { public_prefix } => Storage::Local {
            root: config.paths.uploads_root.clone(),
            public_prefix: public_prefix.clone(),
        },
        StorageConfig::S3 {
            bucket,
            endpoint,
            region,
            access_key,
            secret_key,
            public_base_url,
        } => Storage::S3 {
            bucket: bucket.clone(),
            endpoint: endpoint.clone(),
            region: region.clone(),
            access_key: access_key.clone(),
            secret_key: secret_key.clone(),
            public_base_url: public_base_url.clone(),
        },
    };
    let backend = storage.build()?;
    let attachments = AttachmentService::new(backend, ThumbnailEngine::default());
    tracing::info!(backend = attachments.backend_label(), "attachments ready");

    let search = SearchIndex::open(&config.paths.search_root)?;
    // Empty index after first install → seed it from the live store. After
    // that the per-mutation hooks keep it in sync without a rebuild.
    if search.count() == 0 {
        let indexed = search_sync::rebuild_from_store(&search, &services, &pool).await?;
        tracing::info!(indexed, "search index seeded from store");
    } else {
        tracing::info!(documents = search.count(), "search index loaded from disk");
    }

    let host = RblogHost::new(services.clone());
    if let Ok(cm) = services.configmaps.system().await {
        host.refresh_settings(cm.data.unwrap_or_default());
    }
    let plugins = PluginRuntime::with_host(host.into_arc())?;
    let loaded = plugins
        .reload(&config.paths.plugins_root)
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "plugin reload failed");
            0
        });
    tracing::info!(loaded, "plugin runtime ready");

    let state = AppState::new(
        config.clone(),
        pool,
        services,
        themes,
        pipeline,
        hasher,
        attachments,
        search,
        plugins,
    );
    serve(build_router(state), config.server.bind).await?;
    Ok(())
}
