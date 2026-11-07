<script lang="ts" module>
  // The colour model the picker edits: hue 0..360, saturation, value and
  // alpha 0..1. Kept here, not re-derived from the stored hex, so the hue
  // survives a drag through grey or black.
  type Hsva = { h: number; s: number; v: number; a: number };
  export type ColorFormat = "hex" | "rgb" | "hsl" | "hsb";
  // One format for every picker in the session, as Figma remembers it.
  let format = $state<{ value: ColorFormat }>({ value: "hex" });

  const clamp = (x: number, lo = 0, hi = 1) => Math.max(lo, Math.min(hi, x));
  const byte = (x: number) => Math.round(clamp(x) * 255);
  export function parseHex(text: string | null | undefined): [number, number, number, number] | null {
    const t = (text ?? "").trim().replace(/^#/, "");
    const full = t.length === 3 || t.length === 4 ? [...t].map((c) => c + c).join("") : t;
    if (!/^([0-9a-f]{6}|[0-9a-f]{8})$/i.test(full)) return null;
    const n = (i: number) => parseInt(full.slice(i, i + 2), 16) / 255;
    return [n(0), n(2), n(4), full.length === 8 ? n(6) : 1];
  }
  export function toHex(r: number, g: number, b: number, a = 1) {
    return "#" + [r, g, b, a].map((c) => byte(c).toString(16).padStart(2, "0")).join("");
  }
  function hsvToRgb(h: number, s: number, v: number): [number, number, number] {
    const f = (n: number) => { const k = (n + h / 60) % 6; return v - v * s * Math.max(0, Math.min(k, 4 - k, 1)); };
    return [f(5), f(3), f(1)];
  }
  function rgbToHsv(r: number, g: number, b: number, hue: number): [number, number, number] {
    const max = Math.max(r, g, b), d = max - Math.min(r, g, b);
    let h = hue;
    if (d > 1e-6) h = 60 * (max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4);
    return [h, max ? d / max : 0, max];
  }
  function hsvToHsl(s: number, v: number): [number, number] {
    const l = v * (1 - s / 2);
    return [l === 0 || l === 1 ? 0 : (v - l) / Math.min(l, 1 - l), l];
  }
  function hslToHsv(s: number, l: number): [number, number] {
    const v = l + s * Math.min(l, 1 - l);
    return [v ? 2 * (1 - l / v) : 0, v];
  }
  function hsvaToHex(c: Hsva) { return toHex(...hsvToRgb(c.h, c.s, c.v), c.a); }
</script>

<script lang="ts">
  // A Figma-style colour picker for one colour leaf: a saturation/brightness
  // square, hue and alpha sliders, an eyedropper, typed values (Hex, RGB, HSL,
  // HSB) and the composition's colours as swatches. A drag is one gesture
  // (`inspect_scrub`, channel "color"): live on the canvas, one undo step.
  import { onDestroy, untrack } from "svelte";
  import Pipette from "@lucide/svelte/icons/pipette";
  import Select from "./Select.svelte";
  import type { Property } from "./PropertyField.svelte";
  import { controllerManager } from "../../controller-manager.svelte";

  let { field, node, swatches = [], onSet, onRefresh, onError }: {
    field: Property; node: string; swatches?: string[];
    onSet: (field: Property, text: string) => void; onRefresh: () => void; onError: (error: string) => void;
  } = $props();
  const doc = controllerManager.docClient;
  const locked = $derived(field.edit === "locked");

  let color = $state<Hsva>({ h: 0, s: 0, v: 0.5, a: 1 });
  // The hex this picker last showed or sent, so its own echo from the store
  // does not reset the hue.
  let shown = "";
  $effect(() => {
    const text = field.text;
    if (session || !text || text.toLowerCase() === shown) return;
    const rgba = parseHex(text);
    if (!rgba) return;
    const [h, s, v] = rgbToHsv(rgba[0], rgba[1], rgba[2], untrack(() => color.h));
    color = { h, s, v, a: rgba[3] };
    shown = text.toLowerCase();
  });
  const hex = $derived(hsvaToHex(color));
  const rgb = $derived(hsvToRgb(color.h, color.s, color.v));
  const opaque = $derived(hex.slice(0, 7));

  // ── The drag gesture ───────────────────────────────────────────────────
  type Session = { leaf: string; token: number; failed: boolean; begun: Promise<void>; sending: boolean; next: number | null };
  let session: Session | null = null;
  const options = JSON.stringify({ channel: "color" });
  const packed = (h: string) => parseInt(h.slice(1, 9), 16);

  function begin() {
    onError("");
    const s: Session = { leaf: field.leaf, token: 0, failed: false, begun: Promise.resolve(), sending: false, next: null };
    session = s;
    s.begun = doc.command({ op: "inspect_scrub", node, leaf: s.leaf, phase: "begin", token: 0, value: 0, options }).then((r) => {
      if (!r.ok) throw new Error(r.error);
      s.token = r.token;
    }).catch(() => { s.failed = true; });
  }
  // Only the newest colour goes to the engine; moves queue no deeper than one.
  function pump(s: Session) {
    if (s.sending || s.next === null) return;
    const value = s.next; s.next = null; s.sending = true;
    s.begun.then(async () => {
      if (s.failed) return;
      const r = await doc.command({ op: "inspect_scrub", node, leaf: s.leaf, phase: "move", token: s.token, value, options });
      if (!r.ok) s.failed = true;
    }).catch(() => { s.failed = true; }).finally(() => { s.sending = false; pump(s); });
  }
  function preview(next: Hsva) {
    color = next;
    shown = hsvaToHex(next);
    if (session) { session.next = packed(shown); pump(session); }
  }
  function finish(cancel = false) {
    const s = session;
    if (!s) return;
    session = null;
    const final = shown;
    const settle = async () => {
      await s.begun;
      while (s.sending || s.next !== null) await new Promise((r) => setTimeout(r, 4));
      if (!s.failed && s.token) {
        const r = await doc.command({ op: "inspect_scrub", node, leaf: s.leaf, phase: cancel ? "cancel" : "commit", token: s.token, value: 0, options });
        if (!r.ok) onError(r.error);
      } else if (!cancel) {
        // An older engine, or a property the gesture cannot drive: one edit.
        onSet(field, final);
      }
    };
    settle().catch((e) => onError(String(e))).finally(onRefresh);
  }
  onDestroy(() => finish(true));

  type Axis = (x: number, y: number) => Hsva;
  function drag(axis: Axis) {
    return (e: PointerEvent) => {
      if (locked || e.button !== 0) return;
      const el = e.currentTarget as HTMLElement;
      el.setPointerCapture(e.pointerId);
      el.focus();
      const at = (ev: PointerEvent) => {
        const r = el.getBoundingClientRect();
        preview(axis(clamp((ev.clientX - r.left) / r.width), clamp((ev.clientY - r.top) / r.height)));
      };
      begin();
      at(e);
      const move = (ev: PointerEvent) => at(ev);
      const up = (ev: PointerEvent) => {
        el.removeEventListener("pointermove", move);
        el.removeEventListener("pointerup", up);
        el.removeEventListener("pointercancel", up);
        finish(ev.type === "pointercancel");
      };
      el.addEventListener("pointermove", move);
      el.addEventListener("pointerup", up);
      el.addEventListener("pointercancel", up);
    };
  }
  const square: Axis = (x, y) => ({ ...color, s: x, v: 1 - y });
  const hue: Axis = (x) => ({ ...color, h: x * 360 });
  const alpha: Axis = (x) => ({ ...color, a: x });
  // Escape cancels a drag, before the canvas' window shortcuts see it.
  $effect(() => {
    const key = (e: KeyboardEvent) => { if (e.key === "Escape" && session) { e.preventDefault(); e.stopImmediatePropagation(); finish(true); } };
    window.addEventListener("keydown", key, { capture: true });
    return () => window.removeEventListener("keydown", key, { capture: true });
  });

  // Keyboard: arrows nudge (Shift ×10), one edit per key press.
  function commit(next: Hsva) { color = next; shown = hsvaToHex(next); onSet(field, shown); }
  function keys(step: (dx: number, dy: number) => Hsva) {
    return (e: KeyboardEvent) => {
      const d = e.shiftKey ? 10 : 1;
      const delta: Record<string, [number, number]> = { ArrowLeft: [-d, 0], ArrowRight: [d, 0], ArrowUp: [0, -d], ArrowDown: [0, d] };
      const m = delta[e.key];
      if (!m || locked) return;
      e.preventDefault(); e.stopPropagation();
      commit(step(m[0], m[1]));
    };
  }

  // ── Typed values ───────────────────────────────────────────────────────
  const formats = [{ value: "hex", label: "Hex" }, { value: "rgb", label: "RGB" }, { value: "hsl", label: "HSL" }, { value: "hsb", label: "HSB" }] as { value: ColorFormat; label: string }[];
  const channels = $derived.by((): { label: string; value: number; max: number }[] => {
    if (format.value === "rgb") return rgb.map((c, i) => ({ label: "RGB"[i], value: byte(c), max: 255 }));
    if (format.value === "hsl") { const [s, l] = hsvToHsl(color.s, color.v); return [{ label: "H", value: Math.round(color.h), max: 360 }, { label: "S", value: Math.round(s * 100), max: 100 }, { label: "L", value: Math.round(l * 100), max: 100 }]; }
    return [{ label: "H", value: Math.round(color.h), max: 360 }, { label: "S", value: Math.round(color.s * 100), max: 100 }, { label: "B", value: Math.round(color.v * 100), max: 100 }];
  });
  function setChannel(i: number, text: string) {
    const n = Number(text);
    if (!Number.isFinite(n)) return;
    const vals = channels.map((c) => c.value);
    vals[i] = clamp(n, 0, channels[i].max);
    if (format.value === "rgb") { const [h, s, v] = rgbToHsv(vals[0] / 255, vals[1] / 255, vals[2] / 255, color.h); commit({ h, s, v, a: color.a }); }
    else if (format.value === "hsl") { const [s, v] = hslToHsv(vals[1] / 100, vals[2] / 100); commit({ h: vals[0] % 360, s, v, a: color.a }); }
    else commit({ h: vals[0] % 360, s: vals[1] / 100, v: vals[2] / 100, a: color.a });
  }
  function setHex(text: string) {
    const rgba = parseHex(text);
    if (!rgba) { onError(`"${text}" is not a hex colour`); return; }
    const [h, s, v] = rgbToHsv(rgba[0], rgba[1], rgba[2], color.h);
    // Six digits keep the current alpha; eight set it.
    commit({ h, s, v, a: text.replace("#", "").trim().length > 6 ? rgba[3] : color.a });
  }
  function setAlpha(text: string) {
    const n = Number(text.replace("%", ""));
    if (Number.isFinite(n)) commit({ ...color, a: clamp(n / 100) });
  }
  function pick(text: string) {
    const rgba = parseHex(text);
    if (!rgba) return;
    const [h, s, v] = rgbToHsv(rgba[0], rgba[1], rgba[2], color.h);
    commit({ h, s, v, a: rgba[3] });
  }

  // The browser's eyedropper (Chromium); hidden where there is none.
  const EyeDropperCtor = typeof window !== "undefined" ? (window as unknown as { EyeDropper?: new () => { open(): Promise<{ sRGBHex: string }> } }).EyeDropper : undefined;
  async function sample() {
    if (!EyeDropperCtor || locked) return;
    try {
      const { sRGBHex } = await new EyeDropperCtor().open();
      const rgba = parseHex(sRGBHex);
      if (rgba) { const [h, s, v] = rgbToHsv(rgba[0], rgba[1], rgba[2], color.h); commit({ h, s, v, a: color.a }); }
    } catch { /* dismissed */ }
  }
</script>

<div class="color-picker" class:locked title={field.why ?? undefined}>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="sv" role="slider" tabindex={locked ? -1 : 0} aria-label="Saturation and brightness" aria-valuetext="saturation {Math.round(color.s * 100)}%, brightness {Math.round(color.v * 100)}%" aria-valuenow={Math.round(color.s * 100)}
    style:--hue="hsl({color.h} 100% 50%)" onpointerdown={drag(square)} onkeydown={keys((dx, dy) => ({ ...color, s: clamp(color.s + dx / 100), v: clamp(color.v - dy / 100) }))}>
    <span class="thumb" style:left="{color.s * 100}%" style:top="{(1 - color.v) * 100}%" style:background={opaque}></span>
  </div>

  <div class="sliders">
    {#if EyeDropperCtor}
      <button class="property-icon eyedropper" aria-label="Pick a colour from the screen" title="Pick colour" disabled={locked} onclick={sample}><Pipette size={16} strokeWidth={1.5} /></button>
    {/if}
    <div class="tracks">
      <div class="track hue" role="slider" tabindex={locked ? -1 : 0} aria-label="Hue" aria-valuemin={0} aria-valuemax={360} aria-valuenow={Math.round(color.h)}
        onpointerdown={drag(hue)} onkeydown={keys((dx) => ({ ...color, h: (color.h + dx + 360) % 360 }))}>
        <span class="thumb" style:left="{color.h / 3.6}%" style:background="hsl({color.h} 100% 50%)"></span>
      </div>
      <div class="track alpha" role="slider" tabindex={locked ? -1 : 0} aria-label="Opacity" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(color.a * 100)}
        style:--solid={opaque} onpointerdown={drag(alpha)} onkeydown={keys((dx) => ({ ...color, a: clamp(color.a + dx / 100) }))}>
        <span class="thumb" style:left="{color.a * 100}%"><span style:background={hex}></span></span>
      </div>
    </div>
  </div>

  <div class="values">
    <div class="format"><Select label="Colour format" value={format.value} items={formats} disabled={locked} onChange={(v) => format.value = v} /></div>
    <div class="value-box">
      {#if format.value === "hex"}
        <input class="hex" aria-label="Hex" value={hex.slice(1, 7).toUpperCase()} disabled={locked} spellcheck="false"
          onchange={(e) => setHex(e.currentTarget.value)} onkeydown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }} />
      {:else}
        {#each channels as c, i (format.value + c.label)}
          <input class="channel" aria-label={c.label} title={c.label} inputmode="numeric" value={c.value} disabled={locked}
            onchange={(e) => setChannel(i, e.currentTarget.value)} onkeydown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }} />
        {/each}
      {/if}
      <span class="alpha-input">
        <input aria-label="Opacity" inputmode="numeric" value={Math.round(color.a * 100)} disabled={locked}
          onchange={(e) => setAlpha(e.currentTarget.value)} onkeydown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }} /><span class="unit">%</span>
      </span>
    </div>
  </div>
