//! Typed Halo-compatible Extension kinds.
//!
//! Every type in this crate is wire-compatible with Halo's Java POJO of the
//! same name. That means an existing Halo install's JSON payload deserializes
//! into the corresponding Rust struct, round-trips through serde, and writes
//! back bytes that pass Halo's own validation. The big consequences:
//!
//! - Field names are `camelCase` (we use `#[serde(rename_all = "camelCase")]`).
//! - Unset / null fields are *omitted* from the output (`skip_serializing_if =
//!   "Option::is_none"`), matching Jackson's `NON_NULL` default.
//! - Enum variants are serialized as their Java `name()` (mostly UPPERCASE).
//! - `apiVersion` and `kind` are stored on the struct so a freshly-built
//!   instance is wire-correct without help from a serializer hook.
//!
//! ## Organization
//!
//! Submodules mirror Halo's group taxonomy:
//!
//! | Module     | Group              | Kinds                                                                     |
//! |------------|--------------------|---------------------------------------------------------------------------|
//! | [`infra`]  | _shared types_     | [`Ref`], [`Condition`], [`ConditionList`], [`ConditionStatus`]            |
//! | [`content`]| `content.halo.run` | [`Post`], [`SinglePage`], [`Tag`], [`Category`], [`Snapshot`], [`Comment`], [`Reply`] |
//! | [`core`]   | `` (empty)         | [`User`], [`Role`], [`RoleBinding`], [`Menu`], [`MenuItem`], [`Setting`], [`ConfigMap`], [`Secret`] |
//! | [`storage`]| `storage.halo.run` | [`Attachment`], [`AttachmentGroup`], [`Policy`], [`PolicyTemplate`]      |
//! | [`theme`]  | `theme.halo.run`   | [`Theme`]                                                                 |
//! | [`metrics`]| `metrics.halo.run` | [`Counter`]                                                               |
//! | [`plugin`] | `plugin.halo.run`  | [`Plugin`]                                                                |
//!
//! ## Registering schemes
//!
//! [`register_default_schemes`] populates a [`SchemeRegistry`] with every kind
//! defined here. Call it once at process startup; plugins add additional kinds
//! through the same registry later.

#[macro_use]
mod macros;

pub mod infra;

pub mod content;
pub mod content_wrapper;
pub mod core;
pub mod metrics;
pub mod patch;
pub mod plugin;
pub mod render;
pub mod storage;
pub mod theme;

pub use content_wrapper::{compose_snapshot, ContentWrapper, ContentWrapperError};
pub use patch::{apply_patch, diff_to_json_patch, Delta, DeltaType, PatchError, StringChunk};
pub use render::{render_markdown, MarkdownPipeline, RenderError, RenderOptions, Rendered};

pub use content::{Category, Comment, Post, Reply, SinglePage, Snapshot, Tag};
pub use core::{ConfigMap, Menu, MenuItem, Role, RoleBinding, Secret, Setting, User};
pub use infra::{Condition, ConditionList, ConditionStatus, Ref};
pub use metrics::Counter;
pub use plugin::Plugin;
pub use storage::{Attachment, AttachmentGroup, Policy, PolicyTemplate};
pub use theme::Theme;

use rblog_scheme::{SchemeError, SchemeRegistry};

/// Register every built-in kind with the given registry.
///
/// Returns the first error encountered. The function is idempotent only across
/// distinct registries — calling it twice on the same registry surfaces an
/// `AlreadyRegistered` error.
pub fn register_default_schemes(reg: &SchemeRegistry) -> Result<(), SchemeError> {
    // content.halo.run
    reg.register::<Post>()?;
    reg.register::<SinglePage>()?;
    reg.register::<Tag>()?;
    reg.register::<Category>()?;
    reg.register::<Snapshot>()?;
    reg.register::<Comment>()?;
    reg.register::<Reply>()?;

    // core (no group)
    reg.register::<User>()?;
    reg.register::<Role>()?;
    reg.register::<RoleBinding>()?;
    reg.register::<Menu>()?;
    reg.register::<MenuItem>()?;
    reg.register::<Setting>()?;
    reg.register::<ConfigMap>()?;
    reg.register::<Secret>()?;

    // storage.halo.run
    reg.register::<Attachment>()?;
    reg.register::<AttachmentGroup>()?;
    reg.register::<Policy>()?;
    reg.register::<PolicyTemplate>()?;

    // theme.halo.run
    reg.register::<Theme>()?;

    // metrics.halo.run
    reg.register::<Counter>()?;

    // plugin.halo.run
    reg.register::<Plugin>()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_default_schemes_register_cleanly() {
        let reg = SchemeRegistry::new();
        register_default_schemes(&reg).expect("default schemes must register");
        // Bump the assertion when we add new kinds.
        assert_eq!(reg.len(), 22);
    }
}
