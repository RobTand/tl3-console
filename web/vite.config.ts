import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    // `npm run dev` against a console on this machine, for example the e2e fixture.
    // The console answers only to its own Host, so the proxy sends the target's.
    proxy: {
      "/api": {
        target: process.env.TL3_CONSOLE_URL ?? "http://127.0.0.1:8034",
        changeOrigin: true,
      },
    },
  },
  build: { outDir: "dist", sourcemap: false },
  test: {
    environment: "jsdom",
    globals: true,
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["src/test-setup.ts"],
  },
});
