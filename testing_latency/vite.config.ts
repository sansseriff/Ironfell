import { defineConfig } from 'vite';

export default defineConfig({
  root: '.',
  base: '/',
  server: {
    host: '127.0.0.1',
    port: 5177,
    strictPort: true,
    headers: {
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
});
