<script lang="ts">
  // The timeline panel's states layout (plans/specs/machines.md §6): a
  // component's machines, shown when you are inside the component (its main
  // or a part of it is selected) or, read-only, for a selected instance.
  //
  // One panel, two layouts. **Strip** (a chain or a loop): the states in a
  // row with the transitions between them. **Graph** (it branches): the
  // states and their transitions as a diagram in a pane left of the tracks.
  // Either way, clicking a state previews it on the main (the pin: edits made
  // then shape it); clicking a transition, or picking From and To, shows that
  // transition's tracks below: its length and easing, a row per channel
  // with the tween's slice as a bar to drag, and a playhead that scrubs the
  // transition's progress on the main. For an instance it shows the state
  // it is in now and the transition it is going through.
  import { onDestroy } from "svelte";
  import { ToggleGroup } from "bits-ui";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Play from "@lucide/svelte/icons/play";
  import Plus from "@lucide/svelte/icons/plus";
  import Square from "@lucide/svelte/icons/square";
  import { controllerManager } from "../../controller-manager.svelte";
  import X from "@lucide/svelte/icons/x";
  import Button from "../../lib/ui/Button.svelte";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import Select from "../../lib/ui/Select.svelte";
  import EasingField, { CURVES, bezier, parseBezier } from "../../lib/ui/EasingField.svelte";
  import { machines, type Machine, type Pair, type Transition, type Tween } from "../../lib/machines.svelte";

  let { onTimeline }: { onTimeline: () => void } = $props();

  const stop = machines.use();
  onDestroy(() => {
    stop();
    cancelAnimationFrame(playing);
  });

  const view = $derived(machines.view);
  const editable = $derived(!!view?.main);
  let error = $state("");

  // Layout: automatic (strip for chains and loops), or chosen; kept for
  // the session (machines.md §6: workspace state).
  const layoutOf = (m: Machine) => (machines.layoutChoice === "auto" ? m.layout : machines.layoutChoice);

  // The transition shown in the tracks, per machine: a pair of states, the
  // one chosen; on an instance going through one, that one; else the first.
  const names = (m: Machine) => m.order?.[0] ?? m.states.map((s) => s.name ?? "").filter(Boolean);
  function pairOf(m: Machine): { from: string; to: string } | null {
    const c = machines.chosen[m.id];
    const ns = names(m);
    if (c && ns.includes(c.from) && ns.includes(c.to) && c.from !== c.to) return c;
    const cur = m.current;
    if (!editable && cur?.from && cur?.to) return { from: cur.from, to: cur.to };
    return ns.length >= 2 ? { from: ns[0], to: ns[1] } : null;
  }
  const pairInfo = (m: Machine, p: { from: string; to: string }): Pair | undefined => m.pairs.find((x) => x.from === p.from && x.to === p.to);
  const transitionOf = (m: Machine, id: string | null | undefined): Transition | undefined => m.transitions.find((t) => t.id === id);
  const own = (t: Transition | undefined, p: { from: string; to: string }) => !!t && t.from === p.from && t.to === p.to;
  // The shared transition a pair with its own would otherwise use.
  const sharedFor = (m: Machine, p: { from: string; to: string }, t: Transition | undefined) =>
    m.transitions.find((x) => x !== t && (x.from === "*" || x.from === p.from) && (x.to === "*" || x.to === p.to) && (x.from === "*" || x.to === "*"));
  const describe = (t: Transition) => (t.from === "*" && t.to === "*" ? "any change" : `${t.from === "*" ? "any" : t.from} → ${t.to === "*" ? "any" : t.to}`);

  async function run(p: Promise<{ ok: boolean; error?: string }>) {
    error = "";
    const r = await p;
    if (!r.ok) error = r.error ?? "That did not work";
    return r;
  }

  // ── States ────────────────────────────────────────────────────────────
  // A state is lit when it is previewed (not while a transition is
  // scrubbed: that is a moment between two), or an instance is in it.
  const lit = (m: Machine, s: string) => (editable ? m.pinned === s && !machines.pinned?.scrub : m.current?.state === s);
  function clickState(m: Machine, s: string) {
    if (!view) return;
    stopPlay();
    scrubbing = null;
    if (editable) {
      void run(machines.pin(view.def, m.id, m.pinned === s && !machines.pinned?.scrub ? null : s));
    } else if (view.entity) {
      void run(machines.patch(view.entity, { [`args.${m.port}`]: s }, `set ${m.port}`));
    }
  }
  function choose(m: Machine, from: string, to: string) {
    machines.chosen = { ...machines.chosen, [m.id]: { from, to } };
    stopPlay();
    scrubbing = null;
    if (view && editable && machines.pinned?.scrub) void machines.pin(view.def, m.id, machines.pinned.state);
  }
  const editMain = () => view?.main_of && controllerManager.docClient.command({ op: "narrative_probe", node: view.main_of, action: "select" });

  // ── The strip: states in order, each gap holding both directions ──────
  function between(m: Machine, a: string, b: string) {
    const p = pairInfo(m, { from: a, to: b });
    const t = transitionOf(m, p?.transition);
    return { p, t, shared: !!t && !own(t, { from: a, to: b }), label: t ? `${t.name ?? "change"} · ${Math.round(p?.ms ?? 0)} ms` : "snaps" };
  }

  // ── The graph: states round a circle, an edge per explicit transition ─
  const GW = 260, GH = 200;
  function nodes(m: Machine) {
    const ns = names(m);
    const n = ns.length;
    return ns.map((name, i) => {
      const a = n === 2 ? (i === 0 ? Math.PI : 0) : -Math.PI / 2 + (i * 2 * Math.PI) / n;
      return { name, x: GW / 2 + Math.cos(a) * (GW / 2 - 50), y: GH / 2 + Math.sin(a) * (GH / 2 - 26) };
    });
  }
  // An edge per pair of states that has a transition: solid for the
  // pair's own, dashed for a shared (any-state) one.
  function edges(m: Machine) {
    const at = Object.fromEntries(nodes(m).map((n) => [n.name, n]));
    return m.pairs
      .filter((q) => q.transition && at[q.from] && at[q.to])
      .map((q) => {
        const t = transitionOf(m, q.transition)!;
        const shared = !own(t, q);
        const a = at[q.from], b = at[q.to];
        const dx = b.x - a.x, dy = b.y - a.y, len = Math.hypot(dx, dy) || 1;
        // Offset to the right of travel, so the two directions sit apart;
        // trim to the boxes' edges.
        const ox = (-dy / len) * 7, oy = (dx / len) * 7, trim = 34;
        return { key: `${q.from}>${q.to}`, from: q.from, to: q.to, t, shared, x1: a.x + (dx / len) * trim + ox, y1: a.y + (dy / len) * 16 + oy, x2: b.x - (dx / len) * trim + ox, y2: b.y - (dy / len) * 16 + oy };
      });
  }
  const wild = (m: Machine) => m.transitions.filter((t) => t.from === "*" || t.to === "*");
  const keyActivate = (e: KeyboardEvent, f: () => void) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      f();
    }
  };

  // ── Tracks ────────────────────────────────────────────────────────────
  const LABEL_W = 132;
  function ticks(ms: number) {
    const steps = [10, 20, 50, 100, 200, 250, 500, 1000, 2000, 5000, 10000];
    const step = steps.find((s) => ms / s <= 8) ?? 10000;
    const out: number[] = [];
    for (let t = 0; t <= ms + 1e-6; t += step) out.push(t);
    return out;
  }
  const curveOf = (ease: string | null) => CURVES.find((c) => c.value === ease)?.f ?? (parseBezier(ease) ? bezier(...parseBezier(ease)!) : (t: number) => t);

  // A bar being dragged: the tween, what moves, and the slice so far.
  let drag = $state<{ tween: string; part: "start" | "end" | "both"; x0: number; width: number; a: number; b: number; start: number; end: number } | null>(null);
  function grab(e: PointerEvent, w: Tween, part: "start" | "end" | "both") {
    if (!editable || w.shaped) return;
    const track = (e.currentTarget as HTMLElement).closest(".track") as HTMLElement;
    const r = track.getBoundingClientRect();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    e.stopPropagation();
    const a = w.start ?? 0, b = w.end ?? 1;
    drag = { tween: w.id, part, x0: e.clientX, width: r.width, a, b, start: a, end: b };
  }
  function moveBar(e: PointerEvent) {
    if (!drag) return;
    const d = (e.clientX - drag.x0) / drag.width;
    const snap = (v: number) => Math.round(Math.max(0, Math.min(1, v)) * 100) / 100;
    if (drag.part === "both") {
      const w = drag.b - drag.a;
      const a = snap(Math.max(0, Math.min(1 - w, drag.a + d)));
      drag = { ...drag, start: a, end: snap(a + w) };
    } else if (drag.part === "start") {
      drag = { ...drag, start: snap(Math.min(drag.b - 0.01, drag.a + d)) };
    } else {
      drag = { ...drag, end: snap(Math.max(drag.a + 0.01, drag.b + d)) };
    }
  }
  async function dropBar() {
    if (!drag) return;
    const d = drag;
    if (d.start !== d.a || d.end !== d.b) await run(machines.patch(d.tween, { "tween.start": String(d.start), "tween.end": String(d.end) }, "retime tween"));
    drag = null;
  }
  const slice = (w: Tween) => (drag?.tween === w.id ? [drag.start, drag.end] : [w.start ?? 0, w.end ?? 1]);

  // The playhead: progress 0..1 through the transition shown, previewed on
  // the main (the pin's scrub). `scrubbing` is the machine it is on.
  let scrubbing = $state<string | null>(null);
  let progress = $state(0);
  let playing = 0;
  let isPlaying = $state(false);
  function stopPlay() {
    cancelAnimationFrame(playing);
    isPlaying = false;
  }
  // Stopping the preview anywhere (the canvas chip) stops playback too.
  $effect(() => {
    if (!machines.pinned?.scrub) {
      stopPlay();
      scrubbing = null;
    }
  });
  function scrubTo(m: Machine, p: { from: string; to: string }, value: number) {
    if (!view || !editable) return;
    progress = Math.max(0, Math.min(1, value));
    scrubbing = m.id;
    void machines.pin(view.def, m.id, machines.pinned?.state ?? m.pinned ?? p.from, { from: p.from, to: p.to, progress });
  }
  // A press on the ruler or a track (not on a bar or a control) scrubs.
  let scrubDown = false;
  function scrubFrom(e: PointerEvent, m: Machine, p: { from: string; to: string }) {
    if (!editable || drag) return;
    if (e.type === "pointerdown") {
      const at = e.target as Element;
      if (at.closest(".bar, .row-tools, button, input") || !at.closest(".ruler, .track")) return;
      scrubDown = true;
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      stopPlay();
    } else if (!scrubDown) return;
    const lane = (e.currentTarget as HTMLElement).querySelector(".lane-area") as HTMLElement | null;
    const r = (lane ?? (e.currentTarget as HTMLElement)).getBoundingClientRect();
    scrubTo(m, p, (e.clientX - r.left) / r.width);
  }
  function play(m: Machine, p: { from: string; to: string }, ms: number) {
    stopPlay();
    isPlaying = true;
    const t0 = performance.now();
    const step = () => {
      const k = Math.min(1, (performance.now() - t0) / Math.max(ms, 1));
      scrubTo(m, p, k);
      if (k < 1 && isPlaying) playing = requestAnimationFrame(step);
      else isPlaying = false;
    };
    playing = requestAnimationFrame(step);
  }
  // An instance going through a transition shows where it is.
  const liveProgress = (m: Machine, t: Transition | undefined) => (!editable && t && m.current?.transition === t.id ? (m.current.progress ?? null) : null);
  const head = (m: Machine, t: Transition | undefined) => liveProgress(m, t) ?? (scrubbing === m.id && machines.pinned?.scrub ? progress : null);
