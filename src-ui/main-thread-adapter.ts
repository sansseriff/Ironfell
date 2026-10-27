import { BrowserRuntime } from './runtime/browser_runtime';

/** Worker-compatible transport with synchronous pointer delivery on the UI thread. */
export class MainThreadAdapter extends BrowserRuntime {
  constructor() { super('main'); }
}
