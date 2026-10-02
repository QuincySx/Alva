// INPUT:  vitest config merged with the Vite React config
// OUTPUT: jsdom test environment for the Hub Desktop web layer
// POS:    Closes the acceptance gap "web/ has no test runner" (ACCEPTANCE.md §8).

import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    // Keep raw token/style sources available for appearance contract tests.
    css: { include: [/styles\/(tokens|primitives)\.css/] },
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
