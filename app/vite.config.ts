import { defineConfig } from "vite";

// The Rust side (src-tauri/tauri.conf.json) expects the dev server on 1420 and
// the production bundle in app/dist. Both are pinned here — do not change without
// coordinating with the Tauri config. 1420 is the Tauri-conventional dev port,
// chosen to stay clear of the node-ecosystem defaults (3000s, 5173, 8080).
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    target: "safari15",
    emptyOutDir: true,
  },
});
