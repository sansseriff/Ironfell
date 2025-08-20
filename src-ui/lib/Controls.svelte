<script lang="ts">
  import { controllerManager } from "../controller-manager.svelte";
  import Github from "./Github.svelte";
  // Props
  let { onLaunch } = $props();

  // Inspector test functions
  function testSpawnEntity() {
    if (controllerManager.controller && controllerManager.isInitialized) {
      console.log("Testing spawn entity...");
      controllerManager.controller.inspectorSpawnEntity("123345");
    }
  }

  function testToggleVisibility() {
    if (controllerManager.controller && controllerManager.isInitialized) {
      // Test with a sample entity ID - you would normally get this from the scene
      const entityId = "4294967296"; // Example entity ID as string
      console.log(`Testing toggle visibility for entity ${entityId}...`);
      controllerManager.controller.inspectorToggleVisibility(entityId);
    }
  }

  function testDespawnEntity() {
    if (controllerManager.controller && controllerManager.isInitialized) {
      // Test with a sample entity ID - you would normally get this from the scene
      const entityId = "4294967296"; // Example entity ID as string
      console.log(`Testing despawn entity ${entityId}...`);
      controllerManager.controller.inspectorDespawnEntity(entityId);
    }
  }
  // Runtime mode toggle
  let pendingSwitch = $state(false);
  async function toggleMode(event: Event) {
    const input = event.currentTarget as HTMLInputElement | null;
    if (!input) return;
    const targetMode = input.checked ? "worker" : "main";
    if (!controllerManager.controller) return;
    const canvas = document.getElementById("worker-canvas");
    if (!(canvas instanceof HTMLCanvasElement)) return;
    pendingSwitch = true;
    try {
      await controllerManager.switchMode(canvas, targetMode);
      // If initialized before, re-run size logic via resize event
      if (controllerManager.isInitialized) {
        const resizeEvent = new Event("resize");
        window.dispatchEvent(resizeEvent);
      }
    } finally {
      pendingSwitch = false;
    }
  }
</script>

<section style="background: white; padding: 20px;">
  <h3>Inspector Controls</h3>

  {#if controllerManager.showWebGPUWarning}
    <div
      style="background: #fff3cd; border: 1px solid #ffeaa7; border-radius: 8px; padding: 15px; margin: 10px 0;"
    >
      <p style="margin: 0; color: #856404;">
        <strong>⚠️ WebGPU Not Supported</strong><br />
        This application requires WebGPU support. Please check the main panel for
        instructions.
      </p>
    </div>
  {:else if !controllerManager.isInitialized}
    <p>Launch the app to use inspector controls</p>
    <button onclick={onLaunch} disabled={controllerManager.loadingInProgress}>
      {controllerManager.loadingInProgress ? "Loading..." : "Launch App"}
    </button>
  {:else}
    <div
      style="display: flex; flex-direction: column; gap: 10px; max-width: 200px;"
    >
      <!-- <button onclick={testSpawnEntity}>Spawn Entity</button>
      <button onclick={testToggleVisibility}>Toggle Visibility</button>
      <button onclick={testDespawnEntity}>Despawn Entity</button> -->
      <h3>Square & torus are draggable</h3>
      <h3>Press F for camera controller, WASD to move</h3>
      <Github></Github>
    </div>

    <div
      style="margin-top: 20px; margin-bottom:12px; display:flex; align-items:center; gap:8px;"
    >
      <label
        style="font-size:12px; display:flex; align-items:center; gap:6px; cursor:pointer;"
      >
        <input
          type="checkbox"
          onchange={toggleMode}
          disabled={controllerManager.loadingInProgress || pendingSwitch}
          checked={controllerManager.runtimeMode === "worker"}
        />
        <span style="color: black;"
          >{controllerManager.runtimeMode === "worker"
            ? "Worker Mode"
            : "Main Thread Mode"}</span
        >
      </label>
      {#if pendingSwitch}<span style="font-size:11px; color:#666;"
          >switching...</span
        >{/if}
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
</style>
