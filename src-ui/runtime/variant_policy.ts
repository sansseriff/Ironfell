import { settings } from './settings.svelte';

/**
 * Perf-grid variant selection (mirrors bevy_app::VARIANT_* in Rust).
 * One wasm artifact serves every grid cell; the variant is chosen per page load:
 *   ?bevy=empty        DefaultPlugins only, no cameras/app plugins ("renders nothing")
 *   ?bevy=empty,nolog  same, with LogPlugin (tracing-wasm perf marks) disabled
 *   ?bevy=nolog        normal app without LogPlugin
 *   ?bevy=min          minimal plugin floor (window+render only; implies no log)
 *   ?bevy=classic      initialize classic Vello instead of Vello Hybrid;
 *                      this deliberately pays classic's deferred startup cost.
 *                      Requires a build with the `classic` cargo feature.
 *   ?bevy=always       render every frame (render on demand off), to compare
 *                      pacing and latency on a real display
 *   ?bevy=alpha        add the alpha-blending stress fixture; combine as
 *                      ?bevy=alpha,classic to run that variant with classic
 * No param = the normal Vello Hybrid app.
 */
export function variantFlagsFromUrl(): number {
    const parts = (new URLSearchParams(location.search).get('bevy') || '')
        .split(',')
        .map(s => s.trim().toLowerCase());
    let flags = 0;
    if (parts.includes('nolog')) flags |= 1; // VARIANT_NO_LOG
    if (parts.includes('min')) flags |= 2;   // VARIANT_MIN_PLUGINS
    if (parts.includes('empty')) flags |= 4; // VARIANT_EMPTY
    if (parts.includes('alpha')) flags |= 16; // VARIANT_ALPHA_STRESS
    // The renderer: the URL's choice, else the Settings view's.
    const classic = parts.includes('classic') || (!parts.includes('hybrid') && settings.renderer === 'classic');
    if (classic) flags |= 32; // VARIANT_CLASSIC_BACKEND
    if (parts.includes('always')) flags |= 64; // VARIANT_ALWAYS_RENDER
    return flags;
}

/**
 * Return the same URL with the opposite single-backend app variant selected.
 *
 * The normal app is Hybrid-only. Classic Vello cannot be made lazy merely by
 * gating its systems: installing `VelloPlugin` constructs its renderer and
 * compute pipelines during Bevy plugin finalization. Switching therefore crosses
 * an app-restart boundary. Only the `bevy` query member changes, so unrelated
 * parameters and variants such as `alpha`/`nolog` are preserved.
 */
export function oppositeVectorBackendUrl(current: URL): URL {
    const next = new URL(current.href);
    const variants = (next.searchParams.get('bevy') || '')
        .split(',')
        .map(value => value.trim().toLowerCase())
        .filter(Boolean);
    const classic = variants.includes('classic');
    const kept = variants.filter(value => value !== 'classic' && value !== 'hybrid');
    if (!classic) kept.push('classic');
    if (kept.length > 0) {
        next.searchParams.set('bevy', kept.join(','));
    } else {
        next.searchParams.delete('bevy');
    }
    return next;
}
