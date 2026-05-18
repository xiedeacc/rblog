//! Public-facing dispatch into WASM plugins.
//!
//! Mounts `/api/plugins/:name/*rest` and forwards each request into
//! [`rblog_plugins::PluginRuntime::invoke`]. The body is read up to a
//! 1 MiB cap so plugins are protected from runaway clients; the plugin
//! is then handed `(method, path, body)` and returns a JSON response the
//! HTTP layer translates into a real Axum [`Response`].
//!
//! Mismatched HTTP methods (e.g. POST to a route the manifest only
//! declares as `GET`) return `405 Method Not Allowed` *without* booting
//! the WASM instance.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header::CONTENT_TYPE, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use rblog_plugins::PluginRequest;

use crate::{AppState, HttpError};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/plugins/:name/*rest", any(dispatch))
}

async fn dispatch(
    State(state): State<AppState>,
    Path((name, rest)): Path<(String, String)>,
    method: Method,
    body: Bytes,
) -> Result<Response, HttpError> {
    let route = format!("/{}", rest.trim_start_matches('/'));

    // Method allow-list from the manifest. An empty list (or none
    // declared) means "any verb is fine" for forward compatibility.
    let allowed_methods = state.plugins.routes(&name);
    let route_match = allowed_methods
        .iter()
        .find(|(p, _)| route == *p || route.starts_with(&format!("{p}/")));
    if let Some((_, methods)) = route_match {
        if !methods.is_empty() && !methods.iter().any(|m| m == method.as_str()) {
            return Ok((StatusCode::METHOD_NOT_ALLOWED, "method not allowed").into_response());
        }
    }

    let req = PluginRequest::new(method.as_str(), route, body.to_vec());
    let plugin_resp = state.plugins.invoke(name, req).await?;

    let status = StatusCode::from_u16(plugin_resp.status).unwrap_or(StatusCode::OK);
    let mut builder = Response::builder().status(status);
    let ct = HeaderValue::from_str(&plugin_resp.content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("text/plain; charset=utf-8"));
    builder = builder.header(CONTENT_TYPE, ct);
    for (k, v) in plugin_resp.headers {
        if let (Ok(header_name), Ok(value)) = (HeaderName::try_from(k), HeaderValue::from_str(&v)) {
            builder = builder.header(header_name, value);
        }
    }
    Ok(builder
        .body(plugin_resp.body.into())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}
