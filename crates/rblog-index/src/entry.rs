//! Indexed entries — the projection of an Extension that the engine actually
//! stores.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::IndexError;

/// A projected, indexable copy of an Extension.
///
/// `raw` is the parsed JSON value so field selectors and sort keys can be
/// resolved by path without re-parsing on every query. For the hot path this
/// trades RAM for CPU — fine for a self-hosted blog.
#[derive(Debug, Clone)]
pub struct IndexedExt {
    pub name: String,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub creation_time: Option<DateTime<Utc>>,
    pub deletion_time: Option<DateTime<Utc>>,
    /// Full parsed JSON. Used for field selectors and sort.
    pub raw: serde_json::Value,
}

impl IndexedExt {
    /// Build an [`IndexedExt`] from a typed Extension by serializing it and
    /// pulling metadata out of the rendered JSON.
    ///
    /// Cheaper alternative when you already have the JSON: [`from_value`].
    pub fn from_extension<E: serde::Serialize + rblog_scheme::Extension>(
        ext: &E,
    ) -> Result<Self, IndexError> {
        let value = serde_json::to_value(ext)
            .map_err(|e| IndexError::Invalid(format!("serialize: {e}")))?;
        Self::from_value(value)
    }

    /// Build an [`IndexedExt`] from a raw JSON value. The value must look like
    /// a Halo Extension (`{ "metadata": { "name": "...", ... }, ... }`).
    pub fn from_value(value: serde_json::Value) -> Result<Self, IndexError> {
        let metadata = value
            .get("metadata")
            .ok_or_else(|| IndexError::Invalid("missing `metadata`".to_owned()))?;
        let name = metadata
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| IndexError::Invalid("missing `metadata.name`".to_owned()))?
            .to_owned();

        let labels = metadata
            .get("labels")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                    .collect()
            })
            .unwrap_or_default();

        let annotations = metadata
            .get("annotations")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                    .collect()
            })
            .unwrap_or_default();

        let creation_time = parse_instant(metadata.get("creationTimestamp"));
        let deletion_time = parse_instant(metadata.get("deletionTimestamp"));

        Ok(Self {
            name,
            labels,
            annotations,
            creation_time,
            deletion_time,
            raw: value,
        })
    }

    /// Convenience: does the entry have a non-`null` `metadata.deletionTimestamp`?
    #[must_use]
    pub fn is_deleted(&self) -> bool {
        self.deletion_time.is_some()
    }
}

fn parse_instant(v: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    v.and_then(|x| x.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn from_value_extracts_metadata() {
        let v = serde_json::json!({
            "apiVersion": "content.halo.run/v1alpha1",
            "kind": "Post",
            "metadata": {
                "name": "hello",
                "labels": { "content.halo.run/published": "true" },
                "annotations": { "content.halo.run/last-released-snapshot": "snap-1" },
                "creationTimestamp": "2026-01-01T00:00:00Z"
            },
            "spec": { "title": "Hello", "slug": "hello" }
        });
        let e = IndexedExt::from_value(v).expect("ok");
        assert_eq!(e.name, "hello");
        assert_eq!(
            e.labels
                .get("content.halo.run/published")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            e.annotations
                .get("content.halo.run/last-released-snapshot")
                .map(String::as_str),
            Some("snap-1")
        );
        assert!(e.creation_time.is_some());
        assert!(!e.is_deleted());
    }

    #[test]
    fn deletion_timestamp_marks_deleted() {
        let v = serde_json::json!({
            "metadata": { "name": "p", "deletionTimestamp": "2026-05-01T00:00:00Z" }
        });
        let e = IndexedExt::from_value(v).expect("ok");
        assert!(e.is_deleted());
    }

    #[test]
    fn missing_metadata_is_error() {
        let v = serde_json::json!({ "foo": 1 });
        let err = IndexedExt::from_value(v).expect_err("must fail");
        assert!(matches!(err, IndexError::Invalid(_)));
    }
}
