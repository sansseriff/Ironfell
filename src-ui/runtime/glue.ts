/**
 * Loads the wasm-bindgen glue matching the selected graphics backend.
 *
 * The glue is per-target, not just the wasm. wasm-bindgen emits a JS shim for
 * every browser API the compiled Rust actually calls, so the WebGL2 build —
 * whose GL backend drives WebGL2 from Rust — carries a large set of shims the
 * WebGPU build does not (measured 2026-09: 148,645 vs 89,092 bytes). The two
 * modules export the same functions but are not interchangeable, and pairing a
 * glue with the wrong `.wasm` fails at instantiation.
 *
 * Both specifiers below are string literals in a ternary so the bundler can see
 * them statically: it emits one chunk each and the browser fetches only the one
 * that runs.
 */

import type { Backend } from './backend_policy';

// Types come from the WebGPU glue arbitrarily. Both are generated from the same
// Rust exports, so the shape is identical; only the internal shims differ. If
// they ever diverge, that is a bug worth failing on rather than papering over.
import type * as IronfellGlue from '../wasm/webgpu/ironfell.js';

export type Glue = typeof IronfellGlue;

let loaded: { backend: Backend; glue: Glue } | null = null;

/**
 * Import the glue for `backend`. Idempotent per backend; loading a second,
 * different backend into one page is a programming error rather than a
 * supported switch, because the first module has already instantiated its wasm.
 */
export async function loadGlue(backend: Backend): Promise<Glue> {
    if (loaded) {
        if (loaded.backend !== backend) {
            throw new Error(
                `glue already loaded for ${loaded.backend}; cannot also load ${backend} in the same context`,
            );
        }
        return loaded.glue;
    }
    const glue = (await (backend === 'webgpu'
        ? import('../wasm/webgpu/ironfell.js')
        : import('../wasm/webgl2/ironfell.js'))) as unknown as Glue;
    loaded = { backend, glue };
    return glue;
}
