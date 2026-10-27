import type { UiRequest, UiEvent } from './protocol';
import { NativeAdapter } from './native_adapter';
import { MainThreadAdapter } from '../main-thread-adapter';
import type { RuntimeMode } from './session_adapter';

export type MessageHandler = (data: UiEvent) => void;

export class AdapterBridge {
    private adapter: MainThreadAdapter | NativeAdapter | Worker;
    private handler: MessageHandler | null = null;
    private disposed = false;
    readonly mode: RuntimeMode;

    constructor(mode: RuntimeMode, canvas: HTMLCanvasElement) {
        this.mode = mode;
        if (import.meta.env.MODE === 'native' || mode === 'native') {
            this.adapter = new NativeAdapter();
            this.adapter.onmessage = (e) => this.handler?.(e.data);
        } else if (mode === 'worker') {
            this.adapter = new Worker(new URL('../worker.ts', import.meta.url), { type: 'module' });
            (this.adapter as Worker).onmessage = (e) => this.handler?.(e.data);
            (this.adapter as Worker).onerror = e => this.handler?.({ ty:'runtimeError', error:e.message });
            (this.adapter as Worker).onmessageerror = () => this.handler?.({ ty:'runtimeError', error:'worker message could not be decoded' });
        } else {
            this.adapter = new MainThreadAdapter();
            (this.adapter as MainThreadAdapter).onmessage = (e: any) => this.handler?.(e.data);
        }
    }

    setHandler(handler: MessageHandler) {
        if (!this.disposed) this.handler = handler;
    }

    post(msg: UiRequest, transfer?: Transferable[]) {
        if (this.disposed) throw new Error('session disposed');
        if (this.mode === 'worker') {
            (this.adapter as Worker).postMessage(msg, transfer || []);
        } else {
            const pending = (this.adapter as MainThreadAdapter).postMessage(msg);
            void pending?.catch(error => this.handler?.({ty:'runtimeError', error:String(error)}));
        }
    }

    dispose() {
        if (this.disposed) return;
        this.disposed = true;
        this.handler = null;
        if (this.mode === 'worker') {
            try { (this.adapter as Worker).terminate(); } catch { /* ignore */ }
        } else {
            // For main thread mode, call the adapter's dispose method to clean up RAF
            try { (this.adapter as MainThreadAdapter).dispose(); } catch { /* ignore */ }
        }
    }
}
