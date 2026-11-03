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

/** The artifact fetched and compiled before any engine asked for it
 *  (`prefetchWasm`). */
let early: { backend: Backend; module: Promise<WebAssembly.Module> } | null = null;

function fetchBytes(backend: Backend): Promise<ArrayBuffer> {
    return fetch(wasmUrlFor(backend)).then(r => {
        if (!r.ok) throw new Error(`wasm fetch failed: ${r.status}`);
        return r.arrayBuffer();
    });
}

/**
 * Fetch and compile the artifact ahead of the app (the scopes page,
 * `warm_engine.ts`), so opening a scope skips the download and the compile.
 * The next `WasmLoader` started for this backend takes the module; a module
 * is structured-cloned to the worker, which only instantiates it.
 * Idempotent; resolves when the module is compiled.
 */
export function prefetchWasm(backend: Backend): Promise<WebAssembly.Module> {
    if (!early) {
        const url = wasmUrlFor(backend);
        // Streaming compiles while the bytes arrive; servers that send
        // another type than application/wasm need the buffered path.
        const module = WebAssembly.compileStreaming
            ? WebAssembly.compileStreaming(fetch(url)).catch(() => fetchBytes(backend).then(b => WebAssembly.compile(b)))
            : fetchBytes(backend).then(b => WebAssembly.compile(b));
        early = { backend, module };
    }
    return early.module;
}

/** Whether prefetched bytes are waiting (for the driver and tests). */
export function wasmPrefetched(): Backend | null {
    return early?.backend ?? null;
}

export class WasmLoader {
    private promise: Promise<ArrayBuffer | WebAssembly.Module> | null = null;
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
            if (early?.backend === backend) {
                // A failed prefetch is retried rather than failing the boot.
                this.promise = early.module.catch(() => fetchBytes(backend));
                early = null;
            } else {
                this.promise = fetchBytes(backend);
            }
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
        // Bytes are transferred; a compiled module cannot be, and is cloned.
        const transfer = wasmData instanceof ArrayBuffer ? [wasmData as any as Transferable] : [];
        bridge.post({ ty: 'wasmData', wasmData, variantFlags, backend }, transfer);
    }
}
