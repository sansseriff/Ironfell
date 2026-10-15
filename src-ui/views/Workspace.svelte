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
  /* The dock's colours come from the app's light/dark tokens (app.css).
     Canvas views show the canvas beneath: the theme's group background
     would cover it. HTML views paint their own page. */
  .dock :global(.dockview-theme-light) {
    color-scheme: inherit;
    --dv-group-view-background-color: transparent;
    --dv-separator-border: var(--border);
    --dv-tabs-and-actions-container-background-color: var(--bg-chrome);
    --dv-tabs-and-actions-container-height: 36px;
    --dv-activegroup-visiblepanel-tab-background-color: var(--bg);
    --dv-activegroup-hiddenpanel-tab-background-color: transparent;
    --dv-inactivegroup-visiblepanel-tab-background-color: var(--bg);
    --dv-inactivegroup-hiddenpanel-tab-background-color: transparent;
    --dv-tab-divider-color: transparent;
    --dv-activegroup-visiblepanel-tab-color: var(--fg);
    --dv-activegroup-hiddenpanel-tab-color: var(--fg-muted);
    --dv-inactivegroup-visiblepanel-tab-color: var(--fg-muted);
    --dv-inactivegroup-hiddenpanel-tab-color: var(--fg-faint);
    --dv-icon-hover-background-color: var(--hover);
    --dv-drag-over-background-color: var(--accent-soft);
    --dv-tabs-container-scrollbar-color: var(--border-strong);
  }

  /* Tabs, VS Code's current look: the active tab is outlined on its top
     and sides, and its outline curves out at the bottom into the tab bar's
     bottom line (the ::before/::after rings), joining it to the content. */
  .dock :global(.dv-tabs-and-actions-container) {
    box-shadow: inset 0 -1px var(--tab-outline);
  }
  .dock :global(.dv-tabs-container) {
    padding: 0 10px;
  }
  .dock :global(.dv-tab) {
    position: relative;
    padding: 0;
    margin-top: 5px;
    height: calc(100% - 5px);
    border: 1px solid transparent;
    border-bottom: none;
    border-radius: 8px 8px 0 0;
  }
  /* A hovered inactive tab takes a faint tint over its whole shape, down to
     the bar's line, as in VS Code; its box never changes. `!important`:
     dockview sets every inactive tab's background with a longer selector. */
  .dock :global(.dv-tab.dv-inactive-tab:hover) {
    background: var(--tab-hover) !important;
  }
  /* Faint dividers between unselected tabs; none beside the active tab or a
     hovered one. */
  .dock :global(.dv-tab.dv-inactive-tab + .dv-tab.dv-inactive-tab::before) {
    content: "";
    position: absolute;
    left: -1px;
    top: 25%;
    height: 50%;
    width: 1px;
    background: var(--tab-divider);
    pointer-events: none;
  }
  /* Only dividers: the active tab's ::before is its left fillet. */
  .dock :global(.dv-tab.dv-inactive-tab:hover + .dv-tab.dv-inactive-tab::before),
  .dock :global(.dv-tab.dv-inactive-tab + .dv-tab.dv-inactive-tab:hover::before) {
    display: none;
  }
  .dock :global(.dv-tab.dv-active-tab) {
    z-index: 1;
    border-color: var(--tab-outline);
  }
  .dock :global(.dv-tab.dv-active-tab::before),
  .dock :global(.dv-tab.dv-active-tab::after) {
    content: "";
    position: absolute;
    bottom: 0;
    width: 8px;
    height: 8px;
    pointer-events: none;
  }
  /* A quarter ring of the outline (radii 7–8px), filled with the content
     colour outside it. The 8px box ends on the tab's side border, so the
     ring's top meets that border's 1px column exactly and its fill covers
     the border below the curve; the ring's bottom meets the bar's line. */
  .dock :global(.dv-tab.dv-active-tab::before) {
    left: -8px;
    background: radial-gradient(circle at 0 0, transparent 6.6px, var(--tab-outline) 7.2px, var(--tab-outline) 7.8px, var(--bg) 8.4px);
  }
  .dock :global(.dv-tab.dv-active-tab::after) {
    right: -8px;
    background: radial-gradient(circle at 100% 0, transparent 6.6px, var(--tab-outline) 7.2px, var(--tab-outline) 7.8px, var(--bg) 8.4px);
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

  /* HTML views: a page in the theme's background, scrolling, inside their tab. */
  .dock :global(.html-view) {
    box-sizing: border-box;
    height: 100%;
    overflow: auto;
    padding: 12px;
    background: var(--bg);
    display: flex;
    flex-direction: column;
    gap: 8px;
    color: var(--fg);
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
    color: var(--danger);
    font-size: 12px;
    margin: 0;
  }
  .dock :global(.html-view p) {
    margin: 0;
  }
</style>
