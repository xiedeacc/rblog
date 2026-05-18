//! Kinds under `plugin.halo.run/v1alpha1`.
//!
//! Currently only the [`Plugin`] descriptor itself; the wasmtime runtime
//! lives in `rblog-plugins` and uses these descriptors as its inventory.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rblog_scheme::GroupVersionKind;
use serde::{Deserialize, Serialize};

use crate::infra::{ConditionList, License};

const GROUP: &str = "plugin.halo.run";
const VERSION: &str = "v1alpha1";

const PLUGIN_GVK: GroupVersionKind =
    GroupVersionKind::new(GROUP, VERSION, "Plugin", "plugins", "plugin");

define_kind!(
    /// Plugin descriptor stored in the registry. The actual `.wasm` file lives
    /// at `<work_dir>/plugins/<name>/plugin.wasm`.
    Plugin,
    gvk = PLUGIN_GVK,
    spec = PluginSpec,
    status = PluginStatus,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// SemVer string.
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<PluginAuthor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugin_dependencies: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issues: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<Vec<License>>,
    #[serde(default = "wildcard")]
    pub requires: String,
    /// Whether the user has chosen to enable this plugin. The runtime may
    /// independently flag it `errored` (in `status.phase`).
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setting_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_map_name: Option<String>,
}

fn wildcard() -> String {
    "*".to_owned()
}

impl Default for PluginSpec {
    fn default() -> Self {
        Self {
            display_name: None,
            version: String::new(),
            author: None,
            logo: None,
            plugin_dependencies: BTreeMap::new(),
            homepage: None,
            repo: None,
            issues: None,
            description: None,
            license: None,
            requires: wildcard(),
            enabled: false,
            setting_name: None,
            config_map_name: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginAuthor {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PluginPhase {
    Pending,
    Starting,
    Created,
    Disabling,
    Disabled,
    Resolved,
    Started,
    Stopped,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<PluginPhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditions: Option<ConditionList>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_start_time: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stylesheet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_location: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn plugin_defaults() {
        let p = Plugin::new("hello").with_spec(PluginSpec {
            display_name: Some("Hello".to_owned()),
            version: "0.1.0".to_owned(),
            ..PluginSpec::default()
        });
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["apiVersion"], "plugin.halo.run/v1alpha1");
        assert_eq!(v["spec"]["version"], "0.1.0");
        assert_eq!(v["spec"]["requires"], "*");
        assert_eq!(v["spec"]["enabled"], false);
    }
}
