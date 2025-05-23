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
})
