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
            <Tabs.Trigger class="settings-tab" value={item.id}><item.icon size={16} strokeWidth={1.75} /><span>{item.label}</span></Tabs.Trigger>
          {/each}
        </Tabs.List>
        <div class="settings-body">
          <Tabs.Content class="settings-content" value="appearance">
            <h2>Appearance</h2>
            <p class="intro">Theme and canvas resolution.</p>
            <div class="setting">
              <div class="setting-copy"><h3>Theme</h3><p>Use a light or dark interface, or follow your system.</p></div>
              <Select label="Theme" value={settings.appearance} items={APPEARANCES} onChange={setAppearance} />
            </div>
            {#if !native}
              <div class="setting">
                <div class="setting-copy"><h3>Render scale</h3><p>Balance canvas sharpness and memory use. Display uses {fmt(pixelRatioFor("auto"))}.</p></div>
                <Select label="Render scale" value={settings.renderScale} items={SCALES} onChange={setScale} />
              </div>
            {/if}
          </Tabs.Content>
          <Tabs.Content class="settings-content" value="performance">
            <h2>Performance</h2>
            <p class="intro">Rendering and processing on this device.</p>
            <div class="setting">
              <div class="setting-copy"><h3>2D renderer</h3><p>{build && !build.classic && !native ? "Vello Hybrid is available in this build." : "Choose the renderer used for the canvas."}</p></div>
              <Select label="2D renderer" disabled={busy} value={settings.renderer}
                items={[
                  { value: "hybrid", label: "Vello Hybrid" },
                  { value: "classic", label: "Vello Classic", disabled: !build?.classic },
                ] as { value: Renderer; label: string; disabled?: boolean }[]}
                onChange={(renderer) => apply({ renderer })} />
            </div>
            <div class="setting">
              <div class="setting-copy"><h3>Run in a worker</h3><p>{native ? "The native runtime handles processing and rendering." : "Keep canvas processing off the interface thread."}</p></div>
              {#if !native}<Switch disabled={busy} checked={controllerManager.runtimeMode === "worker"} onChange={setMode} label="Run in a worker" />{/if}
            </div>
            {#if !native}<p class="footnote">Changing the renderer or runtime restarts Iron. Your document is kept; undo history is cleared.</p>{/if}
            {#if busy}<p class="footnote" role="status">Restarting…</p>{/if}
          </Tabs.Content>
          <Tabs.Content class="settings-content" value="workspace">
            <h2>Workspace</h2>
            <p class="intro">Views and panel arrangement.</p>
            <div class="setting">
              <div class="setting-copy"><h3>Reset layout</h3><p>Restore the default panels and their positions.</p></div>
              <Button variant="outline" onclick={resetWorkspace}>Reset layout</Button>
            </div>
          </Tabs.Content>
          <Tabs.Content class="settings-content" value="developer">
            <h2>Developer</h2>
            <p class="intro">Diagnostics and build information.</p>
            <div class="setting">
              <div class="setting-copy"><h3>Developer mode</h3><p>Show rendering tools in composition settings.</p></div>
              <Switch checked={settings.developer} onChange={setDeveloper} label="Developer mode" />
            </div>
            {#if build}
              <section class="build" aria-label="Build information">
                <div class="build-heading"><h3>Current build</h3><span class="ui-badge" class:is-ok={build.fast} class:is-warning={!build.fast}>{build.fast ? "Optimized" : "Development"}</span></div>
                <dl>
                  <div><dt>Profile</dt><dd>{build.profile}</dd></div>
                  <div><dt>Graphics</dt><dd>{build.gfx}</dd></div>
                  <div><dt>Optimization</dt><dd>Level {build.opt}{build.simd ? " · SIMD" : ""}</dd></div>
                </dl>
              </section>
            {/if}
          </Tabs.Content>
        </div>
      </Tabs.Root>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  :global(.settings-dialog) { display: flex; flex-direction: column; width: min(800px, calc(100vw - 48px)); height: min(480px, calc(100dvh - 48px)); max-width: none; max-height: none; padding: 0; gap: 0; border-radius: 12px; overflow: hidden; background: var(--bg); font-family: var(--font-ui); font-size: var(--text-lg); line-height: 1.5; }
  .dialog-header { display: flex; align-items: center; justify-content: space-between; flex: none; height: 64px; padding: 0 16px; border-bottom: 1px solid var(--border); }
  .dialog-header :global(.ui-icon-button) { width: 32px; height: 32px; border-radius: 6px; }
  :global(.settings-title) { margin: 0; font-size: var(--text-xl); font-weight: 600; letter-spacing: -0.2px; }
  :global(.settings-layout) { display: flex; flex: 1; min-height: 0; }
  :global(.settings-sidebar) { display: flex; flex-direction: column; flex: 0 0 180px; gap: 4px; padding: 16px; background: color-mix(in srgb, var(--bg-sunken) 65%, var(--bg)); border-right: 1px solid var(--border); overflow-y: auto; }
  :global(.settings-tab) { display: flex; align-items: center; gap: 9px; flex: none; min-height: 36px; padding: 8px 10px; border: 0; border-radius: 6px; background: transparent; color: var(--fg-muted); font: inherit; font-size: var(--text-lg); line-height: 20px; text-align: left; cursor: pointer; }
  :global(.settings-tab svg) { flex: none; }
  :global(.settings-tab:hover) { background: var(--hover); color: var(--fg); }
  :global(.settings-tab[data-state="active"]) { background: var(--bg); color: var(--fg); font-weight: 600; box-shadow: 0 1px 3px color-mix(in srgb, var(--fg) 10%, transparent), 0 0 0 1px var(--border); }
  :global(.settings-tab:focus-visible) { outline: 2px solid var(--accent); outline-offset: -2px; }
  .settings-body { flex: 1; min-width: 0; overflow-y: auto; overscroll-behavior: contain; }
  :global(.settings-content) { padding: 26px 28px; }
  h2 { margin: 0; font-size: 20px; line-height: 28px; font-weight: 600; letter-spacing: -0.3px; }
  .intro { margin: 5px 0 20px; color: var(--fg-muted); font-size: var(--text-lg); line-height: 20px; }
  .setting { display: flex; align-items: center; justify-content: space-between; gap: 24px; min-height: 88px; padding: 18px 0; }
  .setting-copy { flex: 1; min-width: 0; }
  h3 { margin: 0; color: var(--fg); font-size: 14px; line-height: 20px; font-weight: 500; }
  .setting-copy p { margin: 5px 0 0; color: var(--fg-muted); font-size: var(--text-lg); line-height: 20px; }
  .setting :global(.ui-select-trigger) { flex: 0 0 156px; width: 156px; min-height: 32px; border: 1px solid var(--border); border-radius: 6px; background: var(--bg); font-size: var(--text-lg); }
  .setting :global(.ui-button) { flex: none; min-height: 32px; font-size: var(--text-lg); }
  .setting :global(.ui-switch) { width: 34px; height: 20px; border-radius: 10px; }
  .setting :global(.ui-switch-thumb) { width: 16px; height: 16px; }
  .setting :global(.ui-switch-thumb[data-state="checked"]) { transform: translateX(14px); }
  .footnote { margin: 12px 0 0; padding-top: 8px; color: var(--fg-muted); font-size: var(--text-md); line-height: 18px; }
  .build { padding-top: 18px; }
  .build-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  dl { margin: 12px 0 0; }
  dl div { display: flex; justify-content: space-between; gap: 16px; padding: 6px 0; font-size: var(--text-md); line-height: 18px; }
  dt { color: var(--fg-muted); }
  dd { margin: 0; color: var(--fg); font-variant-numeric: tabular-nums; }
  @media (max-width: 600px) {
    :global(.settings-dialog) { width: calc(100vw - 24px); height: min(480px, calc(100dvh - 24px)); }
    :global(.settings-sidebar) { flex-basis: 148px; padding: 16px; }
    :global(.settings-tab) { gap: 7px; padding: 7px 8px; font-size: var(--text-md); }
    :global(.settings-content) { padding: 20px 18px; }
    .setting { align-items: flex-start; flex-direction: column; gap: 12px; min-height: 0; padding: 16px 0; }
    .setting :global(.ui-select-trigger) { flex-basis: auto; width: 100%; }
  }
</style>
