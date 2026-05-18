//! Admin SPA static asset serving.
//!
//! Mounts the React admin under `/admin/*`. Three concrete strategies, in
//! decreasing order of preference:
//!
//! 1. **`embed-admin` Cargo feature**: bake `admin/dist/` into the binary at
//!    compile time via [`rust_embed::Embed`]. This is the production path:
//!    a single self-contained binary, no filesystem dependency.
//! 2. **`paths.admin_dist` configured**: serve directly from disk via
//!    [`tower_http::services::ServeDir`]. Handy for `pnpm dev`-style
//!    iteration without rebuilding rblog.
//! 3. **Neither**: return a small stub HTML page explaining how to enable
//!    one of the above. The REST API at `/api/admin/*` still works.
//!
//! Client-side routes (`/admin/posts/edit/<id>`, etc.) all fall back to
//! `index.html` so React Router can pick up.

use axum::Router;

use crate::AppState;

#[cfg(feature = "embed-admin")]
pub fn router(_state: &AppState) -> Router<AppState> {
    use axum::routing::get;
    Router::new()
        .route("/admin", get(embedded::index))
        .route("/admin/", get(embedded::index))
        .route("/admin/*path", get(embedded::serve))
}

#[cfg(not(feature = "embed-admin"))]
pub fn router(state: &AppState) -> Router<AppState> {
    use axum::routing::get;

    if state.config.paths.admin_dist.is_some() {
        return Router::new()
            .route("/admin", get(dist::serve))
            .route("/admin/", get(dist::serve))
            .route("/admin/*path", get(dist::serve));
    }
    Router::new()
        .route("/admin", get(stub::index))
        .route("/admin/", get(stub::index))
        .route("/admin/*path", get(stub::fallback))
}

#[cfg(not(feature = "embed-admin"))]
mod dist {
    use std::fs;

    use axum::body::Body;
    use axum::extract::State;
    use axum::http::{header, HeaderValue, Response, StatusCode, Uri};

    use crate::AppState;

    pub(super) async fn serve(State(state): State<AppState>, uri: Uri) -> Response<Body> {
        let Some(dir) = state.config.paths.admin_dist.clone() else {
            return not_found();
        };
        let path = uri
            .path()
            .trim_start_matches("/admin/")
            .trim_start_matches('/');

        if path.starts_with("assets/") && !path.split('/').any(|segment| segment == "..") {
            return serve_file(&dir.join(path), true);
        }

        serve_file(&dir.join("index.html"), false)
    }

    fn serve_file(path: &std::path::Path, immutable: bool) -> Response<Body> {
        let Ok(bytes) = fs::read(path) else {
            return not_found();
        };
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        let cache = if immutable {
            HeaderValue::from_static("public, max-age=31536000, immutable")
        } else {
            HeaderValue::from_static("no-cache")
        };
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, cache)
            .body(Body::from(bytes))
            .unwrap()
    }

    fn not_found() -> Response<Body> {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("not found"))
            .unwrap()
    }
}

#[cfg(not(feature = "embed-admin"))]
mod stub {
    use axum::body::Body;
    use axum::http::{header, HeaderValue, Response, StatusCode, Uri};

    const HTML: &str = include_str!("../admin_stub/index.html");
    const CSS: &str = include_str!("../admin_stub/placeholder.css");

    pub(super) async fn index() -> Response<Body> {
        html()
    }

    pub(super) async fn fallback(uri: Uri) -> Response<Body> {
        let path = uri.path().trim_start_matches("/admin/");
        if path == "assets/placeholder.css" {
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
                .body(Body::from(CSS))
                .unwrap();
        }
        html()
    }

    fn html() -> Response<Body> {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
            .body(Body::from(HTML))
            .unwrap()
    }
}

#[cfg(feature = "embed-admin")]
mod embedded {
    use axum::body::Body;
    use axum::http::{header, HeaderValue, Response, StatusCode, Uri};
    use rust_embed::Embed;

    #[derive(Embed)]
    #[folder = "../../admin/dist/"]
    struct Assets;

    pub(super) async fn index() -> Response<Body> {
        serve_path("index.html")
    }

    pub(super) async fn serve(uri: Uri) -> Response<Body> {
        let path = uri
            .path()
            .trim_start_matches("/admin/")
            .trim_start_matches('/');
        let path = if path.is_empty() {
            "index.html".to_owned()
        } else {
            path.to_owned()
        };
        if Assets::get(&path).is_some() {
            serve_path(&path)
        } else {
            serve_path("index.html")
        }
    }

    fn serve_path(path: &str) -> Response<Body> {
        let Some(file) = Assets::get(path) else {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("not found"))
                .unwrap();
        };
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        let cache = if path == "index.html" {
            HeaderValue::from_static("no-cache")
        } else {
            HeaderValue::from_static("public, max-age=31536000, immutable")
        };
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, cache)
            .body(Body::from(file.data.into_owned()))
            .unwrap()
    }
}
