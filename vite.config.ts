/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  // Tauri expects a fixed dev port and a clean console.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // The Rust target dir is written/locked by cargo during `tauri dev`;
      // watching it crashes Vite on Windows with EBUSY.
      ignored: ["**/src-tauri/**", "**/.omo/**", "**/.codegraph/**"],
    },
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
