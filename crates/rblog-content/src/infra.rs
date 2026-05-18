//! Shared types used by multiple Halo kinds.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// `Ref` is a stable cross-kind reference, used in places like
/// `Snapshot.spec.subjectRef` or `Comment.spec.subjectRef`.
///
/// Halo's Java type has nullable `version`; everything else is required by
/// convention but the underlying JSON is permissive — we follow the JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ref {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

impl Ref {
    /// Constructor mirroring `Ref.of(name)`.
    pub fn of_name(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Constructor mirroring `Ref.of(name, gvk)`.
    pub fn of_gvk(name: impl Into<String>, gvk: &rblog_scheme::GroupVersionKind) -> Self {
        Self {
            group: gvk.group.to_owned(),
            version: Some(gvk.version.to_owned()),
            kind: gvk.kind.to_owned(),
            name: name.into(),
        }
    }

    /// Stable identifier `group/kind/name`, matching Halo's `toIdentifier`.
    #[must_use]
    pub fn to_identifier(&self) -> String {
        format!("{}/{}/{}", self.group, self.kind, self.name)
    }
}

/// `ConditionStatus` mirrors Halo's condition values. rblog serializes the
/// Kubernetes-style title-case values, but accepts Halo's uppercase dump values
/// during restore as well.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionStatus {
    #[serde(alias = "TRUE")]
    True,
    #[serde(alias = "FALSE")]
    False,
    #[serde(alias = "UNKNOWN")]
    Unknown,
}

/// `Condition` records a state transition for an Extension's `status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    pub r#type: String,
    pub status: ConditionStatus,
    pub last_transition_time: DateTime<Utc>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub reason: String,
}

/// Halo serializes `ConditionList` as a JSON array. We mirror that exactly
/// rather than wrapping in an outer object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConditionList(pub Vec<Condition>);

impl ConditionList {
    #[must_use]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, c: Condition) {
        self.0.push(c);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// Author + license info shared by `Theme` and `Plugin`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
}

/// License info shared by `Theme` and `Plugin`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct License {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pretty_assertions::assert_eq;

    #[test]
    fn ref_omits_empty_fields() {
        let r = Ref::of_name("hello");
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v, serde_json::json!({ "name": "hello" }));
    }

    #[test]
    fn ref_of_gvk_carries_group_version_kind() {
        let gvk = rblog_scheme::GroupVersionKind::new(
            "content.halo.run",
            "v1alpha1",
            "Post",
            "posts",
            "post",
        );
        let r = Ref::of_gvk("my-post", &gvk);
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "group": "content.halo.run",
                "version": "v1alpha1",
                "kind": "Post",
                "name": "my-post",
            })
        );
        assert_eq!(r.to_identifier(), "content.halo.run/Post/my-post");
    }

    #[test]
    fn condition_status_serializes_uppercase() {
        assert_eq!(
            serde_json::to_value(ConditionStatus::True).unwrap(),
            serde_json::Value::String("True".to_owned())
        );
    }

    #[test]
    fn condition_round_trips() {
        let c = Condition {
            r#type: "Ready".to_owned(),
            status: ConditionStatus::True,
            last_transition_time: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            message: "ok".to_owned(),
            reason: "Healthy".to_owned(),
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: Condition = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn condition_list_is_transparent() {
        let mut list = ConditionList::new();
        list.push(Condition {
            r#type: "Ready".to_owned(),
            status: ConditionStatus::False,
            last_transition_time: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            message: String::new(),
            reason: String::new(),
        });
        let v = serde_json::to_value(&list).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 1);
    }
}
