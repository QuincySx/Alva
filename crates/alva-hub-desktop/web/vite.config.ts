// INPUT:  vite, @vitejs/plugin-react
// OUTPUT: Hub Desktop Vite development/build configuration
// POS:    Serves the React frontend on the dedicated Tauri development port.

import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1421,
    strictPort: true,
  },
  build: {
    target: ["es2021", "chrome110", "safari15"],
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
