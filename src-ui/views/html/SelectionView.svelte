<script lang="ts">
  // The session's selection (source: selection): what is picked, Link and
  // Show in, and Link's merge question. The seed of the inspector
  // (plans/specs/panels-and-session.md).
  import { controllerManager } from "../../controller-manager.svelte";
  import Plus from "@lucide/svelte/icons/plus";
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
  type Field = { leaf: string; field: string; kind: string | null; text: string | null; bind: string | null; edit: string; via: string | null; why: string | null };
  type Inspected = { node: string; type: string; name: string | null; components: { kind: string; fields: Field[] }[]; addable: string[]; removable: string[] };
  let inspected: Inspected | null = $state(null);
  let setError = $state("");
  $effect(() => {
    const node = doc.selection[0];
    void doc.version;
    if (!node) {
      inspected = null;
      return;
    }
    doc.call("inspect", [node]).then((r) => (inspected = r)).catch(() => (inspected = null));
  });
  async function set(f: Field, text: string) {
    if (!inspected) return;
    setError = "";
    const r = await doc.call("inspect_set", [inspected.node, f.leaf, text]);
    if (!r.ok) setError = `${f.leaf}: ${r.error}`;
    inspected = await doc.call("inspect", [inspected.node]);
  }
  // Optional components (fill, stroke, corners, arc, shadow): one
  // transaction each, "add shadow" or "remove corners".
  async function component(kind: string, add: boolean) {
    if (!inspected) return;
    setError = "";
    const r = await doc.call("component", [inspected.node, kind, add]);
    if (!r.ok) setError = `${kind}: ${r.error}`;
    inspected = await doc.call("inspect", [inspected.node]);
  }
</script>

<section class="html-view">
  <div class="picked">{doc.selection.length ? doc.selection.join(" ") : "nothing selected"}</div>
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
      {#each inspected.components as c (c.kind)}
        <!-- The heading's text is exactly the kind: drivers find fields by it. -->
        <div class="component"><span>{c.kind}</span>{#if inspected.removable.includes(c.kind)}<button class="icon" title="Remove {c.kind}" aria-label="Remove {c.kind}" onclick={() => component(c.kind, false)}><X size={12} strokeWidth={2} /></button>{/if}</div>
        {#each c.fields as f (f.leaf)}
          <label class="field" title={f.why ?? (f.via ? `edits ${f.via} through the binding` : "")}>
            <span class="label" class:bound={f.bind}>{f.field}</span>
            {#if f.kind === "bool"}
              <input type="checkbox" checked={f.text === "true"} disabled={f.edit === "locked"} onchange={(e) => set(f, String((e.currentTarget as HTMLInputElement).checked))} />
            {:else if f.kind === "color"}
              <input type="color" value={(f.text ?? "#000000").slice(0, 7)} disabled={f.edit === "locked"} onchange={(e) => set(f, (e.currentTarget as HTMLInputElement).value + (f.text ?? "").slice(7, 9))} />
            {:else}
              <input
                class:number={f.kind === "number"}
                type={f.kind === "number" ? "number" : "text"}
                step="any"
                value={f.text ?? ""}
                disabled={f.edit === "locked"}
                onchange={(e) => set(f, (e.currentTarget as HTMLInputElement).value)}
              />
            {/if}
          </label>
          {#if f.bind}<div class="bind mono">= {f.bind}</div>{/if}
        {/each}
      {/each}
      {#if inspected.addable.length}
        <div class="add">
          {#each inspected.addable as kind (kind)}
            <button title="Add {kind}" aria-label="Add {kind}" onclick={() => component(kind, true)}><Plus size={12} strokeWidth={2} /> {kind}</button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</section>

<style>
  .picked,
  .mono {
    font-family: monospace;
    font-size: 0.85em;
  }
  .question {
    border: 1px solid var(--danger);
    border-radius: 6px;
    padding: 6px;
    font-size: 0.85em;
  }
  .candidate {
    margin: 4px 0;
  }
  .inspector {
    display: flex;
    flex-direction: column;
    gap: 2px;
    border-top: 1px solid var(--border);
    padding-top: 6px;
  }
  .title {
    margin-bottom: 4px;
  }
  .component {
    font-size: 10px;
    text-transform: uppercase;
    color: var(--fg-faint);
    margin-top: 6px;
  }
  .component {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .component .icon {
    display: flex;
    padding: 1px;
    border: none;
    background: transparent;
    color: var(--fg-faint);
    cursor: pointer;
  }
  .component .icon:hover {
    color: var(--fg);
  }
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 8px;
  }
  .add button {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 11px;
  }
  .field {
    display: grid;
    grid-template-columns: 4.5em 1fr;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }
  .field input {
    min-width: 0;
    font-size: 12px;
  }
  .label.bound {
    color: var(--accent);
  }
  .bind {
    margin-left: 4.5em;
    color: var(--accent);
    overflow-wrap: anywhere;
  }
</style>
