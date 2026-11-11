<script lang="ts">
  // The layers panel (plans/specs/layers-panel.md): one composition's drawing
  // tree, Figma's way. A Compositions section above (Figma's Pages), the
  // layers below as a WAI-ARIA tree over the flattened visible rows the
  // engine's `layers` query returns, only the rows in view mounted. A row's
  // key is its chain, the select tool's, so the session's selection marks it
  // directly. The panel follows the composition view used last, or is
  // pinned to one (its view spec's source).
  import { getContext, tick, type Component } from "svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import EmptyState from "../../lib/ui/EmptyState.svelte";
  import Layers from "@lucide/svelte/icons/layers";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Pin from "@lucide/svelte/icons/pin";
  import Check from "@lucide/svelte/icons/check";
  import Plus from "@lucide/svelte/icons/plus";
  import { DropdownMenu } from "bits-ui";
  import NewCompositionDialog from "../NewCompositionDialog.svelte";
  import ListCollapse from "@lucide/svelte/icons/list-collapse";
  import Shapes from "@lucide/svelte/icons/shapes";
  import SquareDashed from "@lucide/svelte/icons/square-dashed";
  import Square from "@lucide/svelte/icons/square";
  import Circle from "@lucide/svelte/icons/circle";
  import Type from "@lucide/svelte/icons/type";
  import Table from "@lucide/svelte/icons/table";
  import Spline from "@lucide/svelte/icons/spline";
  import Box from "@lucide/svelte/icons/box";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import PictureInPicture2 from "@lucide/svelte/icons/picture-in-picture-2";
  import Video from "@lucide/svelte/icons/video";
  import Locate from "@lucide/svelte/icons/locate";
  import ComponentIcon from "@lucide/svelte/icons/component";
  import Diamond from "@lucide/svelte/icons/diamond";
  import type { ViewSpec } from "../views";
  import { activeView, noteActive } from "../active_view.svelte";
  import { setViewSpec, showComposition, viewShowing, viewSpec } from "../workspace";
  import { showIn } from "../show_in";

  let { spec, panel }: { spec: ViewSpec; panel?: string } = $props();

  type Chip = { composition: string; name: string | null; dims: string; node: string; chain: string };
  type Row = {
    key: string; depth: number; id: string; kind: string; name: string | null; count: number; dims: string;
    embed: [string, string | null] | null; entity: string | null; chips: Chip[];
    /** What is wrong with the node or what it derives, a note a line. */
    problem: string | null;
  };
  type Reply = { ok: boolean; error?: string; composition: string; dims: string; rows: Row[]; total: number };
  type Picked = { node: string; panel: string | null; chain: string | null };

  const ROW = 32;
  const OVERSCAN = 8;
  const uid = Math.random().toString(36).slice(2, 8);

  const doc = controllerManager.docClient;
  const active = getContext<() => boolean>("iron-view-active") ?? (() => true);

  // --- Source: following the last composition view, or pinned ---------------
  // svelte-ignore state_referenced_locally
  let pinned = $state<string | null>(spec.source !== "follow" ? spec.source : null);
  let followed = $state<string | null>(null);
  let followedPanel = $state<string | null>(null);

  const entries = $derived(doc.compositions.filter((c) => !c.session));
  const find = (source: string | null) => (source ? entries.find((c) => c.id === source || c.name === source) : undefined);

  $effect(() => {
    const source = activeView.source;
    const view = activeView.panel;
    if (source && find(source)) {
      followed = source;
      followedPanel = view;
    }
  });

  const current = $derived(find(pinned) ?? find(followed) ?? find("main") ?? entries[0]);

  /** The view a selection made here is recorded against: the followed view,
   *  or the most recent view of the composition. */
  function targetPanel(): string | null {
    if (!current) return null;
    if (!pinned && followedPanel && find(viewSpec(followedPanel)?.source ?? null)?.id === current.id) return followedPanel;
    return viewShowing(current.id);
  }

  function setPinned(on: boolean) {
    pinned = on && current ? current.id : null;
    if (panel) setViewSpec(panel, { source: pinned ?? "follow", notation: "layers" });
  }

  /** Choose a composition: go there (its tab in front), or, pinned, only
   *  show its layers here (layers-panel.md §5). */
  function choose(id: string) {
    if (pinned) {
      pinned = id;
      if (panel) setViewSpec(panel, { source: id, notation: "layers" });
    } else {
      const view = showComposition(id);
      if (view) noteActive(view, id);
    }
  }

  // --- Rows ----------------------------------------------------------------
  /** Open rows' keys, per composition. Panel state, never saved. */
  let opened = $state<Record<string, string[]>>({});
  const openFor = (c: string) => opened[c] ?? [];
  let reply = $state<Reply | null>(null);
  let error = $state("");

  $effect(() => {
    void doc.generation;
    const c = current?.id;
    if (!active() || !c) return;
    const open_json = JSON.stringify([...openFor(c)].sort());
    return doc.subscribe<Reply>(
      { op: "layers", source: c, open_json },
      (r) => {
        if (r.ok) { reply = r; error = ""; } else error = r.error ?? "layers failed";
      },
      (e) => { error = String(e); },
    );
  });

  const rows: Row[] = $derived(reply && reply.composition === current?.id ? reply.rows : []);
  const indexOf = $derived(new Map(rows.map((r, i) => [r.key, i])));

  /** Each row's position among its siblings and their number (aria-posinset,
   *  aria-setsize), since the rows are not nested in the DOM. */
  const position = $derived.by(() => {
    const parentOf = (k: string) => k.slice(0, Math.max(0, k.lastIndexOf(" ")));
    const seen = new Map<string, number>();
    const top = rows.filter((r) => r.depth === 0).length;
    return rows.map((r) => {
      const p = parentOf(r.key);
      const n = (seen.get(p) ?? 0) + 1;
      seen.set(p, n);
      const parent = indexOf.get(p);
      return { pos: n, size: parent === undefined ? top : rows[parent].count };
    });
  });

  function toggle(key: string, open?: boolean) {
    const c = current?.id;
    if (!c) return;
    const set = new Set(openFor(c));
    const want = open ?? !set.has(key);
    if (want) set.add(key);
    else set.delete(key);
    opened = { ...opened, [c]: [...set] };
  }

  function collapseAll() {
    const c = current?.id;
    if (c) opened = { ...opened, [c]: [] };
  }

  // --- The virtual window ----------------------------------------------------
  let tree = $state<HTMLDivElement | undefined>();
  let scrollTop = $state(0);
  let height = $state(0);
  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));
  const mounted = $derived(rows.slice(first, last).map((r, j) => ({ r, i: first + j })));
  const domId = (i: number) => `layers-${uid}-${i}`;

  function ensureVisible(i: number, centre = false) {
    if (!tree) return;
    const top = i * ROW;
    if (centre) tree.scrollTop = Math.max(0, top - (height - ROW) / 2);
    else if (top < tree.scrollTop) tree.scrollTop = top;
    else if (top + ROW > tree.scrollTop + height) tree.scrollTop = top + ROW - height;
    scrollTop = tree.scrollTop;
  }

  // Back to the top when the panel switches composition.
  let shown: string | undefined;
  $effect(() => {
    const c = current?.id;
    if (c !== shown) {
      shown = c;
      if (tree && !pendingReveal) { tree.scrollTop = 0; scrollTop = 0; }
    }
  });

  // --- Selection -------------------------------------------------------------
  let picked = $state<Picked[]>([]);
  const selectedKeys = $derived(new Set(picked.map((p) => p.chain).filter((c): c is string => !!c)));
  const selectedBare = $derived(new Set(picked.filter((p) => !p.chain).map((p) => p.node)));
  /** The row nearest a chain: itself, or its deepest ancestor with a row (a
   *  node inside a shared reference has none until derived parts show,
   *  layers-panel.md phase 3). `ready` is false while rows above it that
   *  can open have not arrived yet. */
  function nearest(chain: string): { key: string | null; ready: boolean } {
    if (indexOf.has(chain)) return { key: chain, ready: true };
    const ids = chain.split(" ");
    for (let n = ids.length - 1; n > 0; n--) {
      const key = ids.slice(0, n).join(" ");
      const i = indexOf.get(key);
      if (i === undefined) continue;
      const r = rows[i];
      const childShown = rows[i + 1]?.depth === r.depth + 1;
      return { key, ready: r.count === 0 || childShown };
    }
    return { key: null, ready: rows.length > 0 };
  }
  /** Rows that hold a selected node with no row of its own. */
  const holding = $derived(new Set(picked.map((p) => (p.chain && !indexOf.has(p.chain) ? nearest(p.chain).key : null)).filter((k): k is string => !!k)));
  const hovered = $derived(new Set(doc.hovered));
  /** The keyboard's row (aria-activedescendant) and the Shift-click anchor. */
  let cursor = $state<string | null>(null);
  let anchor: string | null = null;
  /** The selection this panel asked for last: its echo neither scrolls nor
   *  flashes. And the last selection seen, since the engine re-sends an
   *  unchanged one (a view changing focus), which must not pull the panel
   *  back to the view it was made in. */
  let requested: string | null = null;
  let seen = "";
  const chainsOf = (list: { node: string; chain?: string | null }[]) => list.map((p) => p.chain ?? p.node).join("|");
  let pendingReveal = $state<string | null>(null);
  let flash = $state<string | null>(null);
  let flashTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    void doc.selection;
    let stale = false;
    doc.command<Picked[]>({ op: "selection" }).then((list) => {
      if (!stale) onSelection(list);
    }, () => {});
    return () => { stale = true; };
  });

  function onSelection(list: Picked[]) {
    picked = list;
    const now = chainsOf(list);
    if (now === seen) return;
    seen = now;
    const mine = requested !== null && requested === now;
    requested = null;
    const latest = list[list.length - 1];
    if (!latest?.chain) return;
    // The view the selection was made in is the view used last.
    const viewed = latest.panel ? viewSpec(latest.panel) : undefined;
    if (!pinned && latest.panel && viewed?.notation === "composition" && find(viewed.source)) noteActive(latest.panel, viewed.source);
    const home = viewed?.notation === "composition" ? find(viewed.source)?.id : current?.id;
    if (!home || (pinned && home !== find(pinned)?.id)) return;
    // Open every row above it.
    const ids = latest.chain.split(" ");
    const set = new Set(opened[home] ?? []);
    for (let n = 1; n < ids.length; n++) set.add(ids.slice(0, n).join(" "));
    opened = { ...opened, [home]: [...set] };
    cursor = latest.chain;
    if (!mine) pendingReveal = latest.chain;
  }

  // Scroll a revealed row into view once its rows arrive, and flash it.
  $effect(() => {
    const key = pendingReveal;
    if (!key) return;
    const near = nearest(key);
    if (!near.ready || near.key === null) return;
    const shownKey = near.key;
    const i = indexOf.get(shownKey)!;
    pendingReveal = null;
    if (cursor === key) cursor = shownKey;
    void tick().then(() => {
      const visible = i * ROW >= scrollTop && (i + 1) * ROW <= scrollTop + height;
      if (!visible) ensureVisible(i, true);
      flash = shownKey;
      clearTimeout(flashTimer);
      flashTimer = setTimeout(() => (flash = null), 1100);
    });
  });

  async function select(list: Row[], mode: "replace" | "toggle") {
    const view = targetPanel();
    const items = list.map((r) => ({ node: r.id, panel: view ?? undefined, chain: r.key }));
    // Before sending: the echo may arrive before the reply.
    requested = mode === "replace" ? chainsOf(items) : null;
    const r = await doc.command<{ ok: boolean; error?: string }>({ op: "select", items_json: JSON.stringify(items), mode });
    if (!r.ok) { requested = null; error = r.error ?? "select failed"; }
  }

  function pick(e: MouseEvent, i: number) {
    const r = rows[i];
    const from = anchor === null ? undefined : indexOf.get(anchor);
    if (e.shiftKey && from !== undefined) {
      const [a, b] = from < i ? [from, i] : [i, from];
      void select(rows.slice(a, b + 1), "replace");
    } else if (e.metaKey || e.ctrlKey) {
      anchor = r.key;
      void select([r], "toggle");
    } else {
      anchor = r.key;
      void select([r], "replace");
    }
    cursor = r.key;
    tree?.focus({ preventScroll: true });
  }

  function move(i: number) {
    if (i < 0 || i >= rows.length) return;
    cursor = rows[i].key;
    anchor = cursor;
    ensureVisible(i);
    void select([rows[i]], "replace");
  }

  // Typeahead (Melt's 500 ms): the next visible row whose label starts with
  // what was typed.
  let typed = "";
  let typedTimer: ReturnType<typeof setTimeout> | undefined;
  function typeahead(ch: string) {
    typed += ch.toLowerCase();
    clearTimeout(typedTimer);
    typedTimer = setTimeout(() => (typed = ""), 500);
    const start = cursor === null ? 0 : (indexOf.get(cursor) ?? -1) + (typed.length === 1 ? 1 : 0);
    for (let n = 0; n < rows.length; n++) {
      const i = (start + n) % rows.length;
      if (label(rows[i]).toLowerCase().startsWith(typed)) return move(i);
    }
  }

  function onKey(e: KeyboardEvent) {
    const i = cursor === null ? -1 : (indexOf.get(cursor) ?? -1);
    const r = i >= 0 ? rows[i] : undefined;
    const isOpen = !!r && openFor(current?.id ?? "").includes(r.key);
    switch (e.key) {
      case "ArrowDown": move(i + 1); break;
      case "ArrowUp": move(i < 0 ? 0 : i - 1); break;
      case "Home": move(0); break;
      case "End": move(rows.length - 1); break;
      case "ArrowRight":
        if (!r) move(0);
        else if (r.count && !isOpen) toggle(r.key, true);
        else if (isOpen) move(i + 1);
        break;
      case "ArrowLeft":
        if (r && isOpen) toggle(r.key, false);
        else if (r && r.depth > 0) {
          const parent = indexOf.get(r.key.slice(0, r.key.lastIndexOf(" ")));
          if (parent !== undefined) move(parent);
        }
        break;
      case " ":
      case "Enter":
        if (i >= 0) move(i);
        break;
      default:
        if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) { typeahead(e.key); break; }
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  }

  // --- Hover both ways --------------------------------------------------------
  let hoverId: string | null = null;
  function hover(r: Row) {
    if (r.id === hoverId) return;
    hoverId = r.id;
    void doc.command({ op: "narrative_probe", node: r.id, action: "hover" }).catch(() => {});
  }
  function unhover() {
    if (hoverId === null) return;
    hoverId = null;
    void doc.command({ op: "narrative_probe", node: "", action: "unhover" }).catch(() => {});
  }

  // --- Presentation -----------------------------------------------------------
  const ICONS: Record<string, Component<any>> = {
    group: SquareDashed, rect: Square, ellipse: Circle, text: Type, table: Table, path: Spline, mesh: Box,
    slider: SlidersHorizontal, embed: PictureInPicture2, camera: Video, empty: Locate,
  };
  const iconOf = (r: Row) => (r.kind === "use" ? (r.entity ? ComponentIcon : Diamond) : (ICONS[r.kind] ?? Square));
  const label = (r: Row) => r.name ?? `${r.kind} ${r.id}`;

  // The composition picker (the layout council, 2026-11-09): one row in the
  // tree's header in place of the Compositions list, which repeated the
  // composition tabs and took half the panel. Following, it names the
  // composition in view; picking one goes there (pinned, shows it here).
  let picking = $state(false);
  let naming = $state(false);
