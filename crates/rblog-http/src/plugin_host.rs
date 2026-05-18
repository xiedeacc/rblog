//! Host-side glue for the WASM plugin runtime.
//!
//! Bridges [`rblog_plugins::HostServices`] to the actual rblog stack:
//!
//! - `posts:read` → number of entries the in-memory `Post` kind holds.
//! - `settings:read` → backed by a periodically-refreshed snapshot of
//!   the `system` ConfigMap. Plugins only ever see what the admin chose
//!   to expose; secrets never make it into this map.
//! - `kv` → per-plugin in-memory `BTreeMap`. Survives across requests
//!   within a process but not across restarts. A SQLite-backed
//!   implementation can land in a follow-up commit; the in-memory map
//!   already gives plugins a working contract.
//! - `log` → the default `HostServices::log`, which forwards to
//!   `tracing` with the plugin name as a span field.

use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::RwLock;
use rblog_content::content::Post;
use rblog_core::Services;
use rblog_plugins::HostServices;
use rblog_scheme::Extension;

/// Wires plugin host calls onto rblog services.
pub struct RblogHost {
    services: Services,
    /// Snapshot of public system settings refreshed on demand.
    settings: RwLock<BTreeMap<String, String>>,
    kv: RwLock<BTreeMap<(String, String), Vec<u8>>>,
}

impl RblogHost {
    pub fn new(services: Services) -> Self {
        Self {
            services,
            settings: RwLock::new(BTreeMap::new()),
            kv: RwLock::new(BTreeMap::new()),
        }
    }

    /// Replace the cached system settings snapshot. Called at boot and
    /// after admin settings mutations.
    pub fn refresh_settings(&self, data: BTreeMap<String, String>) {
        *self.settings.write() = data;
    }

    pub fn into_arc(self) -> Arc<dyn HostServices> {
        Arc::new(self)
    }
}

impl HostServices for RblogHost {
    fn posts_count(&self) -> i32 {
        let count = self.services.index.entry_count(&Post::gvk());
        i32::try_from(count).unwrap_or(i32::MAX)
    }

    fn setting_get(&self, key: &str) -> Option<String> {
        self.settings.read().get(key).cloned()
    }

    fn kv_get(&self, plugin: &str, key: &str) -> Option<Vec<u8>> {
        self.kv
            .read()
            .get(&(plugin.to_owned(), key.to_owned()))
            .cloned()
    }

    fn kv_set(&self, plugin: &str, key: &str, value: &[u8]) {
        self.kv
            .write()
            .insert((plugin.to_owned(), key.to_owned()), value.to_vec());
    }
}
