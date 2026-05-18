//! Capability model for WASM plugins.
//!
//! Capabilities are an explicit allow-list of host operations a plugin
//! declares it needs in its `plugin.toml`. The runtime grants exactly
//! what's declared and refuses anything else. Capabilities are
//! orthogonal: a plugin can hold any combination.
//!
//! v1 capabilities:
//!
//! - `log`: write structured log records via the host's `host::log` import.
//! - `kv`: read/write key/value pairs scoped to the plugin's namespace.
//! - `http`: respond to incoming HTTP requests mounted under
//!   `/api/plugins/<plugin>/<route_prefix>/*`.
//! - `posts:read`: read post metadata/content.
//! - `settings:read`: read non-secret ConfigMap entries.
//!
//! Capabilities are deliberately granular. New ones can be added by
//! extending [`Capability`] and the parser in
//! [`CapabilitySet::from_strings`].

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    Log,
    Kv,
    Http,
    PostsRead,
    SettingsRead,
}

impl Capability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Log => "log",
            Self::Kv => "kv",
            Self::Http => "http",
            Self::PostsRead => "posts:read",
            Self::SettingsRead => "settings:read",
        }
    }

    pub fn parse(s: &str) -> Result<Self, CapabilityError> {
        match s.trim() {
            "log" => Ok(Self::Log),
            "kv" => Ok(Self::Kv),
            "http" => Ok(Self::Http),
            "posts:read" => Ok(Self::PostsRead),
            "settings:read" => Ok(Self::SettingsRead),
            other => Err(CapabilityError::Unknown(other.to_owned())),
        }
    }
}

#[derive(Debug, Error)]
pub enum CapabilityError {
    #[error("unknown capability `{0}`")]
    Unknown(String),
    #[error("plugin missing capability `{0}`")]
    Missing(&'static str),
}

/// A plugin's declared capability set. Cheap to clone (`BTreeSet`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitySet(pub BTreeSet<Capability>);

impl CapabilitySet {
    pub fn from_strings<I, S>(values: I) -> Result<Self, CapabilityError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut set = BTreeSet::new();
        for raw in values {
            set.insert(Capability::parse(raw.as_ref())?);
        }
        Ok(Self(set))
    }

    pub fn allows(&self, cap: Capability) -> bool {
        self.0.contains(&cap)
    }

    /// Returns the capability if granted, otherwise [`CapabilityError::Missing`].
    pub fn require(&self, cap: Capability) -> Result<(), CapabilityError> {
        if self.allows(cap) {
            Ok(())
        } else {
            Err(CapabilityError::Missing(cap.as_str()))
        }
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.0.iter().map(Capability::as_str).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_capabilities() {
        let set = CapabilitySet::from_strings(["log", "kv", "http"]).unwrap();
        assert!(set.allows(Capability::Log));
        assert!(set.allows(Capability::Kv));
        assert!(set.allows(Capability::Http));
        assert!(!set.allows(Capability::PostsRead));
    }

    #[test]
    fn rejects_unknown_capability() {
        let err = Capability::parse("delete-everything").unwrap_err();
        assert!(matches!(err, CapabilityError::Unknown(_)));
    }

    #[test]
    fn require_fails_when_missing() {
        let set = CapabilitySet::from_strings(["log"]).unwrap();
        assert!(set.require(Capability::Log).is_ok());
        let err = set.require(Capability::Kv).unwrap_err();
        assert!(matches!(err, CapabilityError::Missing("kv")));
    }
}
