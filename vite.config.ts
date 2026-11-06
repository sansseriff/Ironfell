import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { fileURLToPath } from 'node:url'

// https://vite.dev/config/
export default defineConfig(({ mode }) => ({
  // No preload plugin: it put every chunk (the app, the engine's glue) in
  // index.html, so the scopes page fetched the engine it exists to avoid
  // (scopes.md §10.2). The app's dynamic import preloads its own chunks.
  plugins: [svelte()],
  base: mode === 'native' ? './' : '/Ironfell/', // Updated to match the actual repository name
  root: '.',
  cacheDir: '.agents/vite',
  build: {
    outDir: mode === 'native' ? 'dist-native' : 'dist',

  },
  resolve: {
    alias: [
      ...(mode === 'native' ? [{ find: /^\.\/adapter_bridge$/, replacement: fileURLToPath(new URL('./src-ui/runtime/native_bridge.ts', import.meta.url)) }] : []),
      { find: 'src', replacement: '/src-ui' }
    ]
  },
  // The two pages (src-ui/route.ts) are dynamic imports, so Vite's dev
  // server finds their dependencies late and reloads the page to
  // re-optimise, mid-script for the driver. Listing them up front avoids it.
  optimizeDeps: {
    entries: ['index.html', 'src-ui/App.svelte', 'src-ui/ScopesPage.svelte'],
    include: ['bits-ui', 'dockview-core'],
    // Icons are deep imports, one per file: pre-bundled, every new one
    // would re-optimise and reload. They are plain ESM Svelte, served as is.
    exclude: ['@lucide/svelte'],
  },
  // Worktrees and agent scratch live in .agents/ (AGENTS.md): another
  // agent's edits there (a tsconfig, an html) made Vite reload this app
  // mid-smoke. Nothing the app serves is in it, nor in plans/.
  server: {
    // IRON_COI=1: cross-origin isolation, for SharedArrayBuffer (the frame
    // hold prototype, plans/rendering/worker-frame-hold.md). Dev only for now;
    // GitHub Pages cannot send these headers.
    headers: process.env.IRON_COI ? { 'Cross-Origin-Opener-Policy': 'same-origin', 'Cross-Origin-Embedder-Policy': 'require-corp' } : undefined,
    watch: { ignored: ['**/.agents/**', '**/plans/**', '**/target/**'] },
  },
  // Ensure WASM files are treated as assets
  assetsInclude: ['**/*.wasm'],
  worker: {
    // The worker dynamically imports one of two wasm-bindgen glue modules
    // (webgpu / webgl2), which requires code-splitting. Vite's default worker
    // format is `iife`, which cannot code-split, so the build fails without
    // this. No runtime change: adapter_bridge.ts already constructs the worker
    // with `{ type: "module" }`, so this only makes the bundle match how the
    // worker is actually instantiated.
    format: 'es',
  },
}))
