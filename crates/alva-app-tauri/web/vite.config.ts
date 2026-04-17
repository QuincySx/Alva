import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed port and relative asset paths.
// https://tauri.app/v2/reference/config/#build
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: ["es2021", "chrome110", "safari15"],
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    // Multi-page: main window (index.html) + standalone Inspector window
    // (inspector.html). Tauri opens the inspector window with the second
    // HTML file as the entry.
    rollupOptions: {
      input: {
        main: "index.html",
        inspector: "inspector.html",
      },
    },
  },
});
