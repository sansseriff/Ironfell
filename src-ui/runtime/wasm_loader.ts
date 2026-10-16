// Both URLs are resolved at build time; only the selected one is fetched. `?url`
// yields a URL rather than inlining the bytes, so the unused artifact costs a
// build output, not a download.
import webgpuWasmUrl from '../wasm/webgpu/ironfell_bg.wasm?url';
import webgl2WasmUrl from '../wasm/webgl2/ironfell_bg.wasm?url';
import type { AdapterBridge } from './adapter_bridge';
import type { Backend } from './backend_policy';
import { variantFlagsFromUrl } from './variant_policy';

export function wasmUrlFor(backend: Backend): string {
    return backend === 'webgpu' ? webgpuWasmUrl : webgl2WasmUrl;
}

export class WasmLoader {
    private promise: Promise<ArrayBuffer> | null = null;
    private backend: Backend | null = null;

    /**
     * Begin fetching the artifact for `backend`.
     *
     * The backend is fixed by the first call: the bytes are transferred to the
     * worker, which pairs them with the matching glue, so a later change would
     * have to invalidate both.
     */
    startFetch(backend: Backend) {
        if (!this.promise) {
            this.backend = backend;
            this.promise = fetch(wasmUrlFor(backend)).then(r => r.arrayBuffer());
        } else if (this.backend !== backend) {
            throw new Error(`wasm already being fetched for ${this.backend}; cannot switch to ${backend}`);
        }
        return this.promise;
    }

    async sendToAdapter(bridge: AdapterBridge, backend: Backend) {
        const wasmData = await this.startFetch(backend);
        const variantFlags = variantFlagsFromUrl();
        if (variantFlags !== 0) {
            console.log(`[perf-grid] bevy variant flags = ${variantFlags} (from ?bevy=...)`);
        }
        // `backend` travels with the bytes so the worker imports the glue that
        // matches them rather than re-deriving the decision.
        bridge.post({ ty: 'wasmData', wasmData, variantFlags, backend }, [wasmData as any as Transferable]);
    }
}
