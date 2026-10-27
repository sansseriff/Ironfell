import { BrowserRuntime } from './runtime/browser_runtime';
import type { UiRequest } from './runtime/protocol';

const runtime = new BrowserRuntime('worker');
runtime.onmessage = event => self.postMessage(event.data);
self.onmessage = event => {
  void runtime.postMessage(event.data as UiRequest).catch(error => {
    self.postMessage({ ty: 'runtimeError', error: String(error) });
  });
};
