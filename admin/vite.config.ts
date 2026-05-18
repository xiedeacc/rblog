import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// rblog admin SPA — Vite config.
//
// `base = "/admin/"` matches the path the Rust server mounts the SPA at.
// `proxy` lets `pnpm dev` hit the real backend on :8080 for `/api/*` calls.
// `build.outDir = "dist"` is what `rblog-http` embeds (and serves at
// runtime when `embed-admin` is enabled).
export default defineConfig({
  base: "/admin/",
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8080",
        changeOrigin: false,
        ws: false,
      },
      "/uploads": "http://127.0.0.1:8080",
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    target: "es2022",
    chunkSizeWarningLimit: 1500,
  },
});
