<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import Workspace from "./views/Workspace.svelte";
  import WebGL2Warning from "./lib/WebGL2Warning.svelte";
  import ConvertDialog from "./lib/ConvertDialog.svelte";
  import LinkPopover from "./lib/LinkPopover.svelte";
  import { controllerManager } from "./controller-manager.svelte";

  let canvasEl: HTMLCanvasElement;

  onMount(() => {
    controllerManager.boot(canvasEl);
  });

  onDestroy(() => {
    controllerManager.dispose();
  });
</script>

<!-- One canvas under a dock of views (src-ui/views): every tab or split is a
     view onto a composition, drawn by the canvas, by HTML, or both. -->

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
    <Workspace />
  </div>

  <ConvertDialog />
  <LinkPopover />

  <WebGL2Warning
    show={controllerManager.showBackendWarning}
    onDismiss={() => controllerManager.dismissBackendWarning()}
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

  }



  .loading {
    position: absolute;
    inset: 0;
    z-index: 10;
    background-color: var(--overlay);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    font-family: Arial, sans-serif;
    font-size: 2em;
    color: var(--fg-faint);
    pointer-events: none;
  }
</style>
