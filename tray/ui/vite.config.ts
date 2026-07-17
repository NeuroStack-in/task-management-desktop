import { fileURLToPath, URL } from "node:url";

import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The tray panel is loaded by Tauri from `devUrl` in dev and from `dist/` in a bundle.
// Port is pinned because tauri.conf.json hardcodes it.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Mirrors the `@/*` -> `./src/*` alias in tsconfig.json (Vite doesn't read tsconfig paths).
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  // Tauri owns the terminal; don't wipe its output.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "chrome105",
  },
});
