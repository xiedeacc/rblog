//! Plugin manifest parsing.
//!
//! Each plugin ships a `plugin.toml` next to its `plugin.wasm`. Example:
//!
//! ```toml
//! name = "hello-world"
//! display_name = "Hello, world!"
//! version = "0.1.0"
//! description = "Trivial example plugin"
//! authors = ["me@example.com"]
//! entry = "plugin.wasm"
//! enabled = true
//!
//! capabilities = ["log", "http"]
//!
//! [[routes]]
//! path = "/greet"
//! methods = ["GET"]
//! ```
//!
//! `entry` is resolved relative to the plugin's directory and defaults to
//! `plugin.wasm`. `enabled = false` keeps the plugin on disk but skips
//! loading it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::capability::{CapabilityError, CapabilitySet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Raw capability names. Validated via [`CapabilitySet::from_strings`] when
    /// loading the manifest into the runtime.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// HTTP route prefixes the plugin handles. Mounted under
    /// `/api/plugins/<name>/<path>`.
    #[serde(default)]
    pub routes: Vec<RouteMount>,
}

fn default_version() -> String {
    "0.0.0".into()
}
fn default_entry() -> String {
    "plugin.wasm".into()
}
fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteMount {
    /// Path under `/api/plugins/<name>/`. A leading `/` is optional.
    pub path: String,
    /// HTTP methods to accept. Defaults to `["GET"]`.
    #[serde(default = "default_methods")]
    pub methods: Vec<String>,
}

fn default_methods() -> Vec<String> {
    vec!["GET".to_owned()]
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("plugin directory `{0}` is missing a manifest (plugin.toml)")]
    MissingManifest(PathBuf),
    #[error("read manifest `{path}`: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("parse manifest `{path}`: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("manifest `{path}`: {source}")]
    Capability {
        path: PathBuf,
        source: CapabilityError,
    },
    #[error("manifest `{path}`: plugin name `{declared}` does not match directory `{dir}`")]
    NameMismatch {
        path: PathBuf,
        declared: String,
        dir: String,
    },
    #[error("manifest `{path}`: entry `{entry}` does not exist")]
    MissingEntry { path: PathBuf, entry: String },
}

/// Parse + validate a manifest in a plugin directory.
pub fn load_manifest(dir: &Path) -> Result<(Manifest, CapabilitySet, PathBuf), LoadError> {
    let manifest_path = dir.join("plugin.toml");
    if !manifest_path.exists() {
        return Err(LoadError::MissingManifest(dir.to_path_buf()));
    }
    let body = std::fs::read_to_string(&manifest_path).map_err(|source| LoadError::Read {
        path: manifest_path.clone(),
        source,
    })?;
    let manifest: Manifest = toml::from_str(&body).map_err(|source| LoadError::Parse {
        path: manifest_path.clone(),
        source,
    })?;

    if let Some(stem) = dir.file_name().and_then(|n| n.to_str()) {
        if stem != manifest.name {
            return Err(LoadError::NameMismatch {
                path: manifest_path.clone(),
                declared: manifest.name.clone(),
                dir: stem.to_owned(),
            });
        }
    }

    let caps = CapabilitySet::from_strings(&manifest.capabilities).map_err(|source| {
        LoadError::Capability {
            path: manifest_path.clone(),
            source,
        }
    })?;

    let entry_path = dir.join(&manifest.entry);
    if manifest.enabled && !entry_path.exists() {
        return Err(LoadError::MissingEntry {
            path: manifest_path,
            entry: manifest.entry.clone(),
        });
    }

    Ok((manifest, caps, entry_path))
}

impl RouteMount {
    pub fn normalized_path(&self) -> String {
        let trimmed = self.path.trim_start_matches('/');
        format!("/{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_manifest() {
        let toml = r#"
            name = "hello"
            entry = "plugin.wasm"
            enabled = true
            capabilities = ["log"]
        "#;
        let m: Manifest = toml::from_str(toml).unwrap();
        assert_eq!(m.name, "hello");
        assert_eq!(m.entry, "plugin.wasm");
        assert!(m.enabled);
        assert_eq!(m.capabilities, vec!["log"]);
    }

    #[test]
    fn route_default_method_is_get() {
        let toml = r#"
            name = "h"
            [[routes]]
            path = "/greet"
        "#;
        let m: Manifest = toml::from_str(toml).unwrap();
        assert_eq!(m.routes[0].methods, vec!["GET"]);
        assert_eq!(m.routes[0].normalized_path(), "/greet");
    }

    #[test]
    fn load_manifest_fails_on_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let err = load_manifest(tmp.path()).unwrap_err();
        assert!(matches!(err, LoadError::MissingManifest(_)));
    }

    #[test]
    fn load_manifest_rejects_name_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("alpha");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin.toml"), r#"name = "beta""#).unwrap();
        let err = load_manifest(&dir).unwrap_err();
        assert!(matches!(err, LoadError::NameMismatch { .. }));
    }

    #[test]
    fn load_manifest_disabled_skips_entry_check() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("alpha");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.toml"),
            "name = \"alpha\"\nenabled = false\nentry = \"missing.wasm\"\n",
        )
        .unwrap();
        let (m, _caps, _entry) = load_manifest(&dir).unwrap();
        assert!(!m.enabled);
    }
}
