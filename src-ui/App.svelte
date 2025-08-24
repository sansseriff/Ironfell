<script lang="ts">
  import SplitPane from "./lib/SplitPane5.svelte";
  import Scene from "./lib/Scene.svelte";
  import Controls from "./lib/Controls.svelte";

  let sceneComponent: Scene;
  let timelineSceneComponent: Scene;

  // Handler for SplitPane resize events
  function handleSplitPaneResize() {
    if (sceneComponent) {
      sceneComponent.triggerResize();
    }
    if (timelineSceneComponent) {
      timelineSceneComponent.triggerResize();
    }
  }

  function handleLaunch() {
    // The launch functionality is now handled by the Scene component
    // This is just a placeholder in case we need to do something from the parent
  }
</script>

{#snippet a()}
  <Controls onLaunch={handleLaunch} />
{/snippet}

{#snippet b()}
  <Scene bind:this={sceneComponent} canvasId="viewer-canvas" />
{/snippet}

{#snippet ab()}
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
{/snippet}

{#snippet timeline()}
  <Scene bind:this={timelineSceneComponent} canvasId="timeline-canvas" />
{/snippet}

<SplitPane
  orientation="vertical"
  min="1%"
  max="75%"
  pos="75%"
  --color="black"
  a={ab}
  b={timeline}
  onResize={handleSplitPaneResize}
></SplitPane>

<style>
  /* App-specific styles can go here if needed */
</style>
