<script lang="ts">
  // "+" in a tab group's header: open a view here. Every composition in the
  // document, then the views that are not compositions, then the render
  // benchmark's stress scene at a few sizes (src/document_bridge/stress.rs).
  import type { DockviewApi, DockviewGroupPanel } from "dockview-core";
  import { controllerManager } from "../controller-manager.svelte";
  import { FIXED_VIEWS, SETTINGS_VIEW, titleOf, viewId, type ViewSpec } from "./views";

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

  /** The gear: show the one Settings view, or open it here. */
  function settings() {
    open = false;
    const existing = api.panels.find((p) => (p.params as any)?.spec?.notation === "settings");
    if (existing) existing.api.setActive();
    else add("Settings", SETTINGS_VIEW);
  }

  const STRESS_SIZES = [1000, 2000, 5000, 10000, 20000];

  /** Build (or rebuild at a new size) the `stress` composition, then show
   *  its tab: the one already open, or a new one here. */
  async function stress(shapes: number) {
    open = false;
    await controllerManager.docClient.call("stress", [shapes]);
    const existing = api.getPanel("stress");
    if (existing) {
      existing.api.setActive();
    } else {
      api.addPanel({ id: "stress", component: "view", title: "Stress", params: { spec: { source: "stress", notation: "composition" } }, position: { referenceGroup: group } });
    }
  }
</script>

<div class="add">
  <button class="plus" title="Settings" aria-label="Settings" onclick={settings}>
    <svg viewBox="0 0 20 20" width="14" height="14"><path d="M10 6.5a3.5 3.5 0 1 0 0 7a3.5 3.5 0 1 0 0-7Z M8.6 2h2.8l.4 2.2 1.6.9 2.1-.8 1.4 2.4-1.7 1.5v1.6l1.7 1.5-1.4 2.4-2.1-.8-1.6.9-.4 2.2H8.6l-.4-2.2-1.6-.9-2.1.8-1.4-2.4 1.7-1.5V9.2L3.1 7.7l1.4-2.4 2.1.8 1.6-.9Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" /></svg>
  </button>
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
      <div class="heading">Stress test</div>
      {#each STRESS_SIZES as n (n)}
        <button role="menuitem" onclick={() => stress(n)}>{n.toLocaleString()} shapes</button>
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
