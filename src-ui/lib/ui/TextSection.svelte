<script lang="ts">
  // A text node's inspector section, Figma's layout (plans/specs/text.md §8):
  // family; weight and size; line height and letter spacing; horizontal
  // and vertical alignment, with resizing and figures in the settings. And
  // Iron's own: the content, typed or bound to an expression, so a label
  // can be a live readout.
  import { Popover, DropdownMenu, Toggle } from "bits-ui";
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
  import Segmented from "./Segmented.svelte";
  import Checkbox from "./Checkbox.svelte";
  import IconButton from "./IconButton.svelte";
  import TextInput from "./TextInput.svelte";
  import FontPicker, { type FontCatalog } from "./FontPicker.svelte";
  import ColorPicker, { parseHex } from "./ColorPicker.svelte";
  import Section from "./Section.svelte";
  import PropertyField, { type Property } from "./PropertyField.svelte";
  import { controllerManager } from "../../controller-manager.svelte";

  let {
    node,
    nodes = [node],
    field,
    patch,
    refresh,
    onError,
  }: {
    node: string;
    nodes?: string[];
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
    doc.command({ op: 'fonts' }).then((c: FontCatalog) => (catalog = c)).catch(() => {});
  });

  const family = $derived(field("font", "family").mixed ? "Mixed" : field("font", "family", "Inter", "str").text ?? "Inter");
  const weight = $derived(field("font", "weight").mixed ? NaN : Number(field("font", "weight", "400").text ?? 400));
  const align = $derived(field("font", "align").mixed ? "" : field("font", "align", "left", "enum").text ?? "left");
  const valign = $derived(field("font", "valign").mixed ? "" : field("font", "valign", "top", "enum").text ?? "top");
  const sizing = $derived(field("text", "sizing").mixed ? "" : field("text", "sizing", "auto_width", "enum").text ?? "auto_width");
  const tabular = $derived(!field("font", "tabular").mixed && field("font", "tabular", "false", "bool").text === "true");
  const italic = $derived(!field("font", "italic").mixed && field("font", "italic", "false", "bool").text === "true");
  const content = $derived(field("text", "content", "", "str"));
  const underline = $derived(!field("font", "underline").mixed && field("font", "underline", "false", "bool").text === "true");
  const strike = $derived(!field("font", "strike").mixed && field("font", "strike", "false", "bool").text === "true");
  const fill = $derived(field("fill", "color", "#000000ff", "color"));

  // Characters selected in the canvas edit: styling goes to them alone
  // (plans/specs/text.md §2.1); otherwise to the whole node.
  const rangeSelected = $derived(nodes.length === 1 && doc.textEdit.active && doc.textEdit.node === node && doc.textEdit.anchor !== doc.textEdit.focus);
  async function style(patch: Record<string, unknown>) {
    onError("");
    try {
      const r = await doc.command({ op: 'text_style', node: node, patch: JSON.stringify({ ...patch, nodes }) });
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
  // The colour, of the selected characters or the whole text; a token
  // (`=@name`) binds the node's fill.
  function setColor(f: Property, text: string) {
    if (text.startsWith("=")) return set(f.leaf, text, "text colour");
    const rgba = parseHex(text);
    if (rgba) void style({ color: rgba });
    else onError(`"${text}" is not a hex colour`);
  }

  const known = $derived((catalog as FontCatalog | null)?.families.find((f) => f.name.toLowerCase() === family.toLowerCase()));
  const installed = $derived(!!(catalog as FontCatalog | null)?.system.some((n) => n.toLowerCase() === family.toLowerCase()));
  const missing = $derived(family !== "Mixed" && !!catalog && !known && !installed);
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
    const r = await doc.command({ op: 'inspect_patch', node, patch: JSON.stringify({ nodes, values: text ? { "text.content": `=${text}` } : {}, unbind: text ? [] : ["text.content"], label: "bind text content" }) });
    if (!r.ok) onError(r.error);
    else binding = false;
  }
</script>

<Section title="Text" class="property-section" headerClass="component" data-section="text">

  <div class="family" bind:this={anchor}>
    <Popover.Root bind:open={pickerOpen}>
      <Popover.Trigger class="ui-select-trigger" aria-label="Font family">
        <span class="ui-select-label" class:missing>{family}</span>
        <ChevronDown size={12} strokeWidth={1.75} />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class="ui-popover" side="left" align="start" sideOffset={12} customAnchor={anchor}>
          {#if catalog}
            <FontPicker value={family} {catalog} onClose={() => (pickerOpen = false)} onPick={(name) => { pickerOpen = false; void style({ family: name }); }} />
          {/if}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
    {#if missing}<p class="note">Missing font: drawn in {catalog?.default}</p>{/if}
  </div>

  <div class="pair">
    <Select placeholder="Mixed" value={Number.isNaN(weight) ? "" : String(weights.some((w) => w.value === String(weight)) ? weight : weights[0]?.value)} items={weights} onChange={(v) => style({ weight: Number(v) })} label="Font weight" />
    <div class="size">
      <PropertyField field={field("font", "size", "24")} {node} compact label="Font size" onSet={setField} onRefresh={refresh} {onError} />
      <DropdownMenu.Root>
        <DropdownMenu.Trigger class="ui-icon-button" aria-label="Font sizes" title="Font sizes"><ChevronDown size={14} strokeWidth={1.75} /></DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content class="ui-menu" side="bottom" align="end" sideOffset={4}>
            {#each SIZES as s}<DropdownMenu.Item class="ui-menu-item" onSelect={() => style({ size: s })}>{s}</DropdownMenu.Item>{/each}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </div>
  </div>

  <div class="styles">
    <!-- Four independent toggles in the segmented control's track. -->
    <div class="ui-segmented" role="group" aria-label="Text style">
      {#each [{ what: "bold", icon: Bold, label: "Bold", title: "Bold (⌘B)", on: weight >= 600 }, { what: "italic", icon: Italic, label: "Italic", title: "Italic (⌘I)", on: italic }, { what: "underline", icon: Underline, label: "Underline", title: "Underline (⌘U)", on: underline }, { what: "strike", icon: Strikethrough, label: "Strikethrough", title: "Strikethrough (⌘⇧X)", on: strike }] as const as t (t.what)}
        <Toggle.Root class="ui-segment" aria-label={t.label} title={field("font", t.what === "bold" ? "weight" : t.what).mixed ? `${t.title} · Mixed` : t.title} aria-pressed={field("font", t.what === "bold" ? "weight" : t.what).mixed ? "mixed" : undefined} pressed={!rangeSelected && t.on} onPressedChange={() => toggle(t.what)}><t.icon size={14} strokeWidth={1.75} /></Toggle.Root>
      {/each}
    </div>
    <div class="ui-field color" title={rangeSelected ? "Colour of the selected characters" : "Text colour"}>
      <ColorPicker swatch field={fill} {node} label={rangeSelected ? "Selection colour picker" : "Text colour picker"} heading={rangeSelected ? "Selection colour" : "Text colour"} scrub={!rangeSelected} tokens={!rangeSelected} onSet={setColor} onRefresh={refresh} {onError} />
      <input aria-label={rangeSelected ? "Selection colour" : "Text colour"} placeholder={(fill.mixedColor ?? fill.mixed) ? "Mixed" : undefined} value={(fill.mixedColor ?? fill.mixed) ? "" : (fill.text ?? "").slice(1, 7).toUpperCase()} spellcheck="false" onkeydown={onEnter} onchange={(e) => setColor(fill, e.currentTarget.value.startsWith("#") ? e.currentTarget.value : `#${e.currentTarget.value}`)} />
    </div>
  </div>
  {#if rangeSelected}<p class="note">Styling the selected characters</p>{/if}

  <div class="pair">
    <PropertyField field={field("font", "line_height", "0")} {node} compact label="Line height" zeroLabel="Auto" onSet={setField} onRefresh={refresh} {onError}>
      {#snippet icon()}<Baseline size={14} strokeWidth={1.75} />{/snippet}
    </PropertyField>
    <PropertyField field={field("font", "letter_spacing", "0")} {node} compact label="Letter spacing" onSet={setField} onRefresh={refresh} {onError}>
      {#snippet icon()}<MoveHorizontal size={14} strokeWidth={1.75} />{/snippet}
    </PropertyField>
  </div>

  <div class="ui-subheading">Alignment</div>
  <div class="aligns">
    <Segmented label="Horizontal alignment" iconOnly value={align} onChange={(v) => set("font.align", v, "align text")}
      items={[{ value: "left", icon: AlignStart, label: "Align left" }, { value: "center", icon: AlignCenter, label: "Align centre" }, { value: "right", icon: AlignEnd, label: "Align right" }]} />
    <Segmented label="Vertical alignment" iconOnly value={valign} onChange={(v) => set("font.valign", v, "align text")}
      items={[{ value: "top", icon: ArrowUpToLine, label: "Align top" }, { value: "middle", icon: AlignVerticalCenter, label: "Align middle" }, { value: "bottom", icon: ArrowDownToLine, label: "Align bottom" }].map((a) => ({ ...a, title: sizing === "fixed" ? a.label : `${a.label} (fixed size boxes)` }))} />
    <Popover.Root>
      <Popover.Trigger class="ui-icon-button" aria-label="Type settings" title="Type settings"><SlidersHorizontal size={14} strokeWidth={1.75} /></Popover.Trigger>
      <Popover.Portal><Popover.Content class="ui-popover" side="left" align="center" sideOffset={12} aria-label="Type settings">
        <header><strong>Type settings</strong><Popover.Close class="ui-icon-button" aria-label="Close type settings"><X size={14} strokeWidth={1.75} /></Popover.Close></header>
        <div class="ui-popover-body">
          <div class="group"><div class="ui-subheading">Resizing</div>
            <Select value={sizing} items={[{ value: "auto_width", label: "Auto width" }, { value: "auto_height", label: "Auto height" }, { value: "fixed", label: "Fixed size" }]} onChange={(v) => set("text.sizing", v, "resize text")} label="Resizing" />
          </div>
          <Checkbox label="Italic" checked={italic} onChange={(on) => style({ italic: on })} />
          <div class="check"><Checkbox label="Tabular figures" checked={tabular} onChange={(on) => set("font.tabular", String(on), "set figures")} /><span class="hint">steady digits for readouts</span></div>
        </div>
      </Popover.Content></Popover.Portal>
    </Popover.Root>
  </div>

  <div class="ui-subheading content-heading">
    <span>Content</span>
    {#if content.bind}
      <IconButton icon={Unlink} label="Unbind content" title="Unbind: keep the current words" onclick={() => bind("")} />
    {:else}
      <IconButton icon={Link} label="Bind content" title="Bind to an expression" pressed={binding} onPressedChange={(on) => { binding = on; expr = ""; }} />
    {/if}
  </div>
  {#if content.bind}
    <div class="bound" title="Edit the binding to change it">
      <input class="ui-input mono" aria-label="Content binding" value={content.bind} onkeydown={onEnter} onchange={(e) => bind(e.currentTarget.value)} />
      <div class="reads">{content.text}</div>
    </div>
  {:else if binding}
    <TextInput class="mono" aria-label="Content expression" placeholder={'"P = " + fmt(n12.slider.value, 2)'} bind:value={expr} onkeydown={(e: KeyboardEvent) => { if (e.key === "Enter") bind(expr); if (e.key === "Escape") binding = false; }} />
    <p class="hint">An expression: <code>fmt(x, 2)</code>, <code>fmt(q, "kPa", 1)</code>, <code>sci(x, 3)</code>, joined with <code>+</code>.</p>
  {:else}
    <textarea class="ui-input" aria-label="Text content" rows="2" placeholder={content.mixed ? "Mixed" : undefined} value={content.mixed ? "" : content.text ?? ""} disabled={content.edit === "locked"} title={content.why ?? ""} onchange={(e) => set("text.content", e.currentTarget.value, "edit text")}></textarea>
  {/if}
</Section>

<style>
  .missing { color: var(--danger); }
  .note { margin: 0; font-size: var(--text-sm); color: var(--fg-muted); }
  .family { display: flex; flex-direction: column; gap: 4px; }
  .styles { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 8px; align-items: center; }
  .color { overflow: hidden; }
  .color > :global(.color-swatch) { margin-left: 2px; }
  .color input { flex: 1; width: 100%; min-width: 0; height: calc(var(--control-h) - 2px); padding: 0 8px; border: none; outline: none; background: transparent; color: var(--fg); font: inherit; font-size: var(--text-md); font-variant-numeric: tabular-nums; }
  .pair { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 8px; }
  .size { display: flex; align-items: center; gap: 2px; min-width: 0; }
  .size :global(.field) { flex: 1; }
  .aligns { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 24px; gap: 8px; align-items: center; }
  .group { display: flex; flex-direction: column; gap: 4px; }
  .check { display: flex; align-items: center; gap: 8px; }
  .check > :global(.ui-check-row) { flex: none; }
  .content-heading { display: flex; align-items: center; justify-content: space-between; margin-right: -6px; }
  .mono, section :global(.ui-input.mono) { font-family: var(--font-mono); font-size: var(--text-sm); }
  .bound input { width: 100%; color: var(--accent); }
  .reads { margin-top: 4px; font-size: var(--text-sm); color: var(--fg-muted); overflow-wrap: anywhere; }
  .hint { margin: 0; font-size: var(--text-sm); color: var(--fg-muted); }
  textarea { width: 100%; }
</style>
