<script lang="ts" module>
  export type Property = { leaf: string; field: string; kind: string | null; text: string | null; bind: string | null; edit: string; via: string | null; why: string | null };
</script>

<script lang="ts">
  import { onDestroy } from "svelte";
  import Select from "./Select.svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  let { field: f, node, onSet, onRefresh, onError }: {
    field: Property; node: string;
    onSet: (field: Property, text: string) => void;
    onRefresh: () => void; onError: (error: string) => void;
  } = $props();
  const doc = controllerManager.docClient;
  const locked = $derived(f.edit === "locked");
  const percent = $derived(f.field === "opacity" || f.leaf === "arc.inner");
  const angle = $derived(f.leaf === "transform2d.rot" || f.leaf === "arc.start" || f.leaf === "arc.sweep");
  const factor = $derived(percent ? 100 : angle ? 180 / Math.PI : 1);
  const unit = $derived(percent ? "%" : angle ? "°" : "");
  const options = $derived(({
    "gradient.kind": ["linear", "radial"], "group_clip.shape": ["rect", "ellipse"],
    "stroke.join": ["miter", "round", "bevel"], "stroke.cap": ["butt", "round", "square"],
    "path.fill_rule": ["nonzero", "evenodd"],
  } as Record<string, string[]>)[f.leaf]);
  const labels: Record<string, string> = { x: "X", y: "Y", w: "W", h: "H", rot: "Rotation", sx: "Scale X", sy: "Scale Y", rx: "Radius X", ry: "Radius Y", opacity: "Opacity", kind: "Type", start: "Start", end: "End", enabled: "Clip content", radius: "Radius", color: "Colour", width: "Weight", fill_rule: "Fill rule", dash_offset: "Offset" };
  let alt = $state(false);
  let dragging = $state(false);
  let draft = $state(0);
  let session: { node: string; leaf: string; origin: number; x: number; token: number; failed: boolean; queue: Promise<void> } | null = null;
  const shown = $derived(dragging ? draft : f.kind === "number" ? Math.round(Number(f.text) * factor * 1000) / 1000 : f.text ?? "");

  function start(e: PointerEvent) {
    if (!e.altKey || locked || f.kind !== "number" || e.button !== 0) return;
    e.preventDefault(); e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragging = true; draft = Number(f.text) * factor;
    const s = { node, leaf: f.leaf, origin: draft, x: e.clientX, token: 0, failed: false, queue: Promise.resolve() };
    session = s;
    s.queue = doc.call("inspect_scrub", [s.node, s.leaf, "begin", 0, 0]).then((r) => {
      if (!r.ok) throw new Error(r.error);
      s.token = r.token;
    }).catch((e) => { s.failed = true; onError(String(e)); });
  }
  function move(e: PointerEvent) {
    alt = e.altKey;
    const s = session;
    if (!s || !dragging) return;
    const step = /\.(sx|sy|x0|y0|x1|y1)$/.test(s.leaf) ? 0.01 : 1;
    draft = Math.round((s.origin + (e.clientX - s.x) * step * (e.shiftKey ? 0.1 : 1)) * 1000) / 1000;
    if (percent) draft = Math.max(0, Math.min(100, draft));
    if (/^(size\.|radius\.|corners\.|group_clip\.(w|h|radius)|stroke.width|shadow.blur)/.test(s.leaf)) draft = Math.max(0, draft);
    const value = draft / factor;
    s.queue = s.queue.then(async () => {
      if (s.failed) return;
      const r = await doc.call("inspect_scrub", [s.node, s.leaf, "move", s.token, value]);
      if (!r.ok) { s.failed = true; onError(r.error); }
    }).catch((e) => { s.failed = true; onError(String(e)); });
  }
  function finish(cancel = false) {
    const s = session;
    if (!s) return;
    session = null;
    // Keep the displayed preview until the backend acknowledges the commit.
    s.queue.then(async () => {
      if (s.token) {
        const r = await doc.call("inspect_scrub", [s.node, s.leaf, cancel || s.failed ? "cancel" : "commit", s.token, 0]);
        if (!r.ok && !s.failed) onError(r.error);
      }
    }).catch((e) => onError(String(e))).finally(() => { dragging = false; onRefresh(); });
  }
  // Capture Escape before the canvas' window keyboard listener: this
  // gesture owns it even though Alt-press deliberately leaves focus alone.
  $effect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape" && session) {
        e.preventDefault(); e.stopImmediatePropagation(); finish(true);
      }
    };
    window.addEventListener("keydown", key, { capture: true });
    return () => window.removeEventListener("keydown", key, { capture: true });
  });
  onDestroy(() => finish(true));
