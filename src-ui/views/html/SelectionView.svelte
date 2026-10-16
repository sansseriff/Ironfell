<script lang="ts">
  // The session's selection (source: selection): what is picked, Link and
  // Show in, and Link's merge question. The seed of the inspector
  // (plans/specs/panels-and-session.md).
  import { controllerManager } from "../../controller-manager.svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import PropertyField, { type Property } from "../../lib/ui/PropertyField.svelte";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
  import { Popover, DropdownMenu } from "bits-ui";
  import PaintRow from "../../lib/ui/PaintRow.svelte";
  import AppearanceIcon from "../../lib/ui/AppearanceIcon.svelte";
  import Scan from "@lucide/svelte/icons/scan";
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import type { ViewSpec } from "../views";

  let { spec }: { spec: ViewSpec } = $props();
  void spec;

  const doc = controllerManager.docClient;

  // Linking and Show in (ECM §7.2). Link takes the two picked pictures
  // (shift-click adds the second); a merge question opens the dialog.
  let linkError = $state("");
  async function link(keep = "") {
    linkError = "";
    const pair = doc.question ? [doc.question.aNode, doc.question.bNode] : doc.selection;
    try {
      const r = await doc.link(pair[0], pair[1], keep);
      if (!r.ok && !r.question) linkError = r.error;
    } catch (e) {
      linkError = String(e);
    }
  }
  const showIn = $derived(doc.counterparts.figures.filter((f) => f.node !== doc.selection[0] && f.composition !== null));

  // The inspector (a first, HTML version; plans/specs/panels-and-session.md):
  // the first selected node's fields, each edited through its handle, so a
  // linearly bound value edits its source and a locked one says why.
  type Field = Property;
  type Inspected = { node: string; type: string; name: string | null; components: { kind: string; fields: Field[] }[]; addable: string[]; removable: string[] };
  let inspected: Inspected | null = $state(null);
  let setError = $state("");
  let generation = 0;
  let separateCorners = $state(false);
  let strokeAnchor: HTMLDivElement | undefined = $state();
  async function refresh() {
    const node = doc.selection[0];
    const request = ++generation;
    if (!node) { inspected = null; return; }
    try {
      const r = await doc.call("inspect", [node]);
      if (request === generation && doc.selection[0] === node) inspected = r;
    } catch (e) { if (request === generation) setError = String(e); }
  }
  let previousNode: string | undefined;
  $effect(() => {
    void doc.version;
    const node = doc.selection[0];
    if (previousNode !== node) { previousNode = node; separateCorners = false; inspected = null; setError = ""; }
    void refresh();
  });
  const titles: Record<string, string> = { transform2d: "Position", size: "Layout", radius: "Dimensions", appearance: "Appearance", fill: "Fill", gradient: "Gradient", stroke: "Stroke", corners: "Corner radius", arc: "Arc", shadow: "Drop shadow", group_clip: "Clip content" };
  const order = ["transform2d", "size", "radius", "appearance", "corners", "arc", "group_clip", "fill", "gradient", "stroke", "shadow"];
  const components = $derived.by(() => {
    const current = inspected as Inspected | null;
    return current ? [...current.components].sort((a, b) => (order.indexOf(a.kind) < 0 ? 99 : order.indexOf(a.kind)) - (order.indexOf(b.kind) < 0 ? 99 : order.indexOf(b.kind))) : [];
  });
  function fields(kind: string): Field[] { return (inspected as Inspected | null)?.components.find(c => c.kind === kind)?.fields ?? []; }
  function supports(kind: string): boolean { return !!inspected && (inspected.addable.includes(kind) || inspected.components.some(c => c.kind === kind)); }
  function field(kind: string, name: string, text = "0", valueKind = "number"): Field {
    return fields(kind).find(f => f.field === name) ?? { leaf: `${kind}.${name}`, field: name, text, kind: valueKind, bind: null, edit: "direct", via: null, why: null };
  }
  const cornerLeaves = ["corners.tl", "corners.tr", "corners.br", "corners.bl"];
  const cornerField = $derived.by(() => {
    const corners = ["tl", "tr", "br", "bl"].map(k => field("corners", k));
    const locked = corners.find(f => f.edit === "locked");
    return { ...corners[0], edit: locked ? "locked" : corners[0].edit, why: locked?.why ?? null, bind: corners.find(f => f.bind)?.bind ?? null };
  });
  const mixedCorners = $derived(["tr", "br", "bl"].some(k => field("corners", k).text !== field("corners", "tl").text));
  const otherComponents = $derived(components.filter(c => !["transform2d", "size", "radius", "appearance", "corners", "fill", "gradient", "stroke", "shadow"].includes(c.kind)));
  // Effects have their own list/presenter. Drop shadow is the currently
  // supported effect; future renderable effect components get entries here.
  const effectTypes = [{ kind: "shadow", label: "Drop shadow" }];
  const effects = $derived(effectTypes.filter(e => fields(e.kind).length));
  async function patch(values: Record<string, string>, remove: string[] = [], label = "edit appearance") {
    if (!inspected) return;
    const node = inspected.node;
    setError = "";
    try {
      const r = await doc.call("inspect_patch", [node, JSON.stringify({ values, remove, label })]);
      if (!r.ok) setError = r.error;
      await refresh();
    } catch (e) { setError = String(e); }
  }
  function set(f: Field, text: string) { return patch({ [f.leaf]: text }, [], `set ${f.leaf}`); }
  function setCorners(_f: Field, text: string) { return patch(Object.fromEntries(cornerLeaves.map(k => [k, text])), [], "round corners"); }
  function paintMode(mode: string) { return mode === "solid" ? patch({}, ["gradient"], "solid fill") : patch({ "gradient.kind": mode }, [], `${mode} fill`); }
  // Optional components (fill, stroke, corners, arc, shadow): one
  // transaction each, "add shadow" or "remove corners".
  async function component(kind: string, add: boolean) {
    if (!inspected) return;
    setError = "";
    try {
      const r = await doc.call("component", [inspected.node, kind, add]);
      if (!r.ok) setError = `${kind}: ${r.error}`;
      await refresh();
    } catch (e) { setError = String(e); }
  }
