<script lang="ts">
  import { onMount } from "svelte";

  // import { createEventDispatcher } from 'svelte';
  import { constrain } from "./utils.js";

  // /** @type {ReturnType<typeof createEventDispatcher<{ change: undefined }>>} */
  // const dispatch = createEventDispatcher();
  import type { Snippet } from "svelte";

  interface Props {
    id?: any;
    orientation: string;
    pos?: string;
    min?: string;
    max?: string;
    disabled?: boolean;
    priority?: string;
    a: Snippet;
    b: Snippet;
    onWindowChange?: ChangeFunction;
    onResize?: () => void; // Add new resize notification callback
  }

  type ChangeFunction = () => any;

  let {
    id = undefined,
    orientation,
    pos = "50%",
    min = "0%",
    max = "100%",
    disabled = false,
    priority = "min",
    onWindowChange,
    onResize, // Add new prop
    a,
    b,
  }: Props = $props();

  // /** @type {string | undefined} */
  // export let id = undefined;

  // /** @type {'horizontal' | 'vertical'} */
  // export let type;

  // /** @type {import('./types').Length} */
  // export let pos = '50%';

  // /** @type {import('./types').Length} */
  // export let min = '0%';

  // /** @type {import('./types').Length} */
  // export let max = '100%';

  // export let disabled = false;

  // /** @type {'min' | 'max'}*/
  // export let priority = 'min';

  // /** @type {HTMLElement} */

  let position = $state(pos);

  // this has to be $state() because initially before
  // dom is loaded, the validated_position signal
  // will run with container undefined. After dom loaded
  // the load will re-trigger the validated_position signal
  let container: HTMLElement | undefined = $state();

  let dragging = $state(false);

  // these get bound to clientWidth and clientHeight
  let w = $state(0);
  let h = $state(0);

  onMount(() => {
    // Notify parent when split pane is fully mounted
    if (onResize) {
      onResize();
    }
  });

  let validated_position = $derived.by(() => {
    if (container) {
      const size = orientation === "horizontal" ? w : h;
      return constrain(container, size, min, max, position, priority);
    }
  });

  $effect(() => {
    // When validated position changes, notify parent component
    if (validated_position && onResize) {
      onResize();
    }
  });

  // $: position = pos;

  // // constrain position
  // $: if (container) {
  // 	// if orientation is "horizontal", then set size of w. Else, set size to h
  // 	const size = orientation === 'horizontal' ? w : h;
  // 	position = constrain(container, size, min, max, position, priority);
  // }

  function update(x: number, y: number) {
    // update position based on drag
    if (disabled) return;

    const { top, left } = container
      ? container.getBoundingClientRect()
      : { top: 0, left: 0 };

    const pos_px = orientation === "horizontal" ? x - left : y - top;
    const size = orientation === "horizontal" ? w : h;

    position = pos.endsWith("%") ? `${(100 * pos_px) / size}%` : `${pos_px}px`;

    // dispatch('change');
    if (onWindowChange) {
      onWindowChange();
    }

    // Also trigger resize notification
    if (onResize) {
      onResize();
    }
  }

  // /**
  //  * @param {HTMLElement} node
  //  * @param {(event: PointerEvent) => void} callback
  //  */

  function drag(node: HTMLElement, callback: (event: PointerEvent) => void) {
    // /** @param {PointerEvent} event */
    const pointerdown = (event: PointerEvent) => {
      if (
        (event.pointerType === "mouse" && event.button === 2) ||
        (event.pointerType !== "mouse" && !event.isPrimary)
      )
        return;

      node.setPointerCapture(event.pointerId);

      event.preventDefault();

      dragging = true;

      const onpointerup = () => {
        dragging = false;

        node.setPointerCapture(event.pointerId);

        window.removeEventListener("pointermove", callback, false);
        window.removeEventListener("pointerup", onpointerup, false);
      };

      window.addEventListener("pointermove", callback, false);
      window.addEventListener("pointerup", onpointerup, false);
    };

    node.addEventListener("pointerdown", pointerdown, {
      capture: true,
      passive: false,
    });

    return {
      destroy() {
        node.removeEventListener("pointerdown", pointerdown);
      },
    };
  }
</script>

<div
  data-pane={id}
  class="container {orientation}"
  bind:this={container}
  bind:clientWidth={w}
  bind:clientHeight={h}
  style="--pos: {validated_position}"
>
  <div class="pane">
    <!-- <slot name="a" /> -->
    {@render a()}
  </div>

  <div class="pane">
    <!-- <slot name="b" /> -->
    {@render b()}
  </div>

  {#if pos !== "0%" && pos !== "100%"}
    <div
      class="{orientation} divider"
      class:disabled
      use:drag={(e) => update(e.clientX, e.clientY)}
    ></div>
  {/if}
</div>

{#if dragging}
  <div class="mousecatcher"></div>
{/if}

<style>
  .container {
    --sp-thickness: var(--thickness, 8px);
    --sp-color: var(--color, transparent);
    display: grid;
    position: relative;
    width: 100%;
    height: 100%;
  }

  /* ahh, so this picks up the --pos variable injected in the html section and uses it */
  .container.vertical {
    grid-template-rows: var(--pos) 1fr;
  }

  .container.horizontal {
    grid-template-columns: var(--pos) 1fr;
  }

  .pane {
    width: 100%;
    height: 100%;
    overflow: auto;
  }

  .pane > :global(*) {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }

  .mousecatcher {
    position: absolute;
    left: 0;
    top: 0;
    width: 100%;
    height: 100%;
    background: rgba(255, 255, 255, 0.0001);
  }

  .divider {
    position: absolute;
    touch-action: none !important;
  }

  .divider::after {
    content: "";
    position: absolute;
    background-color: var(--sp-color);
  }

  .horizontal > .divider {
    padding: 0 calc(0.5 * var(--sp-thickness));
    width: 0;
    height: 100%;
    cursor: ew-resize;
    left: var(--pos);
    transform: translate(calc(-0.5 * var(--sp-thickness)), 0);
    background: rgba(0, 0, 0, 0.1);
  }

  .horizontal > .divider.disabled {
    cursor: default;
  }

  .horizontal > .divider::after {
    left: 50%;
    top: 0;
    width: 1px;
    height: 100%;
  }

  .vertical > .divider {
    padding: calc(0.5 * var(--sp-thickness)) 0;
    width: 100%;
    height: 0;
    cursor: ns-resize;
    top: var(--pos);
    transform: translate(0, calc(-0.5 * var(--sp-thickness)));
  }

  .vertical > .divider.disabled {
    cursor: default;
  }

  .vertical > .divider::after {
    top: 50%;
    left: 0;
    width: 100%;
    height: 1px;
  }
</style>
