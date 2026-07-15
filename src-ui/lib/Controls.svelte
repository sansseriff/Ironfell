<script lang="ts">
  import { controllerManager } from "../controller-manager.svelte";
  import Github from "./Github.svelte";

  // Runtime mode toggle
  let pendingSwitch = $state(false);
  async function toggleMode(event: Event) {
    const input = event.currentTarget as HTMLInputElement | null;
    if (!input) return;
    const targetMode = input.checked ? "worker" : "main";

    pendingSwitch = true;
    try {
      await controllerManager.switchMode(targetMode);
    } finally {
      pendingSwitch = false;
    }
  }
</script>

<section class="controls-section">
  <h3>Inspector Controls</h3>

  {#if controllerManager.showWebGPUWarning}
    <div class="webgpu-warning">
      <p class="webgpu-warning-text">
        <strong>⚠️ WebGPU Not Supported</strong><br />
        This application requires WebGPU support. Please check the main panel for
        instructions.
      </p>
    </div>
  {:else if !controllerManager.isInitialized}
    <p>
      {controllerManager.loadingInProgress ? "Loading..." : "Starting up..."}
    </p>
  {:else}
    <div class="controls-container">
      <h3>Square & torus are draggable</h3>
      <h3>Press F for camera controller, WASD to move</h3>
      <Github></Github>
    </div>

    <div class="mode-toggle-container">
      <label class="mode-toggle-label">
        <input
          type="checkbox"
          onchange={toggleMode}
          disabled={controllerManager.loadingInProgress || pendingSwitch}
          checked={controllerManager.runtimeMode === "worker"}
        />
        <span class="mode-text"
          >{controllerManager.runtimeMode === "worker"
            ? "Worker Mode"
            : "Main Thread Mode"}</span
        >
      </label>
      {#if pendingSwitch}<span class="switching-text">switching...</span>{/if}
    </div>
  {/if}
</section>

<style>
  * {
    color: black;
  }

  h3 {
    margin: 0 0 10px;
    font-size: 16px;
    color: #333;
  }

  .controls-section {
    background-color: var(--body-color);
    padding: 20px;
    border-radius: 12px;
    height: 100%;
  }

  .webgpu-warning {
    background-color: var(--warning-bg-color);
    border: 1px solid var(--warning-border-color);
    border-radius: 8px;
    padding: 15px;
    margin: 10px 0;
  }

  .webgpu-warning-text {
    margin: 0;
    color: #856404;
  }

  .controls-container {
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 200px;
  }

  .mode-toggle-container {
    margin-top: 20px;
    margin-bottom: 12px;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .mode-toggle-label {
    font-size: 12px;
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }

  .mode-text {
    color: black;
  }

  .switching-text {
    font-size: 11px;
    color: #666;
  }
</style>
