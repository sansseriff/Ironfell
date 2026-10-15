<script lang="ts">
  // App settings (source: app, notation: settings): how this browser runs
  // Iron — render scale, 2D renderer, runtime — and what build is running.
  // Stored per browser (runtime/settings.svelte.ts). Render scale applies
  // live; the renderer and runtime restart the app, carrying the document.
  import { controllerManager } from "../../controller-manager.svelte";
  import { pixelRatioFor, saveSettings, settings, type RenderScale, type Renderer } from "../../runtime/settings.svelte";
  import type { ViewSpec } from "../views";
  import { resetWorkspace } from "../workspace";

  let { spec }: { spec: ViewSpec } = $props();
  void spec;

  type BuildInfo = { profile: string; opt: string; simd: boolean; fast: boolean; classic: boolean; gfx: string };
  let build: BuildInfo | null = $state(null);
  $effect(() => {
    if (!controllerManager.isInitialized) return;
    controllerManager.docClient.call("build_info", []).then((b: BuildInfo) => (build = b)).catch(() => (build = null));
  });

  const display = window.devicePixelRatio || 1;
  const screenW = window.screen.width;
  const screenH = window.screen.height;
  const SCALES: { value: RenderScale; label: string; note: string }[] = [
    { value: "auto", label: "Display", note: "the browser's ratio" },
    { value: "panel4k", label: "Match a 4K panel", note: "for a 4K display in a scaled (\"looks like\") mode" },
    { value: "2", label: "2×", note: "" },
    { value: "1.5", label: "1.5×", note: "" },
    { value: "1", label: "1×", note: "" },
  ];
  const fmt = (r: number) => `${Math.round(r * 100) / 100}×`;
  const px = (r: number) => `${Math.round(screenW * r)}×${Math.round(screenH * r)}`;

  async function apply(next: Partial<typeof settings>) {
    saveSettings(next);
    await controllerManager.restart();
  }

  function setScale(renderScale: RenderScale) {
    saveSettings({ renderScale });
    controllerManager.refreshPixelRatio();
  }

  async function setMode(worker: boolean) {
    await controllerManager.switchMode(worker ? "worker" : "main");
  }

  const busy = $derived(controllerManager.loadingInProgress);
</script>

<section class="html-view settings">
  <h3>Render scale</h3>
  <p class="small">
    This screen reports {screenW}×{screenH} at {fmt(display)}. A full-screen canvas renders at
    {px(pixelRatioFor(settings.renderScale))} px with the current setting.
  </p>
  {#each SCALES as s (s.value)}
    <label class="option">
      <input
        type="radio"
        name="scale"
        disabled={busy}
        checked={settings.renderScale === s.value}
        onchange={() => setScale(s.value)}
      />
      <span>{s.label} <span class="dim">{fmt(pixelRatioFor(s.value))}{s.note ? ` · ${s.note}` : ""}</span></span>
    </label>
  {/each}
  <p class="note">
    On a 4K display set to "looks like 2560×1440", macOS has the browser draw 5120×2880 and then shrinks
    it to 3840×2160, so ~44% of canvas pixels are never seen. "Match a 4K panel" renders the canvas at
    the panel's density instead; text and controls stay sharp. A browser can't tell this from a native
    5K display, so it is a choice, not a detection.
  </p>

  <h3>2D renderer</h3>
  {#each [["hybrid", "Vello Hybrid (sparse strips)"], ["classic", "Vello Classic (compute)"]] as [value, label] (value)}
    <label class="option">
      <input
        type="radio"
        name="renderer"
        disabled={busy || (value === "classic" && !build?.classic)}
        checked={settings.renderer === value}
        onchange={() => apply({ renderer: value as Renderer })}
      />
      <span>{label}</span>
    </label>
  {/each}
  {#if build && !build.classic}
    <p class="note">Classic needs a WebGPU build with it: <code>./build-wasm.sh webgpu classic</code>.</p>
  {/if}

  <h3>Runtime</h3>
  <label class="option">
    <input type="checkbox" disabled={busy} checked={controllerManager.runtimeMode === "worker"} onchange={(e) => setMode(e.currentTarget.checked)} />
    <span>Run in a worker <span class="dim">(off: on the main thread)</span></span>
  </label>
  <button class="reset" onclick={resetWorkspace}>Reset layout</button>

  <h3>Build</h3>
  {#if build}
    <p class:slow={!build.fast} class="build">
      {build.profile} · opt {build.opt} · {build.simd ? "SIMD" : "no SIMD"} · {build.gfx}{build.fast ? "" : " — slow, not for timing"}
    </p>
    <p class="note"><code>bun run wasm:fast</code> builds the optimized one, <code>bun run wasm</code> the dev one.</p>
  {/if}
  {#if busy}<p class="small">Restarting…</p>{/if}
  <p class="note">Render scale applies at once. A renderer or runtime change restarts the app; the document is kept, its undo history is not.</p>
</section>

<style>
  .settings {
    overflow: auto;
  }
  h3 {
    font-size: 11px;
    text-transform: uppercase;
    color: #667;
    margin: 12px 0 4px;
  }
  h3:first-child {
    margin-top: 0;
  }
  .option {
    display: flex;
    gap: 6px;
    align-items: baseline;
    font-size: 13px;
    padding: 2px 0;
  }
  .dim {
    color: #889;
    font-size: 11px;
  }
  .note {
    color: #778;
    font-size: 11px;
    margin: 4px 0;
  }
  .build {
    font-size: 12px;
    color: #1a7f37;
    margin: 0;
  }
  .build.slow {
    color: #c62828;
  }
  .reset {
    align-self: flex-start;
    margin-top: 6px;
  }
</style>
