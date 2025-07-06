<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { WorkerController } from "./control.svelte";
  import SplitPane from "./lib/SplitPane5.svelte";
  import WebGPUWarning from "./lib/WebGPUWarning.svelte";

  let show_loading = $state(false);
  let controller: WorkerController;
  let loading_in_progress = $state(false);
  let canvasContainer: HTMLDivElement;
  let canvasElement: HTMLCanvasElement;
  let isInitialized = $state(false);
  let webgpuSupported = $state(true);
  let showWebGPUWarning = $state(false);

  function showAlert() {
    alert("Check console for error");
  }

  // Check for WebGPU support
  function checkWebGPUSupport(): boolean {
    if (!(navigator as any).gpu) {
      return false;
    }
    return true;
  }

  function dismissWebGPUWarning() {
    showWebGPUWarning = false;
  }

  async function launch() {
    if (loading_in_progress) return;

    // Check WebGPU support first
    if (!checkWebGPUSupport()) {
      webgpuSupported = false;
      showWebGPUWarning = true;
      return;
    }

    try {
      show_loading = true;
      loading_in_progress = true;

      canvasElement = document.getElementById(
        "worker-canvas"
      ) as HTMLCanvasElement;
      controller = new WorkerController(canvasElement);

      // Initialize the controller (checks WebGPU support, waits for worker, transfers canvas)
      await controller.initialize();
      isInitialized = true;

      // Update canvas size after initialization
      updateCanvasSize();

      // Hide loading screen when everything is ready
      show_loading = false;
    } catch (error) {
      console.error(error);
      webgpuSupported = false;
      showWebGPUWarning = true;
      show_loading = false;
    } finally {
      loading_in_progress = false;
    }
  }

  // Function to handle canvas size updates
  function updateCanvasSize() {
    if (!canvasContainer || !isInitialized) return;

    const rect = canvasContainer.getBoundingClientRect();
    const width = rect.width;
    const height = rect.height;

    // console.log("Updating canvas size from container:", width, "×", height);

    if (width > 0 && height > 0) {
      resize({ width, height });
    }
  }

  // Debounce timer variable
  let resizeTimer: number | null = null;

  // Handler for window resize events with proper debouncing
  function handleWindowResize() {
    // Clear previous timeout if it exists
    if (resizeTimer !== null) {
      clearTimeout(resizeTimer);
    }

    // Set new timeout (250ms is a common debounce delay for resize events)
    resizeTimer = setTimeout(() => {
      // Normal canvas resize
      updateCanvasSize();
    }, 250) as unknown as number;
  }

  // No longer auto-launching on mount
  onMount(() => {
    launch();
    window.addEventListener("keydown", controller?.handleKeyDown);
    window.addEventListener("keyup", controller?.handleKeyUp);
    // Add window resize listener
    window.addEventListener("resize", handleWindowResize);
  });

  onDestroy(() => {
    window.removeEventListener("keydown", controller?.handleKeyDown);
    window.removeEventListener("keyup", controller?.handleKeyUp);
    // Remove window resize listener
    window.removeEventListener("resize", handleWindowResize);
    // Optional: Clean up controller if necessary
    // if (controller) {
    //   controller.dispose(); // Assuming a dispose method if needed
    // }
  });

  function resize({ width = 0, height = 0 }) {
    if (controller && isInitialized) {
      controller.requestCanvasResize(width, height);
    }
  }

  // Handler for SplitPane resize events
  function handleSplitPaneResize() {
    // Push execution to the end of the event queue
    setTimeout(() => {
      // Normal canvas resize
      updateCanvasSize();
    }, 0);
  }

  // Inspector test functions
  function testSpawnEntity() {
    if (controller && isInitialized) {
      console.log("Testing spawn entity...");
      controller.inspectorSpawnEntity("123345");
    }
  }

  function testToggleVisibility() {
    if (controller && isInitialized) {
      // Test with a sample entity ID - you would normally get this from the scene
      const entityId = "4294967296"; // Example entity ID as string
      console.log(`Testing toggle visibility for entity ${entityId}...`);
      controller.inspectorToggleVisibility(entityId);
    }
  }

  function testDespawnEntity() {
    if (controller && isInitialized) {
      // Test with a sample entity ID - you would normally get this from the scene
      const entityId = "4294967296"; // Example entity ID as string
      console.log(`Testing despawn entity ${entityId}...`);
      controller.inspectorDespawnEntity(entityId);
    }
  }
</script>

{#snippet a()}
  <section style="background: white; padding: 20px;">
    <h3>Inspector Controls</h3>

    {#if showWebGPUWarning}
      <div
        style="background: #fff3cd; border: 1px solid #ffeaa7; border-radius: 8px; padding: 15px; margin: 10px 0;"
      >
        <p style="margin: 0; color: #856404;">
          <strong>⚠️ WebGPU Not Supported</strong><br />
          This application requires WebGPU support. Please check the main panel for
          instructions.
        </p>
      </div>
    {:else if !isInitialized}
      <p>Launch the app to use inspector controls</p>
      <button onclick={launch} disabled={loading_in_progress}>
        {loading_in_progress ? "Loading..." : "Launch App"}
      </button>
    {:else}
      <div
        style="display: flex; flex-direction: column; gap: 10px; max-width: 200px;"
      >
        <button onclick={testSpawnEntity}>Spawn Entity</button>
        <button onclick={testToggleVisibility}>Toggle Visibility</button>
        <button onclick={testDespawnEntity}>Despawn Entity</button>
      </div>

      <div style="margin-top: 20px;">
        <h4>Instructions:</h4>
        <ul style="font-size: 12px; margin: 5px 0;">
          <li>Spawn Entity: Creates a new empty entity</li>
          <li>Toggle Visibility: Shows/hides an entity (uses example ID)</li>
          <li>Despawn Entity: Removes an entity (uses example ID)</li>
          <li>Check browser console for results</li>
        </ul>
      </div>
    {/if}
  </section>
{/snippet}

{#snippet b()}
  <div id="app-container">
    <WebGPUWarning show={showWebGPUWarning} onDismiss={dismissWebGPUWarning} />

    {#if show_loading}
      <div id="loading">
        <div class="spinner"></div>
        <p>Loading...</p>
      </div>
    {/if}

    <div bind:this={canvasContainer} id="container">
      <canvas id="worker-canvas" tabindex="0"></canvas>
    </div>
  </div>
{/snippet}

<SplitPane
  orientation="horizontal"
  min="25%"
  max="75%"
  pos="52%"
  --color="black"
  {a}
  {b}
  onResize={handleSplitPaneResize}
></SplitPane>

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
