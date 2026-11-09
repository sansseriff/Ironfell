<script lang="ts" module>
  // The easing curves Iron knows (time.rs `CURVES`), as CSS timing functions
  // for their previews: the quadratics are Iron's own, drawn exactly.
  export const CURVES: { value: string; label: string; f: (t: number) => number }[] = [
    { value: "linear", label: "Linear", f: (t) => t },
    { value: "ease_in", label: "Ease in", f: (t) => t * t },
    { value: "ease_out", label: "Ease out", f: (t) => 1 - (1 - t) * (1 - t) },
    { value: "ease_in_out", label: "Ease in-out", f: (t) => (t < 0.5 ? 2 * t * t : 1 - 2 * (1 - t) * (1 - t)) },
    { value: "ease", label: "Ease (CSS)", f: bezier(0.25, 0.1, 0.25, 1) },
    { value: "easy_ease", label: "Easy Ease", f: bezier(0.33, 0, 0.67, 1) },
    { value: "hold", label: "Hold", f: (t) => (t >= 1 ? 1 : 0) },
  ];
  /** CSS cubic-bezier as y(t), solved for x by bisection. */
  export function bezier(x1: number, y1: number, x2: number, y2: number) {
    const b = (a: number, c: number, s: number) => 3 * a * s * (1 - s) ** 2 + 3 * c * s * s * (1 - s) + s ** 3;
    return (t: number) => {
      let lo = 0, hi = 1;
      for (let i = 0; i < 30; i++) { const m = (lo + hi) / 2; if (b(x1, x2, m) < t) lo = m; else hi = m; }
      return b(y1, y2, (lo + hi) / 2);
    };
  }
  export function parseBezier(text: string | null | undefined): [number, number, number, number] | null {
    const m = (text ?? "").match(/^cubic-bezier\(\s*([-\d.]+)\s*,\s*([-\d.]+)\s*,\s*([-\d.]+)\s*,\s*([-\d.]+)\s*\)$/);
    return m ? [Number(m[1]), Number(m[2]), Number(m[3]), Number(m[4])] : null;
  }
  /** A curve's preview path in a w × h box (y up). */
  export function curvePath(f: (t: number) => number, w: number, h: number, pad = 2) {
    const pts: string[] = [];
    for (let i = 0; i <= 32; i++) {
      const t = i / 32;
      pts.push(`${(pad + t * (w - 2 * pad)).toFixed(1)},${(h - pad - f(t) * (h - 2 * pad)).toFixed(1)}`);
    }
    return "M" + pts.join(" L");
  }
</script>

<script lang="ts">
  // An easing chooser (machines.md §6, the council's ask): the named curves
  // with a preview of each, and a cubic-bezier editor (CSS dev tools' curve:
  // two handles to drag, the four numbers to type) when `custom` allows it.
  import { Popover } from "bits-ui";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  let { value, label, custom = true, disabled = false, onChange }: {
    value: string | null; label: string; custom?: boolean; disabled?: boolean; onChange: (v: string) => void;
  } = $props();
  let open = $state(false);
  const bez = $derived(parseBezier(value));
  const named = $derived(CURVES.find((c) => c.value === value));
  const shown = $derived(named?.f ?? (bez ? bezier(...bez) : (t: number) => t));
  const title = $derived(named?.label ?? (bez ? `cubic-bezier(${bez.join(", ")})` : value ?? "Linear"));
  // The custom editor's handles, in its 0..1 × −0.5..1.5 space.
  let draft = $state<[number, number, number, number]>([0.33, 0, 0.67, 1]);
  $effect(() => { if (bez) draft = bez; });
  const W = 168, H = 168, Y0 = 0.25, YS = H / 1.5;
  const px = (x: number) => 12 + x * (W - 24);
  const py = (y: number) => H - 12 - (y + Y0) * (YS - 16);
  let dragging: 0 | 1 | null = null;
  function drag(i: 0 | 1, e: PointerEvent) {
    dragging = i;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (dragging === null) return;
    const r = (e.currentTarget as SVGElement).getBoundingClientRect();
    const x = Math.max(0, Math.min(1, ((e.clientX - r.left) * (W / r.width) - 12) / (W - 24)));
    const y = Math.max(-0.5, Math.min(1.5, (H - 12 - (e.clientY - r.top) * (H / r.height)) / (YS - 16) - Y0));
    const next = [...draft] as [number, number, number, number];
    next[dragging * 2] = Math.round(x * 100) / 100;
    next[dragging * 2 + 1] = Math.round(y * 100) / 100;
    draft = next;
  }
  function release() {
    if (dragging === null) return;
    dragging = null;
    onChange(`cubic-bezier(${draft.join(", ")})`);
  }
</script>

