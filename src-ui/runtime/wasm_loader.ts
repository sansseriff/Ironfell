import wasmUrl from '../wasm/ironfell_bg.wasm?url';
import type { AdapterBridge } from './adapter_bridge';

/**
 * Perf-grid variant selection (mirrors bevy_app::VARIANT_* in Rust).
 * One wasm artifact serves every grid cell; the variant is chosen per page load:
 *   ?bevy=empty        DefaultPlugins only, no cameras/app plugins ("renders nothing")
 *   ?bevy=empty,nolog  same, with LogPlugin (tracing-wasm perf marks) disabled
 *   ?bevy=nolog        normal app without LogPlugin
 *   ?bevy=min          minimal plugin floor (window+render only; implies no log)
 * No param = the normal app.
 */
export function variantFlagsFromUrl(): number {
    const parts = (new URLSearchParams(location.search).get('bevy') || '')
        .split(',')
        .map(s => s.trim().toLowerCase());
    let flags = 0;
    if (parts.includes('nolog')) flags |= 1; // VARIANT_NO_LOG
    if (parts.includes('min')) flags |= 2;   // VARIANT_MIN_PLUGINS
    if (parts.includes('empty')) flags |= 4; // VARIANT_EMPTY
    return flags;
}

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
        const variantFlags = variantFlagsFromUrl();
        if (variantFlags !== 0) {
            console.log(`[perf-grid] bevy variant flags = ${variantFlags} (from ?bevy=...)`);
        }
        bridge.post({ ty: 'wasmData', wasmData, variantFlags }, [wasmData as any as Transferable]);
    }
}
