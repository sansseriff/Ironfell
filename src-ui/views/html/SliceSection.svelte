<script lang="ts">
  // The Inspector for slices selected inside a transition (machines.md §6):
  // a bar in the state editor's tracks is a property's slice of the
  // transition, not a shape, so the Inspector shows what a slice is: when
  // it starts and ends, and its easing. Several: their easing, and when
  // they start (Mixed where they differ).
  import Section from "../../lib/ui/Section.svelte";
  import EasingField from "../../lib/ui/EasingField.svelte";
  import Button from "../../lib/ui/Button.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import { machines, type Transition } from "../../lib/machines.svelte";

  type Picked = { tween: string; channel: string };
  let { slices }: { slices: Picked[] } = $props();

  const view = $derived(machines.view);
  const pin = $derived(machines.pinned);
  const m = $derived(view?.machines.find((x) => x.id === pin?.machine) ?? view?.machines[0]);
  const t = $derived<Transition | undefined>(m?.transitions.find((x) => x.tweens.some((w) => slices.some((s) => s.tween === w.id))));
  const ms = $derived(m && t ? m.pairs.find((q) => q.from === t.from && q.to === t.to)?.ms ?? t.ms ?? 300 : 300);
  const tweens = $derived(t ? slices.map((s) => t.tweens.find((w) => w.id === s.tween)).filter((w) => !!w) : []);
  // A row's name: the part and the property, from the channels' readers.
  const LABELS: Record<string, string> = { "transform2d.x": "X", "transform2d.y": "Y", "appearance.opacity": "Opacity", "fill.color": "Fill", "size.w": "Width", "size.h": "Height", "text.content": "Text", "transform2d.rot": "Rotation" };
  const nameOf = (channel: string) => {
    const p = view?.channels.find((c) => c.name === channel)?.parts[0];
    return p ? `${p.name} · ${LABELS[p.path] ?? p.path}` : channel;
  };
  const same = <T,>(xs: T[]) => (xs.length && xs.every((x) => x === xs[0]) ? xs[0] : null);
  const startMs = $derived(same(tweens.map((w) => Math.round((w.start ?? 0) * ms))));
  const endMs = $derived(same(tweens.map((w) => Math.round((w.end ?? 1) * ms))));
  const ease = $derived(same(tweens.map((w) => w.ease || "")));

  let error = $state("");
  async function edit(fields: (w: { channel: string | null; start: number | null; end: number | null }) => { start?: number; end?: number; ease?: string }, label: string) {
    if (!view || !m || !t?.from || !t.to) return;
    error = "";
    const r = await machines.editPair(view.def, m.id, t.from, t.to, { slices: tweens.map((w) => ({ channel: w.channel ?? "", ...fields(w) })) }, label);
    if (!r.ok) error = r.error ?? "That did not work";
  }
  const clamp = (v: number) => Math.max(0, Math.min(1, v));

  function setStart(text: string) {
    const v = parseFloat(text);
    if (!Number.isFinite(v)) return;
    // Each keeps its length, as moving the bars does.
    void edit((w) => {
      const d = clamp(v / ms) - (w.start ?? 0);
      return { start: clamp((w.start ?? 0) + d), end: clamp((w.end ?? 1) + d) };
    }, "slice start");
  }
  function setEnd(text: string) {
    const v = parseFloat(text);
    if (Number.isFinite(v)) void edit((w) => ({ end: Math.max(clamp(v / ms), w.start ?? 0) }), "slice end");
  }

  // Stagger: the selected parts start one after another, in the order of
  // the rows (or reversed), a part's properties together. Keep lengths
  // moves each part whole (and can lengthen the transition to fit); Fit
  // shortens them so the last ends with the transition. Times are worked in
  // ms, so lengthening keeps every slice where it is in time.
  let staggerMs = $state(100);
  let order = $state<"down" | "up">("down");
  let mode = $state<"keep" | "fit">("keep");
  const partOf = (channel: string | null) => view?.channels.find((c) => c.name === channel)?.parts[0]?.node ?? channel ?? "";
  const partCount = $derived(new Set(tweens.map((w) => partOf(w.channel))).size);
  type Plan = { slices: { channel: string; start: number; end: number }[]; endMs: number };
  /** The staggered slices in ms from the transition's start. */
  const staggered = $derived.by((): Plan | null => {
    if (!view || partCount < 2) return null;
    const rank = (c: string | null) => view!.channels.findIndex((x) => x.name === c);
    const groups: (typeof tweens)[] = [];
    for (const w of [...tweens].sort((a, b) => rank(a.channel) - rank(b.channel))) {
      const g = groups.find((x) => partOf(x[0].channel) === partOf(w.channel));
      if (g) g.push(w);
      else groups.push([w]);
    }
    if (order === "up") groups.reverse();
    const step = Math.max(0, Number(staggerMs) || 0);
    const first = Math.min(...groups[0].map((w) => (w.start ?? 0) * ms));
    const room = Math.max(20, ms - first - (groups.length - 1) * step);
    const slices = groups.flatMap((g, i) => {
      const lo = Math.min(...g.map((w) => (w.start ?? 0) * ms));
      const hi = Math.max(...g.map((w) => (w.end ?? 1) * ms));
      const k = mode === "fit" ? Math.min(1, room / Math.max(hi - lo, 1e-6)) : 1;
      const at = first + i * step;
      return g.map((w) => ({ channel: w.channel ?? "", start: at + ((w.start ?? 0) * ms - lo) * k, end: at + ((w.end ?? 1) * ms - lo) * k }));
    });
    return { slices, endMs: Math.max(...slices.map((x) => x.end)) };
  });
  const over = $derived(!!staggered && staggered.endMs > ms + 0.5);
  function applyStagger(lengthen: boolean) {
    const p = staggered;
    if (!view || !m || !t?.from || !t.to || !p) return;
    const total = lengthen ? Math.ceil(p.endMs) : ms;
    const frac = (v: number) => clamp(v / total);
    // Lengthening rescales every other slice too, so each stays in time.
    const others = lengthen ? t.tweens.filter((w) => !p.slices.some((x) => x.channel === w.channel)).map((w) => ({ channel: w.channel ?? "", start: frac((w.start ?? 0) * ms), end: frac((w.end ?? 1) * ms) })) : [];
    const slices = [...p.slices.map((x) => ({ channel: x.channel, start: frac(x.start), end: frac(x.end) })), ...others];
    error = "";
    void machines.editPair(view.def, m.id, t.from, t.to, { slices, ...(lengthen ? { ms: total } : {}) }, "stagger").then((r) => { if (!r.ok) error = r.error ?? "That did not work"; });
  }
