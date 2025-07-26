<script lang="ts">
  import SplitPane from "./lib/SplitPane5.svelte";
  import Scene from "./lib/Scene.svelte";
  import Controls from "./lib/Controls.svelte";

  let sceneComponent: Scene;

  // Handler for SplitPane resize events
  function handleSplitPaneResize() {
    if (sceneComponent) {
      sceneComponent.triggerResize();
    }
  }

  function handleLaunch() {
    // The launch functionality is now handled by the Scene component
    // This is just a placeholder in case we need to do something from the parent
  }
</script>

{#snippet a()}
  <Controls
    onLaunch={handleLaunch}
  />
{/snippet}

{#snippet b()}
  <Scene
    bind:this={sceneComponent}
  />
{/snippet}

<SplitPane
  orientation="horizontal"
  min="1%"
  max="75%"
  pos="52%"
  --color="black"
  {a}
  {b}
  onResize={handleSplitPaneResize}
></SplitPane>

<style>
  /* App-specific styles can go here if needed */
</style>
