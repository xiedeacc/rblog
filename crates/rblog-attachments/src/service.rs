//! High-level upload/list/delete orchestration on top of the storage
//! backend and the thumbnail engine.

use std::sync::Arc;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::Serialize;
use thiserror::Error;
use tracing::info;

use crate::backend::{ObjectMetadata, PutResult, StorageBackend, StorageError};
use crate::naming::object_key;
use crate::thumbnail::{ThumbnailEngine, ThumbnailError};

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("thumbnail: {0}")]
    Thumbnail(#[from] ThumbnailError),
    #[error("validation: {0}")]
    Validation(String),
}

#[derive(Debug, Clone)]
pub struct NewAttachment {
    pub filename: String,
    pub media_type: Option<String>,
    pub group: Option<String>,
    pub bytes: Bytes,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredAttachment {
    pub key: String,
    pub url: String,
    pub media_type: Option<String>,
    pub size: u64,
    pub thumbnails: Vec<StoredThumbnail>,
    pub uploaded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredThumbnail {
    pub name: String,
    pub url: String,
    pub key: String,
    pub width: u32,
    pub height: u32,
}

/// Orchestrates uploads, thumbnailing, listing, deletion. Stateless
/// (everything is in the backend + the engine, both Arc-shared), so the
/// caller can keep one of these on `AppState` and clone it freely.
#[derive(Clone)]
pub struct AttachmentService {
    backend: Arc<dyn StorageBackend>,
    thumbnailer: ThumbnailEngine,
    max_bytes: usize,
}

impl AttachmentService {
    #[must_use]
    pub fn new(backend: Arc<dyn StorageBackend>, thumbnailer: ThumbnailEngine) -> Self {
        Self {
            backend,
            thumbnailer,
            max_bytes: 32 * 1024 * 1024,
        }
    }

    #[must_use]
    pub fn with_max_bytes(mut self, max: usize) -> Self {
        self.max_bytes = max;
        self
    }

    #[must_use]
    pub fn backend_label(&self) -> &str {
        self.backend.label()
    }

    /// Upload one file. If the media type is `image/*`, every registered
    /// thumbnail profile is rendered + uploaded next to it.
    pub async fn upload(&self, req: NewAttachment) -> Result<StoredAttachment, ServiceError> {
        if req.bytes.is_empty() {
            return Err(ServiceError::Validation("empty file".into()));
        }
        if req.bytes.len() > self.max_bytes {
            return Err(ServiceError::Validation(format!(
                "file exceeds limit of {} MiB",
                self.max_bytes / (1024 * 1024)
            )));
        }
        let media_type = req.media_type.clone().or_else(|| {
            mime_guess::from_path(&req.filename)
                .first()
                .map(|m| m.essence_str().to_owned())
        });
        let key = object_key(req.group.as_deref(), &req.filename, &req.bytes);
        let bytes = req.bytes.clone();
        let PutResult { url, size, .. } = self.backend.put(&key, bytes).await?;
        info!(backend = self.backend.label(), %key, %url, size, "stored attachment");

        let mut thumbnails = Vec::new();
        if media_type
            .as_deref()
            .is_some_and(|m| m.starts_with("image/"))
        {
            let rendered = self.thumbnailer.render(&req.bytes)?;
            for thumb in rendered {
                let thumb_filename = format!("{}-{}.jpg", strip_ext(&req.filename), thumb.name);
                let thumb_key = object_key(req.group.as_deref(), &thumb_filename, &thumb.bytes);
                let put = self.backend.put(&thumb_key, thumb.bytes).await?;
                thumbnails.push(StoredThumbnail {
                    name: thumb.name,
                    url: put.url,
                    key: put.key,
                    width: thumb.width,
                    height: thumb.height,
                });
            }
        }

        Ok(StoredAttachment {
            key,
            url,
            media_type,
            size,
            thumbnails,
            uploaded_at: Utc::now(),
        })
    }

    /// List stored objects under a prefix.
    pub async fn list(&self, prefix: Option<&str>) -> Result<Vec<ObjectMetadata>, ServiceError> {
        Ok(self.backend.list(prefix).await?)
    }

    /// Delete a single object. The caller is expected to also delete any
    /// thumbnails tracked alongside it.
    pub async fn delete(&self, key: &str) -> Result<(), ServiceError> {
        self.backend.delete(key).await?;
        Ok(())
    }

    /// Direct fetch — local backend only uses this to serve `/uploads/*`
    /// when the HTTP layer's ServeDir would otherwise need a synchronous
    /// path-on-disk; the S3 backend exposes it for tests.
    pub async fn get(&self, key: &str) -> Result<Bytes, ServiceError> {
        Ok(self.backend.get(key).await?)
    }
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Storage;
    use image::{ImageBuffer, ImageFormat, Rgb};
    use std::io::Cursor;

    fn sample_png() -> Vec<u8> {
        let img: ImageBuffer<Rgb<u8>, _> = ImageBuffer::from_fn(400, 300, |_, _| Rgb([10, 10, 10]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .unwrap();
        buf
    }

    #[tokio::test]
    async fn upload_and_list_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let backend = Storage::Local {
            root: tmp.path().to_owned(),
            public_prefix: "/uploads".into(),
        }
        .build()
        .unwrap();
        let svc = AttachmentService::new(backend, ThumbnailEngine::empty());
        let stored = svc
            .upload(NewAttachment {
                filename: "doc.txt".into(),
                media_type: Some("text/plain".into()),
                group: None,
                bytes: Bytes::from_static(b"hello"),
            })
            .await
            .unwrap();
        assert!(stored.url.starts_with("/uploads/"));
        let listed = svc.list(None).await.unwrap();
        assert_eq!(listed.len(), 1);
        svc.delete(&stored.key).await.unwrap();
        let listed = svc.list(None).await.unwrap();
        assert_eq!(listed.len(), 0);
    }

    #[tokio::test]
    async fn images_get_default_thumbnail() {
        let tmp = tempfile::tempdir().unwrap();
        let backend = Storage::Local {
            root: tmp.path().to_owned(),
            public_prefix: "/uploads".into(),
        }
        .build()
        .unwrap();
        let svc = AttachmentService::new(backend, ThumbnailEngine::default());
        let stored = svc
            .upload(NewAttachment {
                filename: "photo.png".into(),
                media_type: Some("image/png".into()),
                group: Some("photos".into()),
                bytes: Bytes::from(sample_png()),
            })
            .await
            .unwrap();
        assert_eq!(stored.thumbnails.len(), 1);
        assert!(stored.thumbnails[0].key.contains("photos/"));
        assert!(std::path::Path::new(&stored.thumbnails[0].url)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jpg")));
        let listed = svc.list(Some("photos")).await.unwrap();
        assert_eq!(listed.len(), 2, "original + thumbnail");
    }

    #[tokio::test]
    async fn empty_upload_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let backend = Storage::Local {
            root: tmp.path().to_owned(),
            public_prefix: "/uploads".into(),
        }
        .build()
        .unwrap();
        let svc = AttachmentService::new(backend, ThumbnailEngine::empty());
        let err = svc
            .upload(NewAttachment {
                filename: "x".into(),
                media_type: None,
                group: None,
                bytes: Bytes::new(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::Validation(_)));
    }
}
