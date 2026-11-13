import { defineConfig, type Plugin } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { fileURLToPath } from 'node:url'
import { execSync } from 'node:child_process'

// The frontend's version for the FPS readout's tooltip: the commit it was
// served or built from, and `+changes` when the working tree differed.
function frontendVersion(): string {
  try {
    const sha = execSync('git rev-parse --short HEAD', { encoding: 'utf8' }).trim()
    const dirty = execSync('git status --porcelain --untracked-files=no', { encoding: 'utf8' }).trim() !== ''
    return `${sha}${dirty ? '+changes' : ''} (${new Date().toISOString().slice(0, 16).replace('T', ' ')} UTC)`
  } catch {
    return 'unknown'
  }
}

// Cross-origin isolation, for SharedArrayBuffer and so the worker frame hold
// (plans/rendering/worker-frame-hold.md §5). On in dev unless IRON_COI=0;
// GitHub Pages cannot send these headers. Set on every response, not through
// `server.headers`: Vite leaves those off its 304s, and the browser keeps the
// headers of the copy it cached, so a page first cached from a server without
// isolation (IRON_COI=0, an older checkout on the same port) stayed
// unisolated on every reload, though each fresh 200 carried the headers.
function crossOriginIsolation(): Plugin {
  return {
    name: 'iron-cross-origin-isolation',
    configureServer(server) {
      if (process.env.IRON_COI === '0') return
      server.middlewares.use((_req, res, next) => {
        res.setHeader('Cross-Origin-Opener-Policy', 'same-origin')
        res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp')
        next()
      })
    },
  }
}

// https://vite.dev/config/
export default defineConfig(({ mode }) => ({
  // No preload plugin: it put every chunk (the app, the engine's glue) in
  // index.html, so the scopes page fetched the engine it exists to avoid
  // (scopes.md §10.2). The app's dynamic import preloads its own chunks.
  plugins: [svelte(), crossOriginIsolation()],
  define: { __IRON_FRONTEND_VERSION__: JSON.stringify(frontendVersion()) },
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
