//! Single concrete error type for the whole HTTP layer.
//!
//! Every fallible handler returns `Result<T, HttpError>` so the router can
//! map storage errors, validation failures, auth denials and so on to
//! coherent HTTP responses without scattered match arms.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rblog_core::ServiceError;
use serde_json::json;

/// HTTP-layer error envelope. Carries an HTTP status, a stable error code,
/// and a free-form human message. The handler tail returns a JSON body
/// shaped like `{ "error": { "code": "...", "message": "..." } }`.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("{message}")]
    BadRequest { code: &'static str, message: String },
    #[error("{message}")]
    Unauthorized { code: &'static str, message: String },
    #[error("{message}")]
    Forbidden { code: &'static str, message: String },
    #[error("{message}")]
    NotFound { code: &'static str, message: String },
    #[error("{message}")]
    Conflict { code: &'static str, message: String },
    #[error("{message}")]
    UnprocessableEntity { code: &'static str, message: String },
    #[error("{message}")]
    TooManyRequests { code: &'static str, message: String },
    #[error("internal: {0}")]
    Internal(#[from] anyhow::Error),
}

impl HttpError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest {
            code: "bad_request",
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized {
            code: "unauthorized",
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden {
            code: "forbidden",
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            code: "not_found",
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict {
            code: "conflict",
            message: message.into(),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::UnprocessableEntity {
            code: "validation",
            message: message.into(),
        }
    }

    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::TooManyRequests {
            code: "rate_limited",
            message: message.into(),
        }
    }

    pub fn rate_limited_retry_after(retry_after_secs: u64) -> Self {
        Self::TooManyRequests {
            code: "rate_limited",
            message: format!("too many requests; retry after {retry_after_secs}s"),
        }
    }

    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            Self::BadRequest { code, message } => (StatusCode::BAD_REQUEST, code, message.clone()),
            Self::Unauthorized { code, message } => {
                (StatusCode::UNAUTHORIZED, code, message.clone())
            }
            Self::Forbidden { code, message } => (StatusCode::FORBIDDEN, code, message.clone()),
            Self::NotFound { code, message } => (StatusCode::NOT_FOUND, code, message.clone()),
            Self::Conflict { code, message } => (StatusCode::CONFLICT, code, message.clone()),
            Self::UnprocessableEntity { code, message } => {
                (StatusCode::UNPROCESSABLE_ENTITY, code, message.clone())
            }
            Self::TooManyRequests { code, message } => {
                (StatusCode::TOO_MANY_REQUESTS, code, message.clone())
            }
            Self::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error".to_owned(),
            ),
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        if status.is_server_error() {
            tracing::error!(?status, ?code, %message, "server error");
        } else if status.as_u16() >= 400 {
            tracing::debug!(?status, ?code, %message, "client error");
        }
        (
            status,
            Json(json!({"error": {"code": code, "message": message}})),
        )
            .into_response()
    }
}

impl From<ServiceError> for HttpError {
    fn from(err: ServiceError) -> Self {
        match err {
            ServiceError::NotFound { kind, name } => {
                Self::not_found(format!("{kind} `{name}` not found"))
            }
            ServiceError::Conflict { kind, name } => {
                Self::conflict(format!("{kind} `{name}` already exists"))
            }
            ServiceError::Validation(msg) => Self::validation(msg),
            ServiceError::Auth(msg) => Self::unauthorized(msg),
            ServiceError::Storage(rblog_store::StoreError::DuplicateName(n)) => {
                Self::conflict(format!("`{n}` already exists"))
            }
            ServiceError::Storage(rblog_store::StoreError::NotFound(n)) => {
                Self::not_found(format!("`{n}` not found"))
            }
            ServiceError::Storage(rblog_store::StoreError::OptimisticLock { name, expected }) => {
                Self::conflict(format!(
                    "optimistic lock conflict on `{name}` (expected version {expected})"
                ))
            }
            other => Self::Internal(anyhow::Error::new(other)),
        }
    }
}

impl From<rblog_index::IndexError> for HttpError {
    fn from(err: rblog_index::IndexError) -> Self {
        Self::Internal(anyhow::Error::new(err))
    }
}

impl From<rblog_store::StoreError> for HttpError {
    fn from(err: rblog_store::StoreError) -> Self {
        ServiceError::Storage(err).into()
    }
}

impl From<minijinja::Error> for HttpError {
    fn from(err: minijinja::Error) -> Self {
        Self::Internal(anyhow::Error::new(err))
    }
}

impl From<rblog_theme::ThemeRendererError> for HttpError {
    fn from(err: rblog_theme::ThemeRendererError) -> Self {
        Self::Internal(anyhow::Error::new(err))
    }
}

impl From<rblog_theme::ThemeRegistryError> for HttpError {
    fn from(err: rblog_theme::ThemeRegistryError) -> Self {
        Self::Internal(anyhow::Error::new(err))
    }
}

impl From<rblog_attachments::ServiceError> for HttpError {
    fn from(err: rblog_attachments::ServiceError) -> Self {
        match err {
            rblog_attachments::ServiceError::Validation(m) => Self::validation(m),
            other => Self::Internal(anyhow::Error::new(other)),
        }
    }
}

impl From<rblog_attachments::StorageError> for HttpError {
    fn from(err: rblog_attachments::StorageError) -> Self {
        Self::Internal(anyhow::Error::new(err))
    }
}

impl From<rblog_search::SearchError> for HttpError {
    fn from(err: rblog_search::SearchError) -> Self {
        Self::Internal(anyhow::Error::new(err))
    }
}

impl From<rblog_plugins::RuntimeError> for HttpError {
    fn from(err: rblog_plugins::RuntimeError) -> Self {
        use rblog_plugins::RuntimeError as R;
        match err {
            R::NotFound(name) => Self::not_found(format!("plugin `{name}` not found")),
            R::Disabled(name) => Self::conflict(format!("plugin `{name}` is disabled")),
            R::Capability(c) => Self::validation(format!("plugin capability: {c}")),
            R::Abi { plugin, message } => {
                Self::Internal(anyhow::anyhow!("plugin `{plugin}` ABI error: {message}"))
            }
            R::BadResponse { plugin, source } => Self::Internal(anyhow::anyhow!(
                "plugin `{plugin}` returned invalid response: {source}"
            )),
            R::Load(e) => Self::Internal(anyhow::Error::new(e)),
            R::Wasm(e) => Self::Internal(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn not_found_maps_to_404_json() {
        let err: HttpError = ServiceError::NotFound {
            kind: "Post",
            name: "missing".into(),
        }
        .into();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["error"]["code"], "not_found");
        assert!(val["error"]["message"]
            .as_str()
            .unwrap()
            .contains("missing"));
    }

    #[tokio::test]
    async fn validation_maps_to_422() {
        let err: HttpError = ServiceError::Validation("bad slug".into()).into();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn duplicate_store_maps_to_409() {
        let err: HttpError =
            ServiceError::Storage(rblog_store::StoreError::DuplicateName("x".into())).into();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }
}
