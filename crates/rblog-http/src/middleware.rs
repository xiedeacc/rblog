//! Tower middleware applied to the top-level router.
//!
//! We expose the layers as a free function that takes ownership of a
//! [`Router`] so the call site stays a one-liner. Returning a
//! `ServiceBuilder` directly would force every consumer to spell out the
//! deeply nested layer-stack type.

use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::header::{HeaderName, HeaderValue};
use axum::http::{header, Method};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Router;
use http::HeaderMap;
use tower_http::compression::CompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};

use crate::config::AppConfig;
use crate::routes::admin::system::is_bootstrapped;
use crate::AppState;

const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
const X_FRAME_OPTIONS: HeaderName = HeaderName::from_static("x-frame-options");
const X_CONTENT_TYPE_OPTIONS: HeaderName = HeaderName::from_static("x-content-type-options");
const REFERRER_POLICY: HeaderName = HeaderName::from_static("referrer-policy");
const X_XSS_PROTECTION: HeaderName = HeaderName::from_static("x-xss-protection");

/// Apply the shared middleware stack: tracing, request ID, security
/// headers, gzip, body limit, timeout. Order is outermost first.
pub fn with_common_layers<S>(router: Router<S>, cfg: &AppConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .layer(TimeoutLayer::new(Duration::from_secs(
            cfg.server.request_timeout_seconds,
        )))
        .layer(RequestBodyLimitLayer::new(
            cfg.server.max_body_mb * 1024 * 1024,
        ))
        .layer(CompressionLayer::new().gzip(true))
        .layer(SetResponseHeaderLayer::if_not_present(
            X_FRAME_OPTIONS,
            HeaderValue::from_static("SAMEORIGIN"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            REFERRER_POLICY,
            HeaderValue::from_static("no-referrer-when-downgrade"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            X_XSS_PROTECTION,
            HeaderValue::from_static("0"),
        ))
        // Order matters: Propagate must be _inside_ Set, so it sees the
        // header on the inbound request after Set assigned one. With Axum's
        // outer-first `.layer()` semantics that means Propagate comes first.
        .layer(PropagateRequestIdLayer::new(X_REQUEST_ID))
        .layer(SetRequestIdLayer::new(X_REQUEST_ID, MakeRequestUuid))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().include_headers(false))
                .on_response(DefaultOnResponse::new()),
        )
}

/// Redirect browser page loads to the admin setup screen until the first user
/// exists, matching Halo's first-run flow.
pub async fn redirect_to_setup_until_bootstrapped(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    if should_check_bootstrap_redirect(&req) {
        match is_bootstrapped(&state).await {
            Ok(false) => return Redirect::temporary("/admin/bootstrap").into_response(),
            Ok(true) => {}
            Err(err) => return err.into_response(),
        }
    }
    next.run(req).await
}

fn should_check_bootstrap_redirect(req: &Request) -> bool {
    if !matches!(*req.method(), Method::GET | Method::HEAD) || !accepts_html(req) {
        return false;
    }

    let path = req.uri().path();
    if path == "/" {
        return true;
    }
    if path == "/admin" || path == "/admin/" {
        return true;
    }
    path.starts_with("/admin/")
        && !path.starts_with("/admin/bootstrap")
        && !path.starts_with("/admin/assets/")
}

fn accepts_html(req: &Request) -> bool {
    req.headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|accept| {
            accept
                .split(',')
                .any(|part| part.trim().starts_with("text/html"))
        })
}

/// Convenience accessor used by handlers that want to log the inbound ID.
#[must_use]
pub fn request_id_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers.get(&X_REQUEST_ID).and_then(|v| v.to_str().ok())
}
