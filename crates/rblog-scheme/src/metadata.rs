//! `Metadata` — the K8s-style metadata block every Extension carries.
//!
//! This struct is wire-compatible with Halo's `run.halo.app.extension.Metadata`
//! Jackson serialization, including:
//!
//! - `camelCase` field names (`generateName`, `creationTimestamp`, ...).
//! - Optional fields omitted from output when null (Halo's default Jackson config).
//! - `finalizers` is a `Set<String>` in Java; we model it as `BTreeSet<String>`
//!   so iteration order is deterministic for tests.
//! - The `version` column from the database is **mirrored** into this struct
//!   on read. It is *not* serialized when the struct itself is sent over the
//!   wire to clients (Halo omits it when null), but we always include it on
//!   loaded objects.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Metadata of an Extension.
///
/// Mirrors Halo's `Metadata` POJO exactly.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    /// Globally unique name within a kind.
    pub name: String,

    /// Server-generated name: the server fills `name` from `generateName`
    /// at create time. Optional, omitted when missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate_name: Option<String>,

    /// Free-form labels used for filtering and selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,

    /// Free-form annotations, used by the system and by themes/plugins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<BTreeMap<String, String>>,

    /// Optimistic-concurrency token. Mirrors the `version` column of the
    /// `extensions` row this object came from. Halo also omits it when null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,

    /// Creation timestamp; Halo sets this at insert time if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<DateTime<Utc>>,

    /// Deletion timestamp; set when soft-deleted, finalizers must drain before
    /// the row is actually removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion_timestamp: Option<DateTime<Utc>>,

    /// Finalizer chain. Java models this as `Set<String>`; using a BTreeSet
    /// gives us deterministic JSON output for tests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finalizers: Option<BTreeSet<String>>,
}

impl Metadata {
    /// Convenience constructor for a freshly-created object: only `name` is set.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// The `name` getter — used in trait methods to avoid two-step field access.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Insert or replace a label in-place.
    pub fn set_label(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.labels
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value.into());
    }

    /// Read a label.
    #[must_use]
    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels.as_ref()?.get(key).map(String::as_str)
    }

    /// Insert or replace an annotation in-place.
    pub fn set_annotation(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.annotations
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value.into());
    }

    /// Read an annotation.
    #[must_use]
    pub fn annotation(&self, key: &str) -> Option<&str> {
        self.annotations.as_ref()?.get(key).map(String::as_str)
    }

    /// Remove an annotation. Returns the previous value if it was present.
    pub fn remove_annotation(&mut self, key: &str) -> Option<String> {
        let removed = self.annotations.as_mut()?.remove(key);
        if let Some(m) = self.annotations.as_ref() {
            if m.is_empty() {
                self.annotations = None;
            }
        }
        removed
    }

    /// Remove a label. Returns the previous value if it was present.
    pub fn remove_label(&mut self, key: &str) -> Option<String> {
        let removed = self.labels.as_mut()?.remove(key);
        if let Some(m) = self.labels.as_ref() {
            if m.is_empty() {
                self.labels = None;
            }
        }
        removed
    }

    /// Whether this object has been soft-deleted.
    #[must_use]
    pub fn is_deleted(&self) -> bool {
        self.deletion_timestamp.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pretty_assertions::assert_eq;

    #[test]
    fn empty_metadata_serializes_to_just_name() {
        let m = Metadata::new("hello");
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, serde_json::json!({ "name": "hello" }));
    }

    #[test]
    fn populated_metadata_round_trips() {
        let mut m = Metadata::new("my-post");
        m.set_label("content.halo.run/published", "true");
        m.set_annotation("content.halo.run/permalink-pattern", "/archives/{slug}");
        m.version = Some(5);
        m.creation_timestamp = Some(Utc.with_ymd_and_hms(2026, 5, 16, 0, 0, 0).unwrap());

        let json = serde_json::to_string(&m).unwrap();
        let back: Metadata = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn deserializes_halo_camelcase() {
        let halo_json = r#"{
            "name": "my-post",
            "generateName": "post-",
            "labels": { "k": "v" },
            "annotations": { "a": "b" },
            "version": 7,
            "creationTimestamp": "2026-05-16T00:00:00Z",
            "deletionTimestamp": null,
            "finalizers": ["post-content-cleanup"]
        }"#;
        let m: Metadata = serde_json::from_str(halo_json).unwrap();
        assert_eq!(m.name, "my-post");
        assert_eq!(m.generate_name.as_deref(), Some("post-"));
        assert_eq!(m.version, Some(7));
        assert_eq!(m.label("k"), Some("v"));
        assert_eq!(m.annotation("a"), Some("b"));
        assert!(m.deletion_timestamp.is_none());
        assert_eq!(
            m.finalizers
                .as_ref()
                .unwrap()
                .iter()
                .next()
                .map(String::as_str),
            Some("post-content-cleanup")
        );
    }

    #[test]
    fn omits_nulls_in_output() {
        let m = Metadata::new("x");
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("null"));
        assert!(!json.contains("generateName"));
        assert!(!json.contains("deletionTimestamp"));
    }
}
