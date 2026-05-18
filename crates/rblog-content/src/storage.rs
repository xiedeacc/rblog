//! Kinds under `storage.halo.run/v1alpha1`: attachments and storage policies.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use rblog_scheme::GroupVersionKind;
use serde::{Deserialize, Serialize};

const GROUP: &str = "storage.halo.run";
const VERSION: &str = "v1alpha1";

// ---------------------------------------------------------------------------
// Attachment
// ---------------------------------------------------------------------------

const ATTACHMENT_GVK: GroupVersionKind =
    GroupVersionKind::new(GROUP, VERSION, "Attachment", "attachments", "attachment");

define_kind!(
    /// One uploaded file. The actual bytes live in whatever storage the
    /// referenced [`Policy`] points at.
    Attachment,
    gvk = ATTACHMENT_GVK,
    spec = AttachmentSpec,
    status = AttachmentStatus,
);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<BTreeSet<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permalink: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnails: Option<BTreeMap<String, String>>,
}

// ---------------------------------------------------------------------------
// Attachment Group (a folder)
// ---------------------------------------------------------------------------

const ATTACHMENT_GROUP_GVK: GroupVersionKind =
    GroupVersionKind::new(GROUP, VERSION, "Group", "groups", "group");

define_kind!(
    /// Attachment grouping ("folder").
    AttachmentGroup,
    gvk = ATTACHMENT_GROUP_GVK,
    spec = AttachmentGroupSpec,
    status = AttachmentGroupStatus,
);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentGroupSpec {
    pub display_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentGroupStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_timestamp: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_attachments: Option<i64>,
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

const POLICY_GVK: GroupVersionKind =
    GroupVersionKind::new(GROUP, VERSION, "Policy", "policies", "policy");

define_kind!(
    /// Concrete storage backend instance (e.g. "local-uploads", "s3-prod").
    Policy,
    gvk = POLICY_GVK,
    spec = PolicySpec,
    status = PolicyStatus,
);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicySpec {
    pub display_name: String,
    /// Reference name of a [`PolicyTemplate`].
    pub template_name: String,
    /// Reference name of a `ConfigMap` holding the policy's settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_map_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyStatus {}

// ---------------------------------------------------------------------------
// PolicyTemplate
// ---------------------------------------------------------------------------

const POLICY_TEMPLATE_GVK: GroupVersionKind = GroupVersionKind::new(
    GROUP,
    VERSION,
    "PolicyTemplate",
    "policytemplates",
    "policytemplate",
);

define_kind!(
    /// Declares a storage-backend *type* (e.g. "local", "s3"). Concrete
    /// `Policy` instances pick a `PolicyTemplate` by name.
    PolicyTemplate,
    gvk = POLICY_TEMPLATE_GVK,
    spec = PolicyTemplateSpec,
    status = PolicyTemplateStatus,
);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyTemplateSpec {
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setting_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyTemplateStatus {}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rblog_scheme::Extension;

    #[test]
    fn attachment_gvk_is_grouped() {
        assert_eq!(Attachment::gvk().group, "storage.halo.run");
        assert_eq!(Attachment::gvk().plural, "attachments");
    }

    #[test]
    fn attachment_wire_shape() {
        let a = Attachment::new("img-1").with_spec(AttachmentSpec {
            display_name: Some("photo.jpg".to_owned()),
            group_name: Some("photos".to_owned()),
            policy_name: Some("local".to_owned()),
            owner_name: Some("admin".to_owned()),
            media_type: Some("image/jpeg".to_owned()),
            size: Some(204_800),
            tags: None,
        });
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(v["apiVersion"], "storage.halo.run/v1alpha1");
        assert_eq!(v["spec"]["displayName"], "photo.jpg");
        assert_eq!(v["spec"]["mediaType"], "image/jpeg");
        assert_eq!(v["spec"]["size"], 204_800);
    }
}
