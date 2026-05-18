# rblog admin SPA

React 19 + Vite + TypeScript + Ant Design v5 + Lexical.

## Development

```bash
pnpm install
pnpm dev          # http://127.0.0.1:5173 (proxies /api to :8080)
```

Start the rblog backend in another terminal:

```bash
cargo run -p rblog
```

## Build

```bash
pnpm build        # outputs to admin/dist/
```

The Rust binary can serve this in two modes:

- **Embedded** (production): rebuild rblog with `--features embed-admin`;
  the contents of `dist/` are baked into the executable.
- **Disk-served** (dev): set `paths.admin_dist = "./admin/dist"` in
  `rblog.toml`; rblog serves from disk via `ServeDir`.

## Codegen

The TypeScript client in `src/api/client.ts` is hand-written. To generate
schema types from the live `/api/admin/openapi.json`:

```bash
pnpm gen:api      # writes src/api/schema.ts
```
