import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  // See launcher/vite.config.ts for why we use relative base instead of
  // a quantum:// URL.
  base: "./",
  build: {
    outDir: "dist",
    rollupOptions: { output: { manualChunks: undefined } },
  },
});
