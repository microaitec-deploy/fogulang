import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// https://vite.dev/config/ — base "./": assets load from the app's own origin
// (Tauri's protocol); fogu desktop build embeds build.outDir ("dist").
export default defineConfig({
  plugins: [react()],
  base: "./",
  build: { outDir: "dist" },
  clearScreen: false,
});
