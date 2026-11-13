<script lang="ts">
  // The Inspector for slices selected inside a transition (machines.md §6):
  // a bar in the state editor's tracks is a property's slice of the
  // transition, not a shape, so the Inspector shows what a slice is: when
  // it starts and ends, and its easing. Several: their easing, and when
  // they start (Mixed where they differ).
  import Section from "../../lib/ui/Section.svelte";
  import EasingField from "../../lib/ui/EasingField.svelte";
  import Button from "../../lib/ui/Button.svelte";
  import Checkbox from "../../lib/ui/Checkbox.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import Check from "@lucide/svelte/icons/check";
  import { machines, type Transition } from "../../lib/machines.svelte";

  type Picked = { tween: string; channel: string };
  let { slices }: { slices: Picked[] } = $props();

  const view = $derived(machines.view);
  const pin = $derived(machines.pinned);
  const m = $derived(view?.machines.find((x) => x.id === pin?.machine) ?? view?.machines[0]);
  // The transition being previewed is the one these slices belong to; a
  // slice of another leg still selected is not shown or edited here.
  const t = $derived<Transition | undefined>(m?.transitions.find((x) => (!pin?.scrub || x.id === m?.pairs.find((q) => q.from === pin.scrub!.from && q.to === pin.scrub!.to)?.transition) && x.tweens.some((w) => slices.some((s) => s.tween === w.id))));
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

  // Times read in seconds, as the transition's header and ruler do.
  const sec = (v: number) => String(Math.round(v / 10) / 100);
  function setStart(text: string) {
    const v = parseFloat(text) * 1000;
    if (!Number.isFinite(v)) return;
    // Each keeps its length, as moving the bars does.
    void edit((w) => {
      const d = clamp(v / ms) - (w.start ?? 0);
      return { start: clamp((w.start ?? 0) + d), end: clamp((w.end ?? 1) + d) };
    }, "slice start");
  }
  function setEnd(text: string) {
    const v = parseFloat(text) * 1000;
    if (Number.isFinite(v)) void edit((w) => ({ end: Math.max(clamp(v / ms), w.start ?? 0) }), "slice end");
  }

  // Stagger: the selected parts start one after another, in the order of
  // the rows (or reversed), a part's properties together. Keep lengths
  // moves each part whole (and can lengthen the transition to fit); Fit
  // shortens them so the last ends with the transition. Times are worked in
  // ms, so lengthening keeps every slice where it is in time.
  let staggerS = $state(0.1);
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
    const step = Math.max(0, Number(staggerS) * 1000 || 0);
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
  // Already as the settings say: nothing to apply.
  const applied = $derived(!!staggered && !over && staggered.slices.every((x) => {
    const w = tweens.find((q) => q.channel === x.channel);
    return !!w && Math.abs((w.start ?? 0) * ms - x.start) < 1 && Math.abs((w.end ?? 1) * ms - x.end) < 1;
  }));
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

<Section title={slices.length === 1 ? nameOf(slices[0].channel) : partCount > 1 ? `${partCount} parts · ${slices.length} properties` : `${slices.length} properties`} class="property-section" data-section="slices" meta={t ? `${t.from} → ${t.to}` : undefined}>
  <div class="property-grid">
    <label class="ui-field"><span>Starts</span><input inputmode="decimal" aria-label="Slice start, seconds" placeholder={startMs === null ? "Mixed" : undefined} value={startMs === null ? "" : sec(startMs)} onchange={(e) => setStart(e.currentTarget.value)} />{#if startMs !== null}<span>s</span>{/if}</label>
    <label class="ui-field"><span>Ends</span><input inputmode="decimal" aria-label="Slice end, seconds" placeholder={endMs === null ? "Mixed" : undefined} value={endMs === null ? "" : sec(endMs)} onchange={(e) => setEnd(e.currentTarget.value)} />{#if endMs !== null}<span>s</span>{/if}</label>
  </div>
  <EasingField label="Slice easing" mixed={ease === null} value={ease === "" ? t?.ease ?? null : ease} onChange={(v) => edit(() => ({ ease: v }), "slice easing")} />
  <Checkbox label="Follow the transition's easing" checked={ease === ""} indeterminate={ease === null} onChange={(on) => edit(() => ({ ease: on ? "" : t?.ease || "easy_ease" }), on ? "follow the transition's easing" : "slice easing")} />
  {#if partCount > 1 && staggered}
    <div class="ui-subheading">Stagger</div>
    <label class="ui-field"><span>Each part</span><input inputmode="decimal" aria-label="Stagger, seconds" bind:value={staggerS} /><span>s later</span></label>
    <Segmented label="Order" value={order} items={[{ value: "down", label: "Top first" }, { value: "up", label: "Bottom first" }]} onChange={(v) => (order = v)} />
    <Segmented label="Lengths" value={mode} items={[{ value: "keep", label: "Keep lengths", title: "Each part moves whole" }, { value: "fit", label: "Fit in transition", title: "Parts are shortened so the last ends with the transition" }]} onChange={(v) => (mode = v)} />
    {#if applied}
      <Button small variant="outline" disabled data-stagger="done"><Check size={14} strokeWidth={1.75} />Staggered</Button>
    {:else if over}
      <Button small variant="primary" data-stagger="lengthen" onclick={() => applyStagger(true)} title="The transition becomes {sec(Math.ceil(staggered.endMs))} s long; every slice keeps its time">Stagger and lengthen to {sec(Math.ceil(staggered.endMs))} s</Button>
    {:else}
      <Button small variant="primary" data-stagger="apply" onclick={() => applyStagger(false)}>Stagger</Button>
    {/if}
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
</Section>

<style>
  .error { margin: 0; color: var(--danger); font-size: var(--text-sm); }
  .ui-field { gap: 6px; padding: 0 8px; color: var(--fg-muted); font-size: var(--text-sm); }
  .ui-field input { flex: 1; width: 100%; min-width: 0; border: 0; background: transparent; color: var(--fg); font: inherit; font-size: var(--text-md); outline: none; text-align: right; }
</style>
