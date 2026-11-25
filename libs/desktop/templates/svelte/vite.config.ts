import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

// https://vite.dev/config/ — base "./": assets load from the app's own origin
// (Tauri's protocol); fogu desktop build embeds build.outDir ("dist").
export default defineConfig({
  plugins: [svelte()],
  base: "./",
  build: { outDir: "dist" },
  clearScreen: false,
});
