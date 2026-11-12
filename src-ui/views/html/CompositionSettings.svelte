<script lang="ts">
  // Composition settings (plans/specs/free-running.md §3): a button in the
  // tab's top right corner. Run: Auto (free running while the composition
  // or anything it shows is live, as the engine reports), Always, or On
  // demand (never; what it shows does not bubble out, and producers seen
  // only through it pause). In developer mode, the render inspector toggle
  // (plans/specs/render-inspector.md §8), which splits the tab. "Show FPS"
  // turns on the view's frame-rate readout, in its lower right corner.
  import { DropdownMenu } from "bits-ui";
  import Check from "@lucide/svelte/icons/check";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import { onDestroy } from "svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  import { settings } from "../../runtime/settings.svelte";
  import type { ViewSpec } from "../views";

  let { spec, inspecting, onInspect, showFps, onShowFps }: { spec: ViewSpec; inspecting: boolean; onInspect: () => void; showFps: boolean; onShowFps: () => void } = $props();

  const doc = controllerManager.docClient;

  type Run = "auto" | "always" | "on_demand";
  type Info = { ok: boolean; run: Run; live: boolean; running: string[] };
  let info = $state<Info | null>(null);
  let open = $state(false);

  const MODES: { id: Run; label: string; hint: string }[] = [
    { id: "auto", label: "Auto", hint: "Runs while something in it is live" },
    { id: "always", label: "Always", hint: "Renders every frame" },
    { id: "on_demand", label: "On demand", hint: "Renders on input; live parts pause" },
  ];

  let queryGeneration = 0;
  async function refresh() {
    const request = ++queryGeneration, generation = doc.generation, source = spec.source;
    try {
      const r: Info = await doc.command({ op: 'composition_info', source: spec.source });
      if (request === queryGeneration && generation === doc.generation && source === spec.source) info = r?.ok ? r : null;
    } catch {
      if (request === queryGeneration && generation === doc.generation) info = null;
    }
  }

  // Liveness changes without a document version (a producer starts or is
  // frozen), so re-read while the menu is open as well as on each version.
  $effect(() => {
    void doc.generation;
    void spec.source;
    refresh();
  });
  // Also until the first answer: a view mounted before the engine is ready
  // gets none, and the button would never appear.
  const poll = setInterval(() => {
    if (open || !info) refresh();
  }, 500);
  onDestroy(() => { queryGeneration++; clearInterval(poll); });

  async function set(run: Run) {
    await doc.command({ op: 'set_run_mode', source: spec.source, mode: run });
    await refresh();
  }

  const status = $derived(
    !info ? "" : info.live ? (info.running.length ? `Running: ${info.running.join(", ")}` : "Running") : "Frozen until input",
  );
</script>

{#if info}
  <div class="settings canvas-ui" class:below-ruler={spec.source === "timeline"}>
    <DropdownMenu.Root bind:open onOpenChange={(o) => o && refresh()}>
      <DropdownMenu.Trigger class="ui-icon-button large trigger" title="Composition settings" aria-label="Composition settings">
        <Settings2 size={16} strokeWidth={1.75} />
        {#if info.live}<span class="live" aria-hidden="true"></span>{/if}
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content class="ui-menu" side="bottom" sideOffset={6} align="end">
          <div class="ui-menu-heading">Run</div>
          {#each MODES as m (m.id)}
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => set(m.id)} data-run={m.id} title={m.hint}>
              <span class="ui-menu-check">{#if info.run === m.id}<Check size={14} strokeWidth={1.75} />{/if}</span>
              <span>{m.label}</span>
            </DropdownMenu.Item>
          {/each}
          <div class="ui-menu-note" data-status>{status}</div>
          <div class="ui-menu-separator"></div>
          <DropdownMenu.Item class="ui-menu-item" onSelect={onShowFps} data-show-fps title="Frames this view renders per second">
            <span class="ui-menu-check">{#if showFps}<Check size={14} strokeWidth={1.75} />{/if}</span>
            <span>Show FPS</span>
          </DropdownMenu.Item>
          {#if settings.developer}
            <div class="ui-menu-separator"></div>
            <div class="ui-menu-heading">Developer</div>
            <DropdownMenu.Item class="ui-menu-item" onSelect={onInspect} data-inspector title="Chunks, culling and tiles beside the view">
              <span class="ui-menu-check">{#if inspecting}<Check size={14} strokeWidth={1.75} />{/if}</span>
              <span>Render inspector</span>
            </DropdownMenu.Item>
          {/if}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  </div>
{/if}

<style>
  .settings {
    position: absolute;
    top: 8px;
    right: 8px;
  }
  /* The timeline's ruler runs the full width at the top, labels and all:
     its button sits just under the ruler (TimelinePanel's 56 px). */
  .settings.below-ruler {
    top: 64px;
  }
  /* A 28 px icon button (`.ui-icon-button.large`) floating over the canvas:
     raised, with the menus' border and shadow so it reads on any picture. */
  .settings :global(.trigger) {
    position: relative;
    border: 1px solid var(--border-strong);
    background: var(--raised);
    box-shadow: var(--menu-shadow);
  }
  .settings :global(.trigger:hover:not(:disabled)) {
    background: var(--raised);
  }
  .live {
    position: absolute;
    top: 3px;
    right: 3px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }
</style>
