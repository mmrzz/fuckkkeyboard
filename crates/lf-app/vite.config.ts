/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The React settings window lives in ui/; Tauri embeds the build from dist/.
export default defineConfig({
  root: "ui",
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { outDir: "../dist", emptyOutDir: true, target: "es2021" },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.tsx"],
    cache: false,
  },
});
