<script lang="ts">
  import { onMount } from "svelte";
  import { WorkerController } from "./control.svelte";

  let show_loading = $state(false);
  let controller: WorkerController;
  let loading_in_progress = $state(false);

  function showAlert() {
    alert("Check console for error");
  }

  async function launch() {
    if (loading_in_progress) return;

    try {
      show_loading = true;
      loading_in_progress = true;

      const canvas = document.getElementById(
        "worker-canvas"
      ) as HTMLCanvasElement;
      controller = new WorkerController(canvas);

      // Initialize the controller (checks WebGPU support, waits for worker, transfers canvas)
      await controller.initialize();

      // Hide loading screen when everything is ready
      show_loading = false;
    } catch (error) {
      console.error(error);
      showAlert();
    } finally {
      loading_in_progress = false;
    }
  }

  // No longer auto-launching on mount
  // onMount(() => {
  //   launch();
  // });
</script>

<div id="app-container">
  <button id="launch-button" onclick={launch} disabled={loading_in_progress}>
    {loading_in_progress ? "Starting..." : "Launch Controller"}
  </button>

  {#if show_loading}
    <div id="loading">
      <div class="spinner"></div>
      <p>Loading...</p>
    </div>
  {/if}

  <div id="container">
    <canvas id="worker-canvas" raw-window-handle="1"></canvas>
  </div>

  <div id="container">
    <canvas id="worker-canvas" raw-window-handle="1"></canvas>
  </div>
</div>

<style>
  canvas {
    border: #ff5a5a 2px solid;
    width: 100%;
    height: 100vh;
  }

  #app-container {
    position: relative;
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

  canvas {
    border: #ff5a5a 2px solid;
    width: 100%;
    height: 100vh;
  }

  /* #container {
    position: relative;
    width: 100%;
    height: 100vh;
  } */

  #loading {
    position: absolute;
    top: 0;
    left: 0;
    background-color: rgba(255, 148, 148, 0.8);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
  }
</style>
