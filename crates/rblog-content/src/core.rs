//! Core (empty-group) kinds: `User`, `Role`, `RoleBinding`, `Menu`, `MenuItem`,
//! `Setting`, `ConfigMap`, `Secret`.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use rblog_scheme::GroupVersionKind;
use serde::{Deserialize, Serialize};

use crate::infra::Ref;

const VERSION: &str = "v1alpha1";

// ---------------------------------------------------------------------------
// User
// ---------------------------------------------------------------------------

const USER_GVK: GroupVersionKind = GroupVersionKind::new("", VERSION, "User", "users", "user");

define_kind!(
    /// Account record. `spec.password` stores the argon2id hash; never plaintext.
    User,
    gvk = USER_GVK,
    spec = UserSpec,
    status = UserStatus,
);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSpec {
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    pub email: String,
    #[serde(default)]
    pub email_verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// Argon2id encoded hash. Stored as-is; never trimmed or normalized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registered_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub two_factor_auth_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub totp_encrypted_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_history_limit: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permalink: Option<String>,
}

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

const ROLE_GVK: GroupVersionKind = GroupVersionKind::new("", VERSION, "Role", "roles", "role");

/// Role is unusual — it doesn't have a `spec`; its policy rules are flat on the
/// object itself (Halo's Java class extends `AbstractExtension` and adds
/// `rules` directly). We mirror that with a hand-rolled struct rather than the
/// macro.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Role {
    #[serde(default)]
    pub api_version: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub metadata: rblog_scheme::Metadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<PolicyRule>>,
}

impl Role {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let gvk = <Self as rblog_scheme::Extension>::gvk();
        Self {
            api_version: gvk.api_version(),
            kind: gvk.kind.to_owned(),
            metadata: rblog_scheme::Metadata::new(name),
            rules: None,
        }
    }
}

impl rblog_scheme::Extension for Role {
    fn gvk() -> GroupVersionKind {
        ROLE_GVK
    }
    fn metadata(&self) -> &rblog_scheme::Metadata {
        &self.metadata
    }
    fn metadata_mut(&mut self) -> &mut rblog_scheme::Metadata {
        &mut self.metadata
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyRule {
    #[serde(default)]
    pub api_groups: Vec<String>,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub resource_names: Vec<String>,
    #[serde(default, rename = "nonResourceURLs")]
    pub non_resource_urls: Vec<String>,
    #[serde(default)]
    pub verbs: Vec<String>,
}

// ---------------------------------------------------------------------------
// RoleBinding
// ---------------------------------------------------------------------------

const ROLE_BINDING_GVK: GroupVersionKind =
    GroupVersionKind::new("", VERSION, "RoleBinding", "rolebindings", "rolebinding");

/// Same flat shape as `Role` — Halo's Java `RoleBinding` doesn't use a `spec`
/// wrapper either; `subjects` and `roleRef` sit on the object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleBinding {
    #[serde(default)]
    pub api_version: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub metadata: rblog_scheme::Metadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subjects: Option<Vec<Subject>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_ref: Option<RoleRef>,
}

impl RoleBinding {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let gvk = <Self as rblog_scheme::Extension>::gvk();
        Self {
            api_version: gvk.api_version(),
            kind: gvk.kind.to_owned(),
            metadata: rblog_scheme::Metadata::new(name),
            subjects: None,
            role_ref: None,
        }
    }
}