</script>

<section class="html-view properties">
  <header class="panel-heading"><span>Design</span><SlidersHorizontal size={15} strokeWidth={1.5} /></header>

  <div class="row">
    <button onclick={() => link()} disabled={doc.selection.length !== 2}>Link</button>
    {#each showIn as f (f.node)}
      <button onclick={() => doc.call("focus", [f.node])}>Show in {f.name ?? f.composition} ▸</button>
    {/each}
  </div>
  {#if linkError}<p class="error">{linkError}</p>{/if}
  {#if doc.question}
    <div class="question" role="dialog" aria-label="Merge entities">
      <p>Both pictures already stand for an entity. Which one should remain?</p>
      {#each [doc.question.a, doc.question.b] as e (e.entity)}
        <div class="candidate">
          <strong>{e.name ?? e.entity}</strong> <span class="mono">{e.entity}</span> · {e.def ?? "no definition"}
          <div class="mono">params: {e.params.length ? e.params.map((p: [string, string]) => `${p[0]}=${p[1]}`).join(", ") : "none"}</div>
          <div class="mono">figures: {e.figures.join(" ")}{e.ports.length ? ` · ports: ${e.ports.join(", ")}` : ""}</div>
          <button onclick={() => link(e.entity)}>Keep {e.name ?? e.entity}</button>
        </div>
      {/each}
      {#if doc.question.port_collisions.length}<p class="error">Ports on both: {doc.question.port_collisions.join(", ")}</p>{/if}
      <button onclick={() => (doc.question = null)}>Cancel</button>
    </div>
  {/if}

  {#if inspected}
    <div class="inspector">
      <div class="title"><strong>{inspected.name ?? inspected.type}</strong> <span class="mono">{inspected.type} {inspected.node}</span></div>
      {#if setError}<p class="error">{setError}</p>{/if}
      {#if doc.selection.length > 1}<p class="selection-note">{doc.selection.length} selected · editing the first item</p>{/if}
      {#if fields("transform2d").length}
        <div class="property-section" data-section="position">
          <div class="component"><span>Position</span></div>
          <div class="field-grid">
            {#each ["x", "y"] as axis}<PropertyField field={field("transform2d", axis)} node={inspected.node} compact prefix={axis.toUpperCase()} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />{/each}
            <PropertyField field={field("transform2d", "rot")} node={inspected.node} label="Rotation" onSet={set} onRefresh={refresh} onError={(e) => setError = e} />
          </div>
          <details class="advanced"><summary>Scale</summary><div class="field-grid">
            {#each ["sx", "sy"] as axis}<PropertyField field={field("transform2d", axis, "1")} node={inspected.node} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />{/each}
          </div></details>
        </div>
      {/if}
      {#if fields("size").length || fields("radius").length}
        <div class="property-section" data-section="layout">
          <div class="component"><span>Layout</span></div>
          <div class="subheading">{fields("size").length ? "Dimensions" : "Radii"}</div>
          <div class="field-grid">
            {#each (fields("size").length ? fields("size") : fields("radius")) as f (f.leaf)}
              <PropertyField field={f} node={inspected.node} compact prefix={f.field.toUpperCase()} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />
            {/each}
          </div>
        </div>
      {/if}
      {#if supports("appearance") || supports("corners")}
        <div class="property-section" data-section="appearance">
          <div class="component"><span>Appearance</span></div>
          <div class="field-grid appearance-grid" class:has-corners={supports("corners")}>
            {#if supports("appearance")}<PropertyField field={field("appearance", "opacity", "1")} node={inspected.node} inlineUnit onSet={set} onRefresh={refresh} onError={(e) => setError = e}>
              {#snippet icon()}<AppearanceIcon kind="opacity" />{/snippet}
            </PropertyField>{/if}
            {#if supports("corners")}
              <PropertyField field={cornerField} node={inspected.node} label="Corner radius" mixed={mixedCorners} scrubLeaves={cornerLeaves} onSet={setCorners} onRefresh={refresh} onError={(e) => setError = e}>
                {#snippet icon()}<AppearanceIcon kind="radius" />{/snippet}
              </PropertyField>
              <button class="property-icon corner-toggle" class:active={separateCorners} aria-label="Individual corners" aria-pressed={separateCorners} title="Individual corners" onclick={() => separateCorners = !separateCorners}><Scan size={16} strokeWidth={1.5} /></button>
            {/if}
          </div>
          {#if separateCorners}
            <div class="field-grid individual-corners">
              {#each [{ key: "tl", label: "Top left" }, { key: "tr", label: "Top right" }, { key: "bl", label: "Bottom left" }, { key: "br", label: "Bottom right" }] as const as corner}
                <PropertyField field={field("corners", corner.key)} node={inspected.node} compact label={corner.label} onSet={set} onRefresh={refresh} onError={(e) => setError = e}>
                  {#snippet icon()}<AppearanceIcon kind={corner.key} />{/snippet}
                </PropertyField>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
      {#if supports("fill") || fields("gradient").length}
        <div class="property-section" data-section="fill">
          <div class="component"><span>Fill</span>{#if !fields("fill").length && !fields("gradient").length}<button class="property-icon" aria-label="Add fill" title="Add fill" onclick={() => component("fill", true)}><Plus size={17} strokeWidth={1.5} /></button>{/if}</div>
          {#if fields("fill").length || fields("gradient").length}
            <PaintRow node={inspected.node} label="Fill" color={field("fill", "color", "#808080ff", "color")} opacity={field("fill", "opacity", "1")} gradient={fields("gradient")} onSet={set} onRefresh={refresh} onError={(e) => setError = e} onMode={paintMode} onRemove={() => patch({}, ["fill", "gradient"], "remove fill")} />
          {:else}<p class="section-empty">No fill</p>{/if}
        </div>
      {/if}
      {#if supports("stroke")}
        <div class="property-section" data-section="stroke">
          <div class="component"><span>Stroke</span>{#if !fields("stroke").length}<button class="property-icon" aria-label="Add stroke" title="Add stroke" onclick={() => component("stroke", true)}><Plus size={17} strokeWidth={1.5} /></button>{/if}</div>
          {#if fields("stroke").length}
            <PaintRow node={inspected.node} label="Stroke" color={field("stroke", "color", "#000000ff", "color")} onSet={set} onRefresh={refresh} onError={(e) => setError = e} onRemove={() => component("stroke", false)} />
            <div class="stroke-options" bind:this={strokeAnchor}>
              <div><div class="subheading">Position</div><div class="readout" title="Strokes currently render centred on the outline">Centre</div></div>
              <PropertyField field={field("stroke", "width", "1")} node={inspected.node} label="Weight" onSet={set} onRefresh={refresh} onError={(e) => setError = e} />
              <Popover.Root>
                <Popover.Trigger class="property-icon stroke-settings" aria-label="Stroke settings" title="Stroke settings"><SlidersHorizontal size={16} strokeWidth={1.5} /></Popover.Trigger>
                <Popover.Portal><Popover.Content class="property-popover" side="left" align="center" sideOffset={12} customAnchor={strokeAnchor} aria-label="Stroke settings">
                  <header><strong>Stroke settings</strong><Popover.Close class="property-icon" aria-label="Close stroke settings"><X size={15} /></Popover.Close></header>
                  <div class="property-popover-body">
                    <div class="subheading">Basic</div>
                    <div class="property-grid">
                      {#each fields("stroke").filter(f => ["join", "cap", "miter", "dash", "dash_offset"].includes(f.field)) as f (f.leaf)}
                        <PropertyField field={f} node={inspected.node} label={{ join: "Join", cap: "Cap", miter: "Miter limit", dash: "Dash pattern", dash_offset: "Dash offset" }[f.field]} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />
                      {/each}
                    </div>
                  </div>
                </Popover.Content></Popover.Portal>
              </Popover.Root>
            </div>
          {:else}<p class="section-empty">No stroke</p>{/if}
        </div>
      {/if}
      {#if supports("shadow")}
        <div class="property-section" data-section="effects">
          <div class="component"><span>Effects</span>
            <DropdownMenu.Root>
              <DropdownMenu.Trigger class="property-icon" aria-label="Add effect" title="Add effect"><Plus size={17} strokeWidth={1.5} /></DropdownMenu.Trigger>
              <DropdownMenu.Portal><DropdownMenu.Content class="ui-menu" side="left" align="start" sideOffset={10}>
                {#each effectTypes as effect}<DropdownMenu.Item class="ui-menu-item" data-effect-type={effect.kind} disabled={fields(effect.kind).length > 0} onSelect={() => component(effect.kind, true)}>{effect.label}</DropdownMenu.Item>{/each}
              </DropdownMenu.Content></DropdownMenu.Portal>
            </DropdownMenu.Root>
          </div>
          {#each effects as effect (effect.kind)}
            <div class="effect-row">
              <Popover.Root>
                <Popover.Trigger class="effect-trigger" aria-label="{effect.label} settings"><Square size={16} strokeWidth={1.5} /><span>{effect.label}</span><SlidersHorizontal size={14} strokeWidth={1.5} /></Popover.Trigger>
                <Popover.Portal><Popover.Content class="property-popover" side="left" align="start" sideOffset={12} aria-label="{effect.label} settings">
                  <header><strong>{effect.label}</strong><Popover.Close class="property-icon" aria-label="Close effect settings"><X size={15} /></Popover.Close></header>
                  <div class="property-popover-body">
                    <PaintRow node={inspected.node} label="Shadow" color={field(effect.kind, "color", "#00000040", "color")} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />
                    <div class="property-grid">
                      {#each fields(effect.kind).filter(f => f.kind === "number") as f (f.leaf)}<PropertyField field={f} node={inspected.node} label={{ dx: "X", dy: "Y", blur: "Blur" }[f.field]} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />{/each}
                    </div>
                  </div>
                </Popover.Content></Popover.Portal>
              </Popover.Root>
              <button class="property-icon" aria-label="Remove {effect.kind}" title="Remove {effect.label.toLowerCase()}" onclick={() => component(effect.kind, false)}><Minus size={16} strokeWidth={1.5} /></button>
            </div>
          {:else}<p class="section-empty">No effects</p>{/each}
        </div>
      {/if}
      {#each otherComponents as c (c.kind)}
        <div class="property-section">
          <div class="component" data-kind={c.kind}><span>{titles[c.kind] ?? c.kind}</span>{#if inspected.removable.includes(c.kind)}<button class="property-icon" aria-label="Remove {c.kind}" onclick={() => component(c.kind, false)}><Minus size={16} /></button>{/if}</div>
          <div class="field-grid">{#each c.fields as f (f.leaf)}<PropertyField field={f} node={inspected.node} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />{/each}</div>
        </div>
      {/each}
      {#if inspected.addable.some(k => ["arc", "group_clip"].includes(k))}
        <div class="add">{#each inspected.addable.filter(k => ["arc", "group_clip"].includes(k)) as kind}<button aria-label="Add {kind}" onclick={() => component(kind, true)}><Plus size={13} />{titles[kind] ?? kind}</button>{/each}</div>
      {/if}
    </div>
  {:else}
    <div class="empty"><MousePointer2 size={28} strokeWidth={1.25} /><strong>Make a selection</strong><span>Select an object to see its properties.</span></div>
  {/if}
  <footer>Alt + drag a number to adjust <span>⇧ Precision</span></footer>
</section>

<style>
  .properties { padding: 0 !important; font-size: 12px; }
  .panel-heading { display: flex; justify-content: space-between; align-items: center; padding: 14px 16px; border-bottom: 1px solid var(--border); font-weight: 600; }
  .panel-heading span { border-bottom: 2px solid var(--accent); padding-bottom: 4px; }
  .row { padding: 8px 16px; display: flex; flex-wrap: wrap; gap: 4px; }
  .row:has(button:only-child:disabled) { display: none; }
  .inspector { display: flex; flex-direction: column; }
  .title { display: flex; flex-wrap: wrap; align-items: center; gap: 7px; padding: 14px 16px; border-bottom: 1px solid var(--border); }
  .title strong { font-size: 13px; flex: 1; }
  .mono { font-family: monospace; font-size: 10px; color: var(--fg-faint); }
  .property-section { padding: 16px; border-bottom: 1px solid var(--border); }
  .component { display: flex; justify-content: space-between; align-items: center; min-height: 22px; margin-bottom: 14px; font-size: 12px; font-weight: 600; }
  .field-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px 8px; }
  .subheading { font-size: 11px; color: var(--fg-muted); margin-bottom: 6px; }
  .section-empty { font-size: 11px; color: var(--fg-faint); margin: 0; }
  .appearance-grid { gap: 12px 10px; }
  .appearance-grid.has-corners { grid-template-columns: repeat(2, minmax(0, 1fr)) 28px; }
  .corner-toggle { align-self: end; width: 28px; height: 32px; background: var(--bg-sunken); border-radius: 5px; }
  .corner-toggle.active { background: var(--accent-soft); color: var(--accent); }
  .individual-corners { margin-top: 10px; padding-right: 38px; gap: 10px; }
  .advanced { margin-top: 10px; }
  .advanced summary { color: var(--fg-muted); font-size: 11px; cursor: pointer; }
  .advanced[open] summary { margin-bottom: 8px; }
  .stroke-options { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 24px; gap: 8px; align-items: end; margin-top: 12px; }
  .stroke-options :global(.stroke-settings) { margin-bottom: 4px; }
  .readout { min-height: 32px; display: flex; align-items: center; padding: 0 9px; border: 1px solid var(--border-strong); border-radius: 5px; font-size: 12px; box-sizing: border-box; }
  .effect-row { display: flex; align-items: center; gap: 6px; }
  .effect-row :global(.effect-trigger) { display: flex; align-items: center; gap: 8px; min-height: 33px; flex: 1; min-width: 0; padding: 6px 8px; border: 1px solid var(--border-strong); border-radius: 5px; background: var(--bg); color: var(--fg); font-size: 12px; cursor: pointer; }
  .effect-row :global(.effect-trigger span) { flex: 1; text-align: left; }
  .effect-row :global(.effect-trigger:hover) { border-color: var(--fg-faint); }
  .add { display: flex; flex-wrap: wrap; gap: 6px; padding: 16px; }
  .add button { display: inline-flex; gap: 4px; align-items: center; font-size: 11px; padding: 5px 7px; background: transparent; }
  .empty { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 10px; padding: 40px 20px; color: var(--fg-faint); }
  .empty strong { font-size: 13px; color: var(--fg-muted); }
  .selection-note { padding: 0 16px; color: var(--fg-muted); font-size: 11px; }
  footer { display: flex; justify-content: space-between; padding: 12px 16px; font-size: 10px; color: var(--fg-faint); }
  .question { border: 1px solid var(--danger); padding: 10px; margin: 8px; border-radius: 6px; }
  .candidate { margin: 8px 0; }
  .error { padding: 0 16px; overflow-wrap: anywhere; }
</style>
