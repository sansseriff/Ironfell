import { MainThreadAdapter } from '../main-thread-adapter';
import type { RuntimeMode } from '../control.svelte';

export type MessageHandler = (data: any) => void;

export class AdapterBridge {
    private adapter: MainThreadAdapter | Worker;
    private handler: MessageHandler | null = null;
    readonly mode: RuntimeMode;

    constructor(mode: RuntimeMode, canvas: HTMLCanvasElement) {
        this.mode = mode;
        if (mode === 'worker') {
            this.adapter = new Worker(new URL('../worker.ts', import.meta.url), { type: 'module' });
            (this.adapter as Worker).onmessage = (e) => this.handler?.(e.data);
        } else {
            this.adapter = new MainThreadAdapter();
            (this.adapter as MainThreadAdapter).onmessage = (e: any) => this.handler?.(e.data);
        }
    }

    setHandler(handler: MessageHandler) {
        this.handler = handler;
    }

    post(msg: any, transfer?: Transferable[]) {
        if (this.mode === 'worker') {
            (this.adapter as Worker).postMessage(msg, transfer || []);
        } else {
            (this.adapter as MainThreadAdapter).postMessage(msg);
        }
    }

    dispose() {
        if (this.mode === 'worker') {
            try { (this.adapter as Worker).terminate(); } catch { /* ignore */ }
        }
    }
}
