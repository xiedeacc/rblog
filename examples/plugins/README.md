# Example plugins

Each subdirectory is a self-contained rblog plugin you can copy under
`paths.plugins_root` (default: `./plugins/`) to play with the runtime.

After copying, restart rblog or hit `POST /api/admin/plugins/reload`
from the admin UI to pick it up. The admin **Plugins** page will then
list it with its capabilities, version, and declared routes.

## hello-world

Returns `Hello, world!` on `GET /api/plugins/hello-world/greet`. Logs
one line per request via the host's `log` capability. Written in raw
WebAssembly text (`plugin.wat`) so it requires no toolchain — the
runtime accepts `.wat` and `.wasm` interchangeably.

```bash
curl -i http://127.0.0.1:8080/api/plugins/hello-world/greet
# HTTP/1.1 200 OK
# content-type: text/plain; charset=utf-8
#
# Hello, world!
```

## Writing your own

A plugin is a single directory containing:

- `plugin.toml` — manifest (see
  [`crates/rblog-plugins/src/manifest.rs`](../../crates/rblog-plugins/src/manifest.rs)
  for the full schema).
- `plugin.wasm` (or `plugin.wat`) — a core WebAssembly module that
  implements the v1 ABI documented in
  [`crates/rblog-plugins/src/abi.rs`](../../crates/rblog-plugins/src/abi.rs).

Capabilities you do not declare in the manifest are **un-callable**:
the runtime traps if a plugin imports e.g. `env.host_kv_get` without
declaring `kv`.
