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
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { controllerManager } from "../../controller-manager.svelte";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import TimelineTransport from "./TimelineTransport.svelte";
  import MachinePanel from "./MachinePanel.svelte";
  import { machines } from "../../lib/machines.svelte";

  let { panel }: { panel: string } = $props();
  const doc = controllerManager.docClient;

  type Row = { kind: "object" | "property"; label: string; y: number; target: string; prop: string | null; clip: string | null; error: boolean; keyed: boolean; lane?: boolean; state?: { state: string | null; running: { name: string | null; progress: number } | null } | null };
  type Rows = { ok: boolean; rows: Row[]; y0: number; zoom: number; label_w: number; ruler_h: number; row_h: number };
  let layout: Rows = $state({ ok: false, rows: [], y0: 0, zoom: 1, label_w: 160, ruler_h: 56, row_h: 22 });
  let root: HTMLDivElement | undefined = $state();

  // Document px to this column's CSS px: the canvas view's zoom is CSS px
  // per document px (the engine applies the display's pixel ratio), so no
  // `devicePixelRatio` here.
  const css = (px: number) => px * layout.zoom;
  const top = (y: number) => (y - layout.y0) * layout.zoom;

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

  // The panel's two layouts (machines.md §6): the document's clock, or the
  // states of the component being edited. Entering a component (selecting
  // its main or a part) switches to its states; leaving switches back. A
  // selected instance offers its states without switching.
  const stopMachines = machines.use();
  onDestroy(stopMachines);
  let mode = $state<"timeline" | "states">("timeline");
  let lastMain: string | null = null;
  $effect(() => {
    const main = machines.view?.main ?? null;
    if (main !== lastMain) {
      lastMain = main;
      mode = main ? "states" : "timeline";
    }
  });
  const offered = $derived(!!machines.view && (!!machines.view.main || machines.view.machines.length > 0));
  // A name selects its object, and hovering it lights the object in every view.
  const probe = (node: string, action: "hover" | "unhover" | "select") => doc.command({ op: "narrative_probe", node, action });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="timeline-column canvas-ui" bind:this={root} style:width="{css(layout.label_w)}px" onwheel={wheel} data-timeline-column>
  <div class="rows" style:top="{css(layout.ruler_h)}px">
    {#each layout.rows as row (row.kind + row.target + (row.prop ?? ""))}
      <div class="row {row.kind}" class:error={row.error} style:top="{top(row.y) - css(layout.ruler_h)}px" style:height="{css(layout.row_h)}px" data-row={row.kind} data-target={row.target} data-prop={row.prop}>
        {#if row.kind === "object"}
          <BitsButton.Root
            class="name"
            title="Select {row.label}"
            onclick={() => probe(row.target, "select")}
            onpointerenter={() => probe(row.target, "hover")}
            onpointerleave={() => probe("", "unhover")}
          >
            {#if row.error}<TriangleAlert size={12} strokeWidth={2} />{/if}
            <span>{row.label}</span>
          </BitsButton.Root>
        {:else}
          <span class="prop" title={row.prop}>{row.label}</span>
          {#if row.lane && row.state}
            <span class="now" title="At the playhead">{#if row.state.running}{row.state.running.name ?? "change"} {Math.round(row.state.running.progress * 100)}%{:else}{row.state.state ?? ""}{/if}</span>
          {/if}
          <span class="keys">
            <IconButton icon={ChevronLeft} label="Previous key of {row.label}" title="Previous key" onclick={() => key("prev", row.clip)} data-key="prev" />
            <IconButton icon={Diamond} label={row.keyed ? `Remove the key of ${row.label} here` : `Add a key of ${row.label} here`} title={row.keyed ? "Remove the key at the playhead" : "Add a key at the playhead"} pressed={row.keyed} onPressedChange={() => keyRow(row)} data-key="toggle" />
            <IconButton icon={ChevronRight} label="Next key of {row.label}" title="Next key" onclick={() => key("next", row.clip)} data-key="next" />
          </span>
        {/if}
      </div>
    {/each}
  </div>
  <div class="corner" style:height="{css(layout.ruler_h)}px">
    <TimelineTransport {area} over={() => over} />
  </div>
</div>
{#if offered && mode === "timeline"}
  <button class="to-states canvas-ui" onclick={() => (mode = "states")} title="Show the states of {machines.view?.name ?? 'this component'}">States · {machines.view?.name ?? "component"}</button>
{/if}
{#if mode === "states"}
  <MachinePanel onTimeline={() => (mode = "timeline")} />
{/if}

<style>
  /* The column matches the canvas half's surface (timeline_view.rs's
     LIGHT and DARK palettes' header and ruler), in the app's theme. */
  .timeline-column {
    --tl-header: #f3f3f3;
    --tl-ruler: #f3f3f3;
    --tl-line: var(--border);
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    overflow: hidden;
    background: var(--tl-header);
    border-right: 1px solid var(--tl-line);
    color: var(--fg);
    pointer-events: auto;
    user-select: none;
  }
  :global(:root[data-theme="dark"]) .timeline-column {
    --fg: #d9d9e0;
    --fg-muted: #9d9da6;
    --fg-faint: #6e6e78;
    --hover: rgba(255, 255, 255, 0.08);
    --border-strong: #45454c;
    --bg: #2b2b2e;
    --bg-sunken: #222226;
    --bg-input: #222226;
    --accent-soft: rgba(0, 120, 212, 0.3);
    --accent: #4aa3ff;
    --tl-header: #2b2b2e;
    --tl-ruler: #333337;
    --tl-line: #3a3a40;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme="light"])) .timeline-column {
    --fg: #d9d9e0;
    --fg-muted: #9d9da6;
    --fg-faint: #6e6e78;
    --hover: rgba(255, 255, 255, 0.08);
    --border-strong: #45454c;
    --bg: #2b2b2e;
    --bg-sunken: #222226;
    --bg-input: #222226;
    --accent-soft: rgba(0, 120, 212, 0.3);
    --accent: #4aa3ff;
    --tl-header: #2b2b2e;
    --tl-ruler: #333337;
    --tl-line: #3a3a40;
    }
  }
  .corner {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    background: var(--tl-ruler);
    border-bottom: 1px solid var(--tl-line);
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
    box-sizing: border-box;
    font-size: 12px;
  }
  .row.property {
    padding-left: 22px;
    color: var(--fg-muted);
  }
  .row.error {
    color: #f07a74;
  }
  .row :global(.name) {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    padding: 1px 4px;
    margin-left: -4px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: inherit;
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .row :global(.name:hover) {
    background: var(--hover);
  }
  .row :global(.name:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
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
    max-width: 84px;
    padding: 1px 6px;
    border-radius: 8px;
    background: var(--hover);
    color: var(--fg);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .to-states {
    position: absolute;
    top: 6px;
    right: 8px;
    height: 24px;
    padding: 0 10px;
    border: 1px solid var(--border-strong);
    border-radius: 12px;
    background: var(--bg);
    color: var(--fg);
    font-size: 11px;
    cursor: pointer;
    pointer-events: auto;
    z-index: 2;
  }
  .to-states:hover {
    border-color: var(--accent);
  }
  .to-states:focus-visible {
    outline: 2px solid var(--accent);
  }
  .keys {
    display: flex;
    margin-left: auto;
  }
  .keys :global(.property-icon) {
    width: 20px;
    height: 20px;
    padding: 3px;
  }
</style>
