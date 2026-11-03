<script lang="ts">
  // The timeline's transport (plans/specs/timeline.md §5): play and pause,
  // back to the start, and the time readout, as HTML over the timeline
  // composition's view. The playhead is the session clock (`clock_read`,
  // `clock_set`): never saved, never undone. Typing a time in the readout
  // seeks.
  import { onDestroy } from "svelte";
  import Play from "@lucide/svelte/icons/play";
  import Pause from "@lucide/svelte/icons/pause";
  import SkipBack from "@lucide/svelte/icons/skip-back";
  import { controllerManager } from "../../controller-manager.svelte";

  const doc = controllerManager.docClient;

  type Clock = { ok: boolean; t: number; playing: boolean; rate: number; end: number };
  let clock: Clock = $state({ ok: false, t: 0, playing: false, rate: 1, end: 0 });
  let editing = $state(false);
  let draft = $state("");

  async function read() {
    try {
      const r = await doc.command<Clock>({ op: "clock_read" });
      if (r.ok) clock = r;
    } catch {
      // No engine session yet.
    }
  }
  async function set(fields: Record<string, number | boolean>) {
    await doc.command({ op: "clock_set", fields: JSON.stringify(fields) });
    await read();
  }
  // Polled: the player advances the clock without telling the shell, and a
  // drag on the playhead is a gesture in the engine.
  const timer = setInterval(read, 100);
  read();
  onDestroy(() => clearInterval(timer));

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

<div class="transport canvas-ui" role="toolbar" aria-label="Transport">
  <button class="tool" title="Back to start" aria-label="Back to start" onclick={() => set({ t: 0 })}>
    <SkipBack size={15} strokeWidth={1.75} />
  </button>
  <button class="tool play" title={clock.playing ? "Pause" : "Play"} aria-label={clock.playing ? "Pause" : "Play"} onclick={toggle}>
    {#if clock.playing}<Pause size={15} strokeWidth={1.75} />{:else}<Play size={15} strokeWidth={1.75} />{/if}
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
