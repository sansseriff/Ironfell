<script lang="ts" module>
  export type Property = { leaf: string; field: string; kind: string | null; text: string | null; bind: string | null; edit: string; via: string | null; why: string | null };
</script>

<script lang="ts">
  import { onDestroy, type Snippet } from "svelte";
  import Select from "./Select.svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  let { field: f, node, onSet, onRefresh, onError, compact = false, label, prefix, icon, inlineUnit = false, channel, scrubLeaves, mixed = false, hideSwatch = false, colorDigits = 8 }: {
    field: Property; node: string;
    onSet: (field: Property, text: string) => void;
    onRefresh: () => void; onError: (error: string) => void;
    compact?: boolean; label?: string; prefix?: string; icon?: Snippet; inlineUnit?: boolean; channel?: "alpha";
    scrubLeaves?: string[]; mixed?: boolean; hideSwatch?: boolean; colorDigits?: 6 | 8;
  } = $props();
  const doc = controllerManager.docClient;
  const locked = $derived(f.edit === "locked");
  const numeric = $derived(channel === "alpha" || f.kind === "number");
  const percent = $derived(channel === "alpha" || f.field === "opacity" || f.leaf === "arc.inner");
  const angle = $derived(f.leaf === "transform2d.rot" || f.leaf === "arc.start" || f.leaf === "arc.sweep");
  const factor = $derived(percent ? 100 : angle ? 180 / Math.PI : 1);
  const unit = $derived(percent ? "%" : angle ? "°" : "");
  const options = $derived(({
    "gradient.kind": ["linear", "radial"], "group_clip.shape": ["rect", "ellipse"],
    "stroke.join": ["miter", "round", "bevel"], "stroke.cap": ["butt", "round", "square"],
    "path.fill_rule": ["nonzero", "evenodd"],
  } as Record<string, string[]>)[f.leaf]);
  const labels: Record<string, string> = { x: "X", y: "Y", w: "W", h: "H", rot: "Rotation", sx: "Scale X", sy: "Scale Y", rx: "Radius X", ry: "Radius Y", opacity: "Opacity", kind: "Type", start: "Start", end: "End", enabled: "Clip content", radius: "Radius", color: "Colour", width: "Weight", fill_rule: "Fill rule", dash_offset: "Offset" };
  const displayLabel = $derived(label ?? labels[f.field] ?? f.field.replaceAll("_", " "));
  const numberValue = $derived(channel === "alpha" ? Math.round(parseInt((f.text ?? "#000000ff").slice(7, 9) || "ff", 16) / 255 * 100) / 100 : Number(f.text));
  function change(text: string) {
    if (channel === "alpha") {
      const alpha = Math.round(Math.max(0, Math.min(100, Number(text))) / 100 * 255).toString(16).padStart(2, "0");
      onSet(f, (f.text ?? "#000000ff").slice(0, 7) + alpha);
    } else { onSet(f, numeric ? String(Number(text) / factor) : text); }
  }
  let alt = $state(false);
  let dragging = $state(false);
  let draft = $state(0);
  let session: { node: string; leaf: string; origin: number; x: number; token: number; options: string; failed: boolean; queue: Promise<void> } | null = null;
  const shown = $derived(dragging ? draft : numeric ? Math.round(numberValue * factor * 1000) / 1000 : f.text ?? "");

  function start(e: PointerEvent) {
    if (!e.altKey || locked || !numeric || e.button !== 0) return;
    e.preventDefault(); e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragging = true; draft = numberValue * factor;
    onError("");
    const s = { node, leaf: f.leaf, origin: draft, x: e.clientX, token: 0, options: JSON.stringify({ channel, leaves: scrubLeaves }), failed: false, queue: Promise.resolve() };
    session = s;
    s.queue = doc.call("inspect_scrub", [s.node, s.leaf, "begin", 0, 0, s.options]).then((r) => {
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
      const r = await doc.call("inspect_scrub", [s.node, s.leaf, "move", s.token, value, s.options]);
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
        const r = await doc.call("inspect_scrub", [s.node, s.leaf, cancel || s.failed ? "cancel" : "commit", s.token, 0, s.options]);
        if (!r.ok && !s.failed) onError(r.error);
      }
    }).catch((e) => onError(String(e))).finally(() => { if (!session) dragging = false; onRefresh(); });
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

