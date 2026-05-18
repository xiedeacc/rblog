//! Domain services for the public blog and the admin API.
//!
//! Each `*Service` wraps a [`TypedStore`] + [`IndexEngine`] pair and adds
//! the rules that distinguish a v1 blog from a generic K/V store:
//!
//! - Cascading writes (creating a `Post` also creates its base `Snapshot`).
//! - Soft-delete labels matching Halo's `content.halo.run/deleted` /
//!   `content.halo.run/published` conventions.
//! - Derived projections (`PostListItem`, `PostDetail`) that combine `Post`
//!   metadata with composed snapshot content, taxonomy lookups, reading
//!   time, and permalinks.
//! - First-run bootstrap: install default theme, default super-admin user,
//!   default role + binding.
//!
//! ## Concurrency
//!
//! Services are `Send + Sync`. They are intentionally stateless — the
//! caller owns the `TypedStore<'_>` pool reference. Indexing happens
//! synchronously in the service call: every successful create / update /
//! delete updates the [`IndexEngine`] before returning.
//!
//! ## Errors
//!
//! All services share [`ServiceError`]. The most common variants:
//!
//! - `NotFound(kind, name)` — the requested extension does not exist.
//! - `Conflict(kind, name)` — typically a duplicate name on create.
//! - `Validation(message)` — slug/title/email constraints.
//! - `Storage(_)` — propagated from `rblog-store`.

pub mod bootstrap;
pub mod comments;
pub mod menus;
pub mod posts;
pub mod settings;
pub mod system;
pub mod taxonomy;
pub mod users;

mod indexing;
mod permalink;

pub use bootstrap::{bootstrap_system, BootstrapOptions};
pub use comments::{CommentService, NewComment, SpamHeuristic, SpamVerdict};
pub use menus::MenuService;
pub use posts::{
    DraftPost, PostDetail, PostListItem, PostListQuery, PostService, PostSettingsUpdate,
    PostStatusFilter, PublishOptions,
};
pub use settings::{ConfigMapService, SettingService};
pub use system::{build_services, resync_all};
pub use taxonomy::{CategoryService, NewCategory, NewTag, TagService};
pub use users::{CreateUser, UserService};

use std::sync::Arc;

use rblog_auth::PasswordHasher;
use rblog_index::IndexEngine;

/// Bundle of every long-lived service, suitable for cloning into Axum state.
#[derive(Clone)]
pub struct Services {
    pub posts: Arc<PostService>,
    pub categories: Arc<CategoryService>,
    pub tags: Arc<TagService>,
    pub comments: Arc<CommentService>,
    pub users: Arc<UserService>,
    pub menus: Arc<MenuService>,
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
