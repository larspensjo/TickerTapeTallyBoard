import { playwright } from "@vitest/browser-playwright";
import { defineConfig } from "vitest/config";

// Computed-style guards run in the installed Chrome (channel "chrome"), so no
// Playwright browser download is ever needed. jsdom cannot resolve the real
// cascade or custom properties, which is what these tests protect.
export default defineConfig({
  test: {
    include: ["src/**/*.browser.test.tsx"],
    css: { include: [/.+/] },
    browser: {
      enabled: true,
      headless: true,
      provider: playwright({ launchOptions: { channel: "chrome" } }),
      instances: [{ browser: "chromium" }],
    },
  },
});
