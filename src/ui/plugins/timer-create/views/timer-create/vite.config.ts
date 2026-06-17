import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import path from "path";

export default defineConfig({
  plugins: [svelte()],
  // See ../../../launcher/views/launcher/vite.config.ts for why we use a
  // relative base instead of a quantum:// URL.
  base: "./",
  build: {
    outDir: "dist",
    rollupOptions: { output: { manualChunks: undefined } },
  },
  resolve: {
    alias: {
      "@quantum/client": path.resolve(__dirname, "../../../../packages/client/src/index.ts"),
    },
  },
  test: {
    environment: "jsdom",
  },
});
