import path from "node:path";

import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "happy-dom",
    setupFiles: ["./vitest.setup.ts"],
  },
  resolve: {
    alias: {
      "@ps": path.resolve(__dirname, "./lib"),
      "@": path.resolve(__dirname, "."),
    },
  },
});
