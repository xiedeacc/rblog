//! Domain services for the public blog and the admin API.
//!
//! Each `*Service` wraps rblog's relational tables and adds the domain rules
//! that distinguish a blog from a generic database:
//!
//! - Relational post/page CRUD with markdown rendering.
//! - Derived projections (`PostListItem`, `PostDetail`) that combine content,
//!   taxonomy lookups, reading counts, and permalinks.
//! - First-run bootstrap: create the initial admin user and site settings.
//!
//! ## Concurrency
//!
//! Services are `Send + Sync`. They are intentionally stateless — the
//! caller owns the SQL pool reference. Indexing is only used for lightweight
//! runtime projections that templates need synchronously.
//!
//! ## Errors
//!
//! All services share [`ServiceError`]. The most common variants:
//!
//! - `NotFound(kind, name)` — the requested record does not exist.
//! - `Conflict(kind, name)` — typically a duplicate name on create.
//! - `Validation(message)` — slug/title/email constraints.
//! - `Storage(_)` — propagated from `rblog-store`.

pub mod bootstrap;
pub mod clean_pages;
pub mod clean_posts;
pub mod clean_settings;
pub mod clean_taxonomy;
pub mod clean_users;
pub mod comments;
pub mod system;

mod indexing;
mod permalink;

pub use bootstrap::{bootstrap_system, BootstrapOptions};
pub use clean_pages::{
    PageDetail, PageListItem, PageListQuery, PageService, PageSettingsUpdate, PageStatusFilter,
};
pub use clean_posts::{
    DraftPost, PostDetail, PostListItem, PostListQuery, PostService, PostSettingsUpdate,
    PostStatusFilter, PublishOptions,
};
pub use clean_settings::{ConfigMapService, SettingService, SYSTEM_CONFIGMAP};
pub use clean_taxonomy::{CategoryService, NewCategory, NewTag, TagService};
pub use clean_users::{AuthenticatedUser, CreateUser, UserService};
pub use comments::{CommentService, NewComment, SpamHeuristic, SpamVerdict};
pub use system::{build_services, resync_all};

use std::sync::Arc;

use rblog_auth::PasswordHasher;
use rblog_index::IndexEngine;

/// Bundle of every long-lived service, suitable for cloning into Axum state.
#[derive(Clone)]
pub struct Services {
    pub pages: Arc<PageService>,
    pub posts: Arc<PostService>,
    pub categories: Arc<CategoryService>,
    pub tags: Arc<TagService>,
    pub comments: Arc<CommentService>,
    pub users: Arc<UserService>,
    pub settings: Arc<SettingService>,
    pub configmaps: Arc<ConfigMapService>,
    pub hasher: Arc<PasswordHasher>,
    pub index: Arc<IndexEngine>,
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("{kind} `{name}` not found")]
    NotFound { kind: &'static str, name: String },
    #[error("{kind} `{name}` already exists")]
    Conflict { kind: &'static str, name: String },
    #[error("invalid request: {0}")]
    Validation(String),
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("storage: {0}")]
    Storage(#[from] rblog_store::StoreError),
    #[error("indexing: {0}")]
    Index(#[from] rblog_index::IndexError),
    #[error("password: {0}")]
    Password(#[from] rblog_auth::password::PasswordError),
    #[error("content: {0}")]
    Content(String),
    #[error("internal: {0}")]
    Internal(String),
}

/// Convenience constructor for [`ServiceError::NotFound`].
fn not_found(kind: &'static str, name: impl Into<String>) -> ServiceError {
    ServiceError::NotFound {
        kind,
        name: name.into(),
    }
}

/// Convenience constructor for [`ServiceError::Conflict`].
fn conflict(kind: &'static str, name: impl Into<String>) -> ServiceError {
    ServiceError::Conflict {
        kind,
        name: name.into(),
    }
}
