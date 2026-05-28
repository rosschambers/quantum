import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  base: "quantum://theme/default/views/widgets/clock/",
  build: {
    outDir: "dist",
    rollupOptions: { output: { manualChunks: undefined } },
  },
});
