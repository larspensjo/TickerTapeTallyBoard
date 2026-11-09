import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    // Browser tests need a real browser; run them with `npm run test:browser`.
    exclude: [...configDefaults.exclude, "src/**/*.browser.test.tsx"],
    globals: false,
    setupFiles: ["src/test/setup.ts"],
  },
});
