<script lang="ts">
  // The tools, in the tab bar of every group showing a composition view:
  // select (V) and pen (P). The engine reports which is active.
  import type { DockviewGroupPanel } from "dockview-core";
  import { onDestroy } from "svelte";
  import { controllerManager } from "../controller-manager.svelte";

  let { group }: { group: DockviewGroupPanel } = $props();

  const doc = controllerManager.docClient;
  const composition = () => (group.activePanel?.params as any)?.spec?.notation === "composition";
  let shown = $state(composition());
  const sub = group.api.onDidActivePanelChange(() => (shown = composition()));
  onDestroy(() => sub.dispose());

  const tools = [
    { id: "select", key: "V", label: "Select", icon: "M4 2 L4 15 L7.5 11.5 L10 17 L12 16 L9.5 10.5 L14 10.5 Z" },
    { id: "pen", key: "P", label: "Pen", icon: "M10 2 L15 9 L12 16 L8 16 L5 9 Z M10 2 L10 9 M10 9 m-1.3 0 a1.3 1.3 0 1 0 2.6 0 a1.3 1.3 0 1 0 -2.6 0" },
  ] as const;
</script>

{#if shown}
  <div class="tools" role="toolbar" aria-label="Tools">
    {#each tools as t (t.id)}
      <button class:active={doc.tool === t.id} title="{t.label} ({t.key})" aria-label={t.label} onclick={() => doc.setTool(t.id)}>
        <svg viewBox="0 0 20 20" width="15" height="15"><path d={t.icon} fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" /></svg>
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
    color: #444;
    cursor: pointer;
    display: flex;
    pointer-events: auto;
  }
  button:hover {
    background: #e4e7ee;
  }
  button.active {
    background: #0d99ff;
    color: white;
  }
</style>
