<script lang="ts">
  // Ask the node (`EN-14`, `Store::inspect`; plans/specs/patches.md §5): where
  // the selected node sits, what it derives from, why each leaf has its value
  // (the layers it is read through, the last winning), what drives it and
  // what it drives. Read only while open, so a closed section costs nothing.
  import Section from "../../lib/ui/Section.svelte";
  import Disclosure from "../../lib/ui/Disclosure.svelte";
  import EmptyState from "../../lib/ui/EmptyState.svelte";
  import { controllerManager } from "../../controller-manager.svelte";

  type NodeRef = { id: string; type: string; name: string | null };
  type Layer = { kind: string; from: string | null; value: string };
  type Why = { leaf: string; value: string | null; layers: Layer[] };
  type Link = { kind: string; node: string; leaf: string | null; detail: string | null };
  type Inspection = { node: NodeRef; path: NodeRef[]; derives: string[]; why: Why[]; driven_by: Link[]; drives: Link[] };

  let { node }: { node: string } = $props();
  const doc = controllerManager.docClient;
  let open = $state(false);
  let inspection: Inspection | null = $state(null);
  let error = $state("");

  $effect(() => {
    void doc.generation;
    const id = node;
    inspection = null; error = "";
    if (!open) return;
    return doc.subscribe<Inspection | null>({ op: "connections", node: id }, (r) => { inspection = r; }, (e) => { error = String(e); });
  });

  const pathText = (p: NodeRef[]) => p.map((r) => r.name ?? `${r.type} ${r.id}`).join(" › ");
  const linkText = (l: Link) => [l.leaf, l.detail].filter(Boolean).join(" ← ");
</script>

<Section title="Connections" class="property-section" headerClass="component" data-section="connections">
  <Disclosure label={open ? "Hide" : "Why it looks and moves as it does"} bind:open>
    {#if error}<p class="note">{error}</p>
    {:else if !inspection}<p class="note">Reading…</p>
    {:else}
      <div class="connections">
        {#if inspection.path.length}<div class="row"><span class="label">In</span><span class="text">{pathText(inspection.path)}</span></div>{/if}
        {#each inspection.derives as d (d)}<div class="row"><span class="label">From</span><span class="text">{d}</span></div>{/each}
        {#each inspection.why as w (w.leaf)}
          <div class="why">
            <div class="row"><span class="mono">{w.leaf}</span><span class="value">{w.value ?? "?"}</span></div>
            <ol class="layers">
              {#each w.layers as l, i (i)}
                <li class:wins={i === w.layers.length - 1}><span class="kind">{l.kind}{l.from ? ` ${l.from}` : ""}</span><span class="value">{l.value}</span></li>
              {/each}
            </ol>
          </div>
        {/each}
        {#each [["Driven by", inspection.driven_by], ["Drives", inspection.drives]] as [title, links] (title)}
          {#if (links as Link[]).length}
            <div class="group-title">{title}</div>
            {#each links as Link[] as l (`${l.kind}:${l.node}:${l.leaf}:${l.detail}`)}
              <div class="row"><span class="label">{l.kind}</span><span class="mono">{l.node}</span><span class="text">{linkText(l)}</span></div>
            {/each}
          {/if}
        {/each}
        {#if !inspection.why.length && !inspection.driven_by.length && !inspection.drives.length}<EmptyState inline>Nothing drives it or reads it</EmptyState>{/if}
      </div>
    {/if}
  </Disclosure>
</Section>

<style>
  .connections { display: flex; flex-direction: column; gap: 4px; padding-top: 6px; font-size: var(--text-sm); }
  .row { display: flex; align-items: baseline; gap: 8px; min-height: 20px; }
  .label { flex: none; min-width: 56px; color: var(--fg-muted); }
  .text { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .mono { font-family: var(--font-mono); font-size: var(--text-xs); color: var(--fg-faint); }
  .value { margin-left: auto; font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 60%; }
  .why { padding: 2px 0; }
  .layers { margin: 0; padding: 0 0 0 12px; list-style: none; border-left: 1px solid var(--border); }
  .layers li { display: flex; gap: 8px; color: var(--fg-muted); }
  .layers li.wins { color: var(--fg); }
  .layers li.wins .kind { color: var(--accent); }
  .group-title { margin-top: 6px; color: var(--fg-muted); font-weight: 600; }
  .note { margin: 6px 0 0; color: var(--fg-muted); }
</style>
