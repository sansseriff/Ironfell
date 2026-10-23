<script lang="ts">
  // "+" in a tab group's header: open a view here. Every composition in the
  // document, then the views that are not compositions, then the render
  // benchmark's stress scene at a few sizes (src/document_bridge/stress.rs).
  import type { DockviewApi, DockviewGroupPanel } from "dockview-core";
  import { DropdownMenu } from "bits-ui";
  import Plus from "@lucide/svelte/icons/plus";
  import Settings from "@lucide/svelte/icons/settings";
  import { controllerManager } from "../controller-manager.svelte";
  import { FIXED_VIEWS, SETTINGS_VIEW, titleOf, viewId, type ViewSpec } from "./views";

  let { api, group }: { api: DockviewApi; group: DockviewGroupPanel } = $props();

  let open = $state(false);
  let compositions: { id: string; name: string | null; dims: string }[] = $state([]);

  async function opened(now: boolean) {
    if (!now) return;
    listWatchable();
    try {
      compositions = await controllerManager.docClient.call("compositions", []);
    } catch {
      compositions = [];
    }
  }

  function add(title: string, spec: ViewSpec) {
    open = false;
    const id = viewId(spec, (candidate) => !!api.getPanel(candidate));
    api.addPanel({ id, component: "view", title, params: { spec }, position: { referenceGroup: group } });
  }

  /** The gear: show the one Settings view, or open it here. */
  function settings() {
    open = false;
    const existing = api.panels.find((p) => (p.params as any)?.spec?.notation === "settings");
    if (existing) existing.api.setActive();
    else add("Settings", SETTINGS_VIEW);
  }

  const STRESS_SIZES = [1000, 2000, 5000, 10000, 20000];

  /** The open composition views a render inspector can watch. */
  let watchable: { id: string; title: string }[] = $state([]);
  function listWatchable() {
    watchable = api.panels
      .filter((p) => (p.params as any)?.spec?.notation === "composition")
      .map((p) => ({ id: p.id, title: p.title ?? p.id }));
  }

  /** Build (or rebuild at a new size) the `stress` composition, then show
   *  its tab: the one already open, or a new one here. */
  async function stress(shapes: number) {
    open = false;
    await controllerManager.docClient.call("stress", [shapes]);
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
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => add(titleOf(c.name ?? c.id), { source: c.name ?? c.id, notation: "composition" })}>
              {titleOf(c.name ?? c.id)} <span class="ui-hint">{c.dims}</span>
            </DropdownMenu.Item>
          {/each}
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
          <DropdownMenu.GroupHeading class="ui-menu-heading">Render inspector</DropdownMenu.GroupHeading>
          {#each watchable as w (w.id)}
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => add(`Render: ${w.title}`, { source: w.id, notation: "render-inspector" })}>Watch {w.title}</DropdownMenu.Item>
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