</script>

<div class="html-view layers">
  <section class="tree-section">
    <header class="ui-panel-header">
      <DropdownMenu.Root bind:open={picking}>
        <DropdownMenu.Trigger class="ui-quiet-trigger comp-pick" aria-label="Composition: {current?.name ?? 'none'}" title={pinned ? "Pinned to this composition. Choose another to show its layers here" : "Following the composition in view. Choose one to go to it"} data-comp-pick>
          {#if current?.dims === "3d"}<Box size={14} strokeWidth={1.75} />{:else}<Shapes size={14} strokeWidth={1.75} />{/if}
          <span>{current?.name ?? current?.id ?? "No composition"}</span>
          <ChevronDown size={12} strokeWidth={1.75} />
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content class="ui-menu" align="start" sideOffset={4}>
            <DropdownMenu.Group>
            <DropdownMenu.GroupHeading class="ui-menu-heading">{pinned ? "Show here" : "Go to"}</DropdownMenu.GroupHeading>
            {#each entries as c (c.id)}
              <DropdownMenu.Item class="ui-menu-item" onSelect={() => choose(c.id)}>
                <span class="ui-menu-check">{#if c.id === current?.id}<Check size={14} strokeWidth={1.75} />{/if}</span>
                <span>{c.name ?? c.id}</span>
                <span class="ui-hint">{c.dims.toUpperCase()}</span>
              </DropdownMenu.Item>
            {/each}
            </DropdownMenu.Group>
            <DropdownMenu.Separator class="ui-menu-separator" />
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => { picking = false; naming = true; }}><span class="ui-menu-check"><Plus size={14} strokeWidth={1.75} /></span><span>New composition…</span></DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      <span class="ui-spacer"></span>
      <IconButton icon={Pin} label={pinned ? "Unpin: follow the view in use" : "Pin to this composition"} pressed={pinned !== null} onPressedChange={setPinned} />
      <IconButton icon={ListCollapse} label="Collapse all" onclick={collapseAll} />
    </header>
    {#if !current}
      <EmptyState icon={Layers} title="No composition to show">Open a composition to see its layers.</EmptyState>
    {:else}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="tree" role="tree" aria-label="Layers of {current.name ?? current.id}" aria-multiselectable="true" tabindex="0"
        aria-activedescendant={cursor !== null && indexOf.has(cursor) && indexOf.get(cursor)! >= first && indexOf.get(cursor)! < last ? domId(indexOf.get(cursor)!) : undefined}
        bind:this={tree} bind:clientHeight={height} onscroll={() => (scrollTop = tree?.scrollTop ?? 0)}
        onkeydown={onKey} onpointerleave={unhover}>
        <div class="spacer" style:height="{rows.length * ROW}px">
          {#each mounted as { r, i } (r.key)}
            {@const Icon = iconOf(r)}
            {@const open = openFor(current.id).includes(r.key)}
            {@const selected = selectedKeys.has(r.key) || selectedBare.has(r.id)}
            <div class="layer" id={domId(i)} role="treeitem" aria-level={r.depth + 1} aria-setsize={position[i]?.size} aria-posinset={position[i]?.pos}
              aria-expanded={r.count ? open : undefined} aria-selected={selected} tabindex="-1"
              class:selected class:holding={!selected && holding.has(r.key)} class:cursor={cursor === r.key} class:probe={hovered.has(r.id)} class:flash={flash === r.key}
              style:top="{i * ROW}px" style:padding-left="{8 + r.depth * 24}px"
              onclick={(e) => pick(e, i)} onpointerenter={() => hover(r)} onkeydown={() => {}}>
              <span class="twisty" class:open aria-hidden="true"
                onclick={(e) => { e.stopPropagation(); if (r.count) toggle(r.key); }} onkeydown={() => {}} role="presentation">
                {#if r.count}<ChevronRight size={12} strokeWidth={1.75} />{/if}
              </span>
              <span class="icon" class:figure={!!r.entity}><Icon size={14} strokeWidth={1.5} /></span>
              <span class="name">{label(r)}</span>
              {#if r.embed}<span class="tag" title="An embed of {r.embed[1] ?? r.embed[0]}: editing inside it changes that composition">→ {r.embed[1] ?? r.embed[0]}</span>{/if}
              {#if r.count >= 10 && !open}<span class="tag">{r.count}</span>{/if}
              <span class="grow"></span>
              {#if r.problem}<span class="problem" role="img" title={r.problem} aria-label="Problem: {r.problem}"><TriangleAlert size={13} strokeWidth={1.75} /></span>{/if}
              {#each r.chips.slice(0, 2) as chip (chip.composition)}
                <button class="chip" tabindex="-1" title="Show in {chip.name ?? chip.composition}" aria-label="Show in {chip.name ?? chip.composition}"
                  onclick={(e) => { e.stopPropagation(); void showIn({ node: chip.node, composition: chip.composition, chain: chip.chain }); }}>
                  {#if chip.dims === "3d"}<Box size={11} strokeWidth={1.75} />{:else}<Shapes size={11} strokeWidth={1.75} />{/if}
                  <span>{chip.name ?? chip.composition}</span>
                </button>
              {/each}
              {#if r.chips.length > 2}
                <span class="chip more" title={r.chips.slice(2).map((c) => c.name ?? c.composition).join(", ")}>+{r.chips.length - 2}</span>
              {/if}
            </div>
          {/each}
        </div>
      </div>
      <footer>{rows.length.toLocaleString()} rows</footer>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
  </section>
</div>

<NewCompositionDialog bind:open={naming} onCreated={(c) => choose(c.id)} />

<style>
  /* An HTML view (Workspace.svelte's `.html-view` takes the pointer), without its page padding. */
  .layers { display: flex; flex-direction: column; height: 100%; min-height: 0; padding: 0 !important; gap: 0 !important; overflow: hidden !important; font-size: var(--text-md); color: var(--fg); background: var(--bg); }
  section { display: flex; flex-direction: column; min-height: 0; }
  .layers :global(.comp-pick) { font-weight: 600; }
  .layers :global(.comp-pick > svg:first-child) { flex: none; color: var(--fg-muted); }
  .tree-section { flex: 1; }
  .tree { position: relative; flex: 1; min-height: 0; overflow: auto; outline: none; }
  .spacer { position: relative; }
  .layer { position: absolute; left: 8px; right: 8px; height: 32px; display: flex; align-items: center; gap: 6px; padding-right: 6px; border-radius: var(--radius); white-space: nowrap; cursor: default; }
  .layer:hover, .layer.probe { background: var(--hover); }
  .layer.selected { background: var(--accent-soft); }
  .layer.holding { box-shadow: inset 0 0 0 1px var(--accent-soft); }
  .tree:focus-visible .layer.cursor { box-shadow: inset 0 0 0 1px var(--accent); }
  .layer.flash { animation: flash 1.1s ease-out; }
  @keyframes flash { from { box-shadow: inset 0 0 0 2px var(--accent); } to { box-shadow: inset 0 0 0 2px transparent; } }
  .twisty { width: 16px; height: 16px; flex: none; display: inline-flex; align-items: center; justify-content: center; margin-right: -2px; color: var(--fg-faint); border-radius: var(--radius-sm); }
  .twisty :global(svg) { transition: transform .12s; }
  .twisty.open :global(svg) { transform: rotate(90deg); }
  .twisty:hover { color: var(--fg); }
  .icon { display: inline-flex; flex: none; color: var(--fg-faint); }
  .icon.figure { color: var(--accent); }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .grow { flex: 1; }
  .tag { flex: none; font-size: var(--text-sm); color: var(--fg-faint); }
  .chip { display: inline-flex; align-items: center; gap: 3px; flex: none; max-width: 96px; height: 20px; padding: 0 6px 0 5px; border: 1px solid var(--border-strong); border-radius: 10px; background: var(--bg); color: var(--fg-muted); font: inherit; font-size: var(--text-sm); cursor: pointer; }
  .chip span { overflow: hidden; text-overflow: ellipsis; }
  .chip:hover { border-color: var(--accent); color: var(--fg); }
  .chip.more { cursor: default; }
  .problem { display: inline-flex; flex: none; color: var(--danger); }
  footer { flex: none; padding: 6px 16px; border-top: 1px solid var(--border); font-size: var(--text-xs); color: var(--fg-faint); }
  .error { padding: 12px var(--panel-pad); color: var(--danger); overflow-wrap: anywhere; }
</style>
