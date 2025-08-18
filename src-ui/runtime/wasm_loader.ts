import wasmUrl from '../wasm/ironfell_bg.wasm?url';
import type { AdapterBridge } from './adapter_bridge';

export class WasmLoader {
    private promise: Promise<ArrayBuffer> | null = null;

    startFetch() {
        if (!this.promise) {
            this.promise = fetch(wasmUrl).then(r => r.arrayBuffer());
        }
        return this.promise;
    }

    async sendToAdapter(bridge: AdapterBridge) {
        const wasmData = await this.startFetch();
        bridge.post({ ty: 'wasmData', wasmData }, [wasmData as any as Transferable]);
    }
}
