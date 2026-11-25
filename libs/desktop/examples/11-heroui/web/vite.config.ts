import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// https://vite.dev/config/ — base "./": assets load from the app's own origin
// (Tauri's protocol); fogu desktop build embeds build.outDir ("dist").
// Tailwind CSS 4 runs as a Vite plugin: no tailwind.config file is needed.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: "./",
  build: { outDir: "dist" },
  clearScreen: false,
});