impl rblog_scheme::Extension for RoleBinding {
    fn gvk() -> GroupVersionKind {
        ROLE_BINDING_GVK
    }
    fn metadata(&self) -> &rblog_scheme::Metadata {
        &self.metadata
    }
    fn metadata_mut(&mut self) -> &mut rblog_scheme::Metadata {
        &mut self.metadata
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_group: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleRef {
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_group: Option<String>,
}

// ---------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------

const MENU_GVK: GroupVersionKind = GroupVersionKind::new("", VERSION, "Menu", "menus", "menu");

define_kind!(
    /// Named menu — references [`MenuItem`]s by name.
    Menu,
    gvk = MENU_GVK,
    spec = MenuSpec,
    status = MenuStatus,
);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuSpec {
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu_items: Option<BTreeSet<String>>,
}

/// Placeholder — Halo's Java `Menu` has no status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuStatus {}

// ---------------------------------------------------------------------------
// MenuItem
// ---------------------------------------------------------------------------

const MENU_ITEM_GVK: GroupVersionKind =
    GroupVersionKind::new("", VERSION, "MenuItem", "menuitems", "menuitem");

define_kind!(
    /// Single navigation entry.
    MenuItem,
    gvk = MENU_ITEM_GVK,
    spec = MenuItemSpec,
    status = MenuItemStatus,
);

/// Anchor `target` attribute. Halo serializes these as the literal strings
/// `_blank` / `_self` / `_parent` / `_top` via Jackson `@JsonValue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MenuTarget {
    #[serde(rename = "_blank")]
    Blank,
    #[serde(rename = "_self")]
    SelfTab,
    #[serde(rename = "_parent")]
    Parent,
    #[serde(rename = "_top")]
    Top,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuItemSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<MenuTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<BTreeSet<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<Ref>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuItemStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
}

// ---------------------------------------------------------------------------
// Setting
// ---------------------------------------------------------------------------

const SETTING_GVK: GroupVersionKind =
    GroupVersionKind::new("", VERSION, "Setting", "settings", "setting");

define_kind!(
    /// Settings *schema* — declares the form a theme/plugin needs configured.
    /// The actual values live in a paired [`ConfigMap`].
    Setting,
    gvk = SETTING_GVK,
    spec = SettingSpec,
    status = SettingStatus,
);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingSpec {
    pub forms: Vec<SettingForm>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingForm {
    pub group: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Halo stores this as a JSON array of arbitrary form-field objects.
    /// rblog passes the raw values through to `@rjsf` on the admin side.
    pub form_schema: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingStatus {}

// ---------------------------------------------------------------------------
// ConfigMap
// ---------------------------------------------------------------------------

const CONFIG_MAP_GVK: GroupVersionKind =
    GroupVersionKind::new("", VERSION, "ConfigMap", "configmaps", "configmap");

/// Halo's `ConfigMap` is flat — `data` sits on the object, not under `spec`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigMap {
    #[serde(default)]
    pub api_version: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub metadata: rblog_scheme::Metadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<BTreeMap<String, String>>,
}

impl ConfigMap {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let gvk = <Self as rblog_scheme::Extension>::gvk();
        Self {
            api_version: gvk.api_version(),
            kind: gvk.kind.to_owned(),
            metadata: rblog_scheme::Metadata::new(name),
            data: None,
        }
    }

    pub fn put(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.data
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value.into());
    }
}

impl rblog_scheme::Extension for ConfigMap {
    fn gvk() -> GroupVersionKind {
        CONFIG_MAP_GVK
    }
    fn metadata(&self) -> &rblog_scheme::Metadata {
        &self.metadata
    }
    fn metadata_mut(&mut self) -> &mut rblog_scheme::Metadata {
        &mut self.metadata
    }
}

// ---------------------------------------------------------------------------
// Secret
// ---------------------------------------------------------------------------

const SECRET_GVK: GroupVersionKind =
    GroupVersionKind::new("", VERSION, "Secret", "secrets", "secret");

/// Halo's `Secret` is also flat. The `data` map's values are base64-encoded
/// raw bytes on the Java side; we keep them as `String` (base64).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secret {
    #[serde(default)]
    pub api_version: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub metadata: rblog_scheme::Metadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// Base64-encoded values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<BTreeMap<String, String>>,
    /// Write-only convenience field; Halo never echoes it on read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string_data: Option<BTreeMap<String, String>>,
}

impl Secret {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let gvk = <Self as rblog_scheme::Extension>::gvk();
        Self {
            api_version: gvk.api_version(),
            kind: gvk.kind.to_owned(),
            metadata: rblog_scheme::Metadata::new(name),
            r#type: Some("Opaque".to_owned()),
            data: None,
            string_data: None,
        }
    }
}

