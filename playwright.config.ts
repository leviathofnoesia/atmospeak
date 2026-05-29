import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  use: {
    baseURL: "http://127.0.0.1:1420",
    trace: "on-first-retry",
    ...devices["Desktop Chrome"],
  },
  webServer: {
    command: "bun run dev -- --host 127.0.0.1",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: true,
  },
});
