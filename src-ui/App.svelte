<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import SplitPane from "./lib/SplitPane5.svelte";
  import BevyPanel from "./lib/BevyPanel.svelte";
  import Controls from "./lib/Controls.svelte";
  import WebGPUWarning from "./lib/WebGPUWarning.svelte";
  import { controllerManager } from "./controller-manager.svelte";
  import { UIState } from "./ui-state.svelte";

  const ui_state = new UIState();

  let canvasEl: HTMLCanvasElement;

  // SplitPane drags don't resize the canvas — they only move panel rects.
  function handleSplitPaneResize() {
    controllerManager.syncAllPanels();
  }

  onMount(() => {
    controllerManager.boot(canvasEl);
  });

  onDestroy(() => {
    controllerManager.dispose();
  });
</script>

<!-- Layout: left HTML panel | center 3D viewer | right vello UI panel, timeline across the bottom -->

{#snippet controlsPane()}
  <div class="controls-pane">
    <Controls />
  </div>
{/snippet}

{#snippet viewer()}
  <BevyPanel id="viewer" kind="viewer" />
{/snippet}

{#snippet sidebar()}
  <BevyPanel id="sidebar" kind="ui" />
{/snippet}

{#snippet centerRight()}
  <SplitPane
    orientation="horizontal"
    min="40%"
    max="95%"
    pos="72%"
    --color="black"
    a={viewer}
    b={sidebar}
    onResize={handleSplitPaneResize}
  ></SplitPane>
{/snippet}

{#snippet top()}
  <SplitPane
    orientation="horizontal"
    min="5%"
    max="60%"
    pos="24%"
    --color="black"
    a={controlsPane}
    b={centerRight}
    onResize={handleSplitPaneResize}
  ></SplitPane>
{/snippet}

{#snippet timeline()}
  <BevyPanel id="timeline" kind="timeline" />
{/snippet}

<div class="stage">
  <!-- The one canvas: full window, never resized during pane drags -->
  <canvas
    bind:this={canvasEl}
    id="bevy-canvas"
    tabindex="0"
    oncontextmenu={(e) => {
      e.preventDefault();
    }}
  ></canvas>

  <!-- HTML UI layer floats above; only interactive elements re-enable pointer events -->
  <div class="ui-layer">
    <SplitPane
      orientation="vertical"
      min="1%"
      max="75%"
      pos="75%"
      --color="black"
      a={top}
      b={timeline}
      onResize={handleSplitPaneResize}
    ></SplitPane>
  </div>

  <WebGPUWarning
    show={controllerManager.showWebGPUWarning}
    onDismiss={() => controllerManager.dismissWebGPUWarning()}
  />

  {#if controllerManager.loadingInProgress}
    <div class="loading">
      <div class="spinner"></div>
      <p>Loading...</p>
    </div>
  {/if}
</div>

<style>
  .stage {
    position: fixed;
    inset: 0;
    overflow: hidden;
  }

  #bevy-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
    z-index: 0;
  }

  #bevy-canvas:focus {
    outline: none;
  }

  .ui-layer {
    position: absolute;
    inset: 0;
    z-index: 1;
    /* Input falls through to the canvas except where children opt back in */
    pointer-events: none;
    padding: 2.5px;
    /* SplitPane containers must not paint over the canvas */
    --split-pane-bg: transparent;
  }

  .controls-pane {
    /* Docked pane styled as a floating card over the canvas */
    pointer-events: auto;
    height: 100%;
    overflow: auto;
    margin: 6px;
    width: calc(100% - 12px);
    height: calc(100% - 12px);
    border-radius: 12px;
    background-color: var(--body-color);
    border: 1px solid var(--outer-border-color);
    box-shadow: 0 8px 24px rgba(16, 36, 94, 0.12);
  }

  .loading {
    position: absolute;
    inset: 0;
    z-index: 10;
    background-color: rgba(255, 255, 255, 0.6);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    font-family: Arial, sans-serif;
    font-size: 2em;
    color: #9aa4b8;
    pointer-events: none;
  }
</style>
