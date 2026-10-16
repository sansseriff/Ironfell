<script lang="ts">
  // The session's selection (source: selection): what is picked, Link and
  // Show in, and Link's merge question. The seed of the inspector
  // (plans/specs/panels-and-session.md).
  import { controllerManager } from "../../controller-manager.svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import PropertyField, { type Property } from "../../lib/ui/PropertyField.svelte";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
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
    if (previousNode !== node) { previousNode = node; inspected = null; setError = ""; }
    void refresh();
  });
  const titles: Record<string, string> = { transform2d: "Position", size: "Layout", radius: "Dimensions", appearance: "Appearance", fill: "Fill", gradient: "Gradient", stroke: "Stroke", corners: "Corner radius", arc: "Arc", shadow: "Drop shadow", group_clip: "Clip content" };
  const order = ["transform2d", "size", "radius", "appearance", "corners", "arc", "group_clip", "fill", "gradient", "stroke", "shadow"];
  const components = $derived.by(() => {
    const current = inspected as Inspected | null;
    return current ? [...current.components].sort((a, b) => (order.indexOf(a.kind) < 0 ? 99 : order.indexOf(a.kind)) - (order.indexOf(b.kind) < 0 ? 99 : order.indexOf(b.kind))) : [];
  });
  async function set(f: Field, text: string) {
    if (!inspected) return;
    const node = inspected.node;
    setError = "";
    try {
      const r = await doc.call("inspect_set", [node, f.leaf, text]);
      if (!r.ok) setError = `${f.leaf}: ${r.error}`;
      await refresh();
    } catch (e) { setError = String(e); }
  }
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
      {#each components as c (c.kind)}
        <div class="property-section">
          <div class="component" data-kind={c.kind}>
            <span>{titles[c.kind] ?? c.kind}</span>
            {#if inspected.removable.includes(c.kind)}<button class="icon" title="Remove {titles[c.kind] ?? c.kind}" aria-label="Remove {c.kind}" onclick={() => component(c.kind, false)}><X size={14} strokeWidth={1.5} /></button>{/if}
          </div>
          {#if c.kind === "gradient"}
            <div class="gradient-preview" style:background={`${c.fields.find(f => f.field === "kind")?.text === "radial" ? "radial-gradient(circle" : "linear-gradient(135deg"}, ${c.fields.find(f => f.field === "start")?.text ?? "#000000"}, ${c.fields.find(f => f.field === "end")?.text ?? "#ffffff"})`}></div>
          {/if}
          <div class="field-grid">
            {#each c.fields as f (f.leaf)}
              <PropertyField field={f} node={inspected.node} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />
            {/each}
          </div>
        </div>
      {/each}
      {#if inspected.addable.length}
        <div class="add">
          {#each inspected.addable as kind (kind)}
            <button title="Add {kind}" aria-label="Add {kind}" onclick={() => component(kind, true)}><Plus size={13} strokeWidth={1.75} /> {titles[kind] ?? kind}</button>
          {/each}
        </div>
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
  .property-section { padding: 13px 16px 16px; border-bottom: 1px solid var(--border); }
  .component { display: flex; justify-content: space-between; align-items: center; min-height: 22px; margin-bottom: 10px; font-size: 12px; font-weight: 600; }
  .icon { display: grid; place-items: center; background: transparent; color: var(--fg-muted); padding: 3px; border: none; border-radius: 4px; cursor: pointer; }
  .icon:hover { background: var(--hover); color: var(--fg); }
  .field-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px 8px; }
  .gradient-preview { height: 36px; margin-bottom: 12px; border-radius: 5px; border: 1px solid var(--border); }
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
