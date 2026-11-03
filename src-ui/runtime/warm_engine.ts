/**
 * Warm the engine from the scopes page (route.ts, scopes.md §10.2): while the
 * person picks a scope, fetch what opening one needs, in the background and
 * only once the page is idle. The wasm for the backend this browser will
 * use (the bytes wait in `wasm_loader`'s prefetch slot for the engine), and
 * the app's code. Opening a scope swaps pages in place (`main.ts`), so both
 * are used as they are, not refetched. Skipped when the browser asks to save
 * data.
 */
import { chooseBackend } from './backend_policy';

let started: Promise<void> | null = null;

export function warmEngine(): Promise<void> {
  if (started) return started;
  const connection = (navigator as Navigator & { connection?: { saveData?: boolean } }).connection;
  if (connection?.saveData) return (started = Promise.resolve());
  started = new Promise<void>(resolve => {
    const run = async () => {
      try {
        const backend = await chooseBackend();
        const { prefetchWasm } = await import('./wasm_loader');
        await Promise.all([prefetchWasm(backend), import('../App.svelte')]);
      } catch (e) {
        // Only a head start: opening a scope fetches whatever is missing.
        console.warn('[scopes] could not warm the engine', e);
      }
      resolve();
    };
    if ('requestIdleCallback' in window) requestIdleCallback(() => void run(), { timeout: 1500 });
    else setTimeout(() => void run(), 300);
  });
  return started;
}
