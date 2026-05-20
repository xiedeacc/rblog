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

fn site_title(state: &AppState) -> String {
    crate::routes::public::context::site_context(state)
        .get("title")
        .and_then(serde_json::Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("rblog")
        .to_owned()
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn with_site_title(state: &AppState, html: String) -> String {
    let title = format!("<title>{}</title>", escape_html(&site_title(state)));
    let Some(start) = html.find("<title>") else {
        return html;
    };
    let Some(relative_end) = html[start..].find("</title>") else {
        return html;
    };
    let end = start + relative_end + "</title>".len();
    format!("{}{}{}", &html[..start], title, &html[end..])
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
            return serve_file(&dir.join(path), true, None);
        }

        serve_file(&dir.join("index.html"), false, Some(&state))
    }

    fn serve_file(
        path: &std::path::Path,
        immutable: bool,
        state: Option<&AppState>,
    ) -> Response<Body> {
        let Ok(bytes) = fs::read(path) else {
            return not_found();
        };
        let body = if path.file_name().is_some_and(|name| name == "index.html") {
            let html = String::from_utf8_lossy(&bytes).into_owned();
            Body::from(
                state
                    .map(|state| super::with_site_title(state, html.clone()))
                    .unwrap_or(html),
            )
        } else {
            Body::from(bytes)
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
            .body(body)
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
    use axum::extract::State;
    use axum::http::{header, HeaderValue, Response, StatusCode, Uri};
    use rust_embed::Embed;

    use crate::AppState;

    #[derive(Embed)]
    #[folder = "../../admin/dist/"]
    struct Assets;

    pub(super) async fn index(State(state): State<AppState>) -> Response<Body> {
        serve_path("index.html", Some(&state))
    }

    pub(super) async fn serve(State(state): State<AppState>, uri: Uri) -> Response<Body> {
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
            serve_path(&path, Some(&state))
        } else {
            serve_path("index.html", Some(&state))
        }
    }

    fn serve_path(path: &str, state: Option<&AppState>) -> Response<Body> {
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
        let body = if path == "index.html" {
            let html = String::from_utf8_lossy(&file.data).into_owned();
            Body::from(
                state
                    .map(|state| super::with_site_title(state, html.clone()))
                    .unwrap_or(html),
            )
        } else {
            Body::from(file.data.into_owned())
        };
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, cache)
            .body(body)
            .unwrap()
    }
}
