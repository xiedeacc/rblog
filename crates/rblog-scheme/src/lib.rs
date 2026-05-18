//! Halo-compatible Extension model.
//!
//! This crate defines the core types every rblog "kind" is built on:
//!
//! - [`GroupVersionKind`] — Halo's K8s-style identifier of a kind.
//! - [`Metadata`] — the metadata object every Extension carries.
//! - [`Extension`] — the marker trait every concrete kind implements.
//! - [`Scheme`] — the runtime registration for a kind, used by the store and
//!   the index engine.
//! - [`SchemeRegistry`] — the in-process catalog of all known schemes.
//!
//! ## Wire format
//!
//! Every Extension is serialized as JSON shaped like:
//!
//! ```json
//! {
//!   "apiVersion": "<group>/<version>",
//!   "kind": "<Kind>",
//!   "metadata": { ... },
//!   "spec":   { ... },
//!   "status": { ... }
//! }
//! ```
//!
//! When `group` is empty (core kinds like `User`, `Setting`, `ConfigMap`),
//! `apiVersion` is just `<version>`.
//!
//! This format is **byte-for-byte compatible** with Halo's Java Jackson output,
//! so an existing Halo database can be migrated by simply replaying its
//! `extensions` table into rblog.

mod gvk;
mod metadata;
mod registry;
mod store_name;

pub use gvk::GroupVersionKind;
pub use metadata::Metadata;
pub use registry::{Scheme, SchemeError, SchemeRegistry};
pub use store_name::{build_store_name, build_store_name_prefix, parse_store_name, StoreNameError};

use serde::{de::DeserializeOwned, Serialize};

/// Every kind stored in the Extension table implements this trait.
///
/// Concrete kinds (`Post`, `Tag`, `User`, ...) usually derive [`serde::Serialize`]
/// and [`serde::Deserialize`] and provide their own `spec` / `status` types.
pub trait Extension: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The Group/Version/Kind triple for this kind.
    ///
    /// Returned as a `const` so callers can build store paths without instantiating
    /// the type.
    fn gvk() -> GroupVersionKind;

    /// Borrow the metadata block.
    fn metadata(&self) -> &Metadata;

    /// Mutably borrow the metadata block. Used by the store to set `version`
    /// after a successful insert / update.
    fn metadata_mut(&mut self) -> &mut Metadata;

    /// Build the store path for this instance: `/registry/[group/]plural/name`.
    fn store_name(&self) -> String {
        build_store_name(&Self::gvk(), self.metadata().name())
    }
}
