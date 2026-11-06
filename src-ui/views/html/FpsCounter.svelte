<script lang="ts">
  // A composition view's FPS readout, in its lower right corner: how many
  // frames per second this view rendered (src/view_rates.rs). Off by
  // default; the composition settings menu's "Show FPS" turns it on, kept
  // with the layout (the panel's `fps` parameter).
  import { onDestroy } from "svelte";
  import { frameStats } from "../../runtime/frame_stats.svelte";
  import { frameMode } from "../../runtime/frame_mode.svelte";
  import { settings } from "../../runtime/settings.svelte";

  let { panel }: { panel: string } = $props();

  const stop = frameStats.use();
  onDestroy(stop);

  const fps = $derived(frameStats.fps[panel]);
  const idle = $derived(fps !== undefined && fps < 0.5);
  // The worker canvas's frame mode (plans/rendering/worker-frame-hold.md §6g).
  // Normally shown only when it is worth knowing: late and half rate (warning
  // colour), and ahead while this view animates. In developer mode always,
  // including early and off, so it is plain whether frame modes are running.
  const modeLabel = $derived.by(() => {
    const m = frameMode.mode;
    if (settings.developer) return m;
    if (m === "late" || m === "half") return m;
    return m === "ahead" && fps !== undefined && fps >= 0.5 ? m : null;
  });
  const modeDetail = $derived.by(() => {
    const m = frameMode.mode;
    const what = m === "off" ? "Frame mode: off (frame modes are not running)"
      : m === "early" ? "Frame mode: early (Vello in the same frame as the page)"
      : m === "late" ? "Frame mode: late (Vello one frame behind the page, steady)"
      : m === "half" ? "Frame mode: half rate (Vello every second frame, steady)"
      : "Frame mode: ahead (between interactions: frames drawn for the frame they are shown at)";
    const n = frameMode.numbers;
    return [what, frameMode.reason, n && m === "early" ? `Limit ${n.limit.toFixed(1)} ms · tick p90 ${n.tick.toFixed(1)} ms · page after p90 ${n.after.toFixed(1)} ms` : ""].filter(Boolean).join("\n");
  });
  const detail = $derived.by(() => {
    const b = frameStats.build;
    const lines = [
      modeDetail,
      "Frames this view rendered per second. A view renders only when what it shows changes.",
      frameStats.tick != null ? `App frame rate ${frameStats.tick.toFixed(1)}` : "",
      b ? `Engine build ${b.profile} (opt ${b.opt}, ${b.simd ? "SIMD" : "no SIMD"})${b.fast ? "" : " - slow build"} · ${b.gfx}` : "",
      `Frontend ${__IRON_FRONTEND_VERSION__}`,
      frameStats.canvas ? `Canvas ${frameStats.canvas[0]}×${frameStats.canvas[1]} px` : "",
    ];
    return lines.filter(Boolean).join("\n");
  });
</script>

<div class="fps canvas-ui" class:idle title={detail} data-fps={panel}>
  <span class="value">{fps === undefined ? "–" : fps.toFixed(0)}</span>
  <span class="unit">fps</span>
  {#if modeLabel}<span class="mode" class:warn={modeLabel === "late" || modeLabel === "half"} class:off={modeLabel === "off"} data-frame-mode={modeLabel}>{modeLabel}</span>{/if}
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
  .mode {
    margin-left: 3px;
    padding-left: 6px;
    border-left: 1px solid var(--border);
    color: var(--fg-muted);
  }
  .mode.warn {
    color: var(--warning);
    font-weight: 600;
  }
  .mode.off {
    color: var(--fg-faint);
    text-decoration: line-through;
  }
</style>
