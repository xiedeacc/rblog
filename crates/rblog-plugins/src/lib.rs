//! WASM plugin runtime for rblog.
//!
//! Each plugin lives in `<plugins_root>/<name>/` with a `plugin.toml`
//! manifest and a `plugin.wasm` module:
//!
//! ```text
//! plugins/
//!   hello-world/
//!     plugin.toml
//!     plugin.wasm
//! ```
//!
//! `plugin.toml` declares the plugin's identity, capabilities, and routes
//! (see [`Manifest`]). The [`PluginRuntime`] type compiles every plugin
//! into a [`wasmtime::Module`] at startup and keeps them in a parking-lot
//! [`RwLock`] indexed by name. Per-request execution happens through
//! [`PluginRuntime::invoke`], which the HTTP layer wires up under
//! `/api/plugins/<name>/*`.
//!
//! The capability model is enforced at host-function time: each host
//! call checks `Capability::allows` against the plugin's declared set
//! before performing the operation. A plugin can therefore declare
//! `["log"]` and the runtime will refuse to give it `kv` or `http`
//! access even if it tries to import the host functions.
//!
//! Threading model:
//! - One [`wasmtime::Engine`] per process (cheap to clone).
//! - One [`wasmtime::Store`] per request (created inside `invoke`).
//! - The compiled [`wasmtime::Module`] is shared across requests.

pub mod abi;
pub mod capability;
pub mod host;
pub mod manifest;
pub mod runtime;

pub use abi::{PluginRequest, PluginResponse};
pub use capability::{Capability, CapabilityError, CapabilitySet};
pub use host::{noop as noop_host, HostHandle, HostServices, NoopHost};
pub use manifest::{LoadError, Manifest, RouteMount};
pub use runtime::{InstanceState, PluginInfo, PluginRuntime, RuntimeError};
