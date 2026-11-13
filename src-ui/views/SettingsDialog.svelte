<script lang="ts">
  // App settings in a modal, independent of the workspace: how this browser runs
  // Iron — appearance, render scale, 2D renderer, runtime, developer mode — and what build
  // is running.
  // Stored per browser (runtime/settings.svelte.ts). Render scale applies
  // live; the renderer and runtime restart the app, carrying the document.
  import { isNativeHost } from "../runtime/native_host";
  const native = isNativeHost();
  import { controllerManager } from "../controller-manager.svelte";
  import { pixelRatioFor, saveSettings, settings, type Appearance, type RenderScale, type Renderer } from "../runtime/settings.svelte";
  import { setAppearance } from "../runtime/theme.svelte";
  import Switch from "../lib/ui/Switch.svelte";
  import Select from "../lib/ui/Select.svelte";
  import Button from "../lib/ui/Button.svelte";
  import Section from "../lib/ui/Section.svelte";
  import { closeRenderInspectors, resetWorkspace } from "./workspace";

  import { Dialog, Tabs } from "bits-ui";
  import X from "@lucide/svelte/icons/x";
  import Palette from "@lucide/svelte/icons/palette";
  import Gauge from "@lucide/svelte/icons/gauge";
  import PanelsTopLeft from "@lucide/svelte/icons/panels-top-left";
  import Code from "@lucide/svelte/icons/code";

  let { open = $bindable(false) }: { open?: boolean } = $props();
  let section = $state("appearance");
  const sections = [
    { id: "appearance", label: "Appearance", icon: Palette },
    { id: "performance", label: "Performance", icon: Gauge },
    { id: "workspace", label: "Workspace", icon: PanelsTopLeft },
    { id: "developer", label: "Developer", icon: Code },
  ];

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

