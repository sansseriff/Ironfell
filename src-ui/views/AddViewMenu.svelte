<script lang="ts">
  // "+" in a tab group's header: open a view here. Every composition in the
  // document, then the views that are not compositions.
  import type { DockviewApi, DockviewGroupPanel } from "dockview-core";
  import { controllerManager } from "../controller-manager.svelte";
  import { FIXED_VIEWS, titleOf, viewId, type ViewSpec } from "./views";

  let { api, group }: { api: DockviewApi; group: DockviewGroupPanel } = $props();

  let open = $state(false);
  let compositions: { id: string; name: string | null; dims: string }[] = $state([]);

  async function toggle() {
    open = !open;
    if (open) {
      try {
        compositions = await controllerManager.docClient.call("compositions", []);
      } catch {
        compositions = [];
      }
    }
  }

  function add(title: string, spec: ViewSpec) {
    open = false;
    const id = viewId(spec, (candidate) => !!api.getPanel(candidate));
    api.addPanel({ id, component: "view", title, params: { spec }, position: { referenceGroup: group } });
  }
</script>

<div class="add">
  <button class="plus" title="Open a view" onclick={toggle}>+</button>
  {#if open}
    <div class="menu" role="menu">
      <div class="heading">Compositions</div>
      {#each compositions as c (c.id)}
        <button role="menuitem" onclick={() => add(titleOf(c.name ?? c.id), { source: c.name ?? c.id, notation: "composition" })}>
          {titleOf(c.name ?? c.id)} <span class="dims">{c.dims}</span>
        </button>
      {/each}
      <div class="heading">Views</div>
      {#each FIXED_VIEWS as v (v.title)}
        <button role="menuitem" onclick={() => add(v.title, v.spec)}>{v.title}</button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .add {
    position: relative;
    height: 100%;
    display: flex;
    align-items: center;
    pointer-events: auto;
  }
  .plus {
    border: none;
    background: transparent;
    font-size: 16px;
    padding: 0 8px;
    cursor: pointer;
    color: #444;
  }
  .menu {
    position: absolute;
    right: 0;
    top: 100%;
    z-index: 100;
    min-width: 160px;
    background: white;
    border: 1px solid #ccd;
    border-radius: 6px;
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.15);
    padding: 4px;
    display: flex;
    flex-direction: column;
  }
  .menu button {
    text-align: left;
    border: none;
    background: transparent;
    padding: 4px 8px;
    border-radius: 4px;
    cursor: pointer;
    color: #222;
  }
  .menu button:hover {
    background: #eef1f7;
  }
  .heading {
    font-size: 10px;
    text-transform: uppercase;
    color: #889;
    padding: 4px 8px 2px;
  }
  .dims {
    color: #889;
    font-size: 10px;
  }
</style>
