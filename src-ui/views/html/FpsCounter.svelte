<script lang="ts">
  // A composition view's FPS readout, in its lower right corner: how many
  // frames per second this view rendered (src/view_rates.rs). Off by
  // default; the composition settings menu's "Show FPS" turns it on, kept
  // with the layout (the panel's `fps` parameter).
  import { onDestroy } from "svelte";
  import { frameStats } from "../../runtime/frame_stats.svelte";

  let { panel }: { panel: string } = $props();

  const stop = frameStats.use();
  onDestroy(stop);

  const fps = $derived(frameStats.fps[panel]);
  const idle = $derived(fps !== undefined && fps < 0.5);
  const detail = $derived.by(() => {
    const b = frameStats.build;
    const lines = [
      "Frames this view rendered per second. A view renders only when what it shows changes.",
      frameStats.tick != null ? `App frame rate ${frameStats.tick.toFixed(1)}` : "",
      b ? `Build ${b.profile} (opt ${b.opt}, ${b.simd ? "SIMD" : "no SIMD"})${b.fast ? "" : " - slow build"} · ${b.gfx}` : "",
      frameStats.canvas ? `Canvas ${frameStats.canvas[0]}×${frameStats.canvas[1]} px` : "",
    ];
    return lines.filter(Boolean).join("\n");
  });
</script>

<div class="fps canvas-ui" class:idle title={detail} data-fps={panel}>
  <span class="value">{fps === undefined ? "–" : fps.toFixed(0)}</span>
  <span class="unit">fps</span>
</div>

<style>
  .fps {
    position: absolute;
    right: 8px;
    bottom: 8px;
    display: flex;
    align-items: baseline;
    gap: 3px;
    padding: 3px 7px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg);
    color: var(--fg);
    box-shadow: var(--menu-shadow);
    font-size: 11px;
    line-height: 1.3;
    font-variant-numeric: tabular-nums;
    user-select: none;
  }
  .value {
    min-width: 2ch;
    text-align: right;
    font-weight: 600;
  }
  .unit {
    color: var(--fg-muted);
  }
  .idle .value {
    color: var(--fg-faint);
  }
</style>
