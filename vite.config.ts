import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import preload from "vite-plugin-preload";

// https://vite.dev/config/
export default defineConfig({
  plugins: [preload(), svelte()],
  base: '/Ironfell/', // Updated to match the actual repository name
  root: '.',
  build: {
    outDir: 'dist',

  },
  resolve: {
    alias: {
      'src': '/src-ui'
    }
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
})