</div>

{#if swatches.length}
  <div class="swatches-section">
    <div class="swatches-title">On this page</div>
    <div class="swatches">
      {#each swatches as s (s)}
        <button class="swatch" class:current={s.toLowerCase() === hex} aria-label="Use {s.toUpperCase()}" title={s.slice(7) === "ff" ? s.slice(0, 7).toUpperCase() : s.toUpperCase()} disabled={locked} onclick={() => pick(s)}>
          <span style:--c={s} style:--solid={s.slice(0, 7)}></span>
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .color-picker { display: flex; flex-direction: column; gap: 14px; padding: 16px; }
  .locked { opacity: .55; pointer-events: none; }
  .sv { position: relative; height: 0; padding-bottom: 100%; border-radius: 4px; cursor: crosshair; touch-action: none; outline: none;
    background: linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, var(--hue)); box-shadow: inset 0 0 0 1px rgba(0, 0, 0, .1); }
  .sv:focus-visible { box-shadow: 0 0 0 2px var(--accent); }
  .thumb { position: absolute; width: 14px; height: 14px; border-radius: 50%; transform: translate(-50%, -50%); pointer-events: none;
    box-shadow: 0 0 0 3px #fff, 0 0 0 4px rgba(0, 0, 0, .25), 0 1px 4px 3px rgba(0, 0, 0, .25); }
  .sliders { display: flex; align-items: center; gap: 8px; }
  .eyedropper { flex: 0 0 28px; width: 28px; height: 28px; padding: 5px; }
  .tracks { flex: 1; display: flex; flex-direction: column; gap: 12px; min-width: 0; padding: 0 7px; }
  .track { position: relative; height: 12px; border-radius: 6px; cursor: pointer; touch-action: none; outline: none; box-shadow: inset 0 0 0 1px rgba(0, 0, 0, .1); }
  .track:focus-visible { box-shadow: 0 0 0 2px var(--accent); }
  .track .thumb { top: 50%; width: 12px; height: 12px; }
  .hue { background: linear-gradient(to right, #f00 0%, #ff0 16.67%, #0f0 33.33%, #0ff 50%, #00f 66.67%, #f0f 83.33%, #f00 100%); }
  .alpha, .swatch span { background-color: #fff; background-image: linear-gradient(45deg, #ccc 25%, transparent 25%, transparent 75%, #ccc 75%), linear-gradient(45deg, #ccc 25%, transparent 25%, transparent 75%, #ccc 75%); background-size: 8px 8px; background-position: 0 0, 4px 4px; }
  .alpha::before { content: ""; position: absolute; inset: 0; border-radius: inherit; background: linear-gradient(to right, transparent, var(--solid)); }
  .alpha .thumb { background: #fff; overflow: hidden; }
  .alpha .thumb span { position: absolute; inset: 0; }
  .values { display: grid; grid-template-columns: 72px minmax(0, 1fr); gap: 6px; }
  .format :global(.ui-select-trigger) { width: 100%; min-width: 0; height: 30px; background: transparent; }
  .value-box { display: flex; min-width: 0; height: 30px; background: var(--bg-sunken); border: 1px solid transparent; border-radius: 5px; overflow: hidden; }
  .value-box:hover { border-color: var(--border-strong); }
  .value-box:focus-within { border-color: var(--accent); }
  .value-box input { flex: 1 1 0; width: 0; min-width: 0; border: 0; background: transparent; color: var(--fg); font-size: 12px; padding: 0 6px; outline: none; font-variant-numeric: tabular-nums; }
  .value-box .hex { flex: 3 1 0; padding-left: 8px; text-transform: uppercase; }
  .value-box .channel + .channel { border-left: 1px solid var(--bg); }
  .alpha-input { display: flex; align-items: center; flex: 0 0 54px; border-left: 1px solid var(--bg); }
  .alpha-input input { padding-right: 0; }
  .unit { color: var(--fg-faint); padding-right: 7px; font-size: 12px; }
  .swatches-section { display: flex; flex-direction: column; gap: 10px; padding: 12px 16px 16px; border-top: 1px solid var(--border); }
  .swatches-title { color: var(--fg-muted); font-size: 11px; }
  .swatches { display: grid; grid-template-columns: repeat(auto-fill, 20px); gap: 8px; }
  .swatch { width: 20px; height: 20px; padding: 0; border: 0; border-radius: 4px; background: transparent; cursor: pointer; }
  .swatch span { position: relative; display: block; width: 100%; height: 100%; border-radius: 4px; overflow: hidden; box-shadow: inset 0 0 0 1px rgba(0, 0, 0, .1); }
  /* Opaque on the left half, as it is, over a checkerboard, on the right. */
  .swatch span::after { content: ""; position: absolute; inset: 0; background: linear-gradient(to right, var(--solid) 50%, var(--c) 50%); }
  .swatch:hover span, .swatch:focus-visible span { box-shadow: 0 0 0 2px var(--border-strong); }
  .swatch.current span { box-shadow: 0 0 0 2px var(--accent); }
  .swatch:focus-visible { outline: none; }
</style>
