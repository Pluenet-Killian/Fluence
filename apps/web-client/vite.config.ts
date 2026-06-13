// SPDX-License-Identifier: AGPL-3.0-only

import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";

// The composer is served by the hub as same-origin static files (PLAN 5.3), so
// the build is a plain SPA. The SDK is aliased to its source for a build-order-
// free monorepo dev/typecheck experience.
export default defineConfig({
  resolve: {
    alias: {
      "@fluence/sdk": fileURLToPath(new URL("../../packages/sdk/src/index.ts", import.meta.url)),
    },
  },
  build: {
    outDir: "dist",
    sourcemap: true,
    target: "es2022",
  },
  // Dev only: proxy the hub so the browser sees a single origin (no CORS); in
  // production the hub serves the built files directly (same origin).
  server: {
    proxy: {
      "/api": { target: "http://127.0.0.1:7411", changeOrigin: true },
      "/pair": { target: "http://127.0.0.1:7411", changeOrigin: true },
      "/ws": { target: "http://127.0.0.1:7411", ws: true, changeOrigin: true },
    },
  },
});
