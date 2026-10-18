<script lang="ts">
  // A text node's inspector section, Figma's layout (plans/specs/text.md §8):
  // family; weight and size; line height and letter spacing; horizontal
  // and vertical alignment, with resizing and figures in the settings. And
  // Iron's own: the content, typed or bound to an expression, so a label
  // can be a live readout.
  import { Popover, DropdownMenu } from "bits-ui";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Baseline from "@lucide/svelte/icons/baseline";
  import MoveHorizontal from "@lucide/svelte/icons/move-horizontal";
  import AlignStart from "@lucide/svelte/icons/text-align-start";
  import AlignCenter from "@lucide/svelte/icons/text-align-center";
  import AlignEnd from "@lucide/svelte/icons/text-align-end";
  import ArrowUpToLine from "@lucide/svelte/icons/arrow-up-to-line";
  import AlignVerticalCenter from "@lucide/svelte/icons/align-vertical-justify-center";
  import ArrowDownToLine from "@lucide/svelte/icons/arrow-down-to-line";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Link from "@lucide/svelte/icons/link";
  import Unlink from "@lucide/svelte/icons/unlink";
  import X from "@lucide/svelte/icons/x";
  import Bold from "@lucide/svelte/icons/bold";
  import Italic from "@lucide/svelte/icons/italic";
  import Underline from "@lucide/svelte/icons/underline";
  import Strikethrough from "@lucide/svelte/icons/strikethrough";
  import Select from "./Select.svelte";
  import FontPicker, { type FontCatalog } from "./FontPicker.svelte";
  import PropertyField, { type Property } from "./PropertyField.svelte";
  import { controllerManager } from "../../controller-manager.svelte";

  let {
    node,
    field,
    patch,
    refresh,
    onError,
  }: {
    node: string;
    field: (kind: string, name: string, text?: string, valueKind?: string) => Property;
    patch: (values: Record<string, string>, remove?: string[], label?: string) => Promise<void>;
    refresh: () => void;
    onError: (e: string) => void;
  } = $props();
  // Numbers go through PropertyField: typed, or Alt-dragged as one
  // previewed gesture that commits once, like every other inspector field.
  const setField = (f: Property, text: string) => (f.leaf === "font.size" && rangeSelected ? style({ size: Number(text) }) : set(f.leaf, text, `set ${f.leaf}`));

  const doc = controllerManager.docClient;

  // The catalog and installed families, asked once per picker session.
  let catalog: FontCatalog | null = $state(null);
  $effect(() => {
    void doc.loadedFonts;
    doc.call("fonts", []).then((c: FontCatalog) => (catalog = c)).catch(() => {});
  });

  const family = $derived(field("font", "family", "Inter", "str").text ?? "Inter");
  const weight = $derived(Number(field("font", "weight", "400").text ?? 400));
  const align = $derived(field("font", "align", "left", "enum").text ?? "left");
  const valign = $derived(field("font", "valign", "top", "enum").text ?? "top");
  const sizing = $derived(field("text", "sizing", "auto_width", "enum").text ?? "auto_width");
  const tabular = $derived(field("font", "tabular", "false", "bool").text === "true");
  const italic = $derived(field("font", "italic", "false", "bool").text === "true");
  const content = $derived(field("text", "content", "", "str"));
  const underline = $derived(field("font", "underline", "false", "bool").text === "true");
  const strike = $derived(field("font", "strike", "false", "bool").text === "true");
  const fillColor = $derived(field("fill", "color", "#000000ff", "color").text ?? "#000000ff");

  // Characters selected in the canvas edit: styling goes to them alone
  // (plans/specs/text.md §2.1); otherwise to the whole node.
  const rangeSelected = $derived(doc.textEdit.active && doc.textEdit.node === node && doc.textEdit.anchor !== doc.textEdit.focus);
  async function style(patch: Record<string, unknown>) {
    onError("");
    try {
      const r = await doc.call("text_style", [node, JSON.stringify(patch)]);
      if (!r.ok) onError(r.error);
      refresh();
    } catch (e) { onError(String(e)); }
  }
  // Toggles inside an edit use the engine's rule (on unless all of it is);
  // on the node they flip its own value.
  function toggle(what: "bold" | "italic" | "underline" | "strike") {
    if (rangeSelected) {
      doc.textToggle(what);
      return;
    }
    const patch = { bold: { weight: weight >= 600 ? 400 : 700 }, italic: { italic: !italic }, underline: { underline: !underline }, strike: { strike: !strike } }[what];
    void style(patch);
  }
  function hexToRgba(hex: string): number[] {
    const h = hex.replace("#", "");
    const ch = (i: number) => parseInt(h.slice(i, i + 2), 16) / 255;
    return [ch(0), ch(2), ch(4), h.length >= 8 ? ch(6) : 1];
  }

  const known = $derived(catalog?.families.find((f) => f.name.toLowerCase() === family.toLowerCase()));
  const installed = $derived(!!catalog?.system.some((n) => n.toLowerCase() === family.toLowerCase()));
  const missing = $derived(!!catalog && !known && !installed);
  const weights = $derived.by(() => {
    const all = catalog?.weights ?? [{ weight: 400, name: "Regular" }];
    const [lo, hi] = known?.weights ?? [100, 900];
    const fit = all.filter((w) => w.weight >= lo && w.weight <= hi);
    return (fit.length ? fit : all).map((w) => ({ value: String(w.weight), label: w.name }));
  });
  const SIZES = [10, 11, 12, 13, 14, 16, 20, 24, 32, 36, 40, 48, 64, 96, 128];

  let pickerOpen = $state(false);
  let anchor: HTMLDivElement | undefined = $state();

  function set(leaf: string, text: string, label = `set ${leaf}`) {
    return patch({ [leaf]: text }, [], label);
  }
  function onEnter(e: KeyboardEvent) {
    if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
  }

  // Content: typed, or bound to an expression (a live readout).
  let binding = $state(false);
  let expr = $state("");
  async function bind(text: string) {
    onError("");
    const r = await doc.call("inspect_bind", [node, "text.content", text]);
    if (!r.ok) onError(r.error);
    else binding = false;
  }
