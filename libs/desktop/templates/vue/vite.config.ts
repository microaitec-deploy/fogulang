import vue from "@vitejs/plugin-vue";
import { defineConfig } from "vite";

// https://vite.dev/config/ — base "./": assets load from the app's own origin
// (Tauri's protocol); fogu desktop build embeds build.outDir ("dist").
export default defineConfig({
  plugins: [vue()],
  base: "./",
  build: { outDir: "dist" },
  clearScreen: false,
});
