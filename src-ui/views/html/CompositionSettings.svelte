<script lang="ts">
  // Composition settings (plans/specs/free-running.md §3): a button in the
  // tab's top right corner. Run: Auto (free running while the composition
  // or anything it shows is live, as the engine reports), Always, or On
  // demand (never; what it shows does not bubble out, and producers seen
  // only through it pause). In developer mode, the render inspector toggle
  // (plans/specs/render-inspector.md §8), which splits the tab. "Show FPS"
  // turns on the view's frame-rate readout, in its lower right corner. For a
  // document's 2D composition, its grid: off, lines or dots, the spacing,
  // and whether moves, resizes and new shapes snap to it (the composition's
  // `grid`, `grid_spacing` and `grid_snap`, edited as in the inspector's
  // Composition section, one `inspect_patch` transaction each).
  import { DropdownMenu } from "bits-ui";
  import Check from "@lucide/svelte/icons/check";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
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

  // ── Grid ──────────────────────────────────────────────────────────────
  type Grid = "off" | "lines" | "dots";
  const GRIDS: { id: Grid; label: string }[] = [
    { id: "off", label: "Off" },
    { id: "lines", label: "Lines" },
    { id: "dots", label: "Dots" },
  ];
  const SPACINGS = [4, 8, 10, 16, 20, 24, 32, 40, 50, 64, 100];
  const comp = $derived(doc.compositions.find((c) => !c.session && c.dims === "2d" && (c.id === spec.source || c.name === spec.source)));
  type Field = { field: string; text: string | null };
  let grid = $state<{ node: string; mode: Grid; spacing: number; snap: boolean } | null>(null);
  async function loadGrid() {
    const node = comp?.id;
    if (!node) { grid = null; return; }
    try {
      const r = await doc.command<{ components: { kind: string; fields: Field[] }[] } | null>({ op: "inspect", node });
      const fields = r?.components?.find((c) => c.kind === "composition")?.fields ?? [];
      const text = (f: string) => fields.find((x) => x.field === f)?.text ?? "";
      if (node === comp?.id) grid = { node, mode: (text("grid") || "off") as Grid, spacing: Number(text("grid_spacing")) || 20, snap: text("grid_snap") === "true" };
    } catch { grid = null; }
  }
  $effect(() => {
    void doc.generation;
    void comp?.id;
    if (open) void loadGrid();
  });
  async function setGrid(leaf: "grid" | "grid_spacing" | "grid_snap", value: string) {
    if (!grid) return;
    await doc.command({ op: "inspect_patch", node: grid.node, patch: JSON.stringify({ values: { [`composition.${leaf}`]: value }, label: "edit composition settings" }) });
    await loadGrid();
  }

  const status = $derived(
    !info ? "" : info.live ? (info.running.length ? `Running: ${info.running.join(", ")}` : "Running") : "Frozen until input",
  );
</script>

{#if info}
  <div class="settings canvas-ui" class:below-ruler={spec.source === "timeline"}>
    <DropdownMenu.Root bind:open onOpenChange={(o) => o && refresh()}>
      <DropdownMenu.Trigger class="property-icon large trigger" title="Composition settings" aria-label="Composition settings">
        <Settings2 size={16} strokeWidth={1.75} />
        {#if info.live}<span class="live" aria-hidden="true"></span>{/if}
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content class="ui-menu" side="bottom" sideOffset={6} align="end">
          <div class="ui-menu-heading">Run</div>
          {#each MODES as m (m.id)}
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => set(m.id)} data-run={m.id}>
              <span class="ui-menu-check">{#if info.run === m.id}<Check size={14} strokeWidth={1.75} />{/if}</span>
              <span>{m.label}</span>
              <span class="ui-hint">{m.hint}</span>
            </DropdownMenu.Item>
          {/each}
          <div class="ui-menu-separator"></div>
          <div class="ui-menu-heading status" data-status>{status}</div>
          {#if grid}
            <div class="ui-menu-separator"></div>
            <div class="ui-menu-heading">Grid</div>
            {#each GRIDS as g (g.id)}
              <DropdownMenu.Item class="ui-menu-item" closeOnSelect={false} onSelect={() => setGrid("grid", g.id)} data-grid={g.id}>
                <span class="ui-menu-check">{#if grid.mode === g.id}<Check size={14} strokeWidth={1.75} />{/if}</span>
                <span>{g.label}</span>
              </DropdownMenu.Item>
            {/each}
            <DropdownMenu.Sub>
              <DropdownMenu.SubTrigger class="ui-menu-item" disabled={grid.mode === "off"} data-grid-spacing>
                <span class="ui-menu-check"></span>
                <span>Spacing</span>
                <span class="ui-hint">{grid.spacing} px</span>
                <ChevronRight class="ui-submenu-chevron" size={14} strokeWidth={1.75} />
              </DropdownMenu.SubTrigger>
              <DropdownMenu.SubContent class="ui-menu" sideOffset={4}>
                {#each SPACINGS as s (s)}
                  <DropdownMenu.Item class="ui-menu-item" onSelect={() => setGrid("grid_spacing", String(s))} data-spacing={s}>
                    <span class="ui-menu-check">{#if grid.spacing === s}<Check size={14} strokeWidth={1.75} />{/if}</span>
                    <span>{s} px</span>
                  </DropdownMenu.Item>
                {/each}
                {#if !SPACINGS.includes(grid.spacing)}
                  <DropdownMenu.Item class="ui-menu-item" disabled>
                    <span class="ui-menu-check"><Check size={14} strokeWidth={1.75} /></span>
                    <span>{grid.spacing} px</span>
                    <span class="ui-hint">set in the inspector</span>
                  </DropdownMenu.Item>
                {/if}
              </DropdownMenu.SubContent>
            </DropdownMenu.Sub>
            <DropdownMenu.Item class="ui-menu-item" closeOnSelect={false} disabled={grid.mode === "off"} onSelect={() => setGrid("grid_snap", String(!grid!.snap))} data-grid-snap>
              <span class="ui-menu-check">{#if grid.snap}<Check size={14} strokeWidth={1.75} />{/if}</span>
              <span>Snap to grid</span>
              <span class="ui-hint">Ctrl or Cmd places freely</span>
            </DropdownMenu.Item>
          {/if}
          <div class="ui-menu-separator"></div>
          <DropdownMenu.Item class="ui-menu-item" onSelect={onShowFps} data-show-fps>
            <span class="ui-menu-check">{#if showFps}<Check size={14} strokeWidth={1.75} />{/if}</span>
            <span>Show FPS</span>
            <span class="ui-hint">Frames this view renders per second</span>
          </DropdownMenu.Item>
          {#if settings.developer}
            <div class="ui-menu-separator"></div>
            <div class="ui-menu-heading">Developer</div>
            <DropdownMenu.Item class="ui-menu-item" onSelect={onInspect} data-inspector>
              <span class="ui-menu-check">{#if inspecting}<Check size={14} strokeWidth={1.75} />{/if}</span>
              <span>Render inspector</span>
              <span class="ui-hint">Chunks, culling and tiles beside the view</span>
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
  /* A 28 px icon button (`.property-icon.large`) floating over the canvas:
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
  .status {
    font-weight: 400;
  }
</style>
