//! Kinds under `theme.halo.run/v1alpha1`.

use rblog_scheme::GroupVersionKind;
use serde::{Deserialize, Serialize};

use crate::infra::{Author, ConditionList, License};

const GROUP: &str = "theme.halo.run";
const VERSION: &str = "v1alpha1";

const THEME_GVK: GroupVersionKind =
    GroupVersionKind::new(GROUP, VERSION, "Theme", "themes", "theme");

define_kind!(
    /// Installed theme descriptor. Files live under `<work_dir>/themes/<name>/`.
    Theme,
    gvk = THEME_GVK,
    spec = ThemeSpec,
    status = ThemeStatus,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSpec {
    pub display_name: String,
    pub author: Author,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issues: Option<String>,
    #[serde(default = "wildcard_version")]
    pub version: String,
    #[serde(default = "wildcard_version")]
    pub requires: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setting_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_map_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<Vec<License>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_templates: Option<CustomTemplates>,
}

fn wildcard_version() -> String {
    "*".to_owned()
}

impl Default for ThemeSpec {
    fn default() -> Self {
        Self {
            display_name: String::new(),
            author: Author::default(),
            description: None,
            logo: None,
            homepage: None,
            repo: None,
            issues: None,
            version: wildcard_version(),
            requires: wildcard_version(),
            setting_name: None,
            config_map_name: None,
            license: None,
            custom_templates: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomTemplates {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<Vec<TemplateDescriptor>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Vec<TemplateDescriptor>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<Vec<TemplateDescriptor>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateDescriptor {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
    pub file: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ThemePhase {
    Ready,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<ThemePhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditions: Option<ConditionList>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn theme_defaults_to_wildcard_version() {
        let t = Theme::new("default").with_spec(ThemeSpec {
            display_name: "Default".to_owned(),
            author: Author {
                name: "rblog".to_owned(),
                website: None,
            },
            ..ThemeSpec::default()
        });
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["spec"]["version"], "*");
        assert_eq!(v["spec"]["requires"], "*");
        assert_eq!(v["apiVersion"], "theme.halo.run/v1alpha1");
    }
}
