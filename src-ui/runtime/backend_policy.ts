/**
 * Which graphics backend this browser should run.
 *
 * Two artifacts are built from one source tree (see `[features]` in Cargo.toml).
 * They are not interchangeable: bevy compiles its WebGL2 paths out when the
 * `webgpu` feature is present, so the choice is made here and the matching wasm
 * and wasm-bindgen glue are loaded together.
 *
 * # Why this is not simply "use WebGPU if available"
 *
 * WebGPU being *present* does not make it the faster option. Measured on this
 * app (2026-09), Firefox and Safari render WebGPU noticeably slower and less
 * efficiently than the WebGL2 build, so shipping WebGPU wherever
 * `navigator.gpu` exists would actively degrade them. Chrome on Linux, mean-
 * while, frequently has no WebGPU at all.
 *
 * So the decision has two independent halves, and both are required:
 *
 *   policy      — *should* we prefer WebGPU on this browser and OS?
 *   capability  — *can* this browser actually give us an adapter?
 *
 * Policy alone would hand a WebGPU build to a Chrome instance with WebGPU
 * disabled by flag or a blocklisted GPU. Capability alone would hand it to
 * Firefox, where it is slower. Only the conjunction is correct.
 *
 * # Why `navigator.userAgentData` rather than parsing the UA string
 *
 * UA Client Hints are implemented by Chromium and declined by both Firefox and
 * Safari, so its mere presence is a reliable "this is Chromium" signal — and
 * `.platform` gives the OS without the guesswork of substring-matching a UA
 * string. Platform values are compared case-insensitively rather than against
 * exact spellings, which are not worth depending on.
 *
 * # Revisiting
 *
 * The performance claim above is an empirical observation with a date on it, not
 * a property of the browsers. When Firefox or Safari improve, this function is
 * the only thing that needs to change — and `?gfx=` overrides it without a
 * rebuild, so the claim can be re-measured on any browser at any time.
 */

export type Backend = 'webgpu' | 'webgl2';

/** Chromium exposes UA Client Hints; Firefox and Safari deliberately do not. */
interface UADataLike {
    platform?: string;
    mobile?: boolean;
}

function uaData(): UADataLike | undefined {
    return (navigator as unknown as { userAgentData?: UADataLike }).userAgentData;
}

/**
 * Whether WebGPU is *preferred* here, ignoring whether it actually works.
 *
 * True only for desktop Chromium on macOS or Windows: the configurations where
 * WebGPU measured faster than WebGL2 for this app.
 */
export function prefersWebGpu(): boolean {
    const data = uaData();
    if (!data) return false; // Firefox / Safari — WebGL2 is faster today.
    if (data.mobile) return false; // Mobile GPUs are not the measured case.
    const platform = (data.platform || '').toLowerCase();
    return platform.includes('mac') || platform.includes('win');
}

/** An explicit `?gfx=webgpu` / `?gfx=webgl2` overrides the policy entirely. */
export function backendOverrideFromUrl(): Backend | null {
    const value = new URLSearchParams(location.search).get('gfx');
    if (value === 'webgpu' || value === 'webgl2') return value;
    return null;
}

/**
 * Can this browser actually produce a WebGPU adapter?
 *
 * `navigator.gpu` existing is not sufficient — it is present while still failing
 * to return an adapter behind a disabled flag, a blocklisted GPU, or a crashed
 * GPU process. Only a non-null adapter proves the WebGPU build can start.
 */
async function canUseWebGpu(): Promise<boolean> {
    const gpu = (navigator as unknown as { gpu?: { requestAdapter(): Promise<unknown> } }).gpu;
    if (!gpu) return false;
    try {
        return (await gpu.requestAdapter()) != null;
    } catch {
        return false;
    }
}

/**
 * Pick the backend to load. Falls back to WebGL2 whenever WebGPU is not both
 * preferred and available, so this never resolves to a build that cannot start.
 */
export async function chooseBackend(): Promise<Backend> {
    const override = backendOverrideFromUrl();
    if (override) {
        // An override may name a backend this browser cannot run. That is the
        // point — it is how a specific configuration gets measured — so it is
        // honoured as given and only reported.
        console.log(`[gfx] backend forced to ${override} by ?gfx=`);
        return override;
    }
    if (prefersWebGpu() && (await canUseWebGpu())) return 'webgpu';
    return 'webgl2';
}
