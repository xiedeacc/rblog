//! Public, anonymous-facing SSR routes.
//!
//! Templates live in the active theme. The HTTP layer builds a `ctx` JSON
//! value with three top-level groups:
//!
//! - `site`: blog-wide info from the system ConfigMap (title, subtitle,
//!   base URL, locale).
//! - `menu`: the navigation menu items (empty if none configured).
//! - `active_theme`: the theme's short name, useful for `/themes/<name>/…`
//!   asset URLs.
//!
//! Routes are mounted under the root path. The home page is `/`, post
//! permalinks `/archives/<slug>`, taxonomy `/tags/<slug>` and
//! `/categories/<slug>`, archives `/archives`. Feeds at `/feed.xml` and
//! `/sitemap.xml`. The bundled `404.html` is rendered for any unmatched
//! path.

pub mod assets;
pub mod comments;
pub mod context;
pub mod feed;
pub mod home;
pub mod plugins;
pub mod post;
pub mod search;
pub mod taxonomy;

use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use tower_http::services::ServeDir;

use crate::AppState;

/// Build the public-facing router. `index.html` + `post.html` + taxonomy
/// + feeds + robots.txt + theme asset serving.
pub fn router(state: &AppState) -> Router<AppState> {
    let mut r = Router::new()
        .route("/", get(home::index))
        .route("/page/:page", get(home::index_paged))
        .route("/archives", get(home::archive))
        .route("/archives/:slug", get(post::detail))
        .route("/tags", get(taxonomy::tags))
        .route("/tags/:slug", get(taxonomy::tag_posts))
        .route("/categories", get(taxonomy::categories))
        .route("/categories/:slug", get(taxonomy::category_posts))
        .route("/feed.xml", get(feed::rss))
        .route("/sitemap.xml", get(feed::sitemap))
        .route("/robots.txt", get(feed::robots))
        .merge(comments::router())
        .merge(search::router())
        .merge(plugins::router())
        .fallback(public_not_found);

    // Serve `/themes/<name>/assets/...` directly from disk so themes can ship
    // their own CSS/JS without the binary having to know about each file.
    let themes_root = state.config.paths.themes_root.clone();
    if themes_root.exists() {
        r = r.nest_service("/themes", ServeDir::new(themes_root));
    }

    // For the local storage backend, hand `/uploads/*` to ServeDir directly.
    // S3-backed deployments don't need this — the URLs point at the bucket.
    if let crate::config::StorageConfig::Local { public_prefix } = &state.config.storage {
        let uploads_root = state.config.paths.uploads_root.clone();
        if uploads_root.exists() {
            let trimmed = public_prefix.trim_end_matches('/');
            if !trimmed.is_empty() {
                r = r.nest_service(trimmed, ServeDir::new(uploads_root));
            }
        }
    }
    r
}

async fn public_not_found(state: axum::extract::State<AppState>) -> (StatusCode, Html<String>) {
    let ctx = context::base_context(&state);
    let body = state
        .themes
        .active()
        .ok()
        .and_then(|t| t.renderer.render("404.html", &ctx).ok())
        .unwrap_or_else(|| "<h1>404 Not Found</h1>".to_owned());
    (StatusCode::NOT_FOUND, Html(body))
}