</script>

<svelte:window onkeydown={(e: KeyboardEvent) => { alt = e.altKey; }} onkeyup={(e: KeyboardEvent) => alt = e.altKey} onblur={() => { alt = false; finish(true); }} />

<label class="field" class:wide={f.kind !== "number" && f.kind !== "bool"} class:locked data-leaf={f.leaf} title={f.why ?? (f.via ? `Edits ${f.via} through the binding` : f.kind === "number" ? "Alt + drag to adjust · Shift for precision · Esc to cancel" : "")}>
  <span class="label" class:bound={f.bind}>{labels[f.field] ?? f.field.replaceAll("_", " ")}</span>
  <span class="control" class:scrub={alt && f.kind === "number" && !locked} class:dragging>
    {#if f.kind === "bool"}
      <input aria-label={labels[f.field] ?? f.field} type="checkbox" checked={f.text === "true"} disabled={locked} onchange={(e) => onSet(f, String(e.currentTarget.checked))} />
    {:else if options}
      <Select label={labels[f.field] ?? f.field} value={f.text ?? options[0]} items={options.map((v) => ({ value: v, label: v[0].toUpperCase() + v.slice(1) }))} disabled={locked} onChange={(v) => onSet(f, v)} />
    {:else if f.kind === "color"}
      <input aria-label={labels[f.field] ?? f.field} class="swatch" type="color" value={(f.text ?? "#000000").slice(0, 7)} disabled={locked} onchange={(e) => onSet(f, e.currentTarget.value + (f.text ?? "").slice(7, 9))} />
      <input aria-label={`${labels[f.field] ?? f.field} hex`} class="hex" value={(f.text ?? "").replace("#", "").toUpperCase()} disabled={locked} onchange={(e) => onSet(f, `#${e.currentTarget.value.replace("#", "")}`)} />
    {:else}
      <input aria-label={labels[f.field] ?? f.field} class:number={f.kind === "number"} type={f.kind === "number" ? "number" : "text"} step="any" value={shown} disabled={locked}
        onpointerdown={start} onpointermove={move} onpointerup={() => finish()} onpointercancel={() => finish(true)} onlostpointercapture={() => { if (session) finish(true); }}
        onchange={(e) => { if (!dragging) onSet(f, f.kind === "number" ? String(Number(e.currentTarget.value) / factor) : e.currentTarget.value); }} />
      {#if unit}<span class="unit">{unit}</span>{/if}
    {/if}
  </span>
  {#if f.bind}<span class="binding">↳ {f.via ?? f.bind}</span>{/if}
</label>

<style>
  .field { display: flex; flex-direction: column; min-width: 0; gap: 5px; font-size: 11px; }
  .label { color: var(--fg-muted); }
  .bound, .binding { color: var(--accent); }
  .binding { font-size: 10px; overflow-wrap: anywhere; }
  .wide { grid-column: 1 / -1; }
  .control { display: flex; align-items: center; min-height: 30px; background: var(--bg-sunken); border: 1px solid transparent; border-radius: 5px; overflow: hidden; }
  .control:focus-within { border-color: var(--accent); }
  .control:hover { border-color: var(--border-strong); }
  input:not([type="checkbox"]) { width: 100%; min-width: 0; border: none; background: transparent; box-shadow: none; font-size: 12px; padding: 6px 8px; outline: none; color: var(--fg); }
  input[type="number"] { appearance: textfield; -moz-appearance: textfield; }
  input::-webkit-inner-spin-button, input::-webkit-outer-spin-button { -webkit-appearance: none; margin: 0; }
  .scrub input, .dragging input { cursor: ew-resize; user-select: none; }
  .dragging { border-color: var(--accent); }
  .unit { color: var(--fg-faint); padding-right: 9px; }
  .control .swatch { width: 26px; height: 26px; padding: 3px; margin-left: 3px; flex: 0 0 26px; cursor: pointer; }
  .hex { font-variant-numeric: tabular-nums; }
  input[type="checkbox"] { margin: 8px; accent-color: var(--accent); }
  .locked { opacity: 0.55; }
  .control :global(.ui-select-trigger) { width: 100%; min-width: 0; border: 0; background: transparent; }
</style>