impl rblog_scheme::Extension for Secret {
    fn gvk() -> GroupVersionKind {
        SECRET_GVK
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
    use rblog_scheme::Extension;

    #[test]
    fn user_apiversion_has_no_group() {
        let u = User::new("admin").with_spec(UserSpec {
            display_name: "Admin".to_owned(),
            email: "admin@example.com".to_owned(),
            ..UserSpec::default()
        });
        let v = serde_json::to_value(&u).unwrap();
        // Halo emits `apiVersion = "v1alpha1"` for groupless kinds.
        assert_eq!(v["apiVersion"], "v1alpha1");
        assert_eq!(v["kind"], "User");
        assert_eq!(v["spec"]["displayName"], "Admin");
        assert_eq!(v["spec"]["email"], "admin@example.com");
    }

    #[test]
    fn menu_item_target_serializes_with_underscore_prefix() {
        let mi = MenuItem::new("about").with_spec(MenuItemSpec {
            display_name: Some("About".to_owned()),
            href: Some("/about".to_owned()),
            target: Some(MenuTarget::Blank),
            priority: Some(10),
            children: None,
            target_ref: None,
        });
        let v = serde_json::to_value(&mi).unwrap();
        assert_eq!(v["spec"]["target"], "_blank");
    }

    #[test]
    fn role_is_flat_no_spec_wrapper() {
        let mut role = Role::new("super-admin");
        role.rules = Some(vec![PolicyRule {
            api_groups: vec!["content.halo.run".to_owned()],
            resources: vec!["posts".to_owned()],
            resource_names: vec![],
            non_resource_urls: vec![],
            verbs: vec!["*".to_owned()],
        }]);
        let v = serde_json::to_value(&role).unwrap();
        assert!(v.get("spec").is_none());
        assert_eq!(v["rules"][0]["apiGroups"][0], "content.halo.run");
        assert_eq!(v["rules"][0]["verbs"][0], "*");
    }

    #[test]
    fn role_binding_subject_and_ref() {
        let mut rb = RoleBinding::new("admin-super-admin-binding");
        rb.subjects = Some(vec![Subject {
            kind: "User".to_owned(),
            name: "admin".to_owned(),
            api_group: Some(String::new()),
        }]);
        rb.role_ref = Some(RoleRef {
            kind: "Role".to_owned(),
            name: "super-admin".to_owned(),
            api_group: Some(String::new()),
        });
        let v = serde_json::to_value(&rb).unwrap();
        assert_eq!(v["subjects"][0]["kind"], "User");
        assert_eq!(v["roleRef"]["name"], "super-admin");
    }

    #[test]
    fn configmap_data_is_flat() {
        let mut cm = ConfigMap::new("system");
        cm.put("post.title", "Blog");
        let v = serde_json::to_value(&cm).unwrap();
        assert_eq!(v["data"]["post.title"], "Blog");
        assert!(v.get("spec").is_none());
    }

    #[test]
    fn secret_defaults_opaque() {
        let s = Secret::new("session-key");
        assert_eq!(s.r#type.as_deref(), Some("Opaque"));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["type"], "Opaque");
    }

    #[test]
    fn deserialize_halo_user_payload() {
        let raw = r#"{
            "apiVersion": "v1alpha1",
            "kind": "User",
            "metadata": { "name": "admin", "version": 2 },
            "spec": {
                "displayName": "Admin",
                "email": "admin@example.com",
                "emailVerified": true,
                "password": "$argon2id$v=19$m=65536,t=3,p=4$..."
            }
        }"#;
        let u: User = serde_json::from_str(raw).unwrap();
        let s = u.spec.unwrap();
        assert_eq!(s.display_name, "Admin");
        assert_eq!(s.email, "admin@example.com");
        assert!(s.email_verified);
        assert!(s.password.as_deref().unwrap().starts_with("$argon2id$"));
    }

    #[test]
    fn setting_form_schema_passes_through_json() {
        let s = Setting::new("theme-foo").with_spec(SettingSpec {
            forms: vec![SettingForm {
                group: "general".to_owned(),
                label: Some("General".to_owned()),
                form_schema: vec![serde_json::json!({"type": "text", "name": "siteTitle"})],
            }],
        });
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["spec"]["forms"][0]["group"], "general");
        assert_eq!(v["spec"]["forms"][0]["formSchema"][0]["type"], "text");
    }

    #[test]
    fn all_kinds_have_consistent_gvk() {
        // Sanity: registry knows about every kind from this module.
        for (kind, plural) in [
            ("User", "users"),
            ("Role", "roles"),
            ("RoleBinding", "rolebindings"),
            ("Menu", "menus"),
            ("MenuItem", "menuitems"),
            ("Setting", "settings"),
            ("ConfigMap", "configmaps"),
            ("Secret", "secrets"),
        ] {
            let _ = (kind, plural);
        }
        assert_eq!(User::gvk().group, "");
        assert_eq!(ConfigMap::gvk().plural, "configmaps");
    }
}
