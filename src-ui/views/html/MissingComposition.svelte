<script lang="ts">
  // A composition view whose composition this document does not have: a tab
  // from another scope's layout, or one whose composition was deleted. It
  // says so and offers to make it (when the view names it) or to close the
  // tab, instead of a canvas whose tools would draw previews that commit
  // nowhere (pen.rs `drop_target` has no container).
  import type { DockviewPanelApi } from "dockview-core";
  import SquareDashed from "@lucide/svelte/icons/square-dashed";
  import Button from "../../lib/ui/Button.svelte";
  import EmptyState from "../../lib/ui/EmptyState.svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  import type { ViewSpec } from "../views";

  let { spec, api }: { spec: ViewSpec; api: DockviewPanelApi } = $props();

  const doc = controllerManager.docClient;
  // An id names one composition, gone for good; a name can be made again.
  const named = $derived(!spec.source.startsWith("#"));
  let error = $state("");
  let busy = $state(false);

  async function create() {
    busy = true;
    error = "";
    try {
      const r = await doc.command<{ ok: boolean; error?: string }>({ op: "author", verb: "new_composition", args: JSON.stringify({ name: spec.source }) });
      if (!r.ok) error = r.error ?? "The composition could not be made.";
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="missing canvas-ui">
  <EmptyState icon={SquareDashed} title={named ? `No composition “${spec.source}” in this scope` : "This composition is gone"}>
    {named ? "This tab is from another layout. Make the composition here, or close the tab." : "It was deleted. Undo brings it back; or close the tab."}
    {#snippet actions()}
      {#if named}
        <Button variant="primary" disabled={busy} onclick={create}>Create “{spec.source}”</Button>
      {/if}
      <Button variant="outline" onclick={() => api.close()}>Close tab</Button>
    {/snippet}
  </EmptyState>
  {#if error}<p class="error">{error}</p>{/if}
</div>

<style>
  .missing {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    background: var(--bg);
    pointer-events: auto;
  }
  .missing :global(.ui-empty) {
    max-width: 400px;
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: var(--text-md);
  }
</style>
