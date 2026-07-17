import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

// Tauri drives `npm run dev`/`build` (see tauri.conf.json). Fixed dev port; output → dist/
// (frontendDist). Don't clear the screen so Tauri's logs stay visible.
export default defineConfig({
  plugins: [preact()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { outDir: "dist", target: "esnext", emptyOutDir: true },
});
