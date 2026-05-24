import path from "node:path";
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["tests/**/*.test.ts"],
  },
  resolve: {
    alias: {
      "harmont/toolchains": path.resolve(__dirname, "src/toolchains/index.ts"),
      harmont: path.resolve(__dirname, "src/index.ts"),
    },
  },
});