<Popover.Root bind:open>
  <Popover.Trigger class="easing-trigger" aria-label="{label}: {title}" title="{label}: {title}" {disabled}>
    <svg width="28" height="18" viewBox="0 0 28 18" aria-hidden="true"><path d={curvePath(shown, 28, 18)} /></svg>
    <span class="easing-name">{title}</span>
    <ChevronDown size={13} strokeWidth={1.5} />
  </Popover.Trigger>
  <Popover.Portal>
    <Popover.Content class="property-popover easing-popover" side="left" align="start" sideOffset={8} aria-label="{label} curves">
      <div class="easing-list" role="listbox" aria-label={label}>
        {#each CURVES as c (c.value)}
          <button role="option" aria-selected={c.value === value} class:current={c.value === value} onclick={() => { onChange(c.value); open = false; }}>
            <svg width="36" height="24" viewBox="0 0 36 24" aria-hidden="true"><path d={curvePath(c.f, 36, 24)} /></svg>
            <span>{c.label}</span>
          </button>
        {/each}
      </div>
      {#if custom}
        <div class="bezier">
          <div class="bezier-title">Custom curve</div>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <svg class="bezier-box" width={W} height={H} viewBox="0 0 {W} {H}" onpointermove={move} onpointerup={release} onpointercancel={release}>
            <rect class="unit" x={px(0)} y={py(1)} width={px(1) - px(0)} height={py(0) - py(1)} />
            <line class="arm" x1={px(0)} y1={py(0)} x2={px(draft[0])} y2={py(draft[1])} />
            <line class="arm" x1={px(1)} y1={py(1)} x2={px(draft[2])} y2={py(draft[3])} />
            <path class="curve" d={(() => { const f = bezier(...draft); let d = ""; for (let i = 0; i <= 40; i++) { const t = i / 40; d += `${i ? "L" : "M"}${px(t).toFixed(1)},${py(f(t)).toFixed(1)}`; } return d; })()} />
            {#each [0, 1] as const as i (i)}
              <circle class="handle" role="slider" tabindex="0" aria-label="Handle {i + 1}" aria-valuenow={draft[i * 2 + 1]} cx={px(draft[i * 2])} cy={py(draft[i * 2 + 1])} r="6" onpointerdown={(e) => drag(i, e)} />
            {/each}
          </svg>
          <input class="bezier-text" aria-label="cubic-bezier values" value={draft.join(", ")} spellcheck="false"
            onchange={(e) => { const v = e.currentTarget.value.split(",").map(Number); if (v.length === 4 && v.every(Number.isFinite)) { draft = [Math.max(0, Math.min(1, v[0])), v[1], Math.max(0, Math.min(1, v[2])), v[3]]; onChange(`cubic-bezier(${draft.join(", ")})`); } }} />
        </div>
      {/if}
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>

<style>
  :global(.easing-trigger) { display: flex; align-items: center; gap: 6px; width: 100%; min-width: 0; height: 30px; padding: 0 8px; border: 1px solid transparent; border-radius: 5px; background: var(--bg-sunken); color: var(--fg); font-size: 12px; cursor: pointer; }
  :global(.easing-trigger:hover) { border-color: var(--border-strong); }
  :global(.easing-trigger:focus-visible) { outline: none; border-color: var(--accent); }
  :global(.easing-trigger svg path), .easing-list svg path { fill: none; stroke: var(--accent); stroke-width: 1.5; }
  .easing-name { flex: 1; min-width: 0; text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  :global(.easing-popover) { width: 216px; padding: 8px; }
  .easing-list { display: flex; flex-direction: column; gap: 2px; }
  .easing-list button { display: flex; align-items: center; gap: 10px; padding: 4px 6px; border: 0; border-radius: 5px; background: transparent; color: var(--fg); font-size: 12px; cursor: pointer; text-align: left; }
  .easing-list button:hover { background: var(--hover); }
  .easing-list button.current { background: var(--bg-sunken); box-shadow: inset 0 0 0 1px var(--accent); }
  .easing-list button:focus-visible { outline: 1px solid var(--accent); }
  .easing-list svg { flex: 0 0 36px; background: var(--bg-sunken); border-radius: 3px; }
  .bezier { margin-top: 8px; padding-top: 8px; border-top: 1px solid var(--border); display: flex; flex-direction: column; gap: 6px; align-items: center; }
  .bezier-title { align-self: flex-start; color: var(--fg-muted); font-size: 11px; }
  .bezier-box { touch-action: none; }
  .bezier-box .unit { fill: var(--bg-sunken); stroke: var(--border); }
  .bezier-box .arm { stroke: var(--fg-muted); stroke-width: 1; }
  .bezier-box .curve { fill: none; stroke: var(--accent); stroke-width: 2; }
  .bezier-box .handle { fill: var(--bg); stroke: var(--accent); stroke-width: 2; cursor: grab; }
  .bezier-text { width: 100%; height: 26px; border: 1px solid transparent; border-radius: 5px; background: var(--bg-sunken); color: var(--fg); font-size: 12px; font-family: var(--font-mono, ui-monospace, monospace); padding: 0 8px; outline: none; }
  .bezier-text:focus { border-color: var(--accent); }
</style>
