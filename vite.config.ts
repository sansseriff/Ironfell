import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { fileURLToPath } from 'node:url'
import preload from "vite-plugin-preload";

// https://vite.dev/config/
export default defineConfig(({ mode }) => ({
  plugins: [preload(), svelte()],
  base: mode === 'native' ? './' : '/Ironfell/', // Updated to match the actual repository name
  root: '.',
  build: {
    outDir: mode === 'native' ? 'dist-native' : 'dist',

  },
  resolve: {
    alias: [
      ...(mode === 'native' ? [{ find: /^\.\/adapter_bridge$/, replacement: fileURLToPath(new URL('./src-ui/runtime/native_bridge.ts', import.meta.url)) }] : []),
      { find: 'src', replacement: '/src-ui' }
    ]
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
