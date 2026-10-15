import { defineConfig } from 'vitest/config';
import tailwindcss from '@tailwindcss/vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  publicDir: 'static',
  server: {
    proxy: {
      '/api': 'http://localhost:8080',
    },
  },
  test: {
    // `accent.test.ts` reads app.css as text to check the colour tokens; by default Vitest
    // returns an empty module for CSS.
    css: { include: [/app\.css/] },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
});