<label class="field" class:wide={!compact && !numeric && f.kind !== "bool"} class:compact class:locked data-leaf={f.leaf} title={f.why ?? (f.via ? `Edits ${f.via} through the binding` : numeric ? "Alt + drag to adjust · Shift for precision · Esc to cancel" : "")}>
  {#if !compact}<span class="label" class:bound={f.bind}>{displayLabel}</span>{/if}
  <!-- Keyboard editing belongs to the input. This wrapper only extends the
       Alt-drag hit area and cancels the label's synthetic click after a scrub. -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <span class="control" role="group" aria-label={displayLabel} class:scrub={alt && numeric && !locked} class:dragging class:inline-unit={inlineUnit && !!unit} style:--value-length={mixed && !dragging ? 5 : String(shown).length}
    onclick={(e) => { if (e.altKey && numeric) e.preventDefault(); }}
    onpointerdown={start} onpointermove={move} onpointerup={() => finish()} onpointercancel={() => finish(true)} onlostpointercapture={() => { if (session) finish(true); }}>
    {#if icon}<span class="field-icon" aria-hidden="true">{@render icon()}</span>{/if}
    {#if prefix}<span class="prefix">{prefix}</span>{/if}
    {#if f.kind === "bool"}
      <input aria-label={displayLabel} type="checkbox" checked={f.text === "true"} disabled={locked} onchange={(e) => onSet(f, String(e.currentTarget.checked))} />
    {:else if options}
      <Select label={displayLabel} value={f.text ?? options[0]} items={options.map((v) => ({ value: v, label: v[0].toUpperCase() + v.slice(1) }))} disabled={locked} onChange={(v) => onSet(f, v)} />
    {:else if f.kind === "color" && channel !== "alpha"}
      {#if !hideSwatch}<input aria-label={displayLabel} class="swatch" type="color" value={(f.text ?? "#000000").slice(0, 7)} disabled={locked} onchange={(e) => onSet(f, e.currentTarget.value + (f.text ?? "").slice(7, 9))} />{/if}
      <input aria-label={`${displayLabel} hex`} class="hex" value={(f.text ?? "").slice(1, colorDigits + 1).toUpperCase()} disabled={locked} onchange={(e) => onSet(f, `#${e.currentTarget.value.replace("#", "")}`)} />
    {:else}
      <input aria-label={displayLabel} class:number={numeric} type={numeric ? "number" : "text"} step="any" value={mixed && !dragging ? "" : shown} placeholder={mixed ? "Mixed" : undefined} disabled={locked}
        onchange={(e) => { if (!dragging) change(e.currentTarget.value); }} />
      {#if unit}<span class="unit">{unit}</span>{/if}
    {/if}
  </span>
  {#if f.bind}<span class="binding">↳ {f.via ?? f.bind}</span>{/if}
</label>

<style>
  .field { display: flex; flex-direction: column; min-width: 0; gap: 5px; font-size: 11px; }
  .compact { gap: 0; }
  .compact .control { min-height: 32px; }
  .compact .binding { display: none; }
  .prefix { color: var(--fg-faint); padding-left: 10px; font-size: 11px; }
  .field-icon { display: flex; align-items: center; justify-content: center; flex: 0 0 16px; margin-left: 8px; color: var(--fg-muted); }
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
  .scrub, .scrub input, .dragging, .dragging input { cursor: ew-resize; user-select: none; }
  .dragging { border-color: var(--accent); }
  .compact .unit { padding-right: 8px; }
  .unit { color: var(--fg-faint); padding-right: 9px; }
  .inline-unit input { width: calc(var(--value-length) * 1ch + 1px); flex: 0 1 auto; box-sizing: content-box; padding-right: 0; font-variant-numeric: tabular-nums; }
  .inline-unit .unit { color: var(--fg); padding: 0 8px 0 0; font-size: 12px; }
  .control .swatch { width: 26px; height: 26px; padding: 3px; margin-left: 3px; flex: 0 0 26px; cursor: pointer; }
  .hex { font-variant-numeric: tabular-nums; }
  input[type="checkbox"] { margin: 8px; accent-color: var(--accent); }
  .locked { opacity: 0.55; }
  .control :global(.ui-select-trigger) { width: 100%; min-width: 0; border: 0; background: transparent; }
</style>
