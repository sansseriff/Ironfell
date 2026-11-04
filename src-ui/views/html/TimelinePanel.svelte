<script lang="ts">
  // The timeline's DOM half (plans/specs/timeline.md §12.3): its left column,
  // over the canvas composition's header backdrop. The canvas draws what needs
  // the renderer (ruler, bars, keys, playhead); this draws what DOM does
  // better: the transport in the corner over the ruler, then a row per
  // animated object and per property, with names, error marks and each
  // property's ‹ ◇ › (previous key, add or remove a key at the playhead, next
  // key). Both halves come from one plan: the engine serves the rows
  // (`timeline_rows`) with the canvas view's pan, and the column's wheel
  // scrolls that view (`view_pan`), so they stay aligned.
  import { onDestroy } from "svelte";
  import { Button as BitsButton } from "bits-ui";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Diamond from "@lucide/svelte/icons/diamond";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { controllerManager } from "../../controller-manager.svelte";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import TimelineTransport from "./TimelineTransport.svelte";

  let { panel }: { panel: string } = $props();
  const doc = controllerManager.docClient;

  type Row = { kind: "object" | "property"; label: string; y: number; target: string; prop: string | null; clip: string | null; error: boolean; keyed: boolean };
  type Rows = { ok: boolean; rows: Row[]; pan_y: number; zoom: number; label_w: number; ruler_h: number; row_h: number };
  let layout: Rows = $state({ ok: false, rows: [], pan_y: 0, zoom: 1, label_w: 160, ruler_h: 80, row_h: 22 });
  let root: HTMLDivElement | undefined = $state();

  // Document px to this column's CSS px: the canvas view's zoom, over the
  // device's ratio (the panel's rectangle is physical px).
  const css = (px: number) => (px * layout.zoom) / devicePixelRatio;
  const top = (y: number) => (layout.pan_y + y * layout.zoom) / devicePixelRatio;

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

  // The column scrolls the canvas view with it.
  function wheel(e: WheelEvent) {
    e.preventDefault();
    const pan_y = Math.min(0, layout.pan_y - e.deltaY * devicePixelRatio);
    layout = { ...layout, pan_y };
    void doc.command({ op: "view_pan", panel, pan_y });
  }

  const key = (action: "prev" | "next" | "toggle", clip: string | null) => clip && doc.command({ op: "timeline_key", action, clip }).then(read);
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
          <span class="keys">
            <IconButton icon={ChevronLeft} label="Previous key of {row.label}" title="Previous key" onclick={() => key("prev", row.clip)} data-key="prev" />
            <IconButton icon={Diamond} label={row.keyed ? `Remove the key of ${row.label} here` : `Add a key of ${row.label} here`} title={row.keyed ? "Remove the key at the playhead" : "Add a key at the playhead"} pressed={row.keyed} onPressedChange={() => key("toggle", row.clip)} data-key="toggle" />
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

<style>
  /* The column matches the canvas half's dark surface (timeline_view.rs's
     HEADER and RULER colours), whatever the app's theme. */
  .timeline-column {
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
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    overflow: hidden;
    background: #2b2b2e;
    border-right: 1px solid #3a3a40;
    color: var(--fg);
    pointer-events: auto;
    user-select: none;
  }
  .corner {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    background: #333337;
    border-bottom: 1px solid #3a3a40;
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