</script>

<div class="property-section text-section" data-section="text">
  <div class="component"><span>Text</span></div>

  <div class="family" bind:this={anchor}>
    <Popover.Root bind:open={pickerOpen}>
      <Popover.Trigger class="ui-select-trigger family-trigger" aria-label="Font family">
        <span class:missing>{family}</span>
        <ChevronDown size={14} strokeWidth={1.75} />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class="property-popover font-popover" side="left" align="start" sideOffset={12} customAnchor={anchor}>
          {#if catalog}
            <FontPicker value={family} {catalog} onClose={() => (pickerOpen = false)} onPick={(name) => { pickerOpen = false; void style({ family: name }); }} />
          {/if}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
    {#if missing}<p class="note">Missing font: drawn in {catalog?.default}</p>{/if}
  </div>

  <div class="pair">
    <Select value={String(weights.some((w) => w.value === String(weight)) ? weight : weights[0]?.value)} items={weights} onChange={(v) => style({ weight: Number(v) })} label="Font weight" />
    <div class="size">
      <PropertyField field={field("font", "size", "24")} {node} compact label="Font size" onSet={setField} onRefresh={refresh} {onError} />
      <DropdownMenu.Root>
        <DropdownMenu.Trigger class="size-more" aria-label="Font sizes"><ChevronDown size={14} strokeWidth={1.75} /></DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content class="ui-menu" side="bottom" align="end" sideOffset={4}>
            {#each SIZES as s}<DropdownMenu.Item class="ui-menu-item" onSelect={() => style({ size: s })}>{s}</DropdownMenu.Item>{/each}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </div>
  </div>

  <div class="styles">
    <div class="segmented toggles" role="group" aria-label="Text style">
      <button aria-label="Bold" title="Bold (⌘B)" aria-pressed={!rangeSelected && weight >= 600} class:on={!rangeSelected && weight >= 600} onclick={() => toggle("bold")}><Bold size={15} strokeWidth={2} /></button>
      <button aria-label="Italic" title="Italic (⌘I)" aria-pressed={!rangeSelected && italic} class:on={!rangeSelected && italic} onclick={() => toggle("italic")}><Italic size={15} strokeWidth={1.75} /></button>
      <button aria-label="Underline" title="Underline (⌘U)" aria-pressed={!rangeSelected && underline} class:on={!rangeSelected && underline} onclick={() => toggle("underline")}><Underline size={15} strokeWidth={1.75} /></button>
      <button aria-label="Strikethrough" title="Strikethrough (⌘⇧X)" aria-pressed={!rangeSelected && strike} class:on={!rangeSelected && strike} onclick={() => toggle("strike")}><Strikethrough size={15} strokeWidth={1.75} /></button>
    </div>
    <label class="swatch" title={rangeSelected ? "Colour of the selected characters" : "Text colour"}>
      <input type="color" aria-label={rangeSelected ? "Selection colour" : "Text colour"} value={fillColor.slice(0, 7)} onchange={(e) => style({ color: hexToRgba(e.currentTarget.value) })} />
    </label>
  </div>
  {#if rangeSelected}<p class="note">Styling the selected characters</p>{/if}

  <div class="pair">
    <PropertyField field={field("font", "line_height", "0")} {node} label="Line height" zeroLabel="Auto" onSet={setField} onRefresh={refresh} {onError}>
      {#snippet icon()}<Baseline size={14} strokeWidth={1.5} />{/snippet}
    </PropertyField>
    <PropertyField field={field("font", "letter_spacing", "0")} {node} label="Letter spacing" onSet={setField} onRefresh={refresh} {onError}>
      {#snippet icon()}<MoveHorizontal size={14} strokeWidth={1.5} />{/snippet}
    </PropertyField>
  </div>

  <div class="subheading">Alignment</div>
  <div class="aligns">
    <div class="segmented" role="radiogroup" aria-label="Horizontal alignment">
      {#each [{ v: "left", icon: AlignStart, label: "Align left" }, { v: "center", icon: AlignCenter, label: "Align centre" }, { v: "right", icon: AlignEnd, label: "Align right" }] as a (a.v)}
        <button role="radio" aria-checked={align === a.v} aria-label={a.label} title={a.label} class:on={align === a.v} onclick={() => set("font.align", a.v, "align text")}><a.icon size={16} strokeWidth={1.5} /></button>
      {/each}
    </div>
    <div class="segmented" role="radiogroup" aria-label="Vertical alignment">
      {#each [{ v: "top", icon: ArrowUpToLine, label: "Align top" }, { v: "middle", icon: AlignVerticalCenter, label: "Align middle" }, { v: "bottom", icon: ArrowDownToLine, label: "Align bottom" }] as a (a.v)}
        <button role="radio" aria-checked={valign === a.v} aria-label={a.label} title={sizing === "fixed" ? a.label : `${a.label} (fixed size boxes)`} class:on={valign === a.v} onclick={() => set("font.valign", a.v, "align text")}><a.icon size={16} strokeWidth={1.5} /></button>
      {/each}
    </div>
    <Popover.Root>
      <Popover.Trigger class="property-icon" aria-label="Type settings" title="Type settings"><SlidersHorizontal size={16} strokeWidth={1.5} /></Popover.Trigger>
      <Popover.Portal><Popover.Content class="property-popover" side="left" align="center" sideOffset={12} aria-label="Type settings">
        <header><strong>Type settings</strong><Popover.Close class="property-icon" aria-label="Close type settings"><X size={15} /></Popover.Close></header>
        <div class="property-popover-body">
          <div><div class="subheading">Resizing</div>
            <Select value={sizing} items={[{ value: "auto_width", label: "Auto width" }, { value: "auto_height", label: "Auto height" }, { value: "fixed", label: "Fixed size" }]} onChange={(v) => set("text.sizing", v, "resize text")} label="Resizing" />
          </div>
          <label class="check"><input type="checkbox" checked={italic} onchange={(e) => style({ italic: e.currentTarget.checked })} /> Italic</label>
          <label class="check"><input type="checkbox" checked={tabular} onchange={(e) => set("font.tabular", String(e.currentTarget.checked), "set figures")} /> Tabular figures <span class="hint">steady digits for readouts</span></label>
        </div>
      </Popover.Content></Popover.Portal>
    </Popover.Root>
  </div>

  <div class="subheading content-heading">
    <span>Content</span>
    {#if content.bind}
      <button class="property-icon" aria-label="Unbind content" title="Unbind: keep the current words" onclick={() => bind("")}><Unlink size={14} strokeWidth={1.5} /></button>
    {:else}
      <button class="property-icon" class:on={binding} aria-label="Bind content" title="Bind to an expression" onclick={() => { binding = !binding; expr = ""; }}><Link size={14} strokeWidth={1.5} /></button>
    {/if}
  </div>
  {#if content.bind}
    <div class="bound" title="Edit the binding to change it">
      <input class="mono" aria-label="Content binding" value={content.bind} onkeydown={onEnter} onchange={(e) => bind(e.currentTarget.value)} />
      <div class="reads">{content.text}</div>
    </div>
  {:else if binding}
    <input class="mono expr" aria-label="Content expression" placeholder={'"P = " + fmt(n12.slider.value, 2)'} bind:value={expr} onkeydown={(e) => { if (e.key === "Enter") bind(expr); if (e.key === "Escape") binding = false; }} />
    <p class="hint">An expression: <code>fmt(x, 2)</code>, <code>fmt(q, "kPa", 1)</code>, <code>sci(x, 3)</code>, joined with <code>+</code>.</p>
  {:else}
    <textarea aria-label="Text content" rows="2" value={content.text ?? ""} disabled={content.edit === "locked"} title={content.why ?? ""} onchange={(e) => set("text.content", e.currentTarget.value, "edit text")}></textarea>
  {/if}
</div>

<style>
  /* The inspector's section frame; SelectionView's own rules are scoped to it. */
  .text-section { display: flex; flex-direction: column; gap: 10px; padding: 16px; border-bottom: 1px solid var(--border); }
  .text-section .component { display: flex; justify-content: space-between; align-items: center; min-height: 22px; margin-bottom: 4px; font-size: 12px; font-weight: 600; }
  .family :global(.family-trigger) { width: 100%; }
  .missing { color: var(--danger, #c33); }
  .note { margin: 4px 0 0; font-size: 11px; color: var(--fg-muted); }
  .styles { display: grid; grid-template-columns: minmax(0, 1fr) 32px; gap: 8px; align-items: center; }
  .toggles button { color: var(--fg-muted); }
  .swatch { display: flex; height: 32px; border-radius: 6px; overflow: hidden; background: var(--bg-sunken); }
  .swatch input { width: 100%; height: 100%; border: none; padding: 4px; background: transparent; cursor: pointer; }
  .pair { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 8px; }
  .pair :global(.ui-select-trigger) { width: 100%; }
  .size { display: flex; align-items: center; height: 32px; background: var(--bg-sunken); border-radius: 5px; }
  .size :global(.field) { flex: 1; }
  .size :global(.size-more) { display: flex; align-items: center; justify-content: center; width: 28px; height: 100%; border: none; border-left: 1px solid var(--bg); background: transparent; color: var(--fg-muted); cursor: pointer; border-radius: 0 5px 5px 0; }
  .subheading { font-size: 11px; color: var(--fg-muted); margin-bottom: 6px; }
  .aligns { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 28px; gap: 8px; align-items: center; margin-top: -4px; }
  .segmented { display: flex; height: 32px; padding: 2px; background: var(--bg-sunken); border-radius: 6px; }
  .segmented button { flex: 1; display: flex; align-items: center; justify-content: center; border: none; border-radius: 4px; background: transparent; color: var(--fg-muted); cursor: pointer; }
  .segmented button.on { background: var(--bg); color: var(--fg); box-shadow: 0 0 0 1px var(--border-strong); }
  .content-heading { display: flex; align-items: center; justify-content: space-between; margin: 4px 0 -4px; }
  .content-heading :global(.on) { color: var(--accent); }
  textarea, .expr, .bound input { width: 100%; box-sizing: border-box; border: 1px solid var(--border-strong); border-radius: 5px; background: var(--bg); color: var(--fg); padding: 6px 8px; font: inherit; font-size: 12px; resize: vertical; }
  .mono { font-family: ui-monospace, monospace; font-size: 11px; }
  .bound input { color: var(--accent); }
  .reads { margin-top: 4px; font-size: 11px; color: var(--fg-muted); overflow-wrap: anywhere; }
  .hint { font-size: 10px; color: var(--fg-faint); margin: 0; }
  .check { display: flex; align-items: center; gap: 8px; font-size: 12px; }
  :global(.font-popover) { padding: 0 !important; }
</style>
