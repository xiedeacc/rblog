//! Host-side services callable from inside WASM plugins.
//!
//! Plugins reach into the host through a tiny, capability-gated surface
//! defined in [`HostServices`]. Each capability ([`Capability`]) maps to
//! a specific subset of methods on this trait:
//!
//! - `posts:read`     →  [`HostServices::posts_count`]
//! - `settings:read`  →  [`HostServices::setting_get`]
//! - `kv`             →  [`HostServices::kv_get`] / [`HostServices::kv_set`]
//! - `log`            →  [`HostServices::log`]
//!
//! The host is expected to be a process-wide singleton (the HTTP layer
//! injects an `Arc<dyn HostServices>` at runtime construction time). For
//! tests we provide [`NoopHost`] which returns sensible empty answers
//! for every method.
//!
//! All methods take `&self` because they may be invoked from any thread
//! that wasmtime decides to run the plugin on. Implementations must be
//! `Send + Sync + 'static`.
//!
//! [`Capability`]: crate::capability::Capability

use std::sync::Arc;

use parking_lot::Mutex;
use std::collections::BTreeMap;

/// Host-side surface exposed to plugins via the WASM ABI.
pub trait HostServices: Send + Sync + 'static {
    /// Number of published posts. Cheap to compute (already cached by
    /// the in-memory index).
    fn posts_count(&self) -> i32 {
        0
    }

    /// Look up a non-secret system setting. Returns `None` for unknown
    /// keys or anything the host considers sensitive.
    fn setting_get(&self, _key: &str) -> Option<String> {
        None
    }

    /// Look up a per-plugin KV pair. Keys are scoped by the plugin name
    /// so two plugins never collide on the same key.
    fn kv_get(&self, plugin: &str, key: &str) -> Option<Vec<u8>>;

    /// Write a per-plugin KV pair.
    fn kv_set(&self, plugin: &str, key: &str, value: &[u8]);

    /// Structured log record emitted by a plugin via `host_log`.
    ///
    /// `level` is one of `0=trace, 1=debug, 2=info, 3=warn, 4=error`;
    /// any other value is treated as `info`.
    fn log(&self, plugin: &str, level: i32, message: &str) {
        let lvl = match level {
            0 => tracing::Level::TRACE,
            1 => tracing::Level::DEBUG,
            3 => tracing::Level::WARN,
            4 => tracing::Level::ERROR,
            _ => tracing::Level::INFO,
        };
        match lvl {
            tracing::Level::TRACE => tracing::trace!(plugin, "{message}"),
            tracing::Level::DEBUG => tracing::debug!(plugin, "{message}"),
            tracing::Level::WARN => tracing::warn!(plugin, "{message}"),
            tracing::Level::ERROR => tracing::error!(plugin, "{message}"),
            _ => tracing::info!(plugin, "{message}"),
        }
    }
}

/// A no-op host implementation used in tests and as the default when
/// the HTTP layer has not yet wired one in.
#[derive(Default)]
pub struct NoopHost {
    kv: Mutex<BTreeMap<(String, String), Vec<u8>>>,
}

impl HostServices for NoopHost {
    fn kv_get(&self, plugin: &str, key: &str) -> Option<Vec<u8>> {
        self.kv
            .lock()
            .get(&(plugin.to_owned(), key.to_owned()))
            .cloned()
    }

    fn kv_set(&self, plugin: &str, key: &str, value: &[u8]) {
        self.kv
            .lock()
            .insert((plugin.to_owned(), key.to_owned()), value.to_vec());
    }
}

/// Convenience type alias: an `Arc<dyn HostServices>` that's cheap to clone.
pub type HostHandle = Arc<dyn HostServices>;

/// Build a [`HostHandle`] pointing at a fresh in-memory [`NoopHost`].
pub fn noop() -> HostHandle {
    Arc::new(NoopHost::default())
}
