// INPUT:  Playwright runner, local Vite server, and installed Chrome
// OUTPUT: Deterministic desktop visual-test configuration
// POS:    Browser-level acceptance boundary for Hub page and overlay states

import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  retries: 0,
  workers: 1,
  reporter: "line",
  use: {
    ...devices["Desktop Chrome"],
    baseURL: "http://127.0.0.1:1421",
    channel: "chrome",
    colorScheme: "dark",
    reducedMotion: "reduce",
    viewport: { width: 1440, height: 900 },
  },
  expect: {
    toHaveScreenshot: {
      animations: "disabled",
      caret: "hide",
      scale: "css",
    },
  },
  webServer: {
    command: "npm run dev -- --host 127.0.0.1",
    port: 1421,
    reuseExistingServer: true,
    timeout: 120_000,
  },
});
