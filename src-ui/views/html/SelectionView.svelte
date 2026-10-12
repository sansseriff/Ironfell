<script lang="ts">
  // The session's selection (source: selection): what is picked, Link and
  // Show in, and Link's merge question. The seed of the inspector
  // (plans/panels-and-session.md).
  import { controllerManager } from "../../controller-manager.svelte";
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
</section>

<style>
  .picked,
  .mono {
    font-family: monospace;
    font-size: 0.85em;
  }
  .question {
    border: 1px solid #c99;
    border-radius: 6px;
    padding: 6px;
    font-size: 0.85em;
  }
  .candidate {
    margin: 4px 0;
  }
</style>
