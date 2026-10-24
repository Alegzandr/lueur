import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Vite config tuned for Tauri: fixed dev port, no clearing of the Rust logs,
// and a build target matching the platform webviews.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  build: {
    // Tauri's minimum supported webviews.
    target: ["es2021", "chrome100", "safari15"],
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    // The app, and the update splash shown before it (see updater.rs).
    rollupOptions: {
      input: {
        main: "index.html",
        splash: "splash.html",
      },
    },
  },
});
