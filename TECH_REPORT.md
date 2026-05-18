# rblog — Technical Design Report

> A Rust port and re‑architecture of [Halo 2](https://github.com/halo-dev/halo).
> Built for raw speed, SEO, and a single self‑contained binary.

**Status:** Design proposal (no code written yet)
**Date:** 2026‑05‑16
**Upstream reference:** `halo-dev/halo` v2.x (Java 21 / Spring WebFlux / R2DBC / Vue 3 / Tiptap)

---

## 1. Goals and non‑goals

### 1.1 Goals

1. **Public site = SSR**, served by the Rust process directly. No hydration tax, no JS framework on the public path, first contentful paint < 100 ms on cold cache.
2. **Admin = SPA (React 19)**, statically built and **served from a subfolder** of the same Rust binary (e.g. `/console/*`). One process, one port, one container.
3. **Single binary deploy**: `./rblog --config rblog.toml` is all you need. Themes + admin assets are either embedded (release) or read from disk (dev).
4. **Dual database** at runtime: **MySQL** and **SQLite**, behind one query layer.
5. **Halo feature parity for v1**: users/roles, posts, pages, categories, tags, comments, menus, attachments, settings, themes, RSS/Atom/sitemap.
6. **Best‑in‑class editor** for content creators (see §5).
7. **OpenAPI‑first** admin API. The React SPA only talks to a typed, generated TS client.

### 1.2 Non‑goals (v1)

- Multi‑tenant SaaS.
- Real‑time collaborative editing (Y.js / Hocuspocus). Designed‑for, not shipped in v1.
- PostgreSQL (architecturally trivial to add later — same SQLx driver — but excluded to keep v1 scope tight; the user asked for MySQL + SQLite).
- Public plugin marketplace UI / signed‑plugin distribution (we ship the **plugin runtime** in v1; the discovery / store experience can wait).

---

## 2. High‑level architecture

```
                ┌──────────────────────────────────────────────────────────────┐
                │                  rblog (single Rust binary)                  │
                │                                                              │
                │      Axum HTTP server (Tokio runtime, HTTP/1.1 + h2)         │
                │                                                              │
 Browser ──►    │  ┌─────────────────────┐  ┌─────────────────────────────┐    │
 GET /          │  │   Public SSR router │  │   Admin SPA static router   │    │
 GET /posts/.. ─┼─►│  MiniJinja + theme  │  │   /console/*  → rust-embed  │    │
 GET /tags/..   │  │  HTML, RSS, sitemap │  │   /console/index.html       │    │
                │  └──────────┬──────────┘  └─────────────────────────────┘    │
                │             │                                                │
                │             ▼                                                │
                │  ┌──────────────────────┐  ┌────────────────────────────┐    │
                │  │   /api/v1/*  (JSON)  │◄─┤  React SPA (admin)         │    │
                │  │   utoipa OpenAPI 3.1 │  │  Ant Design + Lexical      │    │
                │  └──────────┬───────────┘  └────────────────────────────┘    │
                │             │                                                │
                │             ▼                                                │
                │   Service layer (rblog-core)                                 │
                │   Auth · Posts · Themes · Markdown · Search · Attachments    │
                │             │                ▲                               │
                │             │                │ events / hooks / template fns │
                │             │                │                               │
                │             │      ┌─────────┴────────────┐                  │
                │             │      │ rblog-plugins (host) │                  │
                │             │      │  wasmtime + WASI P2  │                  │
                │             │      │  Component Model     │                  │
                │             │      └──────────┬───────────┘                  │
                │             │                 │                              │
                │             │                 ▼ (.wasm component files)     │
                │             │       <work_dir>/plugins/<name>/{plugin.wasm, │
                │             │        plugin.yaml, ui/}                       │
                │             ▼                                                │
                │   SQLx Pool  ── MySQL ──┐    ┌── ./work/uploads/  (or S3)    │
                │              ── SQLite ─┘    └── ./work/themes/<name>/       │
                └──────────────────────────────────────────────────────────────┘
```

**One process. One port. One container.** The only thing that ever leaves the binary is database I/O and user‑uploaded attachments (filesystem or S3‑compatible object store).

### 2.1 URL surfaces

| Surface                | Path                  | Handler                                | Caching             |
| ---------------------- | --------------------- | -------------------------------------- | ------------------- |
| Public HTML (home)     | `/`                   | SSR via MiniJinja                      | `Cache-Control` + ETag |
| Public HTML (post)     | `/archives/:slug`     | SSR                                    | ETag                |
| Tag / category pages   | `/tags/:slug` etc.    | SSR                                    | ETag                |
| RSS / Atom             | `/feed/rss.xml`       | SSR (string template)                  | s‑maxage 5 min      |
| Sitemap                | `/sitemap.xml`        | SSR                                    | s‑maxage 1 h        |
| Robots                 | `/robots.txt`         | static or template                     | immutable           |
| Theme static assets    | `/themes/:name/static/*` | `tower_http::services::ServeDir`    | immutable + hash    |
| User uploads           | `/upload/*`           | `ServeDir` (or signed S3 redirect)     | per‑file            |
| Admin SPA              | `/console/*`          | `rust-embed` (release) / `ServeDir` (dev) | hashed assets    |
| Admin API              | `/api/v1/*`           | JSON, OpenAPI documented               | no‑cache            |
| Plugin routes          | `/api/plugins/:name/*`| Forwarded into the plugin's WASM handler | no‑cache         |
| Plugin admin UI assets | `/console/plugins/:name/*` | Per‑plugin SPA bundle (signed cache) | hashed assets    |
| OpenAPI docs           | `/api/docs`           | utoipa‑swagger‑ui                      | dev only            |
| Healthz / readyz       | `/healthz` etc.       | trivial JSON                           | no‑cache            |

### 2.2 Process lifecycle

1. Load `rblog.toml` (+ env overrides).
2. Open SQLx pool against MySQL or SQLite.
3. Run pending migrations (`sqlx::migrate!`).
4. Scan `<work_dir>/themes/` and load the active theme into a MiniJinja `Environment`.
5. Scan `<work_dir>/plugins/`, validate manifests, instantiate enabled plugins in `wasmtime`, call each plugin's `on_init` export.
6. Build the Axum `Router` (public + admin + api + plugin routes + static).
7. Bind, serve, graceful shutdown on `SIGTERM` / `SIGINT` (drains plugin instances via `on_shutdown`).

---

## 3. Backend tech stack

> All versions are "latest stable as of design date"; pinned versions go in `Cargo.toml` at implementation time, not in this report.

### 3.1 Core runtime

| Concern             | Crate                              | Why                                                                                              |
| ------------------- | ---------------------------------- | ------------------------------------------------------------------------------------------------ |
| Async runtime       | `tokio` (full)                     | De‑facto standard; multi‑thread scheduler; pairs natively with Axum and SQLx.                    |
| HTTP framework      | `axum`                             | Tower‑based, type‑safe extractors, the obvious choice; the user explicitly asked for it.         |
| Tower middleware    | `tower`, `tower-http`              | Compression, tracing, CORS, timeout, request ID, `ServeDir`.                                     |
| HTTP types          | `http`, `hyper`                    | Pulled in transitively; explicit in Cargo for header construction.                               |
| TLS (optional)      | `axum-server` + `rustls`           | If users want to terminate TLS in the app; default deployment expects a reverse proxy.           |

### 3.2 Database & persistence

> **Architecture note (revised after Halo schema review):** rblog stores **every** entity in a single `extensions(name, data, version)` table — identical to Halo (see **§6**). The SQL layer is therefore tiny: a single `ExtensionStore` repository implementing `list_by_name_prefix`, `fetch`, `create`, `update`, `delete` with optimistic concurrency. Rich queries are served by an in‑process **index engine** (see §6.7), not by SQL filters on JSON.

| Concern              | Crate                                                                                                  | Why                                                                                       |
| -------------------- | ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- |
| SQL driver           | `sqlx` (features `mysql`, `sqlite`, `runtime-tokio`, `tls-rustls`, `macros`, `chrono`, `migrate`)      | Async, both required backends, optimistic‑lock UPDATE is one statement, easy to test.    |
| Migrations           | `sqlx::migrate!("./migrations/<dialect>")`                                                             | Two tiny migrations total: one creates `extensions` on MySQL, one creates it on SQLite.   |
| JSON (de)serializer  | `serde_json`                                                                                           | Wire‑format compatibility with Halo's Jackson output (we control the canonicalization).   |
| In‑process indices   | `std::collections::BTreeMap` + a small `rblog-index` crate                                             | Label / annotation / spec‑field secondary indices, rebuilt at startup, kept in sync on write. |
| Connection pool      | `sqlx::Pool<Any>` or two‑arm enum `Db { Mysql(Pool<MySql>), Sqlite(Pool<Sqlite>) }` (decided in code)  | Pool is built in; the enum avoids dynamic dispatch hot‑path overhead.                     |

**Why not Diesel / SeaORM / Loco?** Same reasoning as before, plus: with a single‑table schema all of them are overkill. We write at most ~120 lines of SQL across the whole product.

**Why not switch to PostgreSQL JSONB and use JSON path queries?** Halo deliberately avoided it for portability. We follow that choice. JSON path queries also wouldn't help on SQLite. The index engine works the same on every backend.

### 3.3 Templating (SSR)

| Concern              | Crate                                | Why                                                                                                                 |
| -------------------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------- |
| Template engine      | **`minijinja`** + `minijinja-autoreload` (dev) | Jinja2 syntax (very close to Halo's Thymeleaf in spirit), **dynamic loading from disk** — required because users install themes at runtime; fastest in the Rust template benchmark group; autoescape HTML by default. |
| Markdown → HTML      | `comrak`                             | CommonMark + GFM tables, footnotes, autolinks, task lists, alerts; syntax highlighting via adapters.                |
| HTML sanitization    | `ammonia`                            | Whitelist‑based sanitizer; applied to rendered HTML and to user comments.                                            |
| Syntax highlighting  | `syntect` (via comrak adapter)       | Server‑side, no JS required for the public site.                                                                     |

**Why not Askama or Tera?**
- Askama compiles templates at build time → fatal for user‑installed themes.
- Tera is fine, but MiniJinja is ~2× faster on common benchmarks, has a smaller dep tree, supports custom loaders trivially, and is maintained by Armin Ronacher (Jinja2 author).

### 3.4 Auth, sessions, security

| Concern                  | Crate                              | Why                                                                                              |
| ------------------------ | ---------------------------------- | ------------------------------------------------------------------------------------------------ |
| Password hashing         | `argon2`                           | OWASP recommendation; Halo also uses argon2id.                                                    |
| Session middleware       | `tower-sessions` + `tower-sessions-sqlx-store` | Cookie‑based sessions backed by the same DB; works with MySQL and SQLite.                   |
| Auth orchestration       | `axum-login`                       | Pluggable user trait + session integration; clean `RequireAuthLayer`.                             |
| Personal access tokens   | `jsonwebtoken`                     | For machine clients (CI, scripts) — short‑lived JWTs signed with HS256/EdDSA.                     |
| CSRF                     | hand‑rolled double‑submit cookie + `Origin` check | The admin is same‑origin with the API, so CSRF risk is bounded; we still gate non‑GET. |
| Rate limiting            | `tower_governor`                   | IP + route key; matches the routes Halo guards with Resilience4j (login, signup, comment, etc.).  |
| TOTP / 2FA (v1.1)        | `totp-rs`                          | Optional second factor for admin users.                                                           |

### 3.5 Validation, serialization, errors

| Concern                  | Crate                              | Why                                                                                              |
| ------------------------ | ---------------------------------- | ------------------------------------------------------------------------------------------------ |
| Serialization            | `serde`, `serde_json`, `serde_yaml`| `_yaml` is for `theme.yaml` and migrated Halo config.                                              |
| Validation               | `validator` (derive) + `garde` (alt) | Field‑level rules with i18n‑friendly messages.                                                  |
| Errors                   | `thiserror` (lib) + `anyhow` (bin) | Typed lib errors → bin layer maps to HTTP via a single `IntoResponse for AppError`.               |
| Config                   | `config` + `serde`                 | Layered: defaults → `rblog.toml` → env (`RBLOG__SERVER__PORT`).                                   |

### 3.6 Observability

| Concern               | Crate                                       | Why                                                                                  |
| --------------------- | ------------------------------------------- | ------------------------------------------------------------------------------------ |
| Logging / tracing     | `tracing`, `tracing-subscriber`, `tower-http::trace::TraceLayer` | Structured logs, span propagation; JSON output in production, pretty in dev. |
| Metrics               | `metrics`, `metrics-exporter-prometheus`    | `/metrics` Prometheus endpoint (admin‑auth gated).                                   |
| Panic & error reporting | `sentry` (optional, behind feature flag)  | Off by default; switch on for production.                                            |

### 3.7 Background work

| Concern                | Crate                       | Why                                                                                  |
| ---------------------- | --------------------------- | ------------------------------------------------------------------------------------ |
| Scheduled jobs         | `tokio-cron-scheduler`      | Comment cleanup, session GC, sitemap regeneration, search reindex.                   |
| In‑process queue       | `tokio::sync::mpsc`         | Email sending, webhook delivery — fire‑and‑forget with retry.                        |
| Caching                | `moka` (async)              | Hot path caches: rendered post HTML, settings, theme template lookups.               |

### 3.7.1 Plugin runtime (WASM)

| Concern                 | Crate / Tool                  | Why                                                                                            |
| ----------------------- | ----------------------------- | ---------------------------------------------------------------------------------------------- |
| WASM engine             | `wasmtime` (`component-model`, `async`, `cranelift`, `pooling-allocator`) | The best‑maintained Rust WASM runtime; first‑class Component Model + WASI Preview 2 support.   |
| WASI standard library   | `wasmtime-wasi`               | Sandboxed filesystem (scoped per plugin), env, clocks. No raw socket access by default.        |
| HTTP for plugins        | `wasmtime-wasi-http`          | `wasi:http/incoming-handler` and `wasi:http/outgoing-handler` — plugins both serve and call HTTP via a capability-gated host. |
| Host‑side bindings      | `wit-bindgen` (host)          | Generates the host trait that the `rblog-plugins` crate implements; type‑safe at compile time. |
| WIT interface authoring | `wit-bindgen-cli`             | Plugin authors compile WIT into bindings for their language (Rust, Go via TinyGo, JS via `componentize-js`, Python via `componentize-py`). |
| Schema validation       | `serde_yaml` + `schemars`     | Validates `plugin.yaml` manifests at load time.                                                |
| Plugin scratch storage  | SQLx + plugin‑scoped tables   | Plugins get a namespaced key/value store via host functions; no raw SQL access.                |

A full breakdown of the plugin runtime, the WIT world, lifecycle, capabilities, and security model is in **§14**.

### 3.8 API documentation

| Concern                | Crate                       | Why                                                                                  |
| ---------------------- | --------------------------- | ------------------------------------------------------------------------------------ |
| OpenAPI generation     | `utoipa`                    | Derive macros on Axum handlers + DTOs → OpenAPI 3.1 spec emitted at build time.       |
| Spec UI                | `utoipa-swagger-ui`, `utoipa-redoc` | Mounted on `/api/docs` (dev only by default).                                |
| TS client generation   | `openapi-typescript-codegen` (Node, in `admin/`) | Generates `admin/src/api/` from the spec; the SPA imports a typed client. |

### 3.9 Content pipeline

| Concern                 | Crate                       | Why                                                                                  |
| ----------------------- | --------------------------- | ------------------------------------------------------------------------------------ |
| Markdown rendering      | `comrak`                    | GFM, footnotes, alerts, math (via extension), tables.                                |
| HTML sanitization       | `ammonia`                   | Whitelist‑based; applied to comments and to AI‑/user‑pasted HTML.                    |
| Slug generation         | `slug`                      | URL‑safe slugs from titles (with CJK transliteration fallback).                      |
| Image processing        | `image` + `fast_image_resize` + `kamadak-exif` | Thumbnail generation, EXIF strip, WebP/AVIF re‑encode. |
| Full‑text search        | `tantivy`                   | Embedded Lucene‑style search index in `<work_dir>/search/`; no Elasticsearch needed. |
| RSS / Atom              | `rss`, `atom_syndication`   | Tiny crates, well maintained.                                                        |
| Sitemap                 | `sitemap` or hand‑rolled    | Trivial; emitted by a MiniJinja template.                                            |

### 3.10 Storage abstraction

| Concern                | Crate                       | Why                                                                                  |
| ---------------------- | --------------------------- | ------------------------------------------------------------------------------------ |
| Object storage trait   | `object_store`              | One trait, multiple backends (Local FS, S3, GCS, Azure, MinIO). Matches Halo's "attachment policy" abstraction. |
| Default backend (v1)   | Local FS (`<work_dir>/uploads/`) | Zero config.                                                                  |
| Optional backend       | S3‑compatible via `object_store::aws` | One config block to switch. Halo treats this as a plugin; for us it's built‑in. |

### 3.11 Embedding the admin SPA

| Concern                | Crate                       | Why                                                                                  |
| ---------------------- | --------------------------- | ------------------------------------------------------------------------------------ |
| Embed assets in binary | `rust-embed` (with `compression`) | Production release bundles `admin/dist/` into the binary; SPA index falls back to `index.html`. |
| Dev‑time pass‑through  | `tower_http::services::ServeDir` | Points at `admin/dist/` so `cargo run` after `pnpm build` works without rebuild. |
| Hot dev mode           | Vite dev server on a side port + a small reverse‑proxy route `/console/*` to `http://127.0.0.1:5173` (dev only feature flag) | Lets us run `pnpm dev` and `cargo run` side‑by‑side. |

---

## 4. Frontend tech stack (admin SPA)

### 4.1 Foundation

| Concern              | Choice                                                    | Why                                                                                                                            |
| -------------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Framework            | **React 19** (function components, hooks, Suspense)       | User requirement. Pairs naturally with Lexical (Meta's editor, React‑first). Largest ecosystem of admin UI libraries.          |
| Language             | TypeScript (strict, `noUncheckedIndexedAccess`)           | Catches API drift; pairs with the generated OpenAPI client.                                                                    |
| Build tool           | **Vite 5+** with `@vitejs/plugin-react-swc`               | Fast HMR with SWC; ESM output; plugin ecosystem.                                                                               |
| Package manager      | **pnpm**                                                  | Deterministic and fast; same as Halo.                                                                                          |
| Router               | **`@tanstack/react-router`**                              | Type‑safe routing with first‑class data loaders; pairs with React Query. (Fallback option: `react-router` v6 if we hit edge cases — both ship in v1 only one will.) |
| Client state         | **`zustand`**                                             | Tiny (~3 KB), no boilerplate, no `Provider` pyramid. We use it only for transient UI state (sidebar collapse, theme); server state belongs to React Query. |
| Server state         | **`@tanstack/react-query` v5**                            | Caches, dedupes, retries — the standard for REST in React.                                                                     |
| HTTP client          | `axios` wrapped by the generated OpenAPI client           | Same as before; auth interceptor for sessions.                                                                                 |

### 4.2 UI

| Concern              | Choice                                                                              | Why                                                                                                                            |
| -------------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Component library    | **Ant Design v5** (`antd`)                                                          | The React analog of Element Plus: comprehensive components (Table with virtualization, Form, Tree, Transfer, Cascader, Steps, ProLayout via `@ant-design/pro-components`), CSS‑in‑JS theming with design tokens, built‑in light/dark, large Chinese ecosystem (matches Halo's contributor base), strongest "admin shell out of the box" story in React. |
| Pro layouts          | **`@ant-design/pro-components`** (`ProLayout`, `ProTable`, `ProForm`)               | Halo's admin patterns map cleanly to ProLayout's nested menu + breadcrumb + content shell.                                     |
| Styling              | **TailwindCSS v3** for utility/layout classes around antd components                | We **prefix Tailwind** (`tw-`) so utility classes never collide with antd's base styles.                                       |
| Theming              | antd v5 design‑token API + `ConfigProvider`                                         | One token set produces light + dark + custom brand colors; theme is reactive and persisted in `zustand`.                       |
| Icons                | **`@ant-design/icons`** for native antd usages + **`lucide-react`** + `@iconify/react` | Antd icons for built‑in components, Lucide for our own UI, Iconify when we need anything else.                                 |
| Forms                | **`react-hook-form`** + **`zod`** + `@hookform/resolvers/zod`                       | Best React form story: uncontrolled by default, fast re‑renders, schema validation that doubles as our DTO types.              |
| Schema‑driven forms  | **`@rjsf/core`** + `@rjsf/antd` + `@rjsf/validator-ajv8`                            | Used for **theme settings** and **plugin settings** that are declared as JSON Schema. Same role FormKit was going to play.     |
| File upload          | **`@uppy/react`** + `@uppy/xhr-upload`                                              | Same Uppy stack Halo uses; battle‑tested chunked / resumable uploads, just the React binding.                                  |
| Code editor          | **`@uiw/react-codemirror`** + CodeMirror 6 language packs                           | Inline editor for YAML / Markdown / HTML / CSS / JS panels.                                                                    |
| i18n                 | **`react-i18next`** + `i18next-browser-languagedetector`                            | Same JSON locale files we'll ship for the public site, just rendered through the React binding.                                |
| Forms / dates / DnD  | antd's `DatePicker`; **`@dnd-kit/core`** for sortable category/menu trees           | dnd‑kit is the modern React DnD library — accessibility built in.                                                              |

### 4.3 Rich‑text editor — see §5.

### 4.4 Build output

- `admin/dist/` is a static site (HTML + hashed JS + hashed CSS).
- In release builds `rust-embed` embeds the directory into the binary.
- All routes resolve to `index.html` so client routing works (`react-router` / `tanstack-router` history mode), with the base path set to `/console/`.

---

## 5. Editor decision

> "Choose the best editor you can know" — user has chosen **Lexical**. We codify that choice and the integration plan.

### 5.1 Shortlist re‑evaluated for React

| Editor                         | Engine          | React support                          | Markdown round‑trip | Slash commands | Tables / Math / Images | Collab ready          | Verdict                                              |
| ------------------------------ | --------------- | -------------------------------------- | ------------------- | -------------- | ---------------------- | --------------------- | ---------------------------------------------------- |
| **Lexical (Meta)**             | Lexical         | **First‑class** (`@lexical/react`)     | `@lexical/markdown` | Custom plugin  | Tables / Images / Code | `@lexical/yjs`        | **Pick.** Used in production at Meta (FB Comments, Workplace, Threads, Messenger). Tiny core (~22 KB), accessibility‑first, headless, fastest of the bunch in benchmarks. |
| Tiptap v3                      | ProseMirror     | `@tiptap/react`                        | `tiptap-markdown`   | Built‑in       | Mature                 | Y.js                  | Strong alternative; the engine Halo uses.            |
| Milkdown                       | ProseMirror     | React adapter                          | First‑class         | Yes            | Yes                    | Y.js                  | Smaller community.                                   |
| Slate                          | Custom          | First‑class                            | Plugin              | Plugin         | Hand‑rolled            | yjs‑plain             | Lots of plumbing required.                           |
| Quill v2                       | Quill           | Wrapper                                | Limited             | No             | Limited                | No                    | Out of fashion for CMS.                              |
| TinyMCE / CKEditor 5           | Proprietary‑ish | Wrapper                                | Plugin              | Plugin         | Yes                    | Commercial            | License & weight.                                    |

### 5.2 Pick: **Lexical**

Why this is the right call now that the SPA is React:

- **Authoring weight matches Meta's production needs**: rich text with millions of daily writers — performance and accessibility are first‑class concerns, not afterthoughts.
- **Headless and node‑first**: every editor state is a serializable tree of `LexicalNode`s. The serialized JSON is stable and round‑trippable, which is exactly what we need to put inside Halo's `Snapshot.spec.rawPatch` field.
- **Built‑in markdown transformers**: `@lexical/markdown` ships `TRANSFORMERS` for headings, lists, code, links, etc. Plus our own custom transformers for any Halo‑specific extensions.
- **Tiny core, paid extensions**: ~22 KB gz core; you only pay for the plugins you import. Halo's Tiptap bundle is much larger.
- **Yjs collab path exists** (`@lexical/yjs`) for the v3 collaborative editing milestone.

### 5.3 Plugins we ship in v1

Core plugins from `@lexical/*`:

- **`@lexical/react`** (`<LexicalComposer>`, `<RichTextPlugin>`, `<HistoryPlugin>`, `<OnChangePlugin>`).
- **`@lexical/rich-text`** — headings, blockquote, paragraphs.
- **`@lexical/list`** — bulleted / numbered / check lists.
- **`@lexical/link`** + **`@lexical/auto-link`**.
- **`@lexical/code`** with **Prism.js** for syntax highlighting in the editor view; the public SSR view re‑highlights with `syntect` server‑side for consistency.
- **`@lexical/table`** — tables with selection / resize.
- **`@lexical/markdown`** with our extended `TRANSFORMERS` array — markdown is the canonical wire format.
- **`@lexical/selection`**, **`@lexical/utils`**, **`@lexical/clipboard`**.

Custom plugins we author (`admin/src/editor/plugins/`):

- **Image upload plugin** — drag‑drop or paste hits `/api/v1/attachments`, inserts an `ImageNode`.
- **Slash‑command menu** — `/heading`, `/image`, `/table`, `/code`, `/quote`, `/divider`, `/embed`.
- **Floating format toolbar** — appears on selection.
- **Block drag handle** — left‑gutter handles for reordering blocks.
- **Halo macro plugin** — renders Halo's `[[...]]` macro syntax (used in some legacy themes) for backwards compatibility on imported content.
- **Mention plugin** (v1.1) — `@user` autocomplete from `User` extensions.
- **Yjs collab plugin** (v3) — `@lexical/yjs` + Hocuspocus server.

### 5.4 Storage model (Halo‑compatible, see §6.5)

- The **canonical stored content** is **Markdown** in a Halo `Snapshot.spec.rawPatch` field (see §6.5). On every save we also persist the rendered HTML in `Snapshot.spec.contentPatch` so the public SSR view doesn't need to re‑render markdown per request.
- Lexical's in‑memory state is its own JSON tree, but **we never persist that JSON to the DB**. We serialize to markdown via `@lexical/markdown` (`$convertToMarkdownString(TRANSFORMERS)`) on save, and parse back via `$convertFromMarkdownString(markdown, TRANSFORMERS)` on load.
- Rationale: Halo's existing posts are markdown; round‑tripping through Lexical's JSON would be lossy for imported content. Markdown is the lingua franca.
- We commit a corpus test that imports a sample of real Halo posts, round‑trips them through Lexical, and asserts the resulting markdown matches the original modulo whitespace.

---

## 6. Data model — **Halo‑compatible extension store**

> **Major design decision (revised after spec review):** rblog adopts **Halo's exact storage primitive** so an existing Halo blog can be migrated by simply pointing rblog at the same database (or by dumping `extensions` rows from Halo and replaying them into rblog). No ETL.

### 6.1 The one and only table

The entire data model is a single table — **byte‑identical DDL to Halo**:

```sql
-- MySQL / MariaDB
CREATE TABLE IF NOT EXISTS extensions (
    name    VARCHAR(255) NOT NULL COLLATE utf8mb4_bin,
    data    longblob,
    version BIGINT,
    PRIMARY KEY (name)
);

-- SQLite (we are adding this dialect; Halo doesn't ship it)
CREATE TABLE IF NOT EXISTS extensions (
    name    TEXT    NOT NULL PRIMARY KEY,
    data    BLOB,
    version INTEGER
);

-- PostgreSQL / H2 schemas exist in Halo verbatim if anyone ever opts in.
```

`name` is the K8s‑style *store path* of one extension. `data` is the **UTF‑8 JSON bytes** of the extension object. `version` is the optimistic concurrency token (the row's authoritative value; mirrored into `metadata.version` on read).

### 6.2 Store name format

```
/registry/<group>/<plural>/<name>     -- when group is non-empty
/registry/<plural>/<name>             -- when group is empty (core kinds)
```

Exactly the rule encoded by Halo's `ExtensionStoreUtil.buildStoreName`. `plural` is the lowercased plural noun from the `@GVK` annotation.

### 6.3 Data payload shape

Every row's `data` is JSON shaped like:

```json
{
  "apiVersion": "<group>/<version>",  // or "v1alpha1" when group is empty
  "kind": "<Kind>",
  "metadata": {
    "name": "string (= <name> in store path)",
    "generateName": "optional string",
    "labels": { "k": "v", "...": "..." },
    "annotations": { "k": "v", "...": "..." },
    "version": 7,                     // mirror of the row's version column
    "creationTimestamp": "2026-05-16T00:00:00Z",
    "deletionTimestamp": null,
    "finalizers": ["…"]
  },
  "spec":   { /* kind-specific */ },
  "status": { /* kind-specific */ }
}
```

This is the wire format that lands in the DB. Reading: `data` → JSON → typed struct, with `version` copied from row → `metadata.version`. Writing: typed struct → JSON → bytes, version constraint enforced by SQL.

### 6.4 GVKs we will support in v1

All groups, versions, and plurals **must be byte‑identical to Halo**:

| Group               | Version    | Kind                  | Plural                | Notes                                |
|---------------------|------------|-----------------------|-----------------------|--------------------------------------|
| `content.halo.run`  | `v1alpha1` | `Post`                | `posts`               | spec carries snapshot refs           |
| `content.halo.run`  | `v1alpha1` | `SinglePage`          | `singlepages`         |                                      |
| `content.halo.run`  | `v1alpha1` | `Tag`                 | `tags`                |                                      |
| `content.halo.run`  | `v1alpha1` | `Category`            | `categories`          |                                      |
| `content.halo.run`  | `v1alpha1` | `Snapshot`            | `snapshots`           | **Holds post content as patches**    |
| `content.halo.run`  | `v1alpha1` | `Comment`             | `comments`            |                                      |
| `content.halo.run`  | `v1alpha1` | `Reply`               | `replies`             |                                      |
| *(core, empty)*     | `v1alpha1` | `User`                | `users`               |                                      |
| *(core, empty)*     | `v1alpha1` | `Role`                | `roles`               |                                      |
| *(core, empty)*     | `v1alpha1` | `RoleBinding`         | `rolebindings`        |                                      |
| *(core, empty)*     | `v1alpha1` | `Menu`                | `menus`               |                                      |
| *(core, empty)*     | `v1alpha1` | `MenuItem`            | `menuitems`           |                                      |
| *(core, empty)*     | `v1alpha1` | `Setting`             | `settings`            |                                      |
| *(core, empty)*     | `v1alpha1` | `ConfigMap`           | `configmaps`          | also the **plugin scratch KV** kind  |
| *(core, empty)*     | `v1alpha1` | `Secret`              | `secrets`             |                                      |
| `storage.halo.run`  | `v1alpha1` | `Attachment`          | `attachments`         |                                      |
| `storage.halo.run`  | `v1alpha1` | `Group`               | `groups`              | attachment grouping                  |
| `storage.halo.run`  | `v1alpha1` | `Policy`              | `policies`            | local / s3 / etc.                    |
| `storage.halo.run`  | `v1alpha1` | `PolicyTemplate`      | `policytemplates`     |                                      |
| `theme.halo.run`    | `v1alpha1` | `Theme`               | `themes`              |                                      |
| `metrics.halo.run`  | `v1alpha1` | `Counter`             | `counters`            | view counts etc.                     |
| `auth.halo.run`     | `v1alpha1` | `AuthProvider`        | `authproviders`       |                                      |
| `security.halo.run` | `v1alpha1` | `PersonalAccessToken` | `personalaccesstokens`|                                      |
| `plugin.halo.run`   | `v1alpha1` | `Plugin`              | `plugins`             | **WASM plugin descriptors live here**|

Future kinds (notifications, RememberMeToken, ReverseProxy, Backup, etc.) get added through the **scheme registry** without any schema migration.

### 6.5 Post content model (Snapshots)

This is the only place where Halo's data model is non‑obvious. A `Post` does **not** carry its content. Instead, `PostSpec` holds three snapshot names:

```text
baseSnapshot      -- the "anchor" containing the raw text
headSnapshot      -- the working draft (most recent edit)
releaseSnapshot   -- what the public site currently shows
```

Each `Snapshot` holds:

- `rawType`: `markdown` | `html` | `json` | `asciidoc` | `latex` (we target markdown).
- `rawPatch`: diff or full text from the previous snapshot.
- `contentPatch`: rendered HTML diff (Halo precomputes HTML; we do the same).
- `parentSnapshotName`: chain link.

To render a post's current content rblog walks the chain from the snapshot referenced in `releaseSnapshot` (or `headSnapshot` for the editor's "preview draft" mode), composing patches into the final text. We implement this **once** in `rblog-content` and write a small fuzzer against a corpus exported from a real Halo install.

### 6.6 Optimistic concurrency rules (matches Halo)

| Operation | Behaviour                                                                                   |
|-----------|--------------------------------------------------------------------------------------------|
| INSERT    | `version` is `1` on the new row. `metadata.creationTimestamp` is set if absent.            |
| UPDATE    | `UPDATE extensions SET data = ?, version = version + 1 WHERE name = ? AND version = ?`. If 0 rows affected → `OptimisticLockException` → HTTP 409. On success the new version is reflected back into `metadata.version`. |
| DELETE    | `DELETE FROM extensions WHERE name = ? AND version = ?` with the same conflict semantics.   |
| Soft delete | Halo never `DELETE`s a "deleted" post immediately — it sets `metadata.deletionTimestamp` and a finalizer chain cleans up. We follow the same pattern. |

### 6.7 Listing and filtering

Halo cannot run rich SQL filters on JSON blobs at scale, so it maintains an **in‑process index engine** that scans the entire prefix on startup and rebuilds label / annotation / spec‑field indices in memory. **We do the same**, in `rblog-index`:

- One in‑memory index per `(GVK, indexed field)`. Sorted `BTreeMap<Vec<u8>, Vec<StoreName>>`.
- On startup, do `SELECT name, data, version FROM extensions WHERE name LIKE '/registry/<group>/<plural>/%'` for every registered scheme, deserialize, and populate indices.
- All writes go through the index engine which updates indices in lockstep (transactional in‑process; the DB stays the single source of truth).
- A label or annotation predicate against the engine returns a sorted list of store names, then we batch‑fetch from the DB.

This mirrors what Halo's `IndexEngine` / `IndexSpec` does. We **do not** need to copy Halo's internal index code; we own the runtime architecture inside, as long as the on‑disk representation is identical.

### 6.8 Migrations

There are **no per‑entity migrations**. The migration story is:

- One initial migration per dialect that creates the `extensions` table (Halo's DDL verbatim for MySQL, our SQLite equivalent).
- Schema evolution of individual kinds (`Post.spec` adding a field, etc.) is **inside the JSON blob** and is forward‑compatible: old documents are missing the new field, deserialization treats missing fields as `None` / default.
- Backfill jobs (e.g. compute a new index field on existing posts) run as **idempotent startup tasks**, not as SQL migrations.

```text
migrations/
├── mysql/    20260516000000_init.up.sql     -- the Halo CREATE TABLE statement
│             20260516000000_init.down.sql
└── sqlite/   20260516000000_init.up.sql     -- our SQLite equivalent
              20260516000000_init.down.sql
```

### 6.9 Why this is good for *us*, not just for migration

1. **Bug‑for‑bug Halo migration**: dump → restore works in either direction.
2. **No schema churn**: adding a field to `Post.spec` is a Rust struct change with `#[serde(default)]`, not a DB migration.
3. **Plugins can declare new kinds** without any DB changes — they just register a GVK with the scheme registry.
4. **The same primitive is the plugin scratch store**: a plugin uses `ConfigMap` kind with a name pattern like `<plugin-name>-<key>` — no extra table.
5. **Backup / restore is two SQL statements**: `SELECT name, data, version FROM extensions`.

The price we pay: complex filtering needs the in‑memory index engine (§6.7). That's acceptable — Halo runs the same way on production blogs with tens of thousands of posts.

---

## 7. Project layout (proposed)

```
rblog/
├── Cargo.toml                       # workspace manifest
├── rblog.example.toml               # commented config template
├── README.md
├── TECH_REPORT.md                   # this file
├── Dockerfile
├── docker-compose.yml               # mysql + adminer for dev
├── .editorconfig
├── .github/workflows/ci.yml
│
├── crates/
│   ├── rblog                        # main binary: wires everything, owns the Router
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── config.rs
│   │       ├── state.rs             # AppState (db pool, theme env, search index, caches)
│   │       └── router/
│   │           ├── mod.rs
│   │           ├── public.rs        # SSR routes
│   │           ├── api.rs           # /api/v1/* router (delegates to rblog-api)
│   │           ├── admin_assets.rs  # rust-embed + fallback to index.html
│   │           └── static_files.rs  # /upload, /themes/<n>/static
│   │
│   ├── rblog-scheme                 # Extension trait, Metadata, GVK, scheme registry
│   │   └── src/{extension,metadata,gvk,scheme,registry}.rs
│   │
│   ├── rblog-store                  # The one-table SQLx repository (mysql + sqlite)
│   │   ├── src/lib.rs               # ExtensionStore: list_by_prefix, fetch, create, update, delete
│   │   ├── src/mysql.rs
│   │   ├── src/sqlite.rs
│   │   └── src/converter.rs         # bytes <-> typed Extension (serde_json)
│   │
│   ├── rblog-index                  # In-memory secondary indices (label/annotation/field)
│   ├── rblog-content                # Halo-compatible kinds: Post, Snapshot (patch composition),
│   │                                 #   Tag, Category, Comment, Reply, SinglePage
│   ├── rblog-core                   # Domain services on top of rblog-store + rblog-index
│   │   └── src/{posts,pages,users,comments,categories,tags,menus,settings,attachments}.rs
│   │
│   ├── rblog-auth                   # password hashing, session, RBAC checks
│   ├── rblog-theme                  # MiniJinja env, theme loader, theme.yaml schema
│   ├── rblog-markdown               # comrak + ammonia + syntect adapter
│   ├── rblog-search                 # tantivy wrapper (index, query, reindex job)
│   ├── rblog-api                    # utoipa handlers + DTOs (the admin REST API)
│   ├── rblog-plugins                # wasmtime host: loader, sandbox, lifecycle, host fns
│   │   ├── wit/                     # WIT world definition (host + guest interface)
│   │   │   ├── world.wit
│   │   │   └── deps/wasi/...
│   │   └── src/{host,loader,events,routes,settings}.rs
│   └── rblog-cli                    # one-shot commands: `rblog migrate`, `rblog admin reset-password`,
│                                     # `rblog plugin install <path>`, `rblog plugin verify <path>`
│
├── migrations/
│   ├── mysql/V0001__init.up.sql ...
│   └── sqlite/V0001__init.up.sql ...
│
├── admin/                           # React SPA
│   ├── package.json
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── tailwind.config.ts
│   ├── index.html
│   ├── src/
│   │   ├── main.tsx
│   │   ├── App.tsx
│   │   ├── router/                  # @tanstack/react-router route tree
│   │   ├── stores/                  # zustand stores (theme, sidebar, ...)
│   │   ├── api/                     # generated TS client (do not edit by hand)
│   │   ├── components/
│   │   ├── layouts/                 # ProLayout shell, auth layout, ...
│   │   ├── routes/                  # one folder per top-level admin section
│   │   │   ├── auth/
│   │   │   ├── dashboard/
│   │   │   ├── posts/
│   │   │   ├── pages/
│   │   │   ├── categories/
│   │   │   ├── tags/
│   │   │   ├── comments/
│   │   │   ├── menus/
│   │   │   ├── attachments/
│   │   │   ├── themes/
│   │   │   ├── users/
│   │   │   ├── plugins/
│   │   │   └── settings/
│   │   ├── editor/                  # Lexical setup, nodes, plugins (slash menu, image, etc.)
│   │   └── i18n/
│   └── locales/
│
├── themes/
│   └── default/                     # ships with the binary; only theme in v1
│       ├── theme.yaml
│       ├── templates/
│       │   ├── index.html
│       │   ├── post.html
│       │   ├── archive.html
│       │   ├── tag.html
│       │   ├── category.html
│       │   ├── page.html
│       │   ├── 404.html
│       │   └── partials/{header,footer,pagination,seo,comments}.html
│       └── static/{css,js,img}/
│
├── plugins/                         # example / first-party plugins, built separately
│   ├── hello-world/                 # minimal Rust component plugin
│   │   ├── Cargo.toml
│   │   ├── wit/                     # imports rblog-plugins/wit/world.wit
│   │   └── src/lib.rs
│   └── README.md                    # plugin author guide
│
└── xtask/                           # custom Cargo commands (lint, sync, gen client)
```

---

## 8. Theme system

- A **theme** is a directory under `<work_dir>/themes/<name>/`.
- `theme.yaml` declares: name, version, author, supported `rblog` API version, custom settings schema, template entry points.
- Templates use **MiniJinja** (`{% block %}`, `{% extends %}`, `{{ post.title }}`).
- Built‑in context exposed to every template:
  - `site` (title, description, url, language, footer, ...)
  - `theme` (settings declared in `theme.yaml`)
  - `nav_menus` (rendered menu trees)
  - `page` (current request: type, params, pagination)
- Per‑view context (e.g. `post.html` gets `post`, `prev`, `next`, `related`, `comments_enabled`).
- **Custom functions / filters** (MiniJinja):
  - `url_for("post", slug=...)`, `image_thumbnail(url, "640")`, `markdown(raw)`, `date(d, "fmt")`, `excerpt(post, 200)`.
- **Live reload** in dev: `minijinja-autoreload` watches the active theme directory.

A theme switch is one DB write + one MiniJinja `Environment::set_loader` swap; no restart needed.

---

## 9. API surface (admin REST, /api/v1)

OpenAPI 3.1, all endpoints behind session/JWT auth and per‑resource permissions.

| Resource          | Endpoints (typical CRUD plus extras)                                                                 |
| ----------------- | ---------------------------------------------------------------------------------------------------- |
| `auth`            | `POST /auth/login`, `POST /auth/logout`, `POST /auth/refresh`, `POST /auth/setup` (first‑run)         |
| `users`           | `GET/POST/PATCH/DELETE /users`, `POST /users/{id}/password`, `POST /users/{id}/2fa`                  |
| `posts`           | `GET/POST/PATCH/DELETE /posts`, `POST /posts/{id}/publish`, `POST /posts/{id}/unpublish`, `GET /posts/{id}/revisions` |
| `pages`           | Same shape as posts                                                                                  |
| `categories`      | CRUD + `POST /categories/reorder`                                                                    |
| `tags`            | CRUD                                                                                                  |
| `comments`        | `GET`, `POST /comments/{id}/approve`, `POST /comments/{id}/spam`, `DELETE`                            |
| `menus`           | CRUD; `PUT /menus/{id}/items` (tree replace)                                                          |
| `attachments`     | `POST /attachments` (multipart), `GET /attachments`, `DELETE`, `POST /attachments/{id}/move`         |
| `themes`          | `GET /themes`, `POST /themes/install` (zip upload), `POST /themes/{name}/activate`, `PATCH /themes/{name}/settings` |
| `settings`        | `GET /settings`, `PATCH /settings`                                                                    |
| `search`          | `POST /search/reindex`, `GET /search?q=...`                                                          |
| `system`          | `GET /system/info`, `GET /system/healthz` (cluster), `GET /system/metrics` (Prometheus)              |

Public, unauthenticated:

- `POST /api/public/comments` (with rate limit + Akismet‑style hook).
- `GET /api/public/search?q=...` (optional, Tantivy‑backed).

---

## 10. Public site (SSR) routes & SEO

| Route                             | Template            | Notes                                                  |
| --------------------------------- | ------------------- | ------------------------------------------------------ |
| `/`                               | `index.html`        | Latest posts, pinned posts, pagination via `?page=`.   |
| `/archives` / `/archives/page/N`  | `archive.html`      |                                                        |
| `/archives/:slug`                 | `post.html`         | Canonical URL pattern (matches Halo).                  |
| `/categories` / `/categories/:slug` | `category.html`   |                                                        |
| `/tags` / `/tags/:slug`           | `tag.html`          |                                                        |
| `/p/:slug`                        | `page.html`         | Static pages.                                          |
| `/authors/:username`              | `author.html`       |                                                        |
| `/feed/rss.xml`, `/feed/atom.xml` | `feed/*.xml`        | s‑maxage 5 min.                                        |
| `/sitemap.xml`, `/sitemap-*.xml`  | `sitemap/*.xml`     | Paginated sitemap if >50k URLs.                        |
| `/robots.txt`                     | static template     | Includes sitemap URL.                                  |
| `/search`                         | `search.html`       | Posts SSR’d + JS enhancement.                          |

SEO defaults baked into every template via a `partials/seo.html`:

- Canonical URL, Open Graph, Twitter Card, JSON‑LD (`BlogPosting`, `BreadcrumbList`, `Person`).
- `<link rel="alternate" type="application/rss+xml">`.
- Auto‑generated `<meta name="description">` from excerpt.

Compression / caching:

- `tower-http::CompressionLayer` (br, gzip, deflate, zstd).
- ETag + `If-None-Match` for all SSR HTML (cheap hash over rendered body).
- Public route handlers return `Cache-Control: public, s-maxage=60, stale-while-revalidate=300` by default; per‑route override available.

---

## 11. Security model

- **Argon2id** with per‑password salt, default parameters tuned for ~250 ms hash time.
- Cookies: `HttpOnly`, `Secure`, `SameSite=Lax` (admin) or `Strict` for sensitive flows.
- CSRF: double‑submit cookie token, validated on all non‑GET admin requests.
- Origin check on `/api/v1/*` for same‑origin enforcement.
- Rate limiting (`tower_governor`):
  - `POST /api/v1/auth/login`: 5/min/IP.
  - `POST /api/public/comments`: 10/min/IP.
  - `POST /api/v1/users` signup (if enabled): 3/h/IP.
  - Mirrors Halo's Resilience4j defaults.
- Content sanitization: `ammonia` on every stored HTML and every comment.
- Audit log for every admin write (`audit_logs` table).
- Strict CSP for the admin SPA: `default-src 'self'; img-src 'self' data: https:; script-src 'self'; style-src 'self' 'unsafe-inline';`. The public site gets a more permissive CSP that themes can extend via `theme.yaml`.

---

## 12. Performance targets

Single 4‑core VM with MySQL on localhost; SSR public site, 1 KB markdown average:

| Metric                              | Target            | Why it's achievable                                  |
| ----------------------------------- | ----------------- | ---------------------------------------------------- |
| Cold start to first request         | < 50 ms           | Static binary, no JIT.                               |
| `GET /` p99 latency                 | < 5 ms (cache hit) / < 20 ms (cache miss) | MiniJinja + moka cache.        |
| `GET /archives/:slug` p99           | < 8 ms            | One indexed lookup + cached HTML.                    |
| Sustained RPS on public site        | > 10 000          | Tokio + h2 + zero‑allocation hot path.                |
| Admin SPA initial bundle (gzip)     | < 450 KB          | Vite + antd tree‑shaking + Lexical core only + iconify on‑demand. antd is heavier than Naive UI, hence the slightly higher budget. |
| Memory footprint at idle            | < 80 MB           | No JVM, no Node, no Python.                          |

---

## 13. Build, test, deploy

### 13.1 Toolchain

- Rust stable (latest), pinned with `rust-toolchain.toml`.
- `cargo-make` or a tiny `xtask` for orchestration commands.
- Node 22 + pnpm for `admin/`.

### 13.2 Commands (proposed)

```bash
# dev
cargo run --bin rblog -- --config rblog.toml          # backend
pnpm -C admin dev                                     # admin SPA HMR
pnpm -C admin api-client:gen                          # regenerate TS client from OpenAPI

# build
pnpm -C admin build                                   # admin assets into admin/dist/
cargo build --release --features embed-admin          # binary with admin assets embedded

# checks
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
pnpm -C admin typecheck && pnpm -C admin lint && pnpm -C admin test:unit

# migrations
cargo run -p rblog-cli -- migrate run
cargo run -p rblog-cli -- migrate revert
```

### 13.3 CI

- GitHub Actions matrix: `{ubuntu-latest} × {mysql:8, sqlite}` for backend tests.
- Cache: `cargo` registry + `target/` + pnpm store + `~/.cache/sqlx-cli`.
- Release workflow:
  1. Build `admin/dist/`.
  2. Build the binary for x86_64‑linux, aarch64‑linux (musl + glibc), x86_64‑darwin, aarch64‑darwin, x86_64‑windows.
  3. Push a multi‑arch Docker image (scratch + ca‑certs + the binary).

### 13.4 Docker

```Dockerfile
# Stage 1: admin SPA
FROM node:22-alpine AS admin
WORKDIR /src
COPY admin/ ./admin/
RUN corepack enable && cd admin && pnpm install --frozen-lockfile && pnpm build

# Stage 2: rust binary (with embedded admin)
FROM rust:1-bookworm AS builder
WORKDIR /src
COPY . .
COPY --from=admin /src/admin/dist ./admin/dist
RUN cargo build --release --features embed-admin --bin rblog

# Stage 3: runtime
FROM gcr.io/distroless/cc-debian12
COPY --from=builder /src/target/release/rblog /rblog
COPY themes/default /themes/default
ENV RBLOG__WORK_DIR=/data
VOLUME ["/data"]
EXPOSE 8090
ENTRYPOINT ["/rblog"]
```

### 13.5 Configuration

```toml
# rblog.toml
[server]
host = "0.0.0.0"
port = 8090
work_dir = "/data"

[database]
# Either:
url = "mysql://user:pass@localhost:3306/rblog"
# Or:
# url = "sqlite:///data/rblog.db"
max_connections = 10

[security]
session_secret = "REPLACE-ME-32-bytes-base64"
cookie_secure = true

[storage]
backend = "local"           # or "s3"
# [storage.s3]
# bucket = "..."

[search]
enabled = true
index_dir = "/data/search"

[theme]
active = "default"
```

Every key is overridable via env: `RBLOG__SERVER__PORT=9000`.

---

## 14. Plugin runtime (WASM, v1)

> Halo loads JARs at runtime through its own ClassLoader hierarchy. Rust can't do that natively, but we can do **better and safer**: every plugin is a sandboxed WebAssembly component. No JIT, no crashy native code, capability‑gated I/O, deterministic loading, multi‑language authoring.

### 14.1 Tech choices

- **Engine:** `wasmtime` with the Component Model (`component-model = true`), `async`, Cranelift backend, and the **pooling allocator** for predictable per‑instance memory cost.
- **Standard:** **WASI Preview 2** (`wasi:cli`, `wasi:filesystem` scoped, `wasi:http`, `wasi:clocks`, `wasi:random`). No `wasi:sockets` exposed in v1.
- **Interface:** **WIT** (WebAssembly Interface Types) — a single `world.wit` describes everything plugins can import (host functions) and export (lifecycle + handlers).
- **Bindings:** `wit-bindgen` generates type‑safe glue for both host and guest.
- **Authoring languages (v1):** Rust (first‑class), TinyGo, JavaScript via `componentize-js`, Python via `componentize-py`.

### 14.2 Plugin package layout

```
<work_dir>/plugins/<name>/
├── plugin.yaml         # manifest: name, version, capabilities, settings schema, ui entry
├── plugin.wasm         # the component
├── ui/                 # optional: SPA bundle loaded into the admin (entry: index.js)
└── assets/             # optional: public static assets served read-only
```

`plugin.yaml` (excerpt):

```yaml
api_version: rblog.app/v1
name: word-count
version: 0.3.1
author: jane@example.com
description: Adds reading-time + word-count to every post on save.
capabilities:
  - events: [post.before_save, post.after_publish]
  - storage: kv
  - http_outbound: false
  - admin_ui: false
settings_schema:
  fields:
    - name: words_per_minute
      type: integer
      default: 200
```

### 14.3 The WIT world (sketch)

```wit
// crates/rblog-plugins/wit/world.wit
package rblog:plugin@1.0.0;

interface host {
    // logging
    log: func(level: string, msg: string);

    // namespaced KV store backed by plugin_kv
    kv-get: func(key: string) -> option<list<u8>>;
    kv-set: func(key: string, value: list<u8>);
    kv-delete: func(key: string);

    // settings (read-only, populated by admin UI)
    setting-get: func(key: string) -> option<string>;

    // posts (read-only projection, no raw SQL)
    post-get: func(id: u64) -> option<post-view>;
    post-search: func(q: string, limit: u32) -> list<post-view>;

    // emit a structured event back into rblog (audit log, metrics)
    emit-event: func(name: string, payload: string);

    record post-view {
        id: u64, slug: string, title: string,
        excerpt: string, status: string, published-at: option<u64>,
    }
}

interface events {
    // The host calls these on the guest at the right moment.
    on-init: func() -> result<_, string>;
    on-shutdown: func();

    on-post-before-save: func(draft: string) -> result<string, string>;
    on-post-after-publish: func(post-id: u64);
    on-comment-created: func(comment-id: u64);
}

interface routes {
    // Optional: register an HTTP route handler. The host mounts these under
    // /api/plugins/<plugin-name>/* and forwards via wasi:http.
    handle: func(req: http-request) -> http-response;
    record http-request  { method: string, path: string, headers: list<tuple<string,string>>, body: list<u8> }
    record http-response { status: u16, headers: list<tuple<string,string>>, body: list<u8> }
}

interface template-fns {
    // MiniJinja-callable functions, e.g. {{ plugin_word_count(post) }}
    list-functions: func() -> list<string>;
    call: func(name: string, json-args: string) -> result<string, string>;
}

world plugin {
    import host;
    export events;
    export routes;
    export template-fns;
}
```

### 14.4 Lifecycle

1. **Discover** — scan `<work_dir>/plugins/*/plugin.yaml`.
2. **Validate** — manifest schema, capability claims, SemVer compat with the host's `rblog.app/v1`.
3. **Verify** — compute SHA‑256 of `plugin.wasm`, compare to `plugins.wasm_sha256`. If changed, mark as needs‑review.
4. **Compile** — `Component::from_file` with cached, AOT‑compiled artifacts on disk under `<work_dir>/cache/wasm/`.
5. **Instantiate** — one `Store` per plugin instance with:
   - Memory limit (default 32 MiB, configurable).
   - Fuel‑metered execution (default 100M units per request).
   - Epoch‑based interruption with a 5 s wall clock timeout for any single export call.
   - `wasmtime-wasi` preopens scoped to a plugin‑private directory (`<work_dir>/plugins/<name>/scratch/`).
6. **`on-init`** — call the guest export; failures move the plugin to `errored` and surface in the admin.
7. **Wire up** — register event subscriptions, plugin HTTP routes (`/api/plugins/<name>/*`), MiniJinja template functions exposed by the plugin, and admin UI assets.
8. **`on-shutdown`** — called on disable / unload / graceful shutdown.

Hot reload: a plugin can be disabled, the `.wasm` swapped, and re‑enabled without restarting `rblog`.

### 14.5 Capability model & sandboxing

Capabilities are declared in `plugin.yaml` and **enforced at the host boundary**:

| Capability          | Enforced by                                                              |
| ------------------- | ------------------------------------------------------------------------ |
| `events`            | Host only dispatches subscribed events; unlisted exports are ignored.    |
| `storage: kv`       | Required to use `kv-*` host fns. Backed by **`ConfigMap` extensions** with name pattern `plugin-<plugin>-<key>`. No extra table. |
| `http_outbound`     | `wasi:http/outgoing-handler` is only added to the linker when enabled.   |
| `admin_ui`          | `ui/` is only served if true.                                            |
| `template_fns`      | Required to expose MiniJinja functions.                                  |
| `route`             | Required to register `/api/plugins/<name>/*`; route prefix is fixed.     |
| `fs: scratch`       | One preopened dir, never the host filesystem.                            |
| `register_gvk`      | Required if the plugin defines new Extension kinds (its own group); host validates uniqueness with the scheme registry. |
| `dangerous: db_raw` | Reserved; **disabled in v1** — no plugin gets raw DB access.             |

Defaults are deny‑all. The admin UI lists the capabilities a plugin requests, the user explicitly approves them at install time, and any later increase triggers a re‑approval prompt.

### 14.6 Admin integration

- `/console/plugins` page: install (zip upload), enable / disable, settings (rendered from `settings_schema` via `@rjsf/antd`), capability review, view error log.
- Per‑plugin SPA assets (if present) are loaded into the admin via dynamic ES module import from `/console/plugins/<name>/index.js`. The plugin runs in an iframe **or** in a same‑origin shadow root with a published JS API (`window.rblog.plugin.*`) for navigation, toasts, and API calls.
- The OpenAPI client is **not** exposed to plugins directly; they receive a typed proxy with permission filtering.

### 14.7 Performance budget

- Cold instantiate: < 30 ms with AOT cache, < 200 ms without.
- Per‑request overhead for an event handler that does only KV: < 200 µs.
- Memory: 32 MiB default cap per instance, pooled. 100 plugin instances ≈ 3.2 GiB worst‑case, but the pooling allocator keeps the working set far lower.

### 14.8 Risk & mitigation

| Risk                                      | Mitigation                                                                                  |
| ----------------------------------------- | ------------------------------------------------------------------------------------------- |
| Plugin hangs a request                    | Fuel + epoch deadline, fail with HTTP 504 to the caller, mark plugin errored after N hits.   |
| Plugin leaks memory                       | Pooling allocator + hard `max_memory_size`, instance recycled on threshold.                 |
| Slow `on_init` blocks startup             | Plugins are instantiated in parallel; failures don't block the rest of the boot.            |
| Compromised plugin attempts DB access     | No raw SQL host function; capability gating; CSP on admin UI.                               |
| Plugin abuses outbound HTTP for SSRF      | `http_outbound` capability + a host‑side denylist (private CIDRs, link‑local, metadata IPs). |

### 14.9 Author DX

Minimal Rust plugin:

```rust
// plugins/word-count/src/lib.rs
wit_bindgen::generate!({ world: "plugin", path: "./wit" });

struct Plugin;
impl exports::rblog::plugin::events::Guest for Plugin {
    fn on_init() -> Result<(), String> {
        rblog::plugin::host::log("info", "word-count plugin ready");
        Ok(())
    }
    fn on_post_before_save(draft: String) -> Result<String, String> {
        // ...count words, attach metadata, return possibly-modified draft...
        Ok(draft)
    }
    // other no-ops elided
}
export!(Plugin);
```

Compile: `cargo build --target wasm32-wasip2 --release`. Drop the `.wasm` and a `plugin.yaml` into `<work_dir>/plugins/word-count/`. Done.

---

## 15. Roadmap (phased)

### Phase 1 — MVP (target: ~8–10 weeks of focused work)

- [ ] Backend skeleton: Axum router, AppState, config, logging, graceful shutdown.
- [ ] SQLx + dual‑dialect migrations + repository layer.
- [ ] Auth: setup wizard, login, sessions, RBAC, password hashing.
- [ ] Posts + categories + tags CRUD via `/api/v1/*` with OpenAPI.
- [ ] Markdown pipeline (comrak + ammonia + syntect) with cached HTML.
- [ ] Attachments (local FS) with image thumbnail generation.
- [ ] **Built‑in comments**: public submission API with rate‑limit + simple heuristic spam check, full moderation in admin (pending / approved / spam / trash, bulk actions, threaded replies).
- [ ] Theme loader + MiniJinja engine + **default theme only** (home, post, archive, tag, category, page, comments partial, RSS, sitemap).
- [ ] Admin SPA shell: **React 19 + Vite + TypeScript + Ant Design (+ pro-components) + Tailwind + @tanstack/react-router + zustand + react-query** + generated TS client.
- [ ] Admin views: login, dashboard, post list/editor (**Lexical** + markdown round‑trip), pages, categories, tags, comments moderation, menus, users, attachments, settings, plugins.
- [ ] **WASM plugin runtime (§14):** loader, sandbox, lifecycle, event bus, KV store, route forwarding, template functions, plugin admin UI, install / enable / disable / settings flows. Ship one reference plugin (`hello-world`).
- [ ] Single‑binary release with embedded admin + Dockerfile.

### Phase 2 — Polish & growth

- [ ] Search (Tantivy) + admin reindex job.
- [ ] S3‑compatible attachment backend.
- [ ] Multi‑language (vue‑i18n + Fluent on the backend).
- [ ] Plugin: more reference plugins (sitemap‑extras, OG image generator, RSS importer).
- [ ] Sitemap pagination, RSS / Atom polish, Webmention support.
- [ ] Audit log UI; 2FA.

### Phase 3 — Beyond Halo

- [ ] Lexical collaborative editing (`@lexical/yjs` + Hocuspocus server module).
- [ ] AI helpers (title suggestions, summaries) via a pluggable LLM provider trait — implemented as a built‑in plugin.
- [ ] PostgreSQL support.
- [ ] Public plugin registry / signed plugin distribution.

---

## 16. Summary

| Layer            | Choice                                                                                |
| ---------------- | ------------------------------------------------------------------------------------- |
| Runtime          | Tokio                                                                                  |
| HTTP             | **Axum** + tower / tower‑http                                                          |
| DB               | **SQLx** with **MySQL** and **SQLite** dialects, native migrations                     |
| SSR templates    | **MiniJinja** (Jinja2) with autoreload in dev, per‑theme loader                        |
| Markdown         | comrak + ammonia + syntect                                                             |
| Auth             | argon2 + tower‑sessions + axum‑login + tower‑governor rate limit                       |
| API docs         | utoipa (OpenAPI 3.1) → TS client generated for the admin SPA                           |
| Search           | Tantivy                                                                                |
| Storage          | `object_store` (local FS default, S3 optional)                                         |
| Admin SPA        | **React 19 + Vite + TypeScript + Ant Design (+ pro-components) + TailwindCSS + zustand + react‑query + @tanstack/react-router** |
| Editor           | **Lexical** (`@lexical/react`) with markdown as the canonical wire format              |
| Comments         | Built‑in, with moderation pipeline + rate limit + heuristic spam check                 |
| Plugins (v1)     | **wasmtime + WASI Preview 2 + Component Model**, capability‑gated, deny‑all default    |
| Themes (v1)      | One default theme; theme installer deferred to Phase 2                                 |
| Embedding        | `rust-embed` ships the SPA inside the release binary                                   |
| Deploy           | One binary, one port, optional Docker image — MySQL or SQLite, your call               |

This stack gives us the speed and footprint of a Go binary, the type safety of Rust, the SSR/SEO story that Halo already proved works, a familiar admin UX for anyone migrating from a Halo install, and a sandboxed plugin runtime that's strictly safer than Halo's JAR loader.
