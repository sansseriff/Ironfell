import { mount, unmount } from 'svelte'
import './app.css'
import './ui.css'

import { isNativeHost } from './runtime/native_host'
import { startTheme } from './runtime/theme.svelte'
import { page } from './route'

if (isNativeHost()) document.documentElement.dataset.native = 'true'

startTheme()

// Two pages (route.ts): the scopes page needs only storage, so the app, and
// with it the engine and its wasm, is imported only when a scope opens (the
// scopes page warms both in the background, runtime/warm_engine.ts).
const target = document.getElementById('app')!
const title = document.title
let current: Record<string, any> | null = null

async function showApp() {
  const { default: App } = await import('./App.svelte')
  if (current) unmount(current)
  document.title = title
  current = mount(App, { target })
}

/** Open the app at `url` (a scope, or a new one) without leaving the page,
 *  so what the scopes page warmed is used as it is. */
export async function openApp(url: string) {
  history.pushState(null, '', url)
  await showApp()
}

// Back from the app to the scopes page: the engine cannot be torn down in
// place, so the page loads afresh at the address it went back to.
window.addEventListener('popstate', () => location.reload())

if (page() === 'scopes') {
  import('./ScopesPage.svelte').then(({ default: ScopesPage }) => {
    current = mount(ScopesPage, { target, props: { openApp } })
  })
} else {
  void showApp()
}
