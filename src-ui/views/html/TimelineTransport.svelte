<script lang="ts">
  // The timeline's transport (plans/specs/timeline.md §5, §6): play and
  // pause, back to the start, loop, fit, and the time readout, as HTML over
  // the timeline composition's view. The playhead is the session clock
  // (`clock_read`, `clock_set`): never saved, never undone. Typing a time in
  // the readout seeks. With the pointer over the timeline its keys are the
  // transport's: Space plays and pauses, Home and End go to 0 and to the
  // last clip's end, ← and → step a frame (1/30 s; with Shift, 1 s), I and
  // O set the loop's ends at the playhead, F fits the clips.
  import { onDestroy } from "svelte";
  import Play from "@lucide/svelte/icons/play";
  import Pause from "@lucide/svelte/icons/pause";
  import SkipBack from "@lucide/svelte/icons/skip-back";
  import Repeat from "@lucide/svelte/icons/repeat";
  import Maximize from "@lucide/svelte/icons/maximize-2";
  import { controllerManager } from "../../controller-manager.svelte";

  const doc = controllerManager.docClient;

  type Clock = { ok: boolean; t: number; playing: boolean; rate: number; end: number; loop_in: number; loop_out: number };
  let clock: Clock = $state({ ok: false, t: 0, playing: false, rate: 1, end: 0, loop_in: 0, loop_out: 0 });
  const looping = $derived(clock.loop_out > clock.loop_in);
  let root: HTMLDivElement | undefined = $state();
  let editing = $state(false);
  let draft = $state("");

  async function read() {
    if (writes) return;
    try {
      const r = await doc.command<Clock>({ op: "clock_read" });
      if (r.ok) clock = r;
    } catch {
      // No engine session yet.
    }
  }
  // Applied here first, so keys pressed faster than a round trip build on
  // each other (End then ← is a frame before the end).
  let writes = 0;
  async function set(fields: Record<string, number | boolean>) {
    clock = { ...clock, ...fields };
    writes++;
    try {
      await doc.command({ op: "clock_set", fields: JSON.stringify(fields) });
    } finally {
      writes--;
    }
    if (!writes) await read();
  }
  // Polled: the player advances the clock without telling the shell, and a
  // drag on the playhead is a gesture in the engine.
  const timer = setInterval(read, 100);
  read();

  /** The timeline view's box: the overlay this sits in covers it. */
  const area = () => root?.parentElement?.getBoundingClientRect();
  let over = false;
  const onMove = (e: PointerEvent) => {
    const r = area();
    over = !!r && e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom;
  };
  const FRAME = 1 / 30;
  function fit() {
    const r = area();
    if (r) doc.command({ op: "timeline_axis", fields: JSON.stringify({ fit: r.width * devicePixelRatio }) });
  }
  function toggleLoop() {
    if (looping) set({ loop_in: 0, loop_out: 0 });
    else set({ loop_in: 0, loop_out: clock.end });
  }
  // Captured before the engine's own key handling, and only with the
  // pointer over the timeline and no text field focused.
  const onKey = (e: KeyboardEvent) => {
    if (!over || e.metaKey || e.ctrlKey || e.altKey) return;
    const t = e.target as HTMLElement | null;
    if (t && (t.isContentEditable || t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return;
    const step = e.shiftKey ? 1 : FRAME;
    const actions: Record<string, () => void> = {
      " ": toggle,
      home: () => set({ t: 0, playing: false }),
      end: () => set({ t: clock.end, playing: false }),
      arrowleft: () => set({ t: Math.max(0, clock.t - step), playing: false }),
      arrowright: () => set({ t: clock.t + step, playing: false }),
      i: () => set({ loop_in: clock.t, loop_out: Math.max(clock.loop_out, clock.t) }),
      o: () => set({ loop_out: clock.t, loop_in: Math.min(clock.loop_in, clock.t) }),
      f: fit,
    };
    const act = actions[e.key.toLowerCase()];
    if (!act) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    act();
  };
  window.addEventListener("pointermove", onMove, true);
  window.addEventListener("keydown", onKey, true);
  onDestroy(() => {
    clearInterval(timer);
    window.removeEventListener("pointermove", onMove, true);
    window.removeEventListener("keydown", onKey, true);
  });

  const fmt = (t: number) => {
    const m = Math.floor(t / 60);
    const s = t - m * 60;
    return `${m}:${s.toFixed(2).padStart(5, "0")}`;
  };
  function toggle() {
    // Play from the start again once the end has been reached.
    if (!clock.playing && clock.end > 0 && clock.t >= clock.end - 1e-6) set({ t: 0, playing: true });
    else set({ playing: !clock.playing });
  }
  function seek(text: string) {
    const parts = text.trim().split(":").map(Number);
    const t = parts.length === 2 ? parts[0] * 60 + parts[1] : parts[0];
    if (Number.isFinite(t) && t >= 0) set({ t });
  }
</script>

<div class="transport canvas-ui" role="toolbar" aria-label="Transport" bind:this={root}>
  <button class="tool" title="Back to start" aria-label="Back to start" onclick={() => set({ t: 0 })}>
    <SkipBack size={15} strokeWidth={1.75} />
  </button>
  <button class="tool play" title={clock.playing ? "Pause" : "Play"} aria-label={clock.playing ? "Pause" : "Play"} onclick={toggle}>
    {#if clock.playing}<Pause size={15} strokeWidth={1.75} />{:else}<Play size={15} strokeWidth={1.75} />{/if}
  </button>
  <button class="tool" class:on={looping} title={looping ? "Loop off" : "Loop every clip (I, O set its ends)"} aria-label="Loop" onclick={toggleLoop}>
    <Repeat size={15} strokeWidth={1.75} />
  </button>
  <button class="tool" title="Fit the clips (F)" aria-label="Fit" onclick={fit}>
    <Maximize size={15} strokeWidth={1.75} />
  </button>
  {#if editing}
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="time"
      autofocus
      bind:value={draft}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          seek(draft);
          editing = false;
        } else if (e.key === "Escape") editing = false;
      }}
      onblur={() => (editing = false)}
    />
  {:else}
    <button
      class="time"
      title="Type a time to seek"
      onclick={() => {
        draft = clock.t.toFixed(2);
        editing = true;
      }}>{fmt(clock.t)}</button
    >
  {/if}
  <span class="end">/ {fmt(clock.end)}</span>
</div>

<style>
  .transport {
    position: absolute;
    right: 12px;
    bottom: 10px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px 8px 4px 4px;
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    background: var(--bg);
    box-shadow: var(--menu-shadow);
    user-select: none;
    font-variant-numeric: tabular-nums;
  }
  .tool {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--fg);
    cursor: pointer;
  }
  .tool:hover {
    background: var(--hover);
  }
  .tool.on {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .time {
    width: 64px;
    height: 24px;
    padding: 0 4px;
    border: 1px solid transparent;
    border-radius: 5px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    text-align: right;
    cursor: text;
  }
  input.time {
    border-color: var(--border-strong);
    outline: none;
  }
  .end {
    font-size: 12px;
    color: var(--fg-muted);
  }
</style>
