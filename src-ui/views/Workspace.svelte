<script lang="ts">
  // The dock: every tab and split is a view (views.ts). The dock floats over
  // the one canvas; its chrome (tabs, sashes, HTML views) takes the pointer
  // and everything else falls through to the canvas, except while a tab is
  // being dragged, when the drop targets over canvas views must see it.
  import { onMount, onDestroy } from "svelte";
  import type { DockviewApi } from "dockview-core";
  import { createWorkspace } from "./workspace";
  import { controllerManager } from "../controller-manager.svelte";

  let el: HTMLDivElement;
  let api: DockviewApi | null = null;
  let dragging = $state(false);

  // A tab or group drag: drop targets over canvas views must see the
  // pointer until it is released (after dockview handles the drop).
  const stop = () => setTimeout(() => (dragging = false), 0);

  onMount(() => {
    api = createWorkspace(el);
    // Splits and tab moves change rectangles without resizing the window.
    api.onDidLayoutChange(() => controllerManager.syncAllPanels());
    api.onWillDragPanel(() => (dragging = true));
    api.onWillDragGroup(() => (dragging = true));
    document.addEventListener("pointerup", stop, true);
    document.addEventListener("pointercancel", stop, true);
  });

  onDestroy(() => {
    document.removeEventListener("pointerup", stop, true);
    document.removeEventListener("pointercancel", stop, true);
    api?.dispose();
  });
</script>

<div bind:this={el} class="dock" class:dragging></div>

<style>
  .dock {
    width: 100%;
    height: 100%;
  }
  /* Canvas views show the canvas beneath: the theme's group background
     would cover it. HTML views paint their own page. */
  .dock :global(.dockview-theme-light) {
    --dv-group-view-background-color: transparent;
    --dv-separator-border: #d6d8de;
  }
  .dock :global(*) {
    pointer-events: none;
  }
  .dock :global(.dv-tabs-and-actions-container),
  .dock :global(.dv-tabs-and-actions-container *),
  .dock :global(.dv-sash),
  .dock :global(.html-view),
  .dock :global(.html-view *),
  .dock :global(.dv-tabs-overflow-dropdown-root),
  .dock :global(.dv-tabs-overflow-dropdown-root *),
  .dock.dragging :global(*) {
    pointer-events: auto;
  }

  /* HTML views: a white page, scrolling, inside their tab. */
  .dock :global(.html-view) {
    box-sizing: border-box;
    height: 100%;
    overflow: auto;
    padding: 12px;
    background: var(--body-color, white);
    display: flex;
    flex-direction: column;
    gap: 8px;
    color: black;
    font-size: 13px;
  }
  .dock :global(.html-view .row) {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .dock :global(.html-view .small),
  .dock :global(.html-view .hint) {
    font-size: 12px;
    margin: 0;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .dock :global(.html-view .error) {
    color: #b3261e;
    font-size: 12px;
    margin: 0;
  }
  .dock :global(.html-view p) {
    margin: 0;
  }
</style>
