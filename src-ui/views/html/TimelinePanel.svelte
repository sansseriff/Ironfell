<script lang="ts">
  // The timeline's DOM half (plans/specs/timeline.md §12.3): its left column,
  // over the canvas composition's header backdrop. The canvas draws what needs
  // the renderer (ruler, bars, keys, playhead); this draws what DOM does
  // better: the transport in the corner over the ruler, then a row per
  // animated object and per property, with names, error marks and each
  // property's ‹ ◇ › (previous key, add or remove a key at the playhead, next
  // key). Both halves come from one plan: the engine serves the rows
  // (`timeline_rows`) with the canvas view's pan, and the column's wheel
  // scrolls its rows (`timeline_axis` `scroll`, the axis's `y0`), so they
  // stay aligned; the ruler never moves.
  import { onDestroy } from "svelte";
  import { Button as BitsButton } from "bits-ui";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Diamond from "@lucide/svelte/icons/diamond";
  import Plus from "@lucide/svelte/icons/plus";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { controllerManager } from "../../controller-manager.svelte";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import TimelineTransport from "./TimelineTransport.svelte";
  import StateEditor from "./StateEditor.svelte";
  import { machines } from "../../lib/machines.svelte";

  let { panel }: { panel: string } = $props();
  const doc = controllerManager.docClient;

  type Row = { kind: "object" | "property"; label: string; y: number; target: string; prop: string | null; clip: string | null; error: boolean; keyed: boolean; lane?: boolean; state?: { state: string | null; running: { name: string | null; progress: number; from?: string; to?: string } | null } | null; tween?: string | null; bars?: string[]; open?: boolean | null };
  /** Inside a transition (machines.md §6): the same timeline at a second
   *  level, under the state editor's header and beside its navigator. */
  type Inside = { from: string; to: string; own: boolean; secs: number; transition: string | null; cursor: number };
  type Rows = { ok: boolean; rows: Row[]; y0: number; zoom: number; label_w: number; ruler_h: number; row_h: number; top: number; inset: number; inside: Inside | null };
  let layout: Rows = $state({ ok: false, rows: [], y0: 0, zoom: 1, label_w: 160, ruler_h: 56, row_h: 22, top: 0, inset: 0, inside: null });
  let root: HTMLDivElement | undefined = $state();

  // Document px to this column's CSS px: the canvas view's zoom is CSS px
  // per document px (the engine applies the display's pixel ratio), so no
  // `devicePixelRatio` here.
  const css = (px: number) => px * layout.zoom;
  const top = (y: number) => (y - layout.y0) * layout.zoom;
  // Inside a transition: a row's slice values and selection (its bars).
  const inside = $derived(layout.inside);
  const view = $derived(machines.view);
  const channel = (name: string | null) => view?.channels.find((c) => c.name === name);
  const machineOf = $derived(view?.machines.find((m) => m.id === machines.pinned?.machine) ?? view?.machines[0]);
  const shown = (v: number | string | null | undefined) => (v === null || v === undefined ? "–" : typeof v === "number" ? String(Math.round(v * 100) / 100) : v.startsWith("#") ? v.slice(0, 7) : `“${v}”`);
  const valueIn = (state: string, c: string | null) => machineOf?.states.find((s) => s.name === state)?.pose[c ?? ""];
  const picked = $derived(new Set(doc.selection));
  const rowPicked = (row: Row) => !!row.bars?.length && row.bars.every((b) => picked.has(b));
  /** A row's name selects its bars (a part's: all of them), Shift adds. */
  function pickBars(row: Row, e: MouseEvent) {
    if (!row.bars?.length) return;
    const add = e.shiftKey || e.metaKey;
    const nodes = add ? [...new Set([...doc.selection, ...row.bars])] : row.bars;
    void doc.call("select", [JSON.stringify(nodes.map((node) => ({ node }))), "replace"]).catch(() => {});
  }
  // A property that follows the transition (added after its timing was
  // made) gets a slice of its own.
  function giveSlice(row: Row) {
    const pin = machines.pinned;
    if (!inside || !pin || !view || !row.prop) return;
    void machines.editPair(view.def, pin.machine, inside.from, inside.to, { slices: [{ channel: row.prop }] }, "slice");
  }

  let pending = false;
  async function read() {
    if (pending) return;
    pending = true;
    try {
      const r = await doc.command<Rows>({ op: "timeline_rows", panel });
      if (r.ok) layout = r;
    } catch {
      // No engine session yet.
    } finally {
      pending = false;
    }
  }

  /** The timeline view's box: the overlay this sits in covers it. */
  const area = () => root?.parentElement?.getBoundingClientRect();
  let over = false;
  const onMove = (e: PointerEvent) => {
    const r = area();
    over = !!r && e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom;
  };
  // Polled: the canvas scrolls and the plan changes without telling the
  // shell. Every frame while the pointer is over the timeline, so the rows
  // follow a canvas scroll; a few times a second otherwise.
  let frame = 0;
  const loop = () => {
    if (over) void read();
    frame = requestAnimationFrame(loop);
  };
  frame = requestAnimationFrame(loop);
  const timer = setInterval(() => !over && read(), 250);
  read();
  window.addEventListener("pointermove", onMove, true);
  onDestroy(() => {
    cancelAnimationFrame(frame);
    clearInterval(timer);
    window.removeEventListener("pointermove", onMove, true);
  });

  // The column's wheel scrolls the rows of both halves (the ruler stays).
  function wheel(e: WheelEvent) {
    e.preventDefault();
    const dy = e.deltaY / layout.zoom;
    void doc.command<{ ok: boolean; y0: number }>({ op: "timeline_axis", fields: JSON.stringify({ scroll: dy }) }).then(r => {
      if (r.ok) layout = { ...layout, y0: r.y0 };
    });
  }

  const key = (action: "prev" | "next" | "toggle", clip: string | null) => clip && doc.command({ op: "timeline_key", action, clip }).then(read);
  // An unkeyed state lane's ◇ starts animating the state at the playhead.
  const keyRow = (row: Row) => (row.clip ? key("toggle", row.clip) : row.prop && doc.command({ op: "inspect_animate", node: row.target, leaf: row.prop, action: "key" }).then(read));

  // The panel's two levels (machines.md §6): the document's clock, and,
  // drilled in, one component's states (StateEditor). The editor is a mode
  // entered on purpose; selection never switches it.
  const stopMachines = machines.use();
  onDestroy(stopMachines);
  // A double-click on a state lane's stretch or a transition band opens it
  // (the pieces' context says `affords.enter="open"`).
  let lastOpen = 0;
  $effect(() => {
    const o = doc.opened;
    if (!o || o.at === lastOpen) return;
    lastOpen = o.at;
    const part = o.name.startsWith("band ") ? "band" : o.name.startsWith("lane ") || o.name.startsWith("key ") ? "lane" : null;
    if (part && o.standsFor) void machines.openKey(o.standsFor, part);
  });
  // A lane's readout opens its component's editor.
  async function openLane(entity: string, name: string) {
    const r = await doc.command<{ ok: boolean; view: { def: string } | null }>({ op: "machine_view", node: entity });
    if (r.ok && r.view) void machines.open(r.view.def, undefined, undefined, name);
  }
  // A name selects its object, and hovering it lights the object in every view.
  const probe = (node: string, action: "hover" | "unhover" | "select") => doc.command({ op: "narrative_probe", node, action });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="timeline-column canvas-ui" class:inside={!!inside} bind:this={root} style:left="{css(layout.inset)}px" style:top="{css(layout.top)}px" style:width="{css(layout.label_w)}px" onwheel={wheel} data-timeline-column>
  <div class="rows" style:top="{css(layout.ruler_h)}px">
    {#each layout.rows as row (row.kind + row.target + (row.prop ?? ""))}
      <div class="row {row.kind}" class:error={row.error} class:picked={inside && rowPicked(row)} style:top="{top(row.y) - css(layout.top + layout.ruler_h)}px" style:height="{css(layout.row_h)}px" data-row={row.kind} data-target={row.target} data-prop={row.prop}>
        {#if inside}
          <!-- Inside a transition: a part, then its properties; a name
               selects the bars, hovering lights the part on the canvas. -->
          {#if row.kind === "object"}
            <IconButton icon={row.open ? ChevronDown : ChevronRight} size={12} class="is-row disclose" label="{row.open ? 'Fold' : 'Unfold'} {row.label}" title={row.open ? "Fold: one row for the part (its bar moves all its slices)" : "Unfold: a row per property"} onclick={() => doc.command({ op: "timeline_axis", fields: JSON.stringify({ open: row.target, on: !row.open }) }).then(read)} />
            <BitsButton.Root class="name" title={row.bars?.length ? `Select ${row.label}'s slices (Shift adds)` : row.label} onclick={(e: MouseEvent) => pickBars(row, e)} onpointerenter={() => probe(row.target, "hover")} onpointerleave={() => probe("", "unhover")}><span>{row.label}</span></BitsButton.Root>
          {:else}
            {@const c = channel(row.prop)}
            <button class="prop pick" title={row.bars?.length ? `Select ${row.label}'s slice (Shift adds)` : row.label} onclick={(e) => pickBars(row, e)} onpointerenter={() => probe(row.target, "hover")} onpointerleave={() => probe("", "unhover")}>{row.label}</button>
            <span class="vals" title="{inside.from} → {inside.to}">{#if c?.kind === "color"}<span class="sw" style:background={String(valueIn(inside.from, row.prop) ?? "")}></span>→<span class="sw" style:background={String(valueIn(inside.to, row.prop) ?? "")}></span>{:else}{shown(valueIn(inside.from, row.prop))} → {shown(valueIn(inside.to, row.prop))}{/if}</span>
            {#if inside.own && !row.tween}<IconButton icon={Plus} size={12} class="is-row" label="Give {row.label} its own slice" title="It follows the transition's curve: give it a slice of its own to move" onclick={() => giveSlice(row)} />{/if}
          {/if}
        {:else if row.kind === "object"}
          <BitsButton.Root
            class="name"
            title="Select {row.label}"
            onclick={() => probe(row.target, "select")}
            onpointerenter={() => probe(row.target, "hover")}
            onpointerleave={() => probe("", "unhover")}
          >
            {#if row.error}<TriangleAlert size={12} strokeWidth={1.75} />{/if}
            <span>{row.label}</span>
          </BitsButton.Root>
        {:else}
          <!-- A state lane's pill names its state; the row's "state" label would only crowd it. -->
          {#if !(row.lane && row.state)}<span class="prop" title={row.prop}>{row.label}</span>{/if}
          {#if row.lane && row.state}
            <button class="now" title={row.state.running ? `${row.label} at the playhead: ${row.state.running.from ?? ""} → ${row.state.running.to ?? ""}, ${Math.round(row.state.running.progress * 100)}%. Click to edit this component's states` : `${row.label} at the playhead. Click to edit this component's states`} onclick={() => openLane(row.target, layout.rows.find((x) => x.kind === "object" && x.target === row.target)?.label ?? "")}>{#if row.state.running}→ {row.state.running.to ?? row.state.running.name ?? ""}{:else}{row.state.state ?? ""}{/if}</button>
          {/if}
          <span class="keys">
            <IconButton icon={ChevronLeft} size={12} class="is-row" label="Previous key of {row.label}" title="Previous key" onclick={() => key("prev", row.clip)} data-key="prev" />
            <IconButton icon={Diamond} size={12} class="is-row" label={row.keyed ? `Remove the key of ${row.label} here` : `Add a key of ${row.label} here`} title={row.keyed ? "Remove the key at the playhead" : "Add a key at the playhead"} pressed={row.keyed} onPressedChange={() => keyRow(row)} data-key="toggle" />
            <IconButton icon={ChevronRight} size={12} class="is-row" label="Next key of {row.label}" title="Next key" onclick={() => key("next", row.clip)} data-key="next" />
          </span>
        {/if}
      </div>
    {/each}
  </div>
  <div class="corner" style:height="{css(layout.ruler_h)}px">
    {#if inside}<span class="corner-label">Part · property</span>
    {:else}<TimelineTransport {area} over={() => over} />{/if}
  </div>
</div>
{#if machines.editing}
  <StateEditor zoom={layout.zoom} inside={layout.inside} {panel} />
{/if}

<style>
  /* One surface with the app: the column and the ruler's corner on
     `--bg-sunken`, as the canvas half's header and ruler are
     (timeline_view.rs's palettes hold the same tokens' values). */
  .timeline-column {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    overflow: hidden;
    background: var(--bg-sunken);
    border-right: 1px solid var(--border);
    color: var(--fg);
    pointer-events: auto;
    user-select: none;
  }
  .corner {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    border-bottom: 1px solid var(--border);
  }
  .rows {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    overflow: hidden;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 6px 0 8px;
    font-size: var(--text-md);
  }
  .row.property {
    padding-left: 22px;
    color: var(--fg-muted);
  }
  .row.error {
    color: var(--danger);
  }
  .row :global(.name) {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    padding: 1px 4px;
    margin-left: -4px;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: inherit;
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .row :global(.name:hover) {
    background: var(--hover);
  }
  .row :global(.name span),
  .prop {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .prop {
    flex: 1;
    min-width: 0;
  }
  .now {
    flex: none;
    border: 0;
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    max-width: 76px;
    padding: 1px 6px;
    border-radius: var(--radius-full);
    background: var(--hover);
    color: var(--fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .now:hover {
    background: var(--active);
  }
  .keys {
    display: flex;
    margin-left: auto;
  }
  /* Inside a transition (machines.md §6). */
  .inside .corner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    padding: 0 8px;
  }
  .corner-label {
    font-size: var(--text-xs);
    color: var(--fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .inside .row.object {
    padding-left: 2px;
  }
  .row.picked {
    background: var(--accent-soft);
  }
  .pick {
    flex: 1;
    min-width: 0;
    padding: 1px 4px;
    margin-left: -4px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pick:hover {
    background: var(--hover);
  }
  .vals {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    color: var(--fg-muted);
    white-space: nowrap;
  }
  .sw {
    width: 10px;
    height: 10px;
    border-radius: 2px;
    box-shadow: inset 0 0 0 1px var(--border-strong);
  }
</style>
