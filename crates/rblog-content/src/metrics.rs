//! Kinds under `metrics.halo.run/v1alpha1`.

use rblog_scheme::GroupVersionKind;
use serde::{Deserialize, Serialize};

const COUNTER_GVK: GroupVersionKind = GroupVersionKind::new(
    "metrics.halo.run",
    "v1alpha1",
    "Counter",
    "counters",
    "counter",
);

/// View / upvote / comment counters keyed by the related Extension's
/// store name. Halo stores these flat (no `spec` / `status`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counter {
    #[serde(default)]
    pub api_version: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub metadata: rblog_scheme::Metadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visit: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upvote: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downvote: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_comment: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_comment: Option<i32>,
}

impl Counter {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let gvk = <Self as rblog_scheme::Extension>::gvk();
        Self {
            api_version: gvk.api_version(),
            kind: gvk.kind.to_owned(),
            metadata: rblog_scheme::Metadata::new(name),
            visit: Some(0),
            upvote: Some(0),
            downvote: None,
            total_comment: Some(0),
            approved_comment: Some(0),
        }
    }
}

impl rblog_scheme::Extension for Counter {
    fn gvk() -> GroupVersionKind {
        COUNTER_GVK
    }
    fn metadata(&self) -> &rblog_scheme::Metadata {
        &self.metadata
    }
    fn metadata_mut(&mut self) -> &mut rblog_scheme::Metadata {
        &mut self.metadata
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn counter_initialized_to_zero() {
        let c = Counter::new("post.first");
        assert_eq!(c.visit, Some(0));
        assert_eq!(c.total_comment, Some(0));
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["apiVersion"], "metrics.halo.run/v1alpha1");
        assert_eq!(v["kind"], "Counter");
        assert_eq!(v["visit"], 0);
    }
}
