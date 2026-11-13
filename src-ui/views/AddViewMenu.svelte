<script lang="ts">
  // "+" in a tab group's header: open a view here. A composition (a
  // submenu, so a long document keeps the menu short) or a new one (named in
  // a dialog), then narratives and the views that are not compositions, then
  // the developer's: Latency and the render benchmark's stress scene at a
  // few sizes (a submenu; src/document_bridge/stress.rs).
  import type { DockviewApi, DockviewGroupPanel } from "dockview-core";
  import { DropdownMenu } from "bits-ui";
  import Plus from "@lucide/svelte/icons/plus";
  import PanelTopClose from "@lucide/svelte/icons/panel-top-close";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import IconButton from "../lib/ui/IconButton.svelte";
  import NewCompositionDialog from "./NewCompositionDialog.svelte";
  import { addCompositionView, setTabBarHidden } from "./workspace";
  import type { CompositionEntry } from "../runtime/document_client.svelte";
  import { controllerManager } from "../controller-manager.svelte";
  import { FIXED_VIEWS, LATENCY_VIEW, titleOf, viewId, type ViewSpec } from "./views";
  import { settings as appSettings } from "../runtime/settings.svelte";

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
      // The Latency view's composition is offered as that view (Developer).
      const listed = list.filter((c) => !(c.session && c.name === "latency"));
      compositions = [...listed.filter((c) => !c.session), ...listed.filter((c) => c.session)];
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
  <IconButton icon={PanelTopClose} size={16} large label="Hide tab bar" title="Hide the tab bar: the views keep a grab handle to switch, drag and show it again" onclick={() => setTabBarHidden(group.id, true)} />
  <DropdownMenu.Root bind:open onOpenChange={opened}>
    <DropdownMenu.Trigger class="ui-icon-button large" title="Open a view" aria-label="Open a view">
      <Plus size={16} strokeWidth={1.75} />
    </DropdownMenu.Trigger>
    <DropdownMenu.Portal>
      <DropdownMenu.Content class="ui-menu" align="end" sideOffset={4}>
        <DropdownMenu.Sub>
          <DropdownMenu.SubTrigger class="ui-menu-item"><span>Open composition</span><ChevronRight class="ui-submenu-chevron" size={14} strokeWidth={1.75} /></DropdownMenu.SubTrigger>
          <DropdownMenu.SubContent class="ui-menu" sideOffset={4}>
            {#each compositions as c (c.id)}
              <DropdownMenu.Item class="ui-menu-item" onSelect={() => show(c)}>
                <span>{titleOf(c.name ?? c.id)}</span><span class="ui-hint">{c.session ? "session" : c.dims}</span>
              </DropdownMenu.Item>
            {/each}
          </DropdownMenu.SubContent>
        </DropdownMenu.Sub>
        <DropdownMenu.Item class="ui-menu-item" onSelect={() => { open = false; naming = true; }}>New composition…</DropdownMenu.Item>
        <DropdownMenu.Separator class="ui-menu-separator" />
        <DropdownMenu.Group>
          <DropdownMenu.GroupHeading class="ui-menu-heading">Narratives</DropdownMenu.GroupHeading>
          {#each narratives as n (n.id)}
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => add(titleOf(n.name || n.id), { source: n.name || n.id, notation: "narrative" })}>{titleOf(n.name || n.id)}</DropdownMenu.Item>
          {/each}
          <DropdownMenu.Item class="ui-menu-item" onSelect={() => add("Notes", { source: "notes", notation: "narrative" })}>New narrative…</DropdownMenu.Item>
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
          <DropdownMenu.GroupHeading class="ui-menu-heading">Developer</DropdownMenu.GroupHeading>
          {#if appSettings.developer}
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => add("Latency", LATENCY_VIEW)} title="Input-to-screen latency">Latency</DropdownMenu.Item>
          {/if}
          <DropdownMenu.Sub>
            <DropdownMenu.SubTrigger class="ui-menu-item"><span>Stress test</span><ChevronRight class="ui-submenu-chevron" size={14} strokeWidth={1.75} /></DropdownMenu.SubTrigger>
            <DropdownMenu.SubContent class="ui-menu" sideOffset={4}>
              {#each STRESS_SIZES as n (n)}
                <DropdownMenu.Item class="ui-menu-item" onSelect={() => stress(n)}>{n.toLocaleString()} shapes</DropdownMenu.Item>
              {/each}
            </DropdownMenu.SubContent>
          </DropdownMenu.Sub>
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
    padding-right: 4px;
    pointer-events: auto;
  }
</style>
