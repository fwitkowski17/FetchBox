import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';
import { defineConfig, type Plugin } from 'vitest/config';
import tailwindcss from '@tailwindcss/vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

/**
 * Write `<file>.br` and `<file>.gz` next to every text file of the build. The server embeds them
 * and serves the variant the browser accepts, so it never compresses anything at request time
 * (it may be a Raspberry Pi). Uses Node's own zlib: no extra dependency, and Node is only needed
 * at build time. Files that do not get smaller are left alone.
 */
function precompress(): Plugin {
  const TEXT = /\.(html|js|css|svg|json|webmanifest|txt)$/;
  const walk = (dir: string): string[] =>
    readdirSync(dir).flatMap((name) => {
      const path = join(dir, name);
      return statSync(path).isDirectory() ? walk(path) : [path];
    });
  return {
    name: 'precompress',
    apply: 'build',
    closeBundle() {
      for (const file of walk('dist').filter((f) => TEXT.test(f))) {
        const raw = readFileSync(file);
        if (raw.length < 256) continue; // not worth a second file
        const br = brotliCompressSync(raw, {
          params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: raw.length },
        });
        const gz = gzipSync(raw, { level: 9 });
        if (br.length < raw.length) writeFileSync(`${file}.br`, br);
        if (gz.length < raw.length) writeFileSync(`${file}.gz`, gz);
      }
    },
  };
}

export default defineConfig({
  plugins: [tailwindcss(), svelte(), precompress()],
  publicDir: 'static',
  server: {
    proxy: {
      // 127.0.0.1, not `localhost`: Node resolves that to ::1 first, where the backend does not listen.
      '/api': 'http://127.0.0.1:8080',
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
