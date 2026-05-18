//! Attachment storage backends and image thumbnailing.
//!
//! This crate wraps [`object_store`](https://docs.rs/object_store) behind a
//! thin trait so the rest of the codebase can pretend storage is "put bytes,
//! get a URL". Two backends are wired in v1:
//!
//! - [`Storage::Local`]: local filesystem under `<root>/uploads/`, served by
//!   the HTTP layer's `ServeDir` at `/uploads/*`.
//! - [`Storage::S3`]: an S3-compatible bucket (AWS S3, MinIO, R2, …). The
//!   public URL is built from the configured `public_base_url`.
//!
//! Image thumbnails are produced eagerly by [`ThumbnailEngine`] when the
//! uploaded media type starts with `image/`. The default profile generates
//! one 320px-wide JPEG named `<stem>-thumb.jpg`; callers can register custom
//! profiles via [`ThumbnailEngine::add_profile`].
//!
//! ## Naming
//!
//! Uploaded objects are placed at `<group>/<yyyy>/<mm>/<sha-prefix>-<name>`,
//! mirroring Halo's default `LocalAttachmentHandler` layout. The SHA prefix
//! gives us a stable per-byte-payload identity that survives renames.

mod backend;
mod naming;
mod service;
mod thumbnail;

pub use backend::{ObjectMetadata, PutResult, Storage, StorageBackend, StorageError};
pub use naming::{object_key, sha_prefix};
pub use service::{AttachmentService, NewAttachment, ServiceError, StoredAttachment};
pub use thumbnail::{ThumbnailEngine, ThumbnailProfile, ThumbnailResult};
