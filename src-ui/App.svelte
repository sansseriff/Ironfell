<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { WorkerController } from "./control.svelte";
  import SplitPane from "./lib/SplitPane5.svelte";

  let show_loading = $state(false);
  let controller: WorkerController;
  let loading_in_progress = $state(false);
  let canvasContainer: HTMLDivElement;
  let canvasElement: HTMLCanvasElement;
  let isInitialized = $state(false);

  function showAlert() {
    alert("Check console for error");
  }

  async function launch() {
    if (loading_in_progress) return;

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
      showAlert();
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

    console.log("Updating canvas size from container:", width, "×", height);

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
    resizeTimer = setTimeout(updateCanvasSize, 250) as unknown as number;
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
    setTimeout(updateCanvasSize, 0);
  }
</script>

{#snippet a()}
  <section style="background: white">
    <!---->
    <!-- <KonvaTest></KonvaTest> -->

    Writing Area
  </section>
{/snippet}

{#snippet b()}
  <div id="app-container">
    {#if show_loading}
      <div id="loading">
        <div class="spinner"></div>
        <p>Loading...</p>
      </div>
    {/if}

    <div bind:this={canvasContainer} id="container">
      <canvas id="worker-canvas" raw-window-handle="1" tabindex="0"></canvas>
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
    /* border: #ff5a5a 2px solid; */
    width: 100%;
    height: 100%;
    display: block; /* Ensure it behaves as a block */
  }

  #app-container {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex; /* Use flexbox for better control */
    flex-direction: column;
  }

  #launch-button {
    position: absolute;
    top: 20px;
    left: 20px;
    z-index: 10;
    padding: 10px 20px;
    background-color: #ff5a5a;
    color: white;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-weight: bold;
  }

  #launch-button:hover {
    background-color: #ff7a7a;
  }

  #launch-button:disabled {
    background-color: #aaa;
    cursor: not-allowed;
  }

  #loading {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background-color: rgba(255, 148, 148, 0.8);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
  }

  #container {
    position: relative;
    width: 100%;
    height: 100%;
    flex: 1; /* Take up all available space */
    display: flex; /* Use flexbox to control the canvas */
  }

  canvas:focus {
    outline: none; /* Optional: remove default focus outline if desired, or style it */
  }
</style>
