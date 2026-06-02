import react from "@vitejs/plugin-react";
import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    strictPort: false,
  },
  test: {
    environment: "jsdom",
    setupFiles: "./vitest.setup.ts",
    exclude: [...configDefaults.exclude, ".direnv/**"],
  },
});
