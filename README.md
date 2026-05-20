# rblog

A Rust port of the [Halo](https://github.com/halo-dev/halo) blog
platform. Single binary, MySQL **or** SQLite, server-side rendered  
public site, React admin SPA, full-text search, and a capability-gated  
WebAssembly plugin runtime. Here is a living example:
[https://blog.xiedeacc.com](https://blog.xiedeacc.com).

The on-disk schema is wire-compatible with Halo's `extensions` table so
existing Halo databases can be pointed at rblog without migration.

```
┌──────────────────────────┐
│  Browser (public + SSR)  │
│  Browser (admin SPA)     │
└────────────┬─────────────┘
             │ HTTP/JSON
             ▼
┌──────────────────────────┐     ┌─────────────────────────────┐
│  rblog (single binary)   │     │  WASM plugins               │
│  ─ Axum router           │◄────┤  wasmtime + capability gate │
│  ─ MiniJinja SSR         │     └─────────────────────────────┘
│  ─ Tantivy search index  │
│  ─ Embedded SPA (opt-in) │
└────┬────────────┬────────┘
     │            │
     ▼            ▼
 ┌─────────┐  ┌─────────┐
 │ SQLite  │  │ MySQL 8 │   (pick one — same `extensions` schema)
 └─────────┘  └─────────┘
```

## Quickstart

```bash
# 1. Build everything (admin SPA + binary with embedded SPA).
cd admin && pnpm install && pnpm run build && cd ..
cargo build --release --bin rblog --features rblog-http/embed-admin

# 2. Run with an ephemeral SQLite database.
./target/release/rblog
#   listening on 127.0.0.1:8080
#   • public site:  http://127.0.0.1:8080/
#   • admin UI:     http://127.0.0.1:8080/admin/
#   • OpenAPI JSON: http://127.0.0.1:8080/api/admin/openapi.json

# 3. First-run bootstrap: open /admin/ and create the initial admin user.
```

### Docker

```bash
docker compose up --build
```

`docker-compose.yaml` boots a MySQL 8 sidecar and the rblog binary
with the SPA baked in (`--features embed-admin`). Use plain
`docker run -p 8080:8080 rblog:latest` for SQLite-only mode.

### Config

`rblog.toml` (or environment variables, prefix `RBLOG__`, `__` = section
nesting). See `[rblog.example.toml](./rblog.example.toml)` for the full
list. The most common knobs:


| Setting              | Default           | Description                                                                          |
| -------------------- | ----------------- | ------------------------------------------------------------------------------------ |
| `server.bind`        | `127.0.0.1:8080`  | TCP listen address.                                                                  |
| `database.url`       | `sqlite::memory:` | `sqlite:./rblog.db` or `mysql://user:pass@host:3306/rblog`.                          |
| `paths.themes_root`  | `./themes`        | MiniJinja templates + static assets.                                                 |
| `paths.uploads_root` | `./uploads`       | Local attachment store (`storage.backend = "local"`).                                |
| `paths.search_root`  | `./search-index`  | Persistent Tantivy directory.                                                        |
| `paths.plugins_root` | `./plugins`       | One subdirectory per WASM plugin.                                                    |
| `paths.admin_dist`   | *unset*           | Disk fallback for the admin SPA when the binary is **not** built with `embed-admin`. |


## Repository layout

```
crates/
  rblog-scheme/     Group/Version/Kind schema, store-name codec.
  rblog-store/     SQLx pool, migrations, `extensions` table CRUD.
  rblog-content/   Domain content types (Post, Tag, Category, …).
  rblog-index/    In-memory secondary index + filter/sort/paginate.
  rblog-search/   Tantivy-backed full-text search.
  rblog-auth/     Argon2 password hashing + session helpers.
  rblog-attachments/  Image/upload pipeline (local + S3 backends).
  rblog-theme/    Theme registry, default theme installer.
  rblog-core/    Service layer (Posts, Comments, Settings, etc.).
  rblog-plugins/  WASM plugin runtime (wasmtime + capability sandbox).
  rblog-http/    Axum app, OpenAPI, routes, error mapping.
  rblog/         Binary entrypoint (cargo run --bin rblog).
admin/            React 19 + Vite admin SPA (Ant Design + Lexical).
examples/plugins/ Example plugins (hello-world WAT).
```

## Features

### Server-side rendering

- Public site rendered with [MiniJinja](https://docs.rs/minijinja). The
default theme is bundled in `rblog-theme/default/` and installed
into `paths.themes_root` on first boot.
- The theme can be swapped by dropping a different directory under
`paths.themes_root` and selecting it from the admin UI.

### Admin SPA

- React 19 + Vite + TypeScript + Ant Design 5 + Lexical editor.
- Two serving modes:
  1. **Embedded** (`--features embed-admin`): the entire SPA is baked
    into the binary via `rust-embed`. No filesystem dependency.
  2. **Disk-served**: set `paths.admin_dist = "/path/to/admin/dist"`
    to serve a freshly-built bundle from disk; ideal for development
     when iterating on the SPA.
- The Vite dev server (`cd admin && pnpm run dev`) proxies `/api` and
`/uploads` to `http://127.0.0.1:8080`, so you can run the SPA and
the backend side-by-side.

### Full-text search

- [Tantivy](https://github.com/quickwit-oss/tantivy) index seeded from
the store on first boot and kept in lockstep with admin
create/update/publish/delete via `crates/rblog-http/src/search_sync.rs`.
- Public endpoints: `GET /search` (themed HTML) and
`GET /api/search?q=…` (JSON).
- Admin: `POST /api/admin/system/search/rebuild` (also exposed in the
dashboard).

### WASM plugin runtime

- Each plugin lives in `<plugins_root>/<name>/` with a `plugin.toml`
manifest plus a `plugin.wasm` (or `plugin.wat`) module.
- Capability allow-list: `log`, `kv`, `http`, `posts:read`,
`settings:read`. Anything not declared in the manifest traps at the
host import boundary.
- ABI documented in
`[crates/rblog-plugins/src/abi.rs](crates/rblog-plugins/src/abi.rs)`.
Plugins export `memory`, `alloc`, and `handle(method, path, body) -> packed_ptr_len`. The runtime returns a JSON response that the HTTP
layer surfaces verbatim under `/api/plugins/<name>/`*.
- Admin endpoints under `/api/admin/plugins/*` and a **Plugins** page
in the admin SPA cover enable/disable/reload and capability
inspection.
- See `[examples/plugins/](examples/plugins/)` for a runnable example
(`hello-world`, written in raw WAT — no toolchain required).

## Development

```bash
# Rust
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Admin SPA
cd admin
pnpm install
pnpm run dev          # http://127.0.0.1:5173
pnpm run typecheck
pnpm run build        # writes admin/dist/
```

CI mirrors the same commands on every push / pull-request — see
`[.github/workflows/ci.yaml](.github/workflows/ci.yaml)`. The
`mysql` job spins up a MySQL 8 service container and runs the
workspace tests against it.

## License

Apache-2.0. The bundled theme assets are MIT (see
`crates/rblog-theme/default/LICENSE`).