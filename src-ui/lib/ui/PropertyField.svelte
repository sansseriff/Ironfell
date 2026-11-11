<script lang="ts" module>
  export type Property = { leaf: string; field: string; kind: string | null; text: string | null; full?: string | null; bind: string | null; edit: string; via: string | null; why: string | null; options?: string[] | null; key?: "none" | "animated" | "on"; error?: string | null;
    /** A component input's declared type (types-and-units.md §3.1). */
    datatype?: string | null; auto?: boolean; unit?: string | null; show?: string | null; group?: string | null };
</script>

<script lang="ts">
  import { onDestroy, tick, type Snippet } from "svelte";
  import SquareFunction from "@lucide/svelte/icons/square-function";
  import Unlink from "@lucide/svelte/icons/unlink";
  import Diamond from "@lucide/svelte/icons/diamond";
  import Select from "./Select.svelte";
  import EasingField from "./EasingField.svelte";
  import { ToggleGroup } from "bits-ui";
  import { controllerManager } from "../../controller-manager.svelte";
  let { field: f, node, onSet, onRefresh, onError, compact = false, label, prefix, icon, inlineUnit = false, channel, scrubLeaves, mixed = false, hideSwatch = false, colorDigits = 8, zeroLabel }: {
    field: Property; node: string;
    onSet: (field: Property, text: string) => void | Promise<void>;
    onRefresh: () => void; onError: (error: string) => void;
    compact?: boolean; label?: string; prefix?: string; icon?: Snippet; inlineUnit?: boolean; channel?: "alpha";
    scrubLeaves?: string[]; mixed?: boolean; hideSwatch?: boolean; colorDigits?: 6 | 8;
    /** Shown instead of a zero value (a text's line height 0 is "Auto"). */
    zeroLabel?: string;
  } = $props();
  const doc = controllerManager.docClient;
  const locked = $derived(f.edit === "locked");
  const numeric = $derived(channel === "alpha" || f.kind === "number");
  const percent = $derived(channel === "alpha" || f.field === "opacity" || f.leaf === "arc.inner");
  const angle = $derived(f.leaf === "transform2d.rot" || f.leaf === "arc.start" || f.leaf === "arc.sweep");
  const factor = $derived(percent ? 100 : angle ? 180 / Math.PI : 1);
  // Stored as a percent already (plans/specs/text.md §2): shown with %, unscaled.
  const percentStored = $derived(f.leaf === "font.line_height" || f.leaf === "font.letter_spacing");
  const unit = $derived(percent || percentStored ? "%" : angle ? "°" : "");
  // The schema's values for an enum field (`facet.rs`).
  const options = $derived(f.options ?? undefined);
  const labels: Record<string, string> = { span: "Span (s)", t: "Time (s)", value: "Value", easing: "Easing", mode: "Pieces", x: "X", y: "Y", w: "W", h: "H", rot: "Rotation", sx: "Scale X", sy: "Scale Y", rx: "Radius X", ry: "Radius Y", opacity: "Opacity", kind: "Type", start: "Start", end: "End", enabled: "Clip content", radius: "Radius", color: "Colour", width: "Weight", fill_rule: "Fill rule", dash_offset: "Offset" };
  const displayLabel = $derived(label ?? labels[f.field] ?? f.field.replaceAll("_", " "));
  const numberValue = $derived(channel === "alpha" ? Math.round(parseInt((f.text ?? "#000000ff").slice(7, 9) || "ff", 16) / 255 * 100) / 100 : Number(f.text));
  // Formulas (plans/specs/wires-and-formulas.md §2): any field takes one,
  // typed after "=" as in a spreadsheet, or through the ƒx button. A bound
  // field shows its formula; Remove keeps the value it shows now.
  let formula = $state(false);
  // A colour bound to a token (`@series-2`, palette.rs) shows as the token.
  const token = $derived(f.kind === "color" ? f.bind?.match(/^@([A-Za-z0-9_-]+)$/)?.[1] ?? null : null);
  const editingFormula = $derived(formula || (!!f.bind && f.edit !== "via" && !token));
  const isFormula = (text: string) => /^\s*(=|\{=)/.test(text);
  // `| auto` (stored as NaN): the field is empty with "Auto" in it, and the
  // Auto toggle puts it back.
  const isAuto = $derived(!!f.auto && (f.text === "NaN" || f.text === "auto"));
  async function change(el: HTMLInputElement) {
    let text = el.value;
    // A bare number in a field with a unit is in that unit (`3` → 3 mm).
    if (f.unit && text.trim() !== "" && Number.isFinite(Number(text))) text = `${text.trim()} ${f.unit}`;
    if (f.auto && text.trim() === "") text = "auto";
    if (isFormula(text) || editingFormula) {
      await onSet(f, isFormula(text) ? text : `=${text}`);
      formula = false;
    } else if (channel === "alpha") {
      const alpha = Math.round(Math.max(0, Math.min(100, Number(text))) / 100 * 255).toString(16).padStart(2, "0");
      await onSet(f, (f.text ?? "#000000ff").slice(0, 7) + alpha);
    } else {
      // A number field also takes a quantity ("400 nm"), sent as typed.
      const n = Number(text);
      await onSet(f, numeric && !f.unit && text.trim() !== "" && Number.isFinite(n) ? String(n / factor) : text);
    }
    // A refused edit leaves the field showing what it holds, not the typo.
    await tick();
    el.value = String(fieldText());
  }
  function fieldText(): string {
    if (editingFormula) return f.bind ? `= ${f.bind}` : "= ";
    if (mixed && !dragging) return "";
    if (isAuto) return "";
    // A quantity in its field's unit shows as the number alone.
    if (f.unit && f.text?.endsWith(` ${f.unit}`)) return f.text.slice(0, -f.unit.length - 1);
    return String(!numeric && f.full ? f.full : shown);
  }
  async function openFormula(e: MouseEvent) {
    formula = true;
    await tick();
    const input = (e.currentTarget as HTMLElement).closest(".field")?.querySelector("input.formula") as HTMLInputElement | null;
    input?.focus();
    input?.setSelectionRange(input.value.length, input.value.length);
  }
  // ◇ (After Effects' stopwatch, timeline.md §12.3): key this property at
  // the playhead, or remove the key there.
  // Numbers, colours and quantities interpolate; a choice (a component's
  // state) steps with hold keys.
  const animatable = $derived(!!f.key && !locked && !f.bind && (numeric || f.kind === "color" || f.kind === "quantity" || !!f.options));
  async function keyHere() {
    onError("");
    const r = await doc.command({ op: 'inspect_animate', node, leaf: f.leaf, action: "key" });
    if (!r.ok) onError(r.error);
    onRefresh();
  }
  async function unbind() {
    onError("");
    const r = await doc.command({ op: 'inspect_patch', node, patch: JSON.stringify({ values: {}, unbind: [f.leaf], label: `remove formula from ${f.leaf}` }) });
    if (!r.ok) onError(r.error);
    formula = false;
    onRefresh();
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
    s.queue = doc.command({ op: 'inspect_scrub', node: s.node, leaf: s.leaf, phase: "begin", token: 0, value: 0, options: s.options }).then((r) => {
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
    if (/^(size\.|radius\.|corners\.|group_clip\.(w|h|radius)|stroke.width|shadow.blur|font\.line_height)/.test(s.leaf)) draft = Math.max(0, draft);
    if (s.leaf === "font.size" || s.leaf === "composition.grid_spacing") draft = Math.max(1, draft);
    const value = draft / factor;
    s.queue = s.queue.then(async () => {
      if (s.failed) return;
      const r = await doc.command({ op: 'inspect_scrub', node: s.node, leaf: s.leaf, phase: "move", token: s.token, value: value, options: s.options });
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
        const r = await doc.command({ op: 'inspect_scrub', node: s.node, leaf: s.leaf, phase: cancel || s.failed ? "cancel" : "commit", token: s.token, value: 0, options: s.options });
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

{#snippet keyButton()}
  {#if animatable && !compact}<button type="button" class="field-action key" class:reveal={f.key === "none"} class:animated={f.key !== "none"} aria-label={f.key === "on" ? `Remove the key on ${displayLabel}` : `Key ${displayLabel} at the playhead`} title={f.key === "on" ? "Remove the key at the playhead" : f.key === "animated" ? "Add a key at the playhead" : "Animate: key this value at the playhead"} onclick={keyHere}><Diamond size={11} strokeWidth={1.75} fill={f.key === "on" ? "currentColor" : "none"} /></button>{/if}
{/snippet}

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
    {#if token && !formula}
      <span class="token-chip" title="Follows the colour token @{token} (light and dark theme). Detach to keep this colour."><span class="dot" style:background={f.text ?? "transparent"}></span>@{token}</span>
      {#if !locked}<button type="button" class="field-action" aria-label={`Detach ${displayLabel} from @${token}`} title="Detach: keep this colour" onclick={unbind}><Unlink size={13} strokeWidth={1.5} /></button>{/if}
    {:else if editingFormula}
      <span class="fx" aria-hidden="true">ƒx</span>
      <input aria-label={`${displayLabel} formula`} class="formula" type="text" spellcheck="false" value={fieldText()} disabled={locked}
        placeholder="= #id.facet.field * 2"
        onkeydown={(e) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); formula = false; e.currentTarget.value = fieldText(); e.currentTarget.blur(); } }}
        onchange={(e) => change(e.currentTarget)} />
      {#if f.bind && !locked}<button type="button" class="field-action" aria-label={`Remove the formula from ${displayLabel}`} title="Remove formula (keep the current value)" onclick={unbind}><Unlink size={13} strokeWidth={1.5} /></button>{/if}
    {:else if f.kind === "bool"}
      <input aria-label={displayLabel} type="checkbox" checked={f.text === "true"} disabled={locked} onchange={(e) => onSet(f, String(e.currentTarget.checked))} />
    {:else if f.field === "easing" || (f.field === "ease" && f.kind === "str")}
      <!-- A curve: previews, and (where text is allowed) a cubic-bezier. -->
      <EasingField label={displayLabel} value={f.text} custom={f.kind === "str"} disabled={locked} onChange={(v) => onSet(f, v)} />
    {:else if options && options.length <= 4}
      <ToggleGroup.Root type="single" class="segmented" value={f.text ?? options[0]} disabled={locked} aria-label={displayLabel} onValueChange={(v) => { if (v && v !== f.text) onSet(f, v); }}>
        {#each options as o (o)}<ToggleGroup.Item class="segment" value={o}>{o[0].toUpperCase() + o.slice(1)}</ToggleGroup.Item>{/each}
      </ToggleGroup.Root>
      {@render keyButton()}
    {:else if options}
      <Select label={displayLabel} value={f.text ?? options[0]} items={options.map((v) => ({ value: v, label: v[0].toUpperCase() + v.slice(1) }))} disabled={locked} onChange={(v) => onSet(f, v)} />
      {@render keyButton()}
    {:else if f.kind === "color" && channel !== "alpha"}
      {#if !hideSwatch}<input aria-label={displayLabel} class="swatch" type="color" value={(f.text ?? "#000000").slice(0, 7)} disabled={locked} onchange={(e) => onSet(f, e.currentTarget.value + (f.text ?? "").slice(7, 9))} />{/if}
      <input aria-label={`${displayLabel} hex`} class="hex" value={(f.text ?? "").slice(1, colorDigits + 1).toUpperCase()} disabled={locked} onchange={(e) => onSet(f, `#${e.currentTarget.value.replace("#", "")}`)} />
    {:else}
      {@const zero = !!zeroLabel && !dragging && numeric && Number(shown) === 0}
      <input aria-label={displayLabel} class:number={numeric} type="text" inputmode={numeric ? "decimal" : undefined} value={zero ? "" : fieldText()} placeholder={mixed ? "Mixed" : zero ? zeroLabel : isAuto ? "Auto" : undefined} disabled={locked}
        oninput={(e) => { if (isFormula(e.currentTarget.value)) formula = true; }}
        onchange={(e) => { if (!dragging) change(e.currentTarget); }} />
      {#if unit && !(zeroLabel && !dragging && numeric && Number(shown) === 0)}<span class="unit">{unit}</span>{/if}
      {#if f.unit && !isAuto}<span class="unit">{f.unit}</span>{/if}
      {#if f.auto && !locked && !isAuto && !editingFormula}<button type="button" class="auto-toggle" title="Back to automatic" onclick={() => onSet(f, "auto")}>Auto</button>{/if}
      {@render keyButton()}
      {#if !locked && !compact}<button type="button" class="field-action reveal" aria-label={`Write a formula for ${displayLabel}`} title="Formula (or type =)" onclick={openFormula}><SquareFunction size={13} strokeWidth={1.5} /></button>{/if}
    {/if}
  </span>
  {#if f.bind && !editingFormula}<span class="binding">↳ {f.via ?? f.bind}</span>{/if}
  {#if f.error}<span class="field-error" role="alert">{f.error}</span>{/if}
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
  .fx { color: var(--accent); font-size: 11px; font-style: italic; padding-left: 8px; }
  input.formula { font-family: var(--font-mono, ui-monospace, monospace); font-size: 11px; color: var(--accent); }
  .field-action { display: flex; align-items: center; justify-content: center; flex: 0 0 22px; height: 22px; margin-right: 4px; border: 0; border-radius: 4px; background: transparent; color: var(--fg-muted); cursor: pointer; padding: 0; }
  .field-action:hover { background: var(--bg-hover, var(--bg)); color: var(--fg); }
  .field-action:focus-visible { outline: 1px solid var(--accent); }
  .reveal { opacity: 0; }
  .key.animated { color: var(--accent); }
  .control :global(.segmented) { display: flex; width: 100%; padding: 2px; gap: 2px; }
  .control :global(.segment) { flex: 1 1 0; min-width: 0; border: 0; border-radius: 4px; background: transparent; color: var(--fg-muted); font-size: 11px; padding: 4px 6px; cursor: pointer; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .control :global(.segment:hover) { color: var(--fg); }
  .control :global(.segment[data-state="on"]) { background: var(--bg); color: var(--fg); box-shadow: 0 0 0 1px var(--border); }
  .control :global(.segment:focus-visible) { outline: 1px solid var(--accent); }
  .auto-toggle { flex: 0 0 auto; margin-right: 4px; border: 1px solid var(--border); border-radius: 4px; background: transparent; color: var(--fg-muted); font-size: 10px; padding: 2px 6px; cursor: pointer; }
  .auto-toggle:focus-visible { outline: 1px solid var(--accent); }
  .token-chip { display: inline-flex; align-items: center; gap: 6px; flex: 1; min-width: 0; padding: 0 8px; font-size: 12px; color: var(--fg); overflow: hidden; white-space: nowrap; }
  .token-chip .dot { flex: 0 0 12px; width: 12px; height: 12px; border-radius: 3px; box-shadow: inset 0 0 0 1px rgba(0, 0, 0, .15); }
  .field-error { color: var(--danger); font-size: 10px; overflow-wrap: anywhere; }
  .control:hover .reveal, .control:focus-within .reveal, .reveal:focus-visible { opacity: 1; }
  .control :global(.ui-select-trigger) { width: 100%; min-width: 0; border: 0; background: transparent; }
</style>
