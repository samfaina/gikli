import { defineConfig } from "vite";

// The UI lives in ui/ and is served to the Tauri window (see src-tauri/tauri.conf.json).
export default defineConfig({
  root: "ui",
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    target: "es2022",
  },
});
