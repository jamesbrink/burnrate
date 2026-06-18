import react from "@vitejs/plugin-react";
import { browserslistToTargets } from "lightningcss";
import { configDefaults, defineConfig } from "vitest/config";

const cssTargets = browserslistToTargets([
  "chrome 108",
  "firefox 102",
  "safari 15",
]);

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  css: {
    transformer: "lightningcss",
    lightningcss: {
      targets: cssTargets,
    },
  },
  build: {
    cssMinify: "lightningcss",
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    watch: {
      ignored: ["**/.direnv/**", "**/dist/**", "**/gen/**", "**/target/**"],
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: "./vitest.setup.ts",
    exclude: [...configDefaults.exclude, ".direnv/**"],
    coverage: {
      provider: "v8",
      reporter: ["text", "lcov"],
      reportsDirectory: "coverage/ui",
      thresholds: {
        branches: 80,
        functions: 80,
        lines: 80,
        statements: 80,
      },
    },
  },
});
