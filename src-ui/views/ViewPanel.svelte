<script lang="ts">
  // One view in a dock tab: its canvas part (a BevyPanel placeholder whose
  // rectangle Rust draws into) and its HTML part, either or both. The canvas
  // part exists only while the tab is visible, so a hidden tab costs Rust
  // nothing; the HTML part stays mounted and keeps its state.
  import type { DockviewPanelApi } from "dockview-core";
  import { onDestroy } from "svelte";
  import BevyPanel from "../lib/BevyPanel.svelte";
  import { NOTATIONS, type ViewSpec } from "./views";

  let { id, spec, api }: { id: string; spec: ViewSpec; api: DockviewPanelApi } = $props();

  const def = $derived(NOTATIONS[spec.notation]);
  let visible = $state(api.isVisible);
  const sub = api.onDidVisibilityChange((e) => (visible = e.isVisible));
  onDestroy(() => sub.dispose());
</script>

<div class="view" class:hybrid={def.canvas && def.html}>
  {#if def.canvas && visible}
    <div class="canvas-part">
      <BevyPanel {id} kind={def.canvas} composition={spec.source} />
    </div>
  {/if}
  {#if def.html}
    {@const Html = def.html}
    <div class="html-part">
      <Html {spec} />
    </div>
  {/if}
  {#if !def.canvas && !def.html}
    <p class="unknown">No notation “{spec.notation}”.</p>
  {/if}
</div>

<style>
  .view {
    position: relative;
    width: 100%;
    height: 100%;
  }
  .canvas-part,
  .html-part {
    position: absolute;
    inset: 0;
  }
  /* Over a canvas part, HTML takes the pointer only where it draws. */
  .hybrid .html-part {
    pointer-events: none;
  }
  .unknown {
    padding: 8px;
    pointer-events: auto;
    background: var(--bg);
  }
</style>
