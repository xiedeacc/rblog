//! Stable host ↔ guest ABI for v1 plugins.
//!
//! Plugins are pure core WASM modules (the runtime keeps the engine
//! configured for the component model too, but core modules are the
//! simplest target a beginner-friendly plugin can compile to).
//!
//! # Required exports
//!
//! - `memory`: the plugin's default linear memory.
//! - `alloc(size: i32) -> i32`: allocate `size` bytes, return a pointer
//!   to a buffer the host may write into. Returning `0` is treated as
//!   out-of-memory.
//! - `handle(method_ptr, method_len, path_ptr, path_len,
//!           body_ptr, body_len) -> i64`:
//!   the request entry point. Returns a packed
//!   `(ptr << 32) | len` pointer into a guest buffer containing a
//!   utf-8 JSON [`PluginResponse`]. Returning `0` is treated as an empty
//!   `204 No Content` response.
//!
//! # Host imports (namespace `env`)
//!
//! Each import is gated by a [`Capability`](crate::capability::Capability).
//! If the plugin's manifest does not declare the corresponding capability
//! the import will trap with a clear error message instead of executing.
//!
//! - `host_log(level, msg_ptr, msg_len)` — `log`
//! - `host_kv_get(key_ptr, key_len) -> i64` — `kv`
//! - `host_kv_set(key_ptr, key_len, val_ptr, val_len)` — `kv`
//! - `host_posts_count() -> i32` — `posts:read`
//! - `host_setting_get(key_ptr, key_len) -> i64` — `settings:read`
//!
//! Any `host_*_get` returning a non-zero packed pointer means the
//! buffer was allocated inside the guest via `alloc` and the host has
//! written the bytes. `0` means "not found".
//!
//! # Why core WASM and not the component model?
//!
//! Core modules build with `cargo build --target wasm32-unknown-unknown`
//! out of the box, no `wasm-tools` or `wit-bindgen` required. Authors
//! can write plugins in Rust, AssemblyScript, or even raw `wat`. The
//! runtime keeps `Engine::async_support` on and `wasm_component_model`
//! enabled so we can add a component-model-backed v2 ABI without
//! re-initialising the engine.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// JSON response shape a plugin must return from `handle`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginResponse {
    /// HTTP status code. Defaults to `200`.
    #[serde(default = "default_status")]
    pub status: u16,
    /// Response content type. Defaults to `text/plain; charset=utf-8`.
    #[serde(default = "default_content_type", rename = "content_type")]
    pub content_type: String,
    /// Extra response headers. Empty map by default.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Response body as a utf-8 string.
    #[serde(default)]
    pub body: String,
}

impl PluginResponse {
    pub fn ok(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            content_type: default_content_type(),
            headers: BTreeMap::new(),
            body: body.into(),
        }
    }
}

fn default_status() -> u16 {
    200
}
fn default_content_type() -> String {
    "text/plain; charset=utf-8".to_owned()
}

/// Request the host hands to the plugin's `handle` export. This is
/// mainly a developer-facing alias — the ABI passes the three string
/// fields directly so the guest doesn't need a JSON parser to dispatch.
#[derive(Debug, Clone)]
pub struct PluginRequest {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
}

impl PluginRequest {
    pub fn new(method: impl Into<String>, path: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            body,
        }
    }

    pub fn get(path: impl Into<String>) -> Self {
        Self::new("GET", path, Vec::new())
    }
}

/// Pack `(ptr, len)` into the i64 the ABI uses.
pub(crate) fn pack(ptr: u32, len: u32) -> i64 {
    (i64::from(ptr) << 32) | i64::from(len)
}

/// Inverse of [`pack`].
pub(crate) fn unpack(v: i64) -> (u32, u32) {
    #[allow(clippy::cast_sign_loss)]
    let u = v as u64;
    let ptr = (u >> 32) as u32;
    let len = (u & 0xFFFF_FFFF) as u32;
    (ptr, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_roundtrips() {
        let cases = [(0_u32, 0_u32), (1, 2), (0xDEAD, 0xBEEF), (u32::MAX, 1)];
        for (p, l) in cases {
            let (pp, ll) = unpack(pack(p, l));
            assert_eq!((pp, ll), (p, l));
        }
    }

    #[test]
    fn response_defaults() {
        let r: PluginResponse = serde_json::from_str("{}").unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.content_type, "text/plain; charset=utf-8");
        assert!(r.body.is_empty());
    }

    #[test]
    fn response_parses_full() {
        let r: PluginResponse = serde_json::from_str(
            r#"{"status":201,"content_type":"application/json","headers":{"x":"y"},"body":"{}"}"#,
        )
        .unwrap();
        assert_eq!(r.status, 201);
        assert_eq!(r.content_type, "application/json");
        assert_eq!(r.headers.get("x").map(String::as_str), Some("y"));
    }
}
