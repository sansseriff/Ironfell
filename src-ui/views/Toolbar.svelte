<script lang="ts">
  // The tools, in the tab bar of every group showing a composition view:
  // select (V) and pen (P). The engine reports which is active.
  import type { DockviewGroupPanel } from "dockview-core";
  import { onDestroy } from "svelte";
  import { controllerManager } from "../controller-manager.svelte";
  import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
  import PenTool from "@lucide/svelte/icons/pen-tool";

  let { group }: { group: DockviewGroupPanel } = $props();

  const doc = controllerManager.docClient;
  const composition = () => (group.activePanel?.params as any)?.spec?.notation === "composition";
  let shown = $state(composition());
  const sub = group.api.onDidActivePanelChange(() => (shown = composition()));
  onDestroy(() => sub.dispose());

  const tools = [
    { id: "select", key: "V", label: "Select", icon: MousePointer2 },
    { id: "pen", key: "P", label: "Pen", icon: PenTool },
  ] as const;
</script>

{#if shown}
  <div class="tools" role="toolbar" aria-label="Tools">
    {#each tools as t (t.id)}
      <button class:active={doc.tool === t.id} title="{t.label} ({t.key})" aria-label={t.label} onclick={() => doc.setTool(t.id)}>
        <t.icon size={15} strokeWidth={1.75} />
      </button>
    {/each}
  </div>
{/if}

<style>
  .tools {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 100%;
    padding: 0 6px;
    pointer-events: auto;
  }
  button {
    border: none;
    background: transparent;
    border-radius: 4px;
    padding: 3px 4px;
    color: var(--fg-muted);
    cursor: pointer;
    display: flex;
    pointer-events: auto;
  }
  button:hover {
    background: var(--hover);
    color: var(--fg);
  }
  button.active {
    background: var(--accent);
    color: var(--accent-fg);
  }
</style>
