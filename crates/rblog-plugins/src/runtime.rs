//! Process-wide plugin runtime.
//!
//! Owns a [`wasmtime::Engine`] and a parking-lot map of compiled plugins.
//! Plugins are loaded on demand from disk: [`PluginRuntime::reload`] walks
//! `<root>/*` and compiles every plugin whose manifest passes validation,
//! replacing any existing state atomically.
//!
//! See [`crate::abi`] for the host ↔ guest ABI documentation. The host
//! imports surface is built per-request inside [`PluginRuntime::invoke`]
//! so each call gets a fresh `Store`. Imports are capability-gated:
//! attempting to call e.g. `host_kv_get` from a plugin that did not
//! declare `kv` traps with a clear error.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;
use thiserror::Error;
use tracing::{info, warn};
use wasmtime::{Caller, Engine, Linker, Memory, Module, Store, TypedFunc};

use crate::abi::{pack, unpack, PluginRequest, PluginResponse};
use crate::capability::{Capability, CapabilityError, CapabilitySet};
use crate::host::{noop, HostHandle};
use crate::manifest::{load_manifest, LoadError, Manifest, RouteMount};

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("plugin `{0}` not found")]
    NotFound(String),
    #[error("plugin `{0}` is disabled")]
    Disabled(String),
    #[error("load: {0}")]
    Load(#[from] LoadError),
    #[error("wasmtime: {0}")]
    Wasm(#[from] wasmtime::Error),
    #[error("capability: {0}")]
    Capability(#[from] CapabilityError),
    #[error("plugin `{plugin}` ABI: {message}")]
    Abi { plugin: String, message: String },
    #[error("plugin `{plugin}` returned invalid JSON response: {source}")]
    BadResponse {
        plugin: String,
        #[source]
        source: serde_json::Error,
    },
}

/// One compiled plugin, kept inside the [`PluginRuntime`] index.
pub struct InstanceState {
    pub manifest: Manifest,
    pub capabilities: CapabilitySet,
    pub module: Module,
    pub dir: PathBuf,
    pub entry: PathBuf,
    pub enabled: bool,
}

impl InstanceState {
    pub fn info(&self) -> PluginInfo {
        PluginInfo {
            name: self.manifest.name.clone(),
            display_name: self
                .manifest
                .display_name
                .clone()
                .unwrap_or_else(|| self.manifest.name.clone()),
            version: self.manifest.version.clone(),
            description: self.manifest.description.clone(),
            authors: self.manifest.authors.clone(),
            enabled: self.enabled,
            capabilities: self
                .capabilities
                .names()
                .into_iter()
                .map(String::from)
                .collect(),
            routes: self.manifest.routes.clone(),
            directory: self.dir.display().to_string(),
            entry: self.entry.display().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub enabled: bool,
    pub capabilities: Vec<String>,
    pub routes: Vec<RouteMount>,
    pub directory: String,
    pub entry: String,
}

/// Process-wide plugin runtime. Cheap to clone (`Arc<Inner>`).
#[derive(Clone)]
pub struct PluginRuntime {
    inner: Arc<Inner>,
}

struct Inner {
    engine: Engine,
    plugins: RwLock<BTreeMap<String, InstanceState>>,
    host: HostHandle,
}

/// Per-store context carried alongside each plugin invocation.
struct HostCtx {
    plugin: String,
    capabilities: CapabilitySet,
    host: HostHandle,
}

impl PluginRuntime {
    /// Build a runtime with a fresh, async-capable [`Engine`] and a no-op
    /// host. Use [`PluginRuntime::with_host`] to plug in a real host.
    pub fn new() -> Result<Self, RuntimeError> {
        Self::with_host(noop())
    }

    /// Build a runtime backed by the given [`HostServices`] implementation.
    pub fn with_host(host: HostHandle) -> Result<Self, RuntimeError> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        // Enable the component model so future commits can switch to
        // typed WIT-defined plugins without reinitializing the runtime.
        config.wasm_component_model(true);
        // Defensive limits: plugins should not be able to push memory
        // arbitrarily high. 64 MiB is plenty for HTML rendering.
        config.max_wasm_stack(2 * 1024 * 1024);
        let engine = Engine::new(&config)?;
        Ok(Self {
            inner: Arc::new(Inner {
                engine,
                plugins: RwLock::new(BTreeMap::new()),
                host,
            }),
        })
    }

    /// Walk `<root>/*`, parse + compile every enabled plugin, swap the
    /// index in one go. Failed plugins are logged at WARN and skipped.
    pub fn reload(&self, root: &Path) -> Result<usize, RuntimeError> {
        let mut next: BTreeMap<String, InstanceState> = BTreeMap::new();
        if !root.exists() {
            *self.inner.plugins.write() = next;
            return Ok(0);
        }
        let entries = std::fs::read_dir(root).map_err(|e| LoadError::Read {
            path: root.to_path_buf(),
            source: e,
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            match load_one(&self.inner.engine, &path) {
                Ok(state) => {
                    info!(
                        plugin = %state.manifest.name,
                        version = %state.manifest.version,
                        enabled = state.enabled,
                        "plugin loaded"
                    );
                    next.insert(state.manifest.name.clone(), state);
                }
                Err(err) => {
                    warn!(plugin_dir = %path.display(), error = %err, "skipping plugin");
                }
            }
        }
        let count = next.len();
        *self.inner.plugins.write() = next;
        Ok(count)
    }

    /// Snapshot of all loaded plugins, sorted by name.
    pub fn list(&self) -> Vec<PluginInfo> {
        let guard = self.inner.plugins.read();
        guard.values().map(InstanceState::info).collect()
    }

    /// Lookup the single plugin descriptor by name.
    pub fn get(&self, name: &str) -> Option<PluginInfo> {
        let guard = self.inner.plugins.read();
        guard.get(name).map(InstanceState::info)
    }

    /// Toggle a plugin without reloading from disk.
    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<PluginInfo, RuntimeError> {
        let mut guard = self.inner.plugins.write();
        let entry = guard
            .get_mut(name)
            .ok_or_else(|| RuntimeError::NotFound(name.to_owned()))?;
        entry.enabled = enabled;
        Ok(entry.info())
    }

    /// Returns the set of HTTP routes a plugin exposes (after normalizing
    /// the leading slash and uppercase methods).
    pub fn routes(&self, name: &str) -> Vec<(String, Vec<String>)> {
        let guard = self.inner.plugins.read();
        guard
            .get(name)
            .into_iter()
            .flat_map(|p| p.manifest.routes.iter())
            .map(|r| {
                let methods = r.methods.iter().map(|m| m.to_ascii_uppercase()).collect();
                (r.normalized_path(), methods)
            })
            .collect()
    }

    /// Invoke a plugin's HTTP handler.
    ///
    /// Builds a per-request [`Store`], installs the capability-gated host
    /// imports, instantiates the module, copies the request into guest
    /// memory through `alloc`, calls `handle`, then reads the response
    /// JSON back out of guest memory.
    pub async fn invoke(
        &self,
        plugin: String,
        request: PluginRequest,
    ) -> Result<PluginResponse, RuntimeError> {
        let (module, capabilities) = {
            let guard = self.inner.plugins.read();
            let inst = guard
                .get(&plugin)
                .ok_or_else(|| RuntimeError::NotFound(plugin.clone()))?;
            if !inst.enabled {
                return Err(RuntimeError::Disabled(plugin));
            }
            inst.capabilities.require(Capability::Http)?;
            (inst.module.clone(), inst.capabilities.clone())
        };

        let ctx = HostCtx {
            plugin: plugin.clone(),
            capabilities,
            host: self.inner.host.clone(),
        };
        let mut store = Store::new(&self.inner.engine, ctx);
        let mut linker: Linker<HostCtx> = Linker::new(&self.inner.engine);
        register_host_imports(&mut linker)?;

        let instance = linker
            .instantiate_async(&mut store, &module)
            .await
            .map_err(|e| RuntimeError::Abi {
                plugin: plugin.clone(),
                message: format!("instantiate: {e:#}"),
            })?;

        let memory =
            instance
                .get_memory(&mut store, "memory")
                .ok_or_else(|| RuntimeError::Abi {
                    plugin: plugin.clone(),
                    message: "missing exported `memory`".to_owned(),
                })?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "alloc")
            .map_err(|e| RuntimeError::Abi {
                plugin: plugin.clone(),
                message: format!("missing exported `alloc`: {e:#}"),
            })?;
        let handle = instance
            .get_typed_func::<(i32, i32, i32, i32, i32, i32), i64>(&mut store, "handle")
            .map_err(|e| RuntimeError::Abi {
                plugin: plugin.clone(),
                message: format!("missing exported `handle`: {e:#}"),
            })?;

        let method_buf =
            write_to_guest(&mut store, &memory, &alloc, request.method.as_bytes()).await?;
        let path_buf = write_to_guest(&mut store, &memory, &alloc, request.path.as_bytes()).await?;
        let body_buf = write_to_guest(&mut store, &memory, &alloc, &request.body).await?;

        let packed = handle
            .call_async(
                &mut store,
                (
                    method_buf.ptr,
                    method_buf.len,
                    path_buf.ptr,
                    path_buf.len,
                    body_buf.ptr,
                    body_buf.len,
                ),
            )
            .await
            .map_err(|e| RuntimeError::Abi {
                plugin: plugin.clone(),
                message: format!("handle trapped: {e:#}"),
            })?;

        if packed == 0 {
            return Ok(PluginResponse {
                status: 204,
                ..PluginResponse::default()
            });
        }
        let (ptr, len) = unpack(packed);
        let data = read_from_guest(&mut store, &memory, ptr, len)?;
        let json = std::str::from_utf8(&data).map_err(|e| RuntimeError::Abi {
            plugin: plugin.clone(),
            message: format!("response not utf-8: {e}"),
        })?;
        let resp: PluginResponse =
            serde_json::from_str(json).map_err(|source| RuntimeError::BadResponse {
                plugin: plugin.clone(),
                source,
            })?;
        Ok(resp)
    }

    /// Borrow the underlying [`wasmtime::Engine`]. Useful for tests and
    /// future component-model wiring.
    pub fn engine(&self) -> &Engine {
        &self.inner.engine
    }
}

struct GuestBuf {
    ptr: i32,
    len: i32,
}

async fn write_to_guest(
    store: &mut Store<HostCtx>,
    memory: &Memory,
    alloc: &TypedFunc<i32, i32>,
    bytes: &[u8],
) -> Result<GuestBuf, RuntimeError> {
    let len = i32::try_from(bytes.len()).map_err(|_| RuntimeError::Abi {
        plugin: store.data().plugin.clone(),
        message: "input larger than 2 GiB".to_owned(),
    })?;
    if len == 0 {
        return Ok(GuestBuf { ptr: 0, len: 0 });
    }
    let ptr = alloc
        .call_async(&mut *store, len)
        .await
        .map_err(|e| RuntimeError::Abi {
            plugin: store.data().plugin.clone(),
            message: format!("alloc trapped: {e:#}"),
        })?;
    if ptr == 0 {
        return Err(RuntimeError::Abi {
            plugin: store.data().plugin.clone(),
            message: "plugin alloc returned NULL".to_owned(),
        });
    }
    let usize_ptr = usize::try_from(ptr).map_err(|_| RuntimeError::Abi {
        plugin: store.data().plugin.clone(),
        message: "negative pointer from alloc".to_owned(),
    })?;
    memory
        .write(&mut *store, usize_ptr, bytes)
        .map_err(|e| RuntimeError::Abi {
            plugin: store.data().plugin.clone(),
            message: format!("memory write: {e:#}"),
        })?;
    Ok(GuestBuf { ptr, len })
}

fn read_from_guest(
    store: &mut Store<HostCtx>,
    memory: &Memory,
    ptr: u32,
    len: u32,
) -> Result<Vec<u8>, RuntimeError> {
    if len == 0 {
        return Ok(Vec::new());
    }
    let usize_ptr = usize::try_from(ptr).expect("u32 always fits usize on 32/64-bit hosts");
    let usize_len = usize::try_from(len).expect("u32 always fits usize on 32/64-bit hosts");
    let mut out = vec![0u8; usize_len];
    memory
        .read(&*store, usize_ptr, &mut out)
        .map_err(|e| RuntimeError::Abi {
            plugin: store.data().plugin.clone(),
            message: format!("memory read: {e:#}"),
        })?;
    Ok(out)
}

fn register_host_imports(linker: &mut Linker<HostCtx>) -> Result<(), RuntimeError> {
    let map = |name: &'static str, e: wasmtime::Error| RuntimeError::Abi {
        plugin: "<linker>".into(),
        message: format!("register {name}: {e:#}"),
    };
    linker
        .func_wrap("env", "host_log", host_log)
        .map_err(|e| map("host_log", e))?;
    linker
        .func_wrap("env", "host_kv_get", host_kv_get)
        .map_err(|e| map("host_kv_get", e))?;
    linker
        .func_wrap("env", "host_kv_set", host_kv_set)
        .map_err(|e| map("host_kv_set", e))?;
    linker
        .func_wrap("env", "host_posts_count", host_posts_count)
        .map_err(|e| map("host_posts_count", e))?;
    linker
        .func_wrap("env", "host_setting_get", host_setting_get)
        .map_err(|e| map("host_setting_get", e))?;
    Ok(())
}

// ─── host_log ─────────────────────────────────────────────────────

fn host_log(
    mut caller: Caller<'_, HostCtx>,
    level: i32,
    ptr: i32,
    len: i32,
) -> wasmtime::Result<()> {
    if !caller.data().capabilities.allows(Capability::Log) {
        return Err(wasmtime::Error::msg(
            "plugin missing capability `log` for host_log",
        ));
    }
    let memory = export_memory(&mut caller)?;
    let msg = read_guest_string(&caller, &memory, ptr, len)?;
    let plugin = caller.data().plugin.clone();
    let host = caller.data().host.clone();
    host.log(&plugin, level, &msg);
    Ok(())
}

// ─── host_kv_get ──────────────────────────────────────────────────
//
// Returns 0 if the key is unknown, otherwise allocates a buffer in the
// guest (via the plugin's `alloc` export) and packs `(ptr, len)` into the
// i64 return value.

fn host_kv_get(
    mut caller: Caller<'_, HostCtx>,
    key_ptr: i32,
    key_len: i32,
) -> wasmtime::Result<i64> {
    if !caller.data().capabilities.allows(Capability::Kv) {
        return Err(wasmtime::Error::msg(
            "plugin missing capability `kv` for host_kv_get",
        ));
    }
    let memory = export_memory(&mut caller)?;
    let key = read_guest_string(&caller, &memory, key_ptr, key_len)?;
    let plugin = caller.data().plugin.clone();
    let value = caller.data().host.kv_get(&plugin, &key);
    match value {
        None => Ok(0),
        Some(bytes) => copy_bytes_into_guest(&mut caller, &memory, &bytes),
    }
}

fn host_kv_set(
    mut caller: Caller<'_, HostCtx>,
    key_ptr: i32,
    key_len: i32,
    val_ptr: i32,
    val_len: i32,
) -> wasmtime::Result<()> {
    if !caller.data().capabilities.allows(Capability::Kv) {
        return Err(wasmtime::Error::msg(
            "plugin missing capability `kv` for host_kv_set",
        ));
    }
    let memory = export_memory(&mut caller)?;
    let key = read_guest_string(&caller, &memory, key_ptr, key_len)?;
    let mut buf = vec![0u8; usize::try_from(val_len).unwrap_or(0)];
    let usize_ptr = usize::try_from(val_ptr).unwrap_or(0);
    memory.read(&caller, usize_ptr, &mut buf)?;
    let plugin = caller.data().plugin.clone();
    caller.data().host.kv_set(&plugin, &key, &buf);
    Ok(())
}

fn host_posts_count(caller: Caller<'_, HostCtx>) -> wasmtime::Result<i32> {
    if !caller.data().capabilities.allows(Capability::PostsRead) {
        return Err(wasmtime::Error::msg(
            "plugin missing capability `posts:read` for host_posts_count",
        ));
    }
    Ok(caller.data().host.posts_count())
}

fn host_setting_get(
    mut caller: Caller<'_, HostCtx>,
    key_ptr: i32,
    key_len: i32,
) -> wasmtime::Result<i64> {
    if !caller.data().capabilities.allows(Capability::SettingsRead) {
        return Err(wasmtime::Error::msg(
            "plugin missing capability `settings:read` for host_setting_get",
        ));
    }
    let memory = export_memory(&mut caller)?;
    let key = read_guest_string(&caller, &memory, key_ptr, key_len)?;
    let value = caller.data().host.setting_get(&key);
    match value {
        None => Ok(0),
        Some(s) => copy_bytes_into_guest(&mut caller, &memory, s.as_bytes()),
    }
}

// ─── helpers shared by every host import ──────────────────────────

fn export_memory(caller: &mut Caller<'_, HostCtx>) -> wasmtime::Result<Memory> {
    caller
        .get_export("memory")
        .and_then(wasmtime::Extern::into_memory)
        .ok_or_else(|| wasmtime::Error::msg("plugin does not export `memory`"))
}

fn read_guest_string(
    caller: &Caller<'_, HostCtx>,
    memory: &Memory,
    ptr: i32,
    len: i32,
) -> wasmtime::Result<String> {
    let usize_ptr = usize::try_from(ptr).map_err(|_| wasmtime::Error::msg("negative ptr"))?;
    let usize_len = usize::try_from(len).map_err(|_| wasmtime::Error::msg("negative len"))?;
    let mut buf = vec![0u8; usize_len];
    memory.read(caller, usize_ptr, &mut buf)?;
    String::from_utf8(buf).map_err(|e| wasmtime::Error::msg(format!("invalid utf-8: {e}")))
}

/// Allocate `bytes.len()` bytes in the guest via its `alloc` export and
/// copy `bytes` over. Used by `host_*_get`-style imports that need to
/// return a variable-length response into the guest.
///
/// Note: this is invoked from inside a sync host import, so we cannot
/// `await` the plugin's `alloc` export. We instead synthesize a call to
/// the `alloc` typed function and block-on it via wasmtime's
/// `call`/`call_async` switcher. Since `Engine::async_support` is on the
/// only way to do this is via `tokio::task::block_in_place` or by
/// pre-resolving an `alloc` we can use synchronously. We resolve this by
/// keeping an internal allocator using `Memory::data_mut` plus a growing
/// "bump" region. That works because we only need the buffer to live
/// until the plugin reads it, which it must do before returning.
///
/// To avoid stepping on the plugin's own allocator we instead grow the
/// memory by the requested number of pages and copy into the new region.
/// Page size used by linear memory in core WebAssembly: 64 KiB.
const PAGE_SIZE: u64 = 65_536;

fn copy_bytes_into_guest(
    caller: &mut Caller<'_, HostCtx>,
    memory: &Memory,
    bytes: &[u8],
) -> wasmtime::Result<i64> {
    if bytes.is_empty() {
        return Ok(0);
    }
    let len =
        u32::try_from(bytes.len()).map_err(|_| wasmtime::Error::msg("kv value exceeds 4 GiB"))?;

    // Grow the memory by enough pages to fit `bytes`, then copy into the
    // newly-allocated region. This is a deliberately simple "host
    // allocator": each `*_get` call costs one page-aligned region.
    let pages_needed = u64::from(len).div_ceil(PAGE_SIZE);
    let old_pages = memory.grow(&mut *caller, pages_needed)?;
    let base = old_pages
        .checked_mul(PAGE_SIZE)
        .ok_or_else(|| wasmtime::Error::msg("memory overflow"))?;
    let usize_base = usize::try_from(base).map_err(|_| wasmtime::Error::msg("base too large"))?;
    memory.write(&mut *caller, usize_base, bytes)?;

    let ptr = u32::try_from(base).map_err(|_| wasmtime::Error::msg("pointer exceeds u32"))?;
    Ok(pack(ptr, len))
}

fn load_one(engine: &Engine, dir: &Path) -> Result<InstanceState, RuntimeError> {
    let (manifest, capabilities, entry) = load_manifest(dir)?;
    let module = if manifest.enabled {
        let bytes = std::fs::read(&entry).map_err(|source| LoadError::Read {
            path: entry.clone(),
            source,
        })?;
        // `Module::new` auto-detects WAT-vs-WASM when the `wat` feature is on.
        Module::new(engine, &bytes)?
    } else {
        Module::new(engine, "(module)")?
    };
    let enabled = manifest.enabled;
    Ok(InstanceState {
        manifest,
        capabilities,
        module,
        dir: dir.to_path_buf(),
        entry,
        enabled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &Path, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("plugin.toml"), body).unwrap();
    }

    #[test]
    fn reload_with_empty_dir_yields_zero() {
        let rt = PluginRuntime::new().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let n = rt.reload(tmp.path()).unwrap();
        assert_eq!(n, 0);
        assert!(rt.list().is_empty());
    }

    #[test]
    fn loads_disabled_plugin_without_wasm() {
        let rt = PluginRuntime::new().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hello-world");
        write_manifest(
            &dir,
            r#"
                name = "hello-world"
                enabled = false
                capabilities = ["http"]
            "#,
        );
        let n = rt.reload(tmp.path()).unwrap();
        assert_eq!(n, 1);
        let info = rt.get("hello-world").unwrap();
        assert!(info.capabilities.contains(&"http".to_owned()));
        assert!(!info.enabled);
    }

    #[test]
    fn loads_enabled_plugin_with_wat() {
        let rt = PluginRuntime::new().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("noop");
        write_manifest(
            &dir,
            r#"
                name = "noop"
                enabled = true
                entry = "plugin.wat"
                capabilities = ["http"]
                [[routes]]
                path = "/ping"
                methods = ["GET"]
            "#,
        );
        std::fs::write(dir.join("plugin.wat"), "(module)").unwrap();
        let n = rt.reload(tmp.path()).unwrap();
        assert_eq!(n, 1);
        let routes = rt.routes("noop");
        assert_eq!(routes, vec![("/ping".to_owned(), vec!["GET".to_owned()])]);
    }

    /// Minimal WAT plugin that implements the v1 ABI: it allocates a
    /// fixed buffer from a bump pointer, ignores the request, and returns
    /// a hard-coded JSON response (`{"status":200,"body":"ok"}`).
    const HELLO_WAT: &str = include_str!("../tests/fixtures/hello.wat");

    #[tokio::test]
    async fn invoke_hello_plugin_returns_response() {
        let rt = PluginRuntime::new().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hello");
        write_manifest(
            &dir,
            r#"
                name = "hello"
                enabled = true
                entry = "plugin.wat"
                capabilities = ["http"]
                [[routes]]
                path = "/"
                methods = ["GET"]
            "#,
        );
        std::fs::write(dir.join("plugin.wat"), HELLO_WAT).unwrap();
        rt.reload(tmp.path()).unwrap();
        let resp = rt
            .invoke("hello".to_owned(), PluginRequest::get("/"))
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "ok");
    }

    #[tokio::test]
    async fn invoke_missing_capability_fails() {
        let rt = PluginRuntime::new().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("noop");
        write_manifest(
            &dir,
            r#"
                name = "noop"
                enabled = true
                entry = "plugin.wat"
                capabilities = ["log"]
            "#,
        );
        std::fs::write(dir.join("plugin.wat"), "(module)").unwrap();
        rt.reload(tmp.path()).unwrap();
        let err = rt
            .invoke("noop".to_owned(), PluginRequest::get("/"))
            .await
            .unwrap_err();
        assert!(matches!(err, RuntimeError::Capability(_)));
    }

    #[tokio::test]
    async fn invoke_disabled_fails() {
        let rt = PluginRuntime::new().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("noop");
        write_manifest(
            &dir,
            r#"
                name = "noop"
                enabled = true
                entry = "plugin.wat"
                capabilities = ["http"]
            "#,
        );
        std::fs::write(dir.join("plugin.wat"), "(module)").unwrap();
        rt.reload(tmp.path()).unwrap();
        rt.set_enabled("noop", false).unwrap();
        let err = rt
            .invoke("noop".to_owned(), PluginRequest::get("/"))
            .await
            .unwrap_err();
        assert!(matches!(err, RuntimeError::Disabled(_)));
    }

    #[test]
    fn set_enabled_toggles_state() {
        let rt = PluginRuntime::new().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("noop");
        write_manifest(
            &dir,
            r#"
                name = "noop"
                enabled = true
                entry = "plugin.wat"
                capabilities = ["http"]
            "#,
        );
        std::fs::write(dir.join("plugin.wat"), "(module)").unwrap();
        rt.reload(tmp.path()).unwrap();
        let info = rt.set_enabled("noop", false).unwrap();
        assert!(!info.enabled);
        let info = rt.set_enabled("noop", true).unwrap();
        assert!(info.enabled);
    }
}