</script>

<div class="machine-panel canvas-ui" data-machine-panel>
  <div class="head">
    <span class="title">States{#if view?.name}<span class="muted"> · {view.name}</span>{/if}</span>
    {#if view && !editable && view.entity}
      <span class="hint">An instance: its state now is marked; timing is the component's</span>
      {#if view.main_of}<Button small variant="outline" onclick={editMain} title="Select the component's main to edit its states and timing">Edit main</Button>{/if}
    {/if}
    <span class="grow"></span>
    {#if error}<span class="error" role="alert">{error}</span>{/if}
    {#if view?.machines.length}
      <ToggleGroup.Root type="single" class="seg" value={machines.layoutChoice} aria-label="Layout" onValueChange={(v) => v && (machines.layoutChoice = v as typeof machines.layoutChoice)}>
        <ToggleGroup.Item class="seg-item" value="auto" title="Strip for a chain or a loop, graph when it branches">Auto</ToggleGroup.Item>
        <ToggleGroup.Item class="seg-item" value="strip">Strip</ToggleGroup.Item>
        <ToggleGroup.Item class="seg-item" value="graph">Graph</ToggleGroup.Item>
      </ToggleGroup.Root>
    {/if}
    {#if editable && view}<Button small variant="outline" onclick={() => run(machines.author("add_state", { def: view!.def }))} title="Add a state; edits made while it is previewed become its look">+ State</Button>{/if}
    <Button small variant="outline" onclick={onTimeline} title="Back to the document's timeline">Timeline</Button>
  </div>

  {#if view && editable && machines.pinned?.def === view.def}
    {@const pin = machines.pinned}
    {@const pm = view.machines.find((x) => x.id === pin.machine)}
    {@const pst = pm?.states.find((x) => x.name === pin.state)}
    <div class="banner" role="status">
      <span class="dot"></span>
      {#if pin.scrub}
        <span>Showing <strong>{pin.scrub.from} → {pin.scrub.to}</strong> at {Math.round(pin.scrub.progress * 100)}%: a moment between two states, so edits now change the parts, not a state.</span>
      {:else}
        <span>Previewing</span>
        {#if pst}<input class="rename" aria-label="Rename state {pin.state}" value={pin.state} onchange={(e) => run(machines.author("rename_state", { state: pst.id, name: e.currentTarget.value }))} />{/if}
        <span>Edit the parts on the canvas to shape this state.</span>
      {/if}
      <span class="grow"></span>
      <Button small variant="outline" onclick={() => { stopPlay(); void machines.pin(pin.def, pin.machine, null); }}>Stop previewing</Button>
    </div>
  {/if}

  {#if !view}
    <div class="empty">Select a component's main, a part of it, or an instance to see its states.</div>
  {:else if !view.machines.length}
    <div class="empty">
      {#if editable}<span><strong>{view.name ?? "This component"}</strong> has no states yet.</span>
        <Button small variant="primary" onclick={() => run(machines.author("add_state", { def: view!.def }))}>+ State</Button>
      {:else}<span><strong>{view.name ?? "This component"}</strong> has no states.</span>{/if}
    </div>
  {:else}
    <div class="body">
      {#each view.machines as m (m.id)}
        {@const p = pairOf(m)}
        {@const info = p ? pairInfo(m, p) : undefined}
        {@const t = transitionOf(m, info?.transition)}
        {@const ms = info?.ms ?? t?.ms ?? 300}
        <section class="machine" class:graph={layoutOf(m) === "graph"} aria-label="Machine {m.port}">
          {#if layoutOf(m) === "graph"}
            <div class="graph-pane">
              <div class="label">{m.port}</div>
              <svg width={GW} height={GH} viewBox="0 0 {GW} {GH}" role="group" aria-label="States of {m.port}">
                <defs><marker id="arrow-{m.id}" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L8,4 L0,8 z" class="arrowhead" /></marker></defs>
                {#each edges(m) as e (e.key)}
                  {@const on = !!p && e.from === p.from && e.to === p.to}
                  <g class="edge" class:on class:shared={e.shared} role="button" tabindex="0" aria-label="{e.from} to {e.to}{e.shared ? ' (shared timing)' : ''}" onclick={() => choose(m, e.from, e.to)} onkeydown={(ev) => keyActivate(ev, () => choose(m, e.from, e.to))}>
                    <title>{e.from} → {e.to}: {e.t.name ?? "change"}{e.shared ? " (shared)" : ""}</title>
                    <line x1={e.x1} y1={e.y1} x2={e.x2} y2={e.y2} class="hit" />
                    <line x1={e.x1} y1={e.y1} x2={e.x2} y2={e.y2} marker-end="url(#arrow-{m.id})" />
                  </g>
                {/each}
                {#each nodes(m) as n (n.name)}
                  <g class="node" class:lit={lit(m, n.name)} role="button" tabindex="0" aria-label="State {n.name}" onclick={() => clickState(m, n.name)} onkeydown={(ev) => keyActivate(ev, () => clickState(m, n.name))}>
                    <rect x={n.x - 34} y={n.y - 14} width="68" height="28" rx="14" />
                    <text x={n.x} y={n.y + 4} text-anchor="middle">{n.name.length > 10 ? n.name.slice(0, 9) + "…" : n.name}</text>
                  </g>
                {/each}
              </svg>
              {#each wild(m) as w (w.id)}<div class="wild">Dashed: {w.name ?? "change"} ({describe(w)}) · {Math.round(w.ms ?? 0)} ms</div>{/each}
            </div>
          {:else}
            <div class="strip" role="group" aria-label="States of {m.port}">
              <span class="label">{m.port}</span>
              {#each names(m) as s, i (s)}
                <button class="state" class:lit={lit(m, s)} aria-pressed={lit(m, s)} onclick={() => clickState(m, s)}
                  title={editable ? (lit(m, s) ? "Previewing on the main: edits shape this state. Click to stop." : "Preview this state on the main and edit its look") : `Set this instance to ${s} (where its state is keyed, this keys it at the playhead)`}>
                  {#if !editable && m.current?.state === s}<span class="dot"></span>{/if}{s}
                </button>
                {#if i < names(m).length - 1}
                  {@const next = names(m)[i + 1]}
                  {@const fwd = between(m, s, next)}
                  {@const back = between(m, next, s)}
                  <span class="gap">
                    <button class="arrow" class:shared={fwd.shared} class:on={p?.from === s && p?.to === next} onclick={() => choose(m, s, next)} title="{s} → {next}{fwd.shared ? ' (shared timing)' : ''}"><span>{fwd.label}</span><ArrowRight size={12} strokeWidth={1.75} /></button>
                    <button class="arrow back" class:shared={back.shared} class:on={p?.from === next && p?.to === s} onclick={() => choose(m, next, s)} title="{next} → {s}{back.shared ? ' (shared timing)' : ''}"><ArrowRight size={12} strokeWidth={1.75} /><span>{back.label}</span></button>
                  </span>
                {/if}
              {/each}
              {#if m.order?.[1] && names(m).length > 2}
                {@const last = names(m)[names(m).length - 1]}
                {@const first = names(m)[0]}
                {@const loop = between(m, last, first)}
                <span class="gap">
                  <button class="arrow" class:shared={loop.shared} class:on={p?.from === last && p?.to === first} onclick={() => choose(m, last, first)} title="{last} → {first} (back to the start)"><span>{loop.label}</span><ArrowRight size={12} strokeWidth={1.75} /></button>
                </span>
                <span class="loop-end">↺ {first}</span>
              {/if}
            </div>
          {/if}

          {#if p}
            <div class="tracks">
              <div class="tracks-head">
                <div class="pair">
                  <Select label="From" value={p.from} items={names(m).filter((n) => n !== p.to).map((n) => ({ value: n, label: n }))} onChange={(v) => choose(m, v, p.to)} />
                  <ArrowRight size={13} strokeWidth={1.75} />
                  <Select label="To" value={p.to} items={names(m).filter((n) => n !== p.from).map((n) => ({ value: n, label: n }))} onChange={(v) => choose(m, p.from, v)} />
                </div>
                {#if t}
                  {#if own(t, p)}
                    <span class="scope own" title="Only {p.from} → {p.to} uses this timing">This pair · {t.name ?? ""}</span>
                    {#if editable && sharedFor(m, p, t)}
                      <Button small variant="outline" title="Remove this pair's own timing: it uses the shared one again" onclick={() => run(machines.author("remove_node", { node: t!.id }))}>Use shared</Button>
                    {/if}
                  {:else}
                    <span class="scope shared" title="Edits here change every pair that uses {t.name ?? 'this transition'}">Shared · {describe(t)}: edits change every pair using it</span>
                    {#if editable}
                      <Button small variant="outline" title="Give {p.from} → {p.to} its own length, easing and slices (starting from these)" onclick={() => run(machines.author("add_transition", { def: view!.def, machine: m.id, from: p.from, to: p.to }))}>Customize this pair</Button>
                    {/if}
                  {/if}
                  <span class="grow"></span>
                  <label class="ms" title={t.ms_formula ? "Set by a formula (an input or an override): edit it in the inspector" : own(t, p) ? "How long it takes" : `How long it takes (shared by ${describe(t)})`}>
                    {#if t.ms_formula}<span class="fx">ƒ</span>{/if}
                    <input inputmode="decimal" aria-label="Length of {t.name}" disabled={!editable || t.ms_formula} value={editable ? (t.ms ?? "") : Math.round(ms)}
                      onchange={(e) => run(machines.patch(t!.id, { "transition.ms": e.currentTarget.value }, "transition length"))} /><span>ms</span>
                  </label>
                  <div class="ease"><EasingField label="Easing of {t.name}" value={t.ease} disabled={!editable} onChange={(v) => run(machines.patch(t!.id, { "transition.ease": v }, "transition easing"))} /></div>
                  {#if editable}
                    {#if isPlaying}<IconButton icon={Square} label="Stop playing" onclick={stopPlay} />
                    {:else}<IconButton icon={Play} label="Play {p.from} to {p.to} on the main" onclick={() => play(m, p, ms)} />{/if}
                  {/if}
                {:else}
                  <span class="uses">No transition: {p.from} → {p.to} snaps.</span>
                  {#if editable}<Button small variant="outline" onclick={() => run(machines.author("add_transition", { def: view!.def, machine: m.id, from: p.from, to: p.to }))}>Add transition</Button>{/if}
                {/if}
              </div>
              {#if t}
                {@const h = head(m, t)}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div class="grid" class:scrubbable={editable} onpointerdown={(e) => scrubFrom(e, m, p)} onpointermove={(e) => scrubFrom(e, m, p)} onpointerup={() => (scrubDown = false)} onpointercancel={() => (scrubDown = false)}>
                  <div class="ruler" style:--label-w="{LABEL_W}px">
                    <span class="row-label">{Math.round(ms)} ms</span>
                    <div class="lane-area">
                      {#each ticks(ms) as tk (tk)}<span class="tick" style:left="{(tk / ms) * 100}%">{tk >= 1000 ? `${tk / 1000}s` : `${tk}`}</span>{/each}
                    </div>
                  </div>
                  {#each view.channels as c (c.id)}
                    {@const w = t.tweens.find((x) => x.channel === c.name)}
                    <div class="row" style:--label-w="{LABEL_W}px">
                      <span class="row-label" title={c.name}>{c.name}</span>
                      <div class="track">
                        {#if w}
                          {@const [a, b] = slice(w)}
                          <!-- svelte-ignore a11y_no_static_element_interactions -->
                          <div class="bar" class:shaped={w.shaped} class:locked={!editable} style:left="{a * 100}%" style:width="{Math.max(b - a, 0.005) * 100}%"
                            title={w.shaped ? "Shaped by a formula (tween.shape)" : `${Math.round(a * ms)}–${Math.round(b * ms)} ms`}
                            onpointerdown={(e) => grab(e, w, "both")} onpointermove={moveBar} onpointerup={dropBar} onpointercancel={dropBar}>
                            {#if !w.shaped}
                              <svg class="curve" viewBox="0 0 100 20" preserveAspectRatio="none" aria-hidden="true"><path d={(() => { const f = curveOf(w.ease); let d = ""; for (let i = 0; i <= 24; i++) { const x = i / 24; d += `${i ? "L" : "M"}${(x * 100).toFixed(1)},${(18 - f(x) * 16).toFixed(1)}`; } return d; })()} /></svg>
                              {#if editable}
                                <!-- svelte-ignore a11y_no_static_element_interactions -->
                                <span class="grip start" onpointerdown={(e) => grab(e, w, "start")} onpointermove={moveBar} onpointerup={dropBar} onpointercancel={dropBar}></span>
                                <!-- svelte-ignore a11y_no_static_element_interactions -->
                                <span class="grip end" onpointerdown={(e) => grab(e, w, "end")} onpointermove={moveBar} onpointerup={dropBar} onpointercancel={dropBar}></span>
                              {/if}
                            {:else}<span class="f">ƒ shape</span>{/if}
                          </div>
                        {:else}
                          <div class="follow" title="Follows the transition's easing over all of it">follows the transition</div>
                        {/if}
                        {#if h !== null}<span class="playhead" style:left="{h * 100}%"></span>{/if}
                      </div>
                      <span class="row-tools">
                        {#if w && editable}
                          {#if !w.shaped}<div class="ease small"><EasingField label="Easing of {c.name}" custom={false} value={w.ease} onChange={(v) => run(machines.patch(w.id, { "tween.ease": v }, "tween easing"))} /></div>{/if}
                          <IconButton icon={X} label="Remove {c.name}'s own timing" title="Follow the transition again" onclick={() => run(machines.author("remove_node", { node: w.id }))} />
                        {:else if editable}
                          <IconButton icon={Plus} label="Give {c.name} its own slice" title="Its own slice and easing in this transition" onclick={() => run(machines.author("add_tween", { transition: t!.id, channel: c.name }))} />
                        {/if}
                      </span>
                    </div>
                  {:else}
                    <div class="row empty-row">No channels yet: preview a state and edit a part, and the edit becomes a channel.</div>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        </section>
      {/each}
    </div>
  {/if}
</div>

<style>
  .machine-panel { position: absolute; inset: 0; display: flex; flex-direction: column; background: var(--bg); color: var(--fg); font-size: 12px; pointer-events: auto; user-select: none; overflow: hidden; z-index: 3; }
  .head { display: flex; align-items: center; gap: 8px; height: 38px; padding: 0 10px; border-bottom: 1px solid var(--border); flex: none; }
  .title { font-weight: 600; white-space: nowrap; }
  .muted, .hint { color: var(--fg-muted); font-weight: 400; }
  .hint { font-size: 11px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .grow { flex: 1; }
  .error { color: #d9534f; font-size: 11px; max-width: 260px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { display: flex; align-items: center; gap: 10px; padding: 16px; color: var(--fg-muted); }
  .body { flex: 1; min-height: 0; overflow: auto; display: flex; flex-direction: column; }
  .machine { border-bottom: 1px solid var(--border); }
  .machine { flex: none; }
  .machine.graph { display: grid; grid-template-columns: auto 1fr; flex: 1 0 150px; min-height: 150px; }
  .label { color: var(--fg-muted); font-size: 11px; font-family: var(--font-mono, ui-monospace, monospace); }

  :global(.seg) { display: flex; padding: 2px; gap: 2px; background: var(--bg-sunken); border-radius: 6px; }
  :global(.seg-item) { border: 0; border-radius: 4px; background: transparent; color: var(--fg-muted); font-size: 11px; padding: 3px 8px; cursor: pointer; }
  :global(.seg-item:hover) { color: var(--fg); }
  :global(.seg-item[data-state="on"]) { background: var(--bg); color: var(--fg); box-shadow: 0 0 0 1px var(--border); }
  :global(.seg-item:focus-visible) { outline: 1px solid var(--accent); }

  /* The strip */
  .strip { display: flex; align-items: center; gap: 6px; padding: 10px 12px; overflow-x: auto; }
  .strip .label { margin-right: 4px; }
  .state { display: inline-flex; align-items: center; gap: 6px; height: 28px; padding: 0 14px; border: 1px solid var(--border-strong); border-radius: 14px; background: var(--bg-sunken); color: var(--fg); font: inherit; font-weight: 600; cursor: pointer; white-space: nowrap; }
  .state:hover { border-color: var(--accent); }
  .state.lit { background: var(--accent-soft); border-color: var(--accent); }
  .state:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--accent); }
  .gap { display: flex; flex-direction: column; gap: 2px; }
  .arrow { display: inline-flex; align-items: center; gap: 4px; height: 20px; padding: 0 8px; border: 1px solid transparent; border-radius: 10px; background: transparent; color: var(--fg-muted); font: inherit; font-size: 11px; cursor: pointer; white-space: nowrap; }
  .arrow.back :global(svg) { transform: scaleX(-1); }
  .arrow:hover { background: var(--hover); color: var(--fg); }
  .arrow.on { border-color: var(--accent); color: var(--fg); background: var(--bg-sunken); }
  .arrow.shared span { font-style: italic; }
  .loop-end { color: var(--fg-muted); font-size: 11px; white-space: nowrap; }
  .banner { display: flex; align-items: center; gap: 8px; padding: 5px 10px; background: rgba(255, 153, 51, 0.16); border-bottom: 1px solid rgba(255, 153, 51, 0.5); font-size: 12px; flex: none; }
  .banner .dot { flex: none; background: #ff9933; }
  .banner .rename { width: 110px; height: 24px; padding: 0 6px; border: 1px solid var(--border-strong); border-radius: 4px; background: var(--bg); color: var(--fg); font: inherit; font-weight: 600; }
  .banner .rename:focus { outline: none; border-color: var(--accent); }
  .scope { font-size: 11px; padding: 2px 8px; border-radius: 9px; white-space: nowrap; }
  .scope.shared { background: rgba(255, 153, 51, 0.16); color: var(--fg); }
  .scope.own { background: var(--accent-soft); color: var(--fg); }
  .fx { color: var(--accent); font-style: italic; }
  .arrow:focus-visible { outline: 1px solid var(--accent); }

  /* The graph */
  .graph-pane { padding: 6px 10px; border-right: 1px solid var(--border); overflow: auto; }
  .graph-pane svg { display: block; }
  .node rect { fill: var(--bg-sunken); stroke: var(--border-strong); stroke-width: 1; }
  .node text { fill: var(--fg); font-size: 11px; font-weight: 600; }
  .node { cursor: pointer; }
  .node:hover rect { stroke: var(--accent); }
  .node.lit rect { fill: var(--accent-soft); stroke: var(--accent); }
  .edge { cursor: pointer; }
  .edge line { stroke: var(--fg-muted); stroke-width: 1.5; }
  .edge .hit { stroke: transparent; stroke-width: 10; }
  .edge.shared line:not(.hit) { stroke-dasharray: 4 3; opacity: 0.7; }
  .edge:hover line:not(.hit), .edge.on line:not(.hit) { stroke: var(--accent); stroke-width: 2; opacity: 1; }
  .edge:focus-visible, .node:focus-visible { outline: none; }
  .edge:focus-visible line:not(.hit) { stroke: var(--accent); stroke-width: 2.5; }
  .node:focus-visible rect { stroke: var(--accent); stroke-width: 2; }
  .arrowhead { fill: var(--fg-muted); }
  .wild { color: var(--fg-muted); font-size: 11px; margin-top: 4px; }

  /* Tracks */
  .tracks { padding: 6px 12px 12px; min-width: 0; }
  .tracks-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; min-height: 34px; }
  .pair { display: flex; align-items: center; gap: 6px; width: 260px; }
  .pair :global(> *:not(svg)) { flex: 1; min-width: 0; }
  .uses { color: var(--fg-muted); font-size: 11px; }
  .ms { display: flex; align-items: center; gap: 4px; height: 26px; padding: 0 8px; border-radius: 5px; background: var(--bg-sunken); color: var(--fg-muted); }
  .ms input { width: 52px; border: 0; background: transparent; color: var(--fg); font: inherit; outline: none; text-align: right; }
  .ms:focus-within { box-shadow: inset 0 0 0 1px var(--accent); }
  .ease { width: 160px; }
  .ease.small { width: 120px; }
  .grid { margin-top: 6px; position: relative; }
  .grid.scrubbable .ruler { cursor: ew-resize; }
  .ruler, .row { display: grid; grid-template-columns: var(--label-w) 1fr 160px; align-items: center; gap: 0 8px; }
  .ruler { height: 22px; color: var(--fg-muted); font-size: 10px; }
  .lane-area { position: relative; height: 100%; border-bottom: 1px solid var(--border); }
  .tick { position: absolute; bottom: 2px; padding-left: 3px; border-left: 1px solid var(--border-strong); line-height: 1; }
  .row { height: 30px; }
  .row-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--fg-muted); font-family: var(--font-mono, ui-monospace, monospace); font-size: 11px; }
  .track { position: relative; height: 20px; background: var(--bg-sunken); border-radius: 4px; }
  .bar { position: absolute; top: 0; bottom: 0; border-radius: 4px; background: var(--accent-soft); box-shadow: inset 0 0 0 1px var(--accent); cursor: grab; touch-action: none; }
  .bar.locked { cursor: default; }
  .bar.shaped { background: var(--hover); box-shadow: inset 0 0 0 1px var(--border-strong); cursor: default; }
  .bar .curve { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; }
  .bar .curve path { fill: none; stroke: var(--accent); stroke-width: 1.5; vector-effect: non-scaling-stroke; }
  .bar .f { position: absolute; left: 6px; top: 3px; font-size: 11px; color: var(--fg-muted); }
  .grip { position: absolute; top: 0; bottom: 0; width: 7px; cursor: ew-resize; }
  .grip.start { left: -3px; }
  .grip.end { right: -3px; }
  .grip:hover { background: var(--accent); opacity: 0.5; border-radius: 3px; }
  .follow { position: absolute; inset: 0; display: flex; align-items: center; padding-left: 8px; color: var(--fg-faint, var(--fg-muted)); font-size: 11px; border: 1px dashed var(--border); border-radius: 4px; }
  .playhead { position: absolute; top: -4px; bottom: -4px; width: 2px; margin-left: -1px; background: #ff9933; pointer-events: none; }
  .row-tools { display: flex; align-items: center; gap: 4px; justify-content: flex-end; }
  .empty-row { display: block; color: var(--fg-muted); padding: 8px 0; height: auto; }
</style>
