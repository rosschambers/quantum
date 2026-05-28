import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  // Use relative base so emitted asset URLs (./assets/index-XXX.js) resolve
  // against the document URL (quantum://theme/default/views/launcher/index.html)
  // — Vite's URL normalization doesn't understand custom schemes, so any
  // absolute base gets stripped to root-relative paths.
  base: './',
  plugins: [svelte()],
  build: {
    outDir: 'dist',
    rollupOptions: {
      output: {
        manualChunks: undefined,
      },
    },
  },
});