</script>

<Section title={slices.length === 1 ? nameOf(slices[0].channel) : `${slices.length} slices`} class="property-section" data-section="slices" meta={t ? `${t.from} → ${t.to}` : undefined}>
  <p class="hint">{slices.length === 1 ? "When it changes during the transition, and how it eases." : "When they change, and how they ease. Moving their bars together keeps their offsets."}</p>
  <div class="property-grid">
    <label class="ui-field"><span>Starts</span><input inputmode="decimal" aria-label="Slice start, ms" placeholder={startMs === null ? "Mixed" : undefined} value={startMs ?? ""} onchange={(e) => setStart(e.currentTarget.value)} /><span>ms</span></label>
    <label class="ui-field"><span>Ends</span><input inputmode="decimal" aria-label="Slice end, ms" placeholder={endMs === null ? "Mixed" : undefined} value={endMs ?? ""} onchange={(e) => setEnd(e.currentTarget.value)} /><span>ms</span></label>
  </div>
  <EasingField label="Slice easing" value={ease === "" ? t?.ease ?? null : ease} onChange={(v) => edit(() => ({ ease: v }), "slice easing")} />
  {#if ease === ""}<p class="hint">Following the transition's easing ({t?.ease ?? "Easy Ease"}).</p>
  {:else}<div class="row"><p class="hint">{ease === null ? "Mixed easings." : "Its own easing."}</p><Button small variant="outline" onclick={() => edit(() => ({ ease: "" }), "follow the transition's easing")} title="Ease as the transition does, and change when it changes">Follow the transition</Button></div>{/if}
  {#if partCount > 1 && staggered}
    <div class="ui-subheading">Stagger {partCount} parts</div>
    <div class="stagger">
      <label class="ui-field"><span>Each</span><input inputmode="decimal" aria-label="Stagger, ms" bind:value={staggerMs} /><span>ms later</span></label>
      <Segmented label="Order" value={order} items={[{ value: "down", label: "Top first" }, { value: "up", label: "Bottom first" }]} onChange={(v) => (order = v)} />
    </div>
    <Segmented label="Lengths" value={mode} items={[{ value: "keep", label: "Keep lengths", title: "Each part moves whole; its movement keeps its speed" }, { value: "fit", label: "Fit in transition", title: "Parts are shortened so the last ends with the transition" }]} onChange={(v) => (mode = v)} />
    <p class="hint" class:warn={over}>The last part ends at {Math.round(staggered.endMs)} ms{over ? `, ${Math.round(staggered.endMs - ms)} ms after the transition (${Math.round(ms)} ms).` : ` of ${Math.round(ms)} ms.`}</p>
    {#if over}
      <div class="row">
        <Button small variant="primary" onclick={() => applyStagger(true)} title="Make the transition {Math.ceil(staggered.endMs)} ms long; every slice keeps its time">Lengthen to {(Math.ceil(staggered.endMs) / 1000).toFixed(2)} s and stagger</Button>
        <Button small variant="outline" onclick={() => (mode = "fit")}>Fit instead</Button>
      </div>
    {:else}
      <Button small variant="outline" onclick={() => applyStagger(false)} title="Start the selected parts one after another">Stagger</Button>
    {/if}
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
</Section>

<style>
  .row { display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
  .warn { color: var(--danger); }
  .stagger { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 8px; }
  .hint { margin: 0; font-size: var(--text-sm); color: var(--fg-muted); }
  .error { margin: 0; color: var(--danger); font-size: var(--text-sm); }
  .ui-field { gap: 6px; padding: 0 8px; color: var(--fg-muted); font-size: var(--text-sm); }
  .ui-field input { flex: 1; width: 100%; min-width: 0; border: 0; background: transparent; color: var(--fg); font: inherit; font-size: var(--text-md); outline: none; text-align: right; }
</style>
