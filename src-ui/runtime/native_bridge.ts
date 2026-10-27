import type { UiRequest, UiEvent } from './protocol';
// Build-time selection keeps wasm artifacts and worker code out of native packages.
import { NativeAdapter } from './native_adapter';
import type { RuntimeMode } from './session_adapter';
export type MessageHandler = (data: UiEvent) => void;
export class AdapterBridge {
  private adapter = new NativeAdapter();
  readonly mode: RuntimeMode = 'native';
  constructor(_mode: RuntimeMode, _canvas: HTMLCanvasElement) {}
  setHandler(handler: MessageHandler) { this.adapter.onmessage = e => handler(e.data); }
  post(message: UiRequest, _transfer?: Transferable[]) { this.adapter.postMessage(message); }
  dispose() { this.adapter.dispose(); }
}
