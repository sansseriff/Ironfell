<script lang="ts">
  // App settings (source: app, notation: settings): how this browser runs
  // Iron — appearance, render scale, 2D renderer, runtime, developer mode — and what build
  // is running.
  // Stored per browser (runtime/settings.svelte.ts). Render scale applies
  // live; the renderer and runtime restart the app, carrying the document.
  import { isNativeHost } from "../../runtime/native_host";
  const native = isNativeHost();
  import { controllerManager } from "../../controller-manager.svelte";
  import { pixelRatioFor, saveSettings, settings, type Appearance, type RenderScale, type Renderer } from "../../runtime/settings.svelte";
  import { setAppearance } from "../../runtime/theme.svelte";
  import { Switch } from "bits-ui";
  import Select from "../../lib/ui/Select.svelte";
  import Button from "../../lib/ui/Button.svelte";
  import Section from "../../lib/ui/Section.svelte";
  import type { ViewSpec } from "../views";
  import { closeRenderInspectors, resetWorkspace } from "../workspace";

  let { spec }: { spec: ViewSpec } = $props();
  void spec;

  type BuildInfo = { profile: string; opt: string; simd: boolean; fast: boolean; classic: boolean; gfx: string };
  let build: BuildInfo | null = $state(null);
  $effect(() => {
    if (!controllerManager.isInitialized) return;
    controllerManager.docClient.command({ op: 'build_info' }).then((b: BuildInfo) => (build = b)).catch(() => (build = null));
  });

  const display = window.devicePixelRatio || 1;
  const screenW = window.screen.width;
  const screenH = window.screen.height;
  const fmt = (r: number) => `${Math.round(r * 100) / 100}×`;
  const SCALES: { value: RenderScale; label: string; hint: string }[] = [
    { value: "auto", label: "Display", hint: fmt(pixelRatioFor("auto")) },
    { value: "panel4k", label: "Match a 4K panel", hint: fmt(pixelRatioFor("panel4k")) },
    { value: "2", label: "2×", hint: "" },
    { value: "1.5", label: "1.5×", hint: "" },
    { value: "1", label: "1×", hint: "" },
  ];
  const APPEARANCES: { value: Appearance; label: string }[] = [
    { value: "system", label: "System" },
    { value: "light", label: "Light" },
    { value: "dark", label: "Dark" },
  ];
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

  function setDeveloper(developer: boolean) {
    saveSettings({ developer });
    if (!developer) closeRenderInspectors();
  }

  const busy = $derived(controllerManager.loadingInProgress);
</script>

<div class="html-view settings">
  <Section title="Appearance">
    <div class="setting">
      <span>Theme</span>
      <Select label="Theme" value={settings.appearance} items={APPEARANCES} onChange={setAppearance} />
    </div>
    <p class="note">System follows macOS's light/dark switching.</p>
  </Section>

  {#if !native}
  <Section title="Render scale">
    <div class="setting">
      <span>Canvas pixels per CSS pixel</span>
      <Select label="Render scale" value={settings.renderScale} items={SCALES} onChange={setScale} />
    </div>
    <p class="note">
      This screen reports {screenW}×{screenH} at {fmt(display)}; a full-screen canvas renders at
      {px(pixelRatioFor(settings.renderScale))} px. On a 4K display set to "looks like 2560×1440", macOS has
      the browser draw 5120×2880 and shrinks it to 3840×2160, so ~44% of canvas pixels are never seen.
      "Match a 4K panel" renders at the panel's density instead; text and controls stay sharp. A browser
      can't tell this from a native 5K display, so it is a choice, not a detection. Applies at once.
    </p>
  </Section>
  {/if}

  <Section title="2D renderer">
    <div class="setting">
      <span>Vector renderer</span>
      <Select
        label="2D renderer"
        disabled={busy}
        value={settings.renderer}
        items={[
          { value: "hybrid", label: "Vello Hybrid", hint: "sparse strips" },
          { value: "classic", label: "Vello Classic", hint: build?.classic ? "compute" : "not in this build", disabled: !build?.classic },
        ] as { value: Renderer; label: string; hint: string; disabled?: boolean }[]}
        onChange={(renderer) => apply({ renderer })}
      />
    </div>
    {#if build && !build.classic && !native}
      <p class="note">Classic needs a WebGPU build with it: ./build-wasm.sh webgpu classic</p>
    {/if}
  </Section>

  <Section title="Runtime">
    {#if native}<p class="note">Native Bevy runtime with parallel simulation and rendering.</p>{:else}
    <div class="setting">
      <span>Run in a worker</span>
      <Switch.Root class="ui-switch" disabled={busy} checked={controllerManager.runtimeMode === "worker"} onCheckedChange={setMode} aria-label="Run in a worker">
        <Switch.Thumb class="ui-switch-thumb" />
      </Switch.Root>
    </div>
    {/if}
    <div class="setting">
      <span>Layout</span>
      <Button variant="outline" small onclick={resetWorkspace}>Reset layout</Button>
    </div>
  </Section>

  <Section title="Developer">
    <div class="setting">
      <span>Developer mode</span>
      <Switch.Root class="ui-switch" checked={settings.developer} onCheckedChange={setDeveloper} aria-label="Developer mode">
        <Switch.Thumb class="ui-switch-thumb" />
      </Switch.Root>
    </div>
    <p class="note">Adds the render inspector to each composition's settings menu (top right of the tab): the composition beside a view of its chunks, culling and raster tiles.</p>
  </Section>

  <Section title="Build">
    {#if build}
      <p class:slow={!build.fast} class="build">
        {build.profile} · opt {build.opt} · {build.simd ? "SIMD" : "no SIMD"} · {build.gfx}{build.fast ? "" : " — slow, not for timing"}
      </p>
      {#if !native}<p class="note">bun run wasm:fast builds the optimized one, bun run wasm the dev one.</p>{/if}
    {/if}
    {#if busy}<p class="note">Restarting…</p>{/if}
    {#if !native}<p class="note">A renderer or runtime change restarts the app; the document is kept, its undo history is not.</p>{/if}
  </Section>
</div>

<style>
  .settings {
    overflow: auto;
    padding: 0 !important;
    gap: 0 !important;
  }
  .setting {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: var(--control-h);
    font-size: var(--text-md);
  }
  .setting > span {
    min-width: 0;
    color: var(--fg-muted);
  }
  .setting :global(.ui-select-trigger) {
    flex: 0 0 auto;
    width: 160px;
  }
  .note {
    color: var(--fg-faint);
    font-size: var(--text-sm);
    line-height: 1.5;
    margin: 0;
  }
  .build {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--ok);
    margin: 0;
  }
  .build.slow {
    color: var(--danger);
  }
</style>
