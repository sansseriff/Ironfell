<script lang="ts">
  // The view selector strip (plans/specs/panel-primitives.md §5), Blender's
  // property tabs as data: a column of icons at a view's left edge, one per
  // view `std:workspace` offers (`workspace.strip`, viewStrip). Click shows
  // that view here in this group; Shift-click opens it in a split below, so
  // two show at once (Layers and the agent, or Layers and a lesson). The lit
  // icon is this group's view; a dot marks one showing elsewhere.
  import type { DockviewPanelApi } from "dockview-core";
  import { onDestroy } from "svelte";
  import { viewStrip } from "./view_strip.svelte";
  import { dockApi, showStripView } from "./workspace";
  import type { ViewSpec } from "./views";

  let { api, spec }: { api: DockviewPanelApi; spec: ViewSpec } = $props();

  // Re-read where views are when the dock changes.
  let tick = $state(0);
  const dock = dockApi();
  const subs = dock ? [dock.onDidLayoutChange(() => tick++), dock.onDidActivePanelChange(() => tick++)] : [];
  onDestroy(() => subs.forEach((s) => s.dispose()));

  function where(item: { spec: ViewSpec }): "here" | "elsewhere" | null {
    void tick;
    const p = dockApi()?.panels.find((x) => {
      const s = (x.params as { spec?: ViewSpec } | undefined)?.spec;
      return s?.notation === item.spec.notation && s.source === item.spec.source;
    });
    if (!p || !p.api.isVisible) return null;
    return p.group.id === api.group.id ? "here" : "elsewhere";
  }
</script>

<nav class="view-strip canvas-ui" aria-label="Views" data-view-strip>
  {#each viewStrip.items as item (item.key)}
    {@const at = where(item)}
    {@const Icon = item.icon}
    <button class="item" class:here={at === "here"} class:elsewhere={at === "elsewhere"} aria-pressed={at === "here"} aria-label={item.label}
      title="{item.label}{at === 'elsewhere' ? ' (showing)' : ''} · Shift-click: open below"
      onclick={(e) => showStripView(api, item.spec, item.id, item.label, e.shiftKey)}>
      <Icon size={17} strokeWidth={1.6} />
    </button>
  {/each}
</nav>

<style>
  .view-strip { position: absolute; left: 0; top: 0; bottom: 0; width: 40px; display: flex; flex-direction: column; align-items: center; gap: 2px; padding-top: 6px; background: var(--bg-chrome); border-right: 1px solid var(--border); pointer-events: auto; z-index: 2; box-sizing: border-box; }
  .item { position: relative; display: grid; place-items: center; width: 32px; height: 32px; border: 0; border-radius: 7px; background: transparent; color: var(--fg-muted); cursor: pointer; }
  .item:hover { background: var(--hover); color: var(--fg); }
  .item:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .item.here { background: var(--accent-soft); color: var(--fg); }
  .item.here::before { content: ""; position: absolute; left: -4px; top: 7px; bottom: 7px; width: 3px; border-radius: 2px; background: var(--accent); }
  .item.elsewhere::after { content: ""; position: absolute; right: 4px; bottom: 4px; width: 5px; height: 5px; border-radius: 50%; background: var(--accent); }
</style>
