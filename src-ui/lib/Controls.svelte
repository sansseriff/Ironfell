<script>
  import { controllerManager } from "../controller-manager.svelte";

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
