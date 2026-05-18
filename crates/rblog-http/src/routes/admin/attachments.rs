//! Admin endpoints for attachments.
//!
//! `POST /api/admin/attachments` accepts `multipart/form-data`:
//!
//! - `file` (required): the upload bytes.
//! - `group` (optional): a logical folder (Halo's `attachment group`).
//!
//! For `image/*` uploads the service eagerly renders a 320px JPEG thumbnail
//! next to the original.

use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use bytes::Bytes;
use rblog_attachments::{NewAttachment, ObjectMetadata, StoredAttachment};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::{AppState, HttpError};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/attachments", get(list).post(upload))
        .route("/api/admin/attachments/*key", delete(remove))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ListQuery {
    #[serde(default)]
    pub prefix: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttachmentListItem {
    pub key: String,
    pub url: String,
    pub size: u64,
}

impl From<ObjectMetadata> for AttachmentListItem {
    fn from(m: ObjectMetadata) -> Self {
        Self {
            key: m.key,
            url: m.url,
            size: m.size,
        }
    }
}

/// List stored attachments under an optional prefix.
#[utoipa::path(
    get,
    path = "/api/admin/attachments",
    tag = "attachments",
    params(ListQuery),
    responses((status = 200, body = Vec<AttachmentListItem>)),
)]
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<AttachmentListItem>>, HttpError> {
    let items = state.attachments.list(q.prefix.as_deref()).await?;
    Ok(Json(items.into_iter().map(Into::into).collect()))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadResponse {
    pub key: String,
    pub url: String,
    pub media_type: Option<String>,
    pub size: u64,
    pub thumbnails: Vec<ThumbnailItem>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ThumbnailItem {
    pub name: String,
    pub url: String,
    pub key: String,
    pub width: u32,
    pub height: u32,
}

impl From<StoredAttachment> for UploadResponse {
    fn from(s: StoredAttachment) -> Self {
        Self {
            key: s.key,
            url: s.url,
            media_type: s.media_type,
            size: s.size,
            thumbnails: s
                .thumbnails
                .into_iter()
                .map(|t| ThumbnailItem {
                    name: t.name,
                    url: t.url,
                    key: t.key,
                    width: t.width,
                    height: t.height,
                })
                .collect(),
        }
    }
}

/// Upload a single file via multipart/form-data. Returns the public URL and,
/// for `image/*` uploads, the generated thumbnail entries.
#[utoipa::path(
    post,
    path = "/api/admin/attachments",
    tag = "attachments",
    request_body(content = String, content_type = "multipart/form-data"),
    responses(
        (status = 201, description = "Uploaded", body = UploadResponse),
        (status = 422, description = "Validation failed (empty / too large / missing file part)"),
    ),
)]
pub async fn upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<UploadResponse>), HttpError> {
    let mut file_bytes: Option<Bytes> = None;
    let mut filename: Option<String> = None;
    let mut media_type: Option<String> = None;
    let mut group: Option<String> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| HttpError::validation(format!("invalid multipart payload: {e}")))?
    {
        let name = field.name().unwrap_or("").to_owned();
        match name.as_str() {
            "file" => {
                filename = field.file_name().map(ToOwned::to_owned);
                media_type = field.content_type().map(ToOwned::to_owned);
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| HttpError::validation(format!("read file part: {e}")))?;
                file_bytes = Some(data);
            }
            "group" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| HttpError::validation(format!("read group part: {e}")))?;
                if !text.trim().is_empty() {
                    group = Some(text.trim().to_owned());
                }
            }
            _ => {} // ignored
        }
    }
    let bytes = file_bytes
        .ok_or_else(|| HttpError::validation("multipart: missing `file` part".to_string()))?;
    let filename = filename.filter(|s| !s.is_empty()).ok_or_else(|| {
        HttpError::validation("multipart: `file` part has no filename".to_string())
    })?;
    let stored = state
        .attachments
        .upload(NewAttachment {
            filename,
            media_type,
            group,
            bytes,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(stored.into())))
}

/// Delete an attachment by storage key. Thumbnails uploaded next to it are
/// not auto-deleted — the SPA fans the request out per key.
#[utoipa::path(
    delete,
    path = "/api/admin/attachments/{key}",
    tag = "attachments",
    params(("key" = String, Path, description = "Storage key (forward-slash separated)")),
    responses((status = 204, description = "Deleted")),
)]
pub async fn remove(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.attachments.delete(&key).await?;
    Ok(StatusCode::NO_CONTENT)
}
