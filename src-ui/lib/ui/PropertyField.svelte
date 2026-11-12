<script lang="ts" module>
  export type Property = { mixed?: boolean; mixedColor?: boolean; mixedAlpha?: boolean; nodes?: string[]; leaf: string; field: string; kind: string | null; text: string | null; full?: string | null; bind: string | null; edit: string; via: string | null; why: string | null; options?: string[] | null; key?: "none" | "animated" | "on"; error?: string | null;
    /** A component input's declared type (types-and-units.md §3.1). */
    datatype?: string | null; auto?: boolean; unit?: string | null; show?: string | null; group?: string | null };
</script>

<script lang="ts">
  import { onDestroy, tick, type Snippet } from "svelte";
  import SquareFunction from "@lucide/svelte/icons/square-function";
  import Unlink from "@lucide/svelte/icons/unlink";
  import Diamond from "@lucide/svelte/icons/diamond";
  import Select from "./Select.svelte";
  import Segmented from "./Segmented.svelte";
  import Checkbox from "./Checkbox.svelte";
  import Button from "./Button.svelte";
  import IconButton from "./IconButton.svelte";
  import EasingField from "./EasingField.svelte";
  import ColorPicker from "./ColorPicker.svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  let { field: f, node, onSet, onRefresh, onError, compact = false, label, prefix, icon, channel, scrubLeaves, hideSwatch = false, colorDigits = 8, mixed = (channel === "alpha" ? f.mixedAlpha : f.kind === "color" && colorDigits === 6 ? f.mixedColor : f.mixed) ?? f.mixed ?? false, zeroLabel }: {
    field: Property; node: string;
    onSet: (field: Property, text: string) => void | Promise<void>;
    onRefresh: () => void; onError: (error: string) => void;
    compact?: boolean; label?: string; prefix?: string; icon?: Snippet; channel?: "alpha";
    scrubLeaves?: string[]; mixed?: boolean; hideSwatch?: boolean; colorDigits?: 6 | 8;
    /** Shown instead of a zero value (a text's line height 0 is "Auto"). */
    zeroLabel?: string;
  } = $props();
  const doc = controllerManager.docClient;
  const locked = $derived(f.edit === "locked");
  const numeric = $derived(channel === "alpha" || f.kind === "number");
  const percent = $derived(channel === "alpha" || f.field === "opacity" || f.leaf === "arc.inner" || f.leaf === "composition.grid_opacity");
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
  const token = $derived(!mixed && f.kind === "color" ? f.bind?.match(/^@([A-Za-z0-9_-]+)$/)?.[1] ?? null : null);
  const editingFormula = $derived(formula || (!mixed && !!f.bind && f.edit !== "via" && !token));
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
      await onSet(f, `alpha:${parseInt(alpha, 16) / 255}`);
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
    const r = await doc.command({ op: 'inspect_patch', node, patch: JSON.stringify({ nodes: f.nodes, values: {}, unbind: [f.leaf], label: `remove formula from ${f.leaf}` }) });
    if (!r.ok) onError(r.error);
    formula = false;
    onRefresh();
  }
  let alt = $state(false);
  let dragging = $state(false);
  let draft = $state(0);
  let session: { node: string; leaf: string; origin: number; x: number; token: number; options: string; failed: boolean; queue: Promise<void> } | null = null;
  const shown = $derived(dragging ? draft : numeric ? Math.round(numberValue * factor * 1000) / 1000 : f.text ?? "");
  // A zero shown as its label ("Auto"), the field empty with it as placeholder.
  const zero = $derived(!!zeroLabel && !dragging && numeric && Number(shown) === 0);
  const placeholder = $derived(mixed ? "Mixed" : zero ? zeroLabel : isAuto ? "Auto" : undefined);
  // A % or ° sits right after the value ("45°"), so the input is as wide as
  // what it shows.
  const valueLength = $derived(Math.max(placeholder?.length ?? 0, mixed && !dragging ? 0 : zero ? 0 : String(shown).length, 1));
  // A segmented choice or a checkbox is its own control, not in a field.
  const segmented = $derived(!!options && options.length <= 4 && !token && !editingFormula && f.kind !== "bool");
  const bare = $derived(segmented || (f.kind === "bool" && !token && !editingFormula));

  function start(e: PointerEvent) {
    if (!e.altKey || locked || !numeric || e.button !== 0) return;
    e.preventDefault(); e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragging = true; draft = numberValue * factor;
    onError("");
    const s = { node, leaf: f.leaf, origin: draft, x: e.clientX, token: 0, options: JSON.stringify({ channel, leaves: scrubLeaves, nodes: f.nodes }), failed: false, queue: Promise.resolve() };
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
  {#if animatable && !compact}<IconButton icon={Diamond} size={12} class="key {f.key === 'none' ? 'reveal' : 'animated'}" data-key={f.key} label={f.key === "on" ? `Remove the key on ${displayLabel}` : `Key ${displayLabel} at the playhead`} title={f.key === "on" ? "Remove the key at the playhead" : f.key === "animated" ? "Add a key at the playhead" : "Animate: key this value at the playhead"} onclick={keyHere} />{/if}
{/snippet}

<svelte:window onkeydown={(e: KeyboardEvent) => { alt = e.altKey; }} onkeyup={(e: KeyboardEvent) => alt = e.altKey} onblur={() => { alt = false; finish(true); }} />

<label class="field" class:wide={!compact && !numeric && f.kind !== "bool"} class:compact class:locked data-leaf={f.leaf} title={f.why ?? (f.via ? `Edits ${f.via} through the binding` : numeric ? "Alt + drag to adjust · Shift for precision · Esc to cancel" : "")}>
  {#if !compact}<span class="label" class:bound={f.bind}>{displayLabel}</span>{/if}
  <!-- Keyboard editing belongs to the input. This wrapper only extends the
       Alt-drag hit area and cancels the label's synthetic click after a scrub. -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <span class="control" class:ui-field={!bare} class:bare role="group" aria-label={displayLabel} data-disabled={locked && !bare ? "" : undefined} class:scrub={alt && numeric && !locked} class:dragging class:inline-unit={!!unit} style:--value-length={valueLength}
    onclick={(e) => { if (e.altKey && numeric) e.preventDefault(); }}
    onpointerdown={start} onpointermove={move} onpointerup={() => finish()} onpointercancel={() => finish(true)} onlostpointercapture={() => { if (session) finish(true); }}>
    {#if icon}<span class="field-icon" aria-hidden="true">{@render icon()}</span>{/if}
    {#if prefix}<span class="prefix">{prefix}</span>{/if}
    {#if token && !formula}
      <span class="token-chip" title="Follows the colour token @{token} (light and dark theme). Detach to keep this colour."><span class="dot" style:background={f.text ?? "transparent"}></span>@{token}</span>
      {#if !locked}<IconButton icon={Unlink} label={`Detach ${displayLabel} from @${token}`} title="Detach: keep this colour" onclick={unbind} />{/if}
    {:else if editingFormula}
      <span class="fx" aria-hidden="true">ƒx</span>
      <input aria-label={`${displayLabel} formula`} class="formula" type="text" spellcheck="false" value={fieldText()} disabled={locked}
        placeholder="= #id.facet.field * 2"
        onkeydown={(e) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); formula = false; e.currentTarget.value = fieldText(); e.currentTarget.blur(); } }}
        onchange={(e) => change(e.currentTarget)} />
      {#if f.bind && !locked}<IconButton icon={Unlink} label={`Remove the formula from ${displayLabel}`} title="Remove formula (keep the current value)" onclick={unbind} />{/if}
    {:else if f.kind === "bool"}
      <Checkbox label={displayLabel} showLabel={false} indeterminate={mixed} checked={!mixed && f.text === "true"} disabled={locked} onChange={(on) => onSet(f, String(on))} />
    {:else if f.field === "easing" || (f.field === "ease" && f.kind === "str")}
      <!-- A curve: previews, and (where text is allowed) a cubic-bezier. -->
      <EasingField label={displayLabel} value={f.text} custom={f.kind === "str"} disabled={locked} onChange={(v) => onSet(f, v)} />
    {:else if options && segmented}
      <Segmented label={displayLabel} value={mixed ? "" : f.text ?? options[0]} items={options.map((v) => ({ value: v, label: v[0].toUpperCase() + v.slice(1) }))} disabled={locked} onChange={(v) => onSet(f, v)} />
      {@render keyButton()}
    {:else if options}
      <Select placeholder={mixed ? "Mixed" : ""} label={displayLabel} value={mixed ? "" : f.text ?? options[0]} items={options.map((v) => ({ value: v, label: v[0].toUpperCase() + v.slice(1) }))} disabled={locked} onChange={(v) => onSet(f, v)} />
      {@render keyButton()}
    {:else if f.kind === "color" && channel !== "alpha"}
      {#if !hideSwatch}<ColorPicker swatch field={f} {node} label={displayLabel} {onSet} {onRefresh} {onError} />{/if}
      <input aria-label={`${displayLabel} hex`} class="hex" placeholder={mixed ? "Mixed" : undefined} value={mixed ? "" : (f.text ?? "").slice(1, colorDigits + 1).toUpperCase()} disabled={locked} onchange={(e) => onSet(f, `#${e.currentTarget.value.replace("#", "")}`)} />
    {:else}
      <input aria-label={displayLabel} class:number={numeric} type="text" inputmode={numeric ? "decimal" : undefined} value={zero ? "" : fieldText()} {placeholder} disabled={locked}
        oninput={(e) => { if (isFormula(e.currentTarget.value)) formula = true; }}
        onchange={(e) => { if (!dragging) change(e.currentTarget); }} />
      {#if unit && !zero}<span class="unit">{unit}</span>{/if}
      {#if f.unit && !isAuto}<span class="unit">{f.unit}</span>{/if}
      {#if f.auto && !locked && !isAuto && !editingFormula}<Button small variant="outline" class="auto-toggle" title="Back to automatic" onclick={() => onSet(f, "auto")}>Auto</Button>{/if}
      {@render keyButton()}
      {#if !locked && !compact}<IconButton icon={SquareFunction} class="reveal" label={`Write a formula for ${displayLabel}`} title="Formula (or type =)" onclick={openFormula} />{/if}
    {/if}
  </span>
  {#if f.bind && !editingFormula}<span class="binding">↳ {f.via ?? f.bind}</span>{/if}
  {#if f.error}<span class="field-error" role="alert">{f.error}</span>{/if}
</label>

<style>
  .field { display: flex; flex-direction: column; min-width: 0; gap: 4px; font-size: var(--text-sm); }
  .compact { gap: 0; }
  .compact .binding { display: none; }
  .label { color: var(--fg-muted); }
  .bound, .binding { color: var(--accent); }
  .binding, .field-error { font-size: var(--text-xs); overflow-wrap: anywhere; }
  .field-error { color: var(--danger); }
  .wide { grid-column: 1 / -1; }
  .locked .label { opacity: 0.4; }
  /* The field's shell is `.ui-field` (ui.css); its parts sit in a row. */
  .control { overflow: hidden; }
  .control > :global(.ui-icon-button) { margin-right: 1px; }
  .control.bare { display: flex; align-items: center; gap: 4px; min-height: var(--control-h); overflow: visible; }
  .control.bare > :global(.ui-segmented) { flex: 1; min-width: 0; }
  .prefix { flex: none; padding-left: 8px; color: var(--fg-faint); }
  .field-icon { display: flex; align-items: center; justify-content: center; flex: 0 0 14px; margin-left: 8px; color: var(--fg-muted); }
  input { width: 100%; min-width: 0; height: calc(var(--control-h) - 2px); padding: 0 8px; border: none; background: transparent; box-shadow: none; color: var(--fg); font-size: var(--text-md); outline: none; }
  .scrub, .scrub input, .dragging, .dragging input { cursor: ew-resize; user-select: none; }
  .control.dragging, .control.dragging.dragging:hover { border-color: var(--accent); }
  .unit { flex: none; padding-right: 8px; color: var(--fg-faint); }
  /* A % or ° reads with its value: the input is the value's width. */
  .inline-unit input { flex: 0 1 auto; width: calc(var(--value-length) * 1ch + 1px); box-sizing: content-box; padding-right: 0; font-variant-numeric: tabular-nums; }
  .inline-unit .unit { flex: 1; padding-left: 1px; color: var(--fg); }
  .control > :global(.color-swatch) { margin-left: 2px; }
  .hex { font-variant-numeric: tabular-nums; }
  .fx { padding-left: 8px; color: var(--accent); font-size: var(--text-sm); font-style: italic; }
  input.formula { font-family: var(--font-mono); font-size: var(--text-sm); color: var(--accent); }
  .control :global(.reveal) { opacity: 0; }
  .control:hover :global(.reveal), .control:focus-within :global(.reveal), .control :global(.reveal:focus-visible) { opacity: 1; }
  .control :global(.key.animated) { color: var(--accent); }
  .control :global(.key[data-key="on"] svg) { fill: currentColor; }
  .control :global(.auto-toggle) { flex: none; margin-right: 2px; }
  .token-chip { display: inline-flex; align-items: center; gap: 6px; flex: 1; min-width: 0; padding: 0 8px; font-size: var(--text-md); color: var(--fg); overflow: hidden; white-space: nowrap; }
  .token-chip .dot { flex: 0 0 12px; width: 12px; height: 12px; border-radius: var(--radius-sm); box-shadow: inset 0 0 0 1px var(--border-strong); }
  /* A select or easing inside the field wears the field's shell, not its own. */
  .control :global(.ui-select-trigger) { width: 100%; min-width: 0; min-height: calc(var(--control-h) - 2px); border: 0; background: transparent; }
</style>