<Dialog.Root bind:open>
  <Dialog.Portal>
    <Dialog.Overlay class="ui-dialog-overlay" />
    <Dialog.Content class="ui-dialog settings-dialog" aria-describedby={undefined}>
      <header class="dialog-header">
        <Dialog.Title class="settings-title">Settings</Dialog.Title>
        <Dialog.Close class="ui-icon-button" aria-label="Close settings" title="Close settings"><X size={18} strokeWidth={1.75} /></Dialog.Close>
      </header>
      <Tabs.Root class="settings-layout" orientation="vertical" bind:value={section}>
        <Tabs.List class="settings-sidebar" aria-label="Settings sections">
          {#each sections as item (item.id)}
            <Tabs.Trigger class="settings-tab" value={item.id}><item.icon size={17} strokeWidth={1.75} /><span>{item.label}</span></Tabs.Trigger>
          {/each}
        </Tabs.List>
        <div class="settings-body">
          <Tabs.Content class="settings-content" value="appearance">
            <h2>Appearance</h2>
            <p class="intro">Make Iron feel at home on your display.</p>
            <Section title="Theme">
              <div class="setting">
                <span>Color scheme</span>
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
                {px(pixelRatioFor(settings.renderScale))} px. On a scaled 4K display, "Match a 4K panel" renders only the
                pixels the panel shows. Applies at once.
              </p>
            </Section>
            {/if}

          </Tabs.Content>
          <Tabs.Content class="settings-content" value="performance">
            <h2>Performance</h2>
            <p class="intro">Choose how Iron renders and runs on this device.</p>
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
                <p class="note">Vello Classic needs a WebGPU build that includes it.</p>
              {/if}
            </Section>

            <Section title="Runtime">
              {#if native}<p class="note">Native Bevy runtime with parallel simulation and rendering.</p>{:else}
              <div class="setting">
                <span>Run in a worker</span>
                <Switch disabled={busy} checked={controllerManager.runtimeMode === "worker"} onChange={setMode} label="Run in a worker" />
              </div>
              {/if}
              {#if !native}<p class="note">Changing the renderer or runtime restarts Iron. Your document is kept; undo history is cleared.</p>{/if}
              {#if busy}<p class="note" role="status">Restarting…</p>{/if}
            </Section>
          </Tabs.Content>
          <Tabs.Content class="settings-content" value="workspace">
            <h2>Workspace</h2>
            <p class="intro">Arrange your views and panels.</p>
            <Section title="Layout">
              <div class="setting">
                <span>Panel arrangement</span>
                <Button variant="outline" small onclick={resetWorkspace}>Reset layout</Button>
              </div>
            </Section>

          </Tabs.Content>
          <Tabs.Content class="settings-content" value="developer">
            <h2>Developer</h2>
            <p class="intro">Inspect rendering and check the current build.</p>
            <Section title="Developer tools">
              <div class="setting">
                <span>Developer mode</span>
                <Switch checked={settings.developer} onChange={setDeveloper} label="Developer mode" />
              </div>
              <p class="note">Adds the render inspector to each composition's settings menu.</p>
            </Section>

            <Section title="Build">
              {#if build}
                <p class="build">
                  <span>{build.profile} · opt {build.opt} · {build.simd ? "SIMD" : "no SIMD"} · {build.gfx}</span>
                  {#if build.fast}<span class="ui-badge is-ok">optimized</span>{:else}<span class="ui-badge is-warning">slow</span><span class="unfit">not for timing</span>{/if}
                </p>
                {#if !native}<p class="note"><code>bun run wasm:fast</code> builds the optimized one, <code>bun run wasm</code> the dev one.</p>{/if}
              {/if}
            </Section>
          </Tabs.Content>
        </div>
      </Tabs.Root>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  :global(.settings-dialog) { display: flex; flex-direction: column; width: min(920px, calc(100vw - 48px)); height: min(640px, calc(100dvh - 48px)); max-width: none; max-height: none; padding: 0; gap: 0; overflow: hidden; background: var(--bg); }
  .dialog-header { display: flex; align-items: center; justify-content: space-between; flex: none; min-height: 60px; padding: 0 24px; border-bottom: 1px solid var(--border); }
  :global(.settings-title) { margin: 0; font-size: var(--text-xl); font-weight: 600; }
  :global(.settings-layout) { display: flex; flex: 1; min-height: 0; }
  :global(.settings-sidebar) { display: flex; flex-direction: column; flex: 0 0 190px; gap: 4px; padding: 20px 12px; border-right: 1px solid var(--border); overflow-y: auto; }
  :global(.settings-tab) { display: flex; align-items: center; gap: 10px; flex: none; min-height: 36px; padding: 8px 12px; border: 0; border-radius: var(--radius); background: transparent; color: var(--fg-muted); font: inherit; font-size: var(--text-lg); text-align: left; cursor: pointer; }
  :global(.settings-tab:hover) { background: var(--hover); color: var(--fg); }
  :global(.settings-tab[data-state="active"]) { background: var(--active); color: var(--fg); }
  :global(.settings-tab:focus-visible) { outline: 2px solid var(--accent); outline-offset: -2px; }
  .settings-body { flex: 1; min-width: 0; overflow-y: auto; overscroll-behavior: contain; }
  :global(.settings-content) { padding: 26px 32px; }
  h2 { margin: 0 0 6px; font-size: var(--text-xl); font-weight: 600; }
  .intro { margin: 0 0 20px; color: var(--fg-muted); font-size: var(--text-lg); }
  :global(.settings-content .ui-section) { padding: 18px 0; border-color: var(--border); }
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
    color: var(--fg);
  }
  .setting :global(.ui-select-trigger) {
    flex: 0 0 auto;
    width: 160px;
  }
  .note code { font-family: var(--font-mono); font-size: var(--text-xs); }
  .note {
    color: var(--fg-muted);
    font-size: var(--text-sm);
    line-height: 1.5;
    margin: 0;
  }
  .build {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--fg);
  }
  .unfit {
    color: var(--fg-muted);
    font-family: inherit;
  }
  @media (max-width: 600px) {
    :global(.settings-dialog) { display: flex; flex-direction: column; width: calc(100vw - 24px); height: calc(100dvh - 24px); }
    :global(.settings-sidebar) { flex-basis: 140px; padding: 12px 8px; }
    :global(.settings-tab) { gap: 7px; padding: 8px; font-size: var(--text-md); }
    :global(.settings-content) { padding: 20px 16px; }
    .setting { align-items: flex-start; flex-direction: column; }
  }
</style>
