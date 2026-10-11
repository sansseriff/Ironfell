<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { controllerManager } from "../controller-manager.svelte";

  // A Bevy panel is an empty placeholder div: the DOM lays it out, and its
  // rectangle is mirrored to a camera viewport / vello region in Rust. A
  // "composition" panel shows one composition (ECM §7.1), named by node id
  // or by name; "" lets Rust pick the first one with matching dimensions.
  let { id, kind, composition = "" }: { id: string; kind: string; composition?: string } = $props();

  let el: HTMLDivElement;

  onMount(() => {
    controllerManager.registerPanel(id, kind, el, composition);
  });

  onDestroy(() => {
    controllerManager.unregisterPanel(id);
  });
</script>

<div bind:this={el} class="bevy-panel" data-panel-id={id}></div>

<style>
  .bevy-panel {
    width: 100%;
    height: 100%;
    /* Input falls through to the full-window canvas beneath the UI layer */
    pointer-events: none;
    background: transparent;
  }
</style>
