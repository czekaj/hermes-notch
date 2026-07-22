import { defineConfig } from "vite";

// The Rust side (src-tauri/tauri.conf.json) expects the dev server on 5173 and
// the production bundle in app/dist. Both are pinned here — do not change without
// coordinating with the Tauri config.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    target: "safari15",
    emptyOutDir: true,
  },
});
