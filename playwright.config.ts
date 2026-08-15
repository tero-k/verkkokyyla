import { defineConfig } from "@playwright/test";

// E2E suite runs against the Vite dev server. Tauri IPC is mocked per-test
// in later todos; the scaffold ships only a placeholder spec.
export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  webServer: {
    command: "npm run dev",
    url: "http://localhost:1420",
    reuseExistingServer: true,
    timeout: 120_000,
  },
  use: {
    baseURL: "http://localhost:1420",
  },
});
