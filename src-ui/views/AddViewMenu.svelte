<script lang="ts">
  // "+" in a tab group's header: open a view here. Every composition in the
  // document, or a new one (named in a dialog), then the views that are not
  // compositions, then the render benchmark's stress scene at a few sizes
  // (src/document_bridge/stress.rs).
  import type { DockviewApi, DockviewGroupPanel } from "dockview-core";
  import { DropdownMenu } from "bits-ui";
  import Plus from "@lucide/svelte/icons/plus";
  import Settings from "@lucide/svelte/icons/settings";
  import NewCompositionDialog from "./NewCompositionDialog.svelte";
  import { addCompositionView } from "./workspace";
  import type { CompositionEntry } from "../runtime/document_client.svelte";
  import { controllerManager } from "../controller-manager.svelte";
  import { FIXED_VIEWS, SETTINGS_VIEW, titleOf, viewId, type ViewSpec } from "./views";

  let { api, group }: { api: DockviewApi; group: DockviewGroupPanel } = $props();

  let open = $state(false);
  let compositions: CompositionEntry[] = $state([]);
  let naming = $state(false);
  let narratives: { id: string; name: string }[] = $state([]);

  async function opened(now: boolean) {
    if (!now) return;
    try {
      // The document's first, then the session's (timeline, library).
      const list = await controllerManager.docClient.refreshCompositions();
      compositions = [...list.filter((c) => !c.session), ...list.filter((c) => c.session)];
    } catch {
      compositions = [];
    }
    try {
      const r = await controllerManager.docClient.command<{ ok: boolean; narratives: { id: string; name: string }[] }>({ op: 'narrative_list' });
      narratives = r.ok ? r.narratives : [];
    } catch {
      narratives = [];
    }
  }

  function add(title: string, spec: ViewSpec) {
    open = false;
    const id = viewId(spec, (candidate) => !!api.getPanel(candidate));
    api.addPanel({ id, component: "view", title, params: { spec }, position: { referenceGroup: group } });
  }

  /** A view of composition `c` in this group, in front. */
  function show(c: CompositionEntry) {
    open = false;
    addCompositionView(api, c, { referenceGroup: group });
  }

  /** The gear: show the one Settings view, or open it here. */
  function settings() {
    open = false;
    const existing = api.panels.find((p) => (p.params as any)?.spec?.notation === "settings");
    if (existing) existing.api.setActive();
    else add("Settings", SETTINGS_VIEW);
  }

  const STRESS_SIZES = [1000, 2000, 5000, 10000, 20000];

  /** Build (or rebuild at a new size) the `stress` composition, then show
   *  its tab: the one already open, or a new one here. */
  async function stress(shapes: number) {
    open = false;
    await controllerManager.docClient.command({ op: 'stress', shapes: shapes });
    const existing = api.getPanel("stress");
    if (existing) {
      existing.api.setActive();
    } else {
      api.addPanel({ id: "stress", component: "view", title: "Stress", params: { spec: { source: "stress", notation: "composition" } }, position: { referenceGroup: group } });
    }
  }
</script>

<div class="add">
  <button class="action" title="Settings" aria-label="Settings" onclick={settings}>
    <Settings size={15} strokeWidth={1.75} />
  </button>
  <DropdownMenu.Root bind:open onOpenChange={opened}>
    <DropdownMenu.Trigger class="action" title="Open a view" aria-label="Open a view">
      <Plus size={16} strokeWidth={1.75} />
    </DropdownMenu.Trigger>
    <DropdownMenu.Portal>
      <DropdownMenu.Content class="ui-menu" align="end" sideOffset={4}>
        <DropdownMenu.Group>
          <DropdownMenu.GroupHeading class="ui-menu-heading">Compositions</DropdownMenu.GroupHeading>
          {#each compositions as c (c.id)}
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => show(c)}>
              {titleOf(c.name ?? c.id)} <span class="ui-hint">{c.session ? "session" : c.dims}</span>
            </DropdownMenu.Item>
          {/each}
          <DropdownMenu.Item class="ui-menu-item" onSelect={() => { open = false; naming = true; }}>New composition…</DropdownMenu.Item>
        </DropdownMenu.Group>
        <DropdownMenu.Separator class="ui-menu-separator" />
        <DropdownMenu.Group>
          <DropdownMenu.GroupHeading class="ui-menu-heading">Narratives</DropdownMenu.GroupHeading>
          {#each narratives as n (n.id)}
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => add(titleOf(n.name || n.id), { source: n.name || n.id, notation: "narrative" })}>{titleOf(n.name || n.id)}</DropdownMenu.Item>
          {/each}
          <DropdownMenu.Item class="ui-menu-item" onSelect={() => add("Notes", { source: "notes", notation: "narrative" })}>New narrative… <span class="ui-hint">notes</span></DropdownMenu.Item>
        </DropdownMenu.Group>
        <DropdownMenu.Separator class="ui-menu-separator" />
        <DropdownMenu.Group>
          <DropdownMenu.GroupHeading class="ui-menu-heading">Views</DropdownMenu.GroupHeading>
          {#each FIXED_VIEWS as v (v.title)}
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => add(v.title, v.spec)}>{v.title}</DropdownMenu.Item>
          {/each}
        </DropdownMenu.Group>
        <DropdownMenu.Separator class="ui-menu-separator" />
        <DropdownMenu.Group>
          <DropdownMenu.GroupHeading class="ui-menu-heading">Stress test</DropdownMenu.GroupHeading>
          {#each STRESS_SIZES as n (n)}
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => stress(n)}>{n.toLocaleString()} shapes</DropdownMenu.Item>
          {/each}
        </DropdownMenu.Group>
      </DropdownMenu.Content>
    </DropdownMenu.Portal>
  </DropdownMenu.Root>
  <NewCompositionDialog bind:open={naming} onCreated={(c) => addCompositionView(api, c, { referenceGroup: group })} />
</div>

<style>
  .add {
    height: 100%;
    display: flex;
    align-items: center;
    gap: 2px;
    padding-right: 6px;
    pointer-events: auto;
  }
  .add :global(.action) {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border: none;
    border-radius: 5px;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
  }
  .add :global(.action:hover),
  .add :global(.action[data-state="open"]) {
    background: var(--hover);
    color: var(--fg);
  }
</style>
