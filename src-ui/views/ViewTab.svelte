<script lang="ts">
  // One tab in the dock, VS Code's way: the view's icon, its title, and a
  // close button shown on hover and on the active tab. Middle-click closes.
  // The rounded shape is the dock's CSS (Workspace.svelte).
  import { onDestroy, type Component } from "svelte";
  import type { DockviewPanelApi } from "dockview-core";
  import Shapes from "@lucide/svelte/icons/shapes";
  import ChartGantt from "@lucide/svelte/icons/chart-gantt";
  import FileCode from "@lucide/svelte/icons/file-code";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Bot from "@lucide/svelte/icons/bot";
  import Settings from "@lucide/svelte/icons/settings";
  import X from "@lucide/svelte/icons/x";
  import type { Notation } from "./views";

  let { api, notation }: { api: DockviewPanelApi; notation: Notation | undefined } = $props();

  const ICONS: Record<Notation, Component> = {
    composition: Shapes,
    timeline: ChartGantt,
    markup: FileCode,
    selection: SlidersHorizontal,
    conversation: Bot,
    settings: Settings,
  };
  const Icon = $derived(notation ? ICONS[notation] : Shapes);

  let title = $state(api.title ?? "");
  const sub = api.onDidTitleChange((e) => (title = e.title));
  onDestroy(() => sub.dispose());
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="view-tab"
  onauxclick={(e) => {
    if (e.button === 1) {
      e.preventDefault();
      api.close();
    }
  }}
>
  <Icon size={14} strokeWidth={1.75} class="icon" />
  <span class="title">{title}</span>
  <button
    class="close"
    title="Close"
    aria-label="Close {title}"
    onpointerdown={(e) => e.stopPropagation()}
    onclick={(e) => {
      e.stopPropagation();
      api.close();
    }}
  >
    <X size={13} strokeWidth={1.75} />
  </button>
</div>

<style>
  .view-tab {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 100%;
    padding: 0 6px 0 12px;
    white-space: nowrap;
  }
  .view-tab :global(.icon) {
    flex-shrink: 0;
    opacity: 0.85;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    margin-left: 2px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    visibility: hidden;
  }
  .close:hover {
    background: var(--hover);
    color: var(--fg);
  }
  :global(.dv-tab.dv-active-tab) .close,
  .view-tab:hover .close {
    visibility: visible;
  }
</style>
