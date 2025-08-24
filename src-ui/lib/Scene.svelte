<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { controllerManager } from "../controller-manager.svelte";
  import WebGPUWarning from "./WebGPUWarning.svelte";

  // Props
  let {
    onResize,
    canvasId = "worker-canvas",
  }: {
    onResize?: () => void;
    canvasId?: string;
  } = $props();

  let canvasContainer: HTMLDivElement;
  let canvasElement: HTMLCanvasElement;
  let windowId: string | null = null;

  function dismissWebGPUWarning() {
    controllerManager.dismissWebGPUWarning();
  }

  async function launch() {
    console.log(`LAUNCHING canvas: ${canvasId}`);

    canvasElement = document.getElementById(canvasId) as HTMLCanvasElement;
    if (!canvasElement) {
      console.error(`Canvas element with id "${canvasId}" not found`);
      return;
    }

    // Register canvas with controller manager
    try {
      const isPrimary = canvasId === "viewer-canvas";
      console.log(`Registering canvas: ${canvasId} (primary: ${isPrimary})`);
      await controllerManager.registerCanvas(canvasElement, isPrimary);
      
      // Update canvas size after registration
      // Wait a bit for initialization to potentially complete
      setTimeout(() => {
        if (controllerManager.isInitialized) {
          updateCanvasSize();
        }
      }, 100);
    } catch (error) {
      console.error(`Failed to register canvas ${canvasId}:`, error);
      return;
    }
  }

  // Function to handle canvas size updates
  function updateCanvasSize() {
    if (!canvasContainer || !controllerManager.isInitialized) return;

    const rect = canvasContainer.getBoundingClientRect();
    const width = rect.width;
    const height = rect.height;

    if (width > 0 && height > 0) {
      resize({ width, height });
    }
  }

  // Debounce timer variable
  let resizeTimer: number | null = null;

  // Handler for window resize events with proper debouncing
  function handleWindowResize() {
    // Clear previous timeout if it exists
    // @ts-ignore
    if (resizeTimer !== null) {
      // @ts-ignore
      clearTimeout(resizeTimer);
    }

    resizeTimer = setTimeout(() => {
      // Normal canvas resize
      updateCanvasSize();
    }, 0);
  }

  // Handler for SplitPane resize events
  function handleSplitPaneResize() {
    // Push execution to the end of the event queue
    setTimeout(() => {
      // Normal canvas resize
      updateCanvasSize();
      if (onResize) onResize();
    }, 0);
  }

  onMount(() => {
    launch();
    // Add window resize listener
    window.addEventListener("resize", handleWindowResize);
  });

  onDestroy(() => {
    window.removeEventListener("resize", handleWindowResize);

    // Remove canvas from controller manager when destroyed
    controllerManager.removeCanvas(canvasId);
  });

  function resize({
    width = 0,
    height = 0,
  }: {
    width?: number;
    height?: number;
  }) {
    if (controllerManager.controller && controllerManager.isInitialized) {
      controllerManager.controller.requestCanvasResize(canvasId, width, height);
    }
  }

  // Export function to trigger resize from parent
  export function triggerResize() {
    handleSplitPaneResize();
  }
</script>

<div id="app-container">
  <WebGPUWarning
    show={controllerManager.showWebGPUWarning}
    onDismiss={dismissWebGPUWarning}
  />

  {#if controllerManager.loadingInProgress}
    <div id="loading">
      <div class="spinner"></div>
      <p>Loading...</p>
    </div>
  {/if}

  <div bind:this={canvasContainer} id="container">
    <canvas
      id={canvasId}
      tabindex="0"
      oncontextmenu={(e) => {
        e.preventDefault(); /* suppress default save image menu */
      }}
    ></canvas>
  </div>
</div>

<style>
  canvas {
    width: 100%;
    height: 100%;
    display: block;
  }

  #app-container {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  #loading {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background-color: rgba(255, 255, 255, 0.8);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    font-family: Arial, sans-serif;
    font-size: 3em;
    color: #d9d8e7;
  }

  #container {
    position: relative;
    width: 100%;
    height: 100%;
    flex: 1;
    display: flex;
  }

  canvas:focus {
    outline: none;
  }
</style>
