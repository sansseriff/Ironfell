<script lang="ts">
  // The state editor (plans/specs/machines.md §6): the timeline drilled into
  // one component, at the timeline panel's own size. A mode, entered on
  // purpose (the inspector's Edit states…, a double-click on a state lane or
  // a transition band) and left with Done or the breadcrumb; selection never
  // switches it.
  //
  // One header row: the breadcrumb, the states as pills (click: edit that
  // state's look; ⋯: rename, make default, delete) with the transitions as
  // arrows between them (click: time that pair), + State, Graph, Done. The
  // body has the timeline's grammar, a name column and a wide area:
  // - a **state** shows the pose table, a row per channel and a column per
  //   state, the edited state's column editable; edits on the canvas record
  //   into it;
  // - a **transition** shows its controls in the corner (pair, length,
  //   easing, play), its scope (shared or this pair's own) over a ruler in
  //   ms, and a row per channel with its slice as a bar to drag.
  // A branching machine also gets a graph pane at the left.
  import { onDestroy } from "svelte";
  import { DropdownMenu, Popover } from "bits-ui";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import House from "@lucide/svelte/icons/house";
  import Network from "@lucide/svelte/icons/network";
  import Play from "@lucide/svelte/icons/play";
  import Plus from "@lucide/svelte/icons/plus";
  import Square from "@lucide/svelte/icons/square";
  import Button from "../../lib/ui/Button.svelte";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import Select from "../../lib/ui/Select.svelte";
  import EasingField, { CURVES, bezier, parseBezier } from "../../lib/ui/EasingField.svelte";
  import { machines, type Focus, type Machine, type Pair, type Transition, type Tween } from "../../lib/machines.svelte";

  const view = $derived(machines.view);
  let error = $state("");
  async function run(p: Promise<{ ok: boolean; error?: string }>) {
    error = "";
    const r = await p;
    if (!r.ok) error = r.error ?? "That did not work";
    return r;
  }
  onDestroy(() => cancelAnimationFrame(playing));

  // One machine at a time (the first, or the one chosen).
  let machineId = $state<string | null>(null);
  const m = $derived(view?.machines.find((x) => x.id === machineId) ?? view?.machines[0] ?? null);
  const names = (mm: Machine) => mm.order?.[0] ?? mm.states.map((s) => s.name ?? "").filter(Boolean);
  // What the editor shows, falling back when it names a state or pair that
  // is gone (deleted, renamed).
  const focusOf = (mm: Machine): Focus => {
    const f = machines.focus[mm.id];
    const ns = names(mm);
    if (f?.kind === "pair" && ns.includes(f.from) && ns.includes(f.to)) return f;
    if (f?.kind === "state" && (f.state === null || ns.includes(f.state))) return f;
    return { kind: "state", state: mm.pinned && ns.includes(mm.pinned) ? mm.pinned : null };
  };
  const setFocus = (mm: Machine, f: Focus) => (machines.focus = { ...machines.focus, [mm.id]: f });
  const pairInfo = (mm: Machine, from: string, to: string): Pair | undefined => mm.pairs.find((x) => x.from === from && x.to === to);
  const transitionOf = (mm: Machine, id: string | null | undefined): Transition | undefined => mm.transitions.find((t) => t.id === id);
  const own = (t: Transition | undefined, from: string, to: string) => !!t && t.from === from && t.to === to;
  const sharedFor = (mm: Machine, from: string, to: string, t: Transition | undefined) =>
    mm.transitions.find((x) => x !== t && (x.from === "*" || x.from === from) && (x.to === "*" || x.to === to) && (x.from === "*" || x.to === "*"));
  const describe = (t: Transition) => (t.from === "*" && t.to === "*" ? "any change" : `${t.from === "*" ? "any" : t.from} → ${t.to === "*" ? "any" : t.to}`);
  const fmt = (v: number | null | undefined) => (v === null || v === undefined || !Number.isFinite(v) ? "–" : String(Math.round(v * 1000) / 1000));

  // ── States ────────────────────────────────────────────────────────────
  const recording = (mm: Machine, s: string) => machines.pinned?.machine === mm.id && machines.pinned.state === s && !machines.pinned.scrub;
  function editState(mm: Machine, s: string) {
    if (!view) return;
    stopPlay();
    setFocus(mm, { kind: "state", state: s });
    machines.paused = null;
    void run(machines.pin(view.def, mm.id, s));
  }
  function timePair(mm: Machine, from: string, to: string) {
    if (!view) return;
    stopPlay();
    setFocus(mm, { kind: "pair", from, to });
    // Timing is not shaping: no state records while a transition is shown.
    if (machines.pinned) void machines.pin(view.def, mm.id, null);
  }
  async function addState() {
    if (!view) return;
    const r = await run(machines.author("add_state", { def: view.def }));
    const mm = machines.view?.machines.find((x) => x.id === m?.id) ?? machines.view?.machines[0];
    if (r.ok && mm && machines.pinned) setFocus(mm, { kind: "state", state: machines.pinned.state });
  }
  let renaming = $state<string | null>(null);
  function between(mm: Machine, a: string, b: string) {
    const p = pairInfo(mm, a, b);
    const t = transitionOf(mm, p?.transition);
    return { t, shared: !!t && !own(t, a, b), label: t ? `${a} → ${b}: ${t.name ?? "change"} · ${Math.round(p?.ms ?? 0)} ms${!own(t, a, b) ? " (shared)" : ""}` : `${a} → ${b} snaps` };
  }

  // ── Graph ─────────────────────────────────────────────────────────────
  // The graph's drawing space; the SVG scales it to the pane (council:
  // the drawing should use the pane, and grow with the screen).
  const GW = 300, GH = 170;
  const showGraph = $derived(!!m && (machines.graph === "on" || (machines.graph === "auto" && m.layout === "graph")));
  function nodes(mm: Machine) {
    const ns = names(mm);
    const n = ns.length;
    return ns.map((name, i) => {
      const a = n === 2 ? (i === 0 ? Math.PI : 0) : -Math.PI / 2 + (i * 2 * Math.PI) / n;
      return { name, x: GW / 2 + Math.cos(a) * (GW / 2 - 50), y: GH / 2 + Math.sin(a) * (GH / 2 - 22) };
    });
  }
  function edges(mm: Machine) {
    const at = Object.fromEntries(nodes(mm).map((n) => [n.name, n]));
    return mm.pairs
      .filter((q) => q.transition && at[q.from] && at[q.to])
      .map((q) => {
        const t = transitionOf(mm, q.transition)!;
        const a = at[q.from], b = at[q.to];
        // Each direction bows to its own side, so a pair's two transitions
        // are two curves, each with its own label and hit area.
        const dx = b.x - a.x, dy = b.y - a.y, len = Math.hypot(dx, dy) || 1;
        const nx = -dy / len, ny = dx / len;
        const x1 = a.x + (dx / len) * 34 + nx * 4, y1 = a.y + (dy / len) * 14 + ny * 4;
        const x2 = b.x - (dx / len) * 34 + nx * 4, y2 = b.y - (dy / len) * 14 + ny * 4;
        const cx = (x1 + x2) / 2 + nx * 18, cy = (y1 + y2) / 2 + ny * 18;
        const lx = (x1 + x2) / 4 + cx / 2 + nx * 6, ly = (y1 + y2) / 4 + cy / 2 + ny * 6;
        return { key: `${q.from}>${q.to}`, from: q.from, to: q.to, t, shared: !own(t, q.from, q.to), d: `M${x1.toFixed(1)},${y1.toFixed(1)} Q${cx.toFixed(1)},${cy.toFixed(1)} ${x2.toFixed(1)},${y2.toFixed(1)}`, lx, ly, ms: q.ms };
      });
  }
  const keyActivate = (e: KeyboardEvent, f: () => void) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      f();
    }
  };

  // ── Transition tracks ─────────────────────────────────────────────────
  const LABEL_W = 160;
  function ticks(ms: number) {
    const steps = [10, 20, 50, 100, 200, 250, 500, 1000, 2000, 5000, 10000];
    const step = steps.find((s) => ms / s <= 8) ?? 10000;
    const out: number[] = [];
    for (let t = 0; t <= ms + 1e-6; t += step) out.push(t);
    return out;
  }
  const curveOf = (ease: string | null) => CURVES.find((c) => c.value === ease)?.f ?? (parseBezier(ease) ? bezier(...parseBezier(ease)!) : (t: number) => t);
  const curvePath = (ease: string | null) => {
    const f = curveOf(ease);
    let d = "";
    for (let i = 0; i <= 24; i++) {
      const x = i / 24;
      d += `${i ? "L" : "M"}${(x * 100).toFixed(1)},${(18 - f(x) * 16).toFixed(1)}`;
    }
    return d;
  };

  let drag = $state<{ tween: string; part: "start" | "end" | "both"; x0: number; width: number; a: number; b: number; start: number; end: number } | null>(null);
  function grab(e: PointerEvent, w: Tween, part: "start" | "end" | "both") {
    if (w.shaped) return;
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
    } else if (drag.part === "start") drag = { ...drag, start: snap(Math.min(drag.b - 0.01, drag.a + d)) };
    else drag = { ...drag, end: snap(Math.max(drag.a + 0.01, drag.b + d)) };
  }
  async function dropBar() {
    if (!drag) return;
    const d = drag;
    if (d.start !== d.a || d.end !== d.b) await run(machines.patch(d.tween, { "tween.start": String(d.start), "tween.end": String(d.end) }, "retime slice"));
    drag = null;
  }
  const slice = (w: Tween) => (drag?.tween === w.id ? [drag.start, drag.end] : [w.start ?? 0, w.end ?? 1]);

  // The playhead: the transition's progress, previewed on the main (the
  // pin's scrub: preview only, nothing records).
  let progress = $state(0);
  let playing = 0;
  let isPlaying = $state(false);
  function stopPlay() {
    cancelAnimationFrame(playing);
    isPlaying = false;
  }
  $effect(() => {
    if (!machines.pinned?.scrub) stopPlay();
  });
  function scrubTo(mm: Machine, from: string, to: string, value: number) {
    if (!view) return;
    progress = Math.max(0, Math.min(1, value));
    void machines.pin(view.def, mm.id, from, { from, to, progress });
  }
  let scrubDown = false;
  function scrubFrom(e: PointerEvent, mm: Machine, from: string, to: string) {
    if (drag) return;
    if (e.type === "pointerdown") {
      const at = e.target as Element;
      if (at.closest(".bar, button, input, .name-col") || !at.closest(".ruler, .track")) return;
      scrubDown = true;
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      stopPlay();
    } else if (!scrubDown) return;
    const lane = (e.currentTarget as HTMLElement).querySelector(".ruler .lane") as HTMLElement | null;
    const r = (lane ?? (e.currentTarget as HTMLElement)).getBoundingClientRect();
    scrubTo(mm, from, to, (e.clientX - r.left) / r.width);
  }
  function play(mm: Machine, from: string, to: string, ms: number) {
    stopPlay();
    isPlaying = true;
    const t0 = performance.now();
    const step = () => {
      const k = Math.min(1, (performance.now() - t0) / Math.max(ms, 1));
      scrubTo(mm, from, to, k);
      if (k < 1 && isPlaying) playing = requestAnimationFrame(step);
      else isPlaying = false;
    };
    playing = requestAnimationFrame(step);
  }
  const head = (mm: Machine, from: string, to: string) => {
    const s = machines.pinned?.scrub;
    return s && machines.pinned?.machine === mm.id && s.from === from && s.to === to ? s.progress : null;
  };
  const pose = (mm: Machine, s: string, c: string) => mm.states.find((x) => x.name === s)?.pose[c] ?? null;
</script>

<div class="state-editor canvas-ui" data-state-editor>
  <div class="head">
    <button class="crumb" onclick={() => machines.close()} title="Back to the document's timeline (Done)"><ChevronLeft size={14} strokeWidth={1.75} />Timeline</button>
    <span class="sep">›</span>
    <span class="comp" title="Editing this component's states: changes affect every instance">{view?.name ?? "component"}</span>
    {#if view && view.machines.length > 1}
      <div class="machine-pick"><Select label="Machine" value={m?.id ?? ""} items={view.machines.map((x) => ({ value: x.id, label: x.port }))} onChange={(v) => (machineId = v)} /></div>
    {/if}
    {#if m}
      <div class="pills" role="group" aria-label="States of {m.port}">
        {#each names(m) as s, i (s)}
          {@const st = m.states.find((x) => x.name === s)}
          {@const f = focusOf(m)}
          <span class="pill" class:rec={recording(m, s)} class:on={f.kind === "state" && f.state === s}>
            {#if renaming === st?.id}
              <!-- svelte-ignore a11y_autofocus -->
              <input class="rename" aria-label="Rename {s}" value={s} autofocus
                onkeydown={(e) => { if (e.key === "Escape") renaming = null; else if (e.key === "Enter") e.currentTarget.blur(); }}
                onblur={(e) => { const v = e.currentTarget.value.trim(); renaming = null; if (st && v && v !== s) void run(machines.author("rename_state", { state: st.id, name: v })); }} />
            {:else}
              <button class="pill-main" aria-pressed={recording(m, s)} onclick={() => editState(m, s)} ondblclick={() => (renaming = st?.id ?? null)}
                title={recording(m, s) ? `Editing ${s}: edits to the parts go into it` : `Edit ${s}'s look (double-click to rename)`}>
                {#if recording(m, s)}<span class="dot"></span>{/if}{s}{#if m.default === s}<span class="def" title="The default state: new instances start in it"><House size={10} strokeWidth={2} /></span>{/if}
              </button>
              <DropdownMenu.Root>
                <DropdownMenu.Trigger class="pill-menu" aria-label="{s} options" title="Rename, make default, delete"><Ellipsis size={12} strokeWidth={2} /></DropdownMenu.Trigger>
                <DropdownMenu.Portal>
                  <DropdownMenu.Content class="ui-menu" side="top" sideOffset={6} align="start">
                    <DropdownMenu.Item class="ui-menu-item" onSelect={() => (renaming = st?.id ?? null)}>Rename</DropdownMenu.Item>
                    <DropdownMenu.Item class="ui-menu-item" disabled={m.default === s} onSelect={() => st && run(machines.author("set_default_state", { state: st.id }))}>Make default</DropdownMenu.Item>
                    <DropdownMenu.Item class="ui-menu-item" disabled={names(m).length < 2} onSelect={() => st && run(machines.author("delete_state", { state: st.id }))}>Delete {s}</DropdownMenu.Item>
                  </DropdownMenu.Content>
                </DropdownMenu.Portal>
              </DropdownMenu.Root>
            {/if}
          </span>
          {#if !showGraph && (i < names(m).length - 1 || (m.order?.[1] && names(m).length > 2))}
            {@const next = names(m)[(i + 1) % names(m).length]}
            {@const fwd = between(m, s, next)}
            {@const back = between(m, next, s)}
            {@const f2 = focusOf(m)}
            <span class="gap">
              <button class="arrow" class:shared={fwd.shared} class:none={!fwd.t} class:on={f2.kind === "pair" && f2.from === s && f2.to === next} onclick={() => timePair(m, s, next)} title={fwd.label} aria-label={fwd.label}>{#if i === names(m).length - 1}<span class="loop">↺ {next}</span>{:else}<ArrowRight size={11} strokeWidth={2} />{/if}</button>
              {#if i < names(m).length - 1}
                <button class="arrow back" class:shared={back.shared} class:none={!back.t} class:on={f2.kind === "pair" && f2.from === next && f2.to === s} onclick={() => timePair(m, next, s)} title={back.label} aria-label={back.label}><ArrowRight size={11} strokeWidth={2} /></button>
              {/if}
            </span>
          {/if}
        {/each}
      </div>
    {/if}
    <IconButton icon={Plus} label="Add a state" title="Add a state: edit the parts to shape it" onclick={addState} />
    {#if m && names(m).length > 1}<IconButton icon={Network} label="Graph" title="Show the states as a graph" pressed={showGraph} onPressedChange={(on) => (machines.graph = on ? "on" : "off")} />{/if}
    <span class="grow"></span>
    {#if error}<span class="error" role="alert" title={error}>{error}</span>{/if}
    {#if machines.paused}
      <span class="paused" role="status">Recording into <strong>{machines.paused.state}</strong> paused <Button small variant="outline" onclick={() => machines.resume()}>Return to main</Button></span>
    {:else if machines.selected}
      <Button small variant="outline" onclick={() => machines.open(machines.selected!.def)} title="Edit that component's states instead">Edit {machines.selected.name ?? "selected"}'s states</Button>
    {/if}
    <Button small variant="primary" onclick={() => machines.close()}>Done</Button>
  </div>

  {#if !view}
    <div class="empty">Loading…</div>
  {:else if !m}
    <div class="empty"><span><strong>{view.name ?? "This component"}</strong> has no states yet. A state is a look the component can be in; instances move between them.</span>
      <Button small variant="primary" onclick={addState}>Add a state</Button></div>
  {:else}
    {@const f = focusOf(m)}
    <div class="body">
      {#if showGraph}
        <div class="graph-pane">
          <svg viewBox="0 0 {GW} {GH}" preserveAspectRatio="xMidYMid meet" role="group" aria-label="States of {m.port} as a graph">
            <defs><marker id="se-arrow-{m.id}" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0,0 L8,4 L0,8 z" class="arrowhead" /></marker></defs>
            {#each edges(m) as e (e.key)}
              {@const on = f.kind === "pair" && f.from === e.from && f.to === e.to}
              <g class="edge" class:on class:shared={e.shared} role="button" tabindex="0" aria-label="Time {e.from} to {e.to}" onclick={() => timePair(m, e.from, e.to)} onkeydown={(ev) => keyActivate(ev, () => timePair(m, e.from, e.to))}>
                <title>{e.from} → {e.to}: {e.t.name ?? "change"}{e.shared ? " (shared)" : ""}</title>
                <path d={e.d} class="hit" />
                <path d={e.d} class="line" marker-end="url(#se-arrow-{m.id})" />
                {#if !e.shared}<text class="elabel" x={e.lx} y={e.ly} text-anchor="middle">{Math.round(e.ms ?? 0)}</text>{/if}
              </g>
            {/each}
            {#each nodes(m) as n (n.name)}
              <g class="node" class:rec={recording(m, n.name)} role="button" tabindex="0" aria-label="Edit state {n.name}" onclick={() => editState(m, n.name)} onkeydown={(ev) => keyActivate(ev, () => editState(m, n.name))}>
                <title>{n.name}</title>
                <rect x={n.x - 36} y={n.y - 12} width="72" height="24" rx="12" />
                <text x={n.x} y={n.y + 4} text-anchor="middle">{n.name.length > 12 ? n.name.slice(0, 11) + "…" : n.name}</text>
              </g>
            {/each}
          </svg>
        </div>
      {/if}

      {#if f.kind === "state"}
        <!-- A state: the pose table (compact; the edited state's column
             editable), and beside it the transitions out of that state. -->
        <div class="state-view">
          {#if !view.channels.length}
            <div class="empty-wide">
              {#if machines.paused}
                <span>Recording into <strong>{machines.paused.state}</strong> is paused: the selection left the component.</span><Button small variant="outline" onclick={() => machines.resume()}>Return to main</Button>
              {:else if f.state && recording(m, f.state)}
                <span>No channels yet. Select a part of <strong>{view.name ?? "the component"}</strong> on the canvas and change a number (opacity, a size, Trim): it records into <strong>{f.state}</strong>, and a row appears here.</span>
              {:else}
                <span>Pick a state above to edit its look; each edit to a part becomes a row here.</span>
              {/if}
            </div>
          {:else}
            <div class="table-wrap">
              <table class="poses">
                <thead>
                  <tr>
                    <th class="name-col">Channel</th>
                    {#each names(m) as s (s)}
                      <th class:col-on={recording(m, s)}><button class="col" onclick={() => editState(m, s)} title="Edit {s}'s look">{#if recording(m, s)}<span class="dot"></span>{/if}{s}</button></th>
                    {/each}
                  </tr>
                </thead>
                <tbody>
                  {#each view.channels as c (c.id)}
                    <tr>
                      <td class="name-col" title={c.name}>{c.name}</td>
                      {#each names(m) as s (s)}
                        {@const st = m.states.find((x) => x.name === s)}
                        <td class:col-on={recording(m, s)} class:rest={!st?.own.includes(c.name)} title={st?.own.includes(c.name) ? undefined : "The channel's rest value: this state does not set it"}>
                          {#if recording(m, s) && st}
                            <input inputmode="decimal" aria-label="{c.name} in {s}" value={fmt(pose(m, s, c.name))} onchange={(e) => run(machines.patch(st.id, { [`args.${c.name}`]: e.currentTarget.value }, `set ${c.name} in ${s}`))} />
                          {:else}{fmt(pose(m, s, c.name))}{/if}
                        </td>
                      {/each}
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}
          {#if f.state}
            {@const out = m.pairs.filter((q) => q.from === f.state)}
            <aside class="outgoing" aria-label="Transitions from {f.state}">
              <div class="aside-title">From <strong>{f.state}</strong></div>
              {#each out as q (q.to)}
                {@const qt = transitionOf(m, q.transition)}
                <button class="out" onclick={() => timePair(m, q.from, q.to)} title="Time {q.from} → {q.to}">
                  <span>→ {q.to}</span>
                  <span class="out-meta">{qt ? `${Math.round(q.ms ?? 0)} ms · ${own(qt, q.from, q.to) ? "own" : "shared"}` : "snaps"}</span>
                </button>
              {/each}
            </aside>
          {/if}
        </div>
      {:else}
        {@const info = pairInfo(m, f.from, f.to)}
        {@const t = transitionOf(m, info?.transition)}
        {@const ms = info?.ms ?? t?.ms ?? 300}
        {@const h = head(m, f.from, f.to)}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="tracks" onpointerdown={(e) => t && scrubFrom(e, m, f.from, f.to)} onpointermove={(e) => t && scrubFrom(e, m, f.from, f.to)} onpointerup={() => (scrubDown = false)} onpointercancel={() => (scrubDown = false)}>
          <!-- One toolbar row (council: the corner was too cramped): the
               pair, play, length, easing, then the timing's scope. -->
          <div class="toolbar">
            <div class="pair-pick"><Select label="Transition" value={`${f.from}>${f.to}`} items={m.pairs.map((q) => ({ value: `${q.from}>${q.to}`, label: `${q.from} → ${q.to}` }))} onChange={(v) => { const [a, b] = v.split(">"); timePair(m, a, b); }} /></div>
            {#if t}
              {#if isPlaying}<IconButton icon={Square} label="Stop" onclick={stopPlay} />
              {:else}<IconButton icon={Play} label="Play {f.from} to {f.to} on the main" onclick={() => play(m, f.from, f.to, ms)} />{/if}
              <label class="ms" title={t.ms_formula ? "Set by a formula: edit it in the inspector" : "How long it takes"}>
                {#if t.ms_formula}<span class="fx">ƒ</span>{/if}
                <input inputmode="decimal" aria-label="Length of {f.from} to {f.to}" disabled={t.ms_formula} value={t.ms ?? ""} onchange={(e) => run(machines.patch(t.id, { "transition.ms": e.currentTarget.value }, "transition length"))} /><span>ms</span>
              </label>
              <div class="ease"><EasingField label="Easing of {f.from} to {f.to}" value={t.ease} onChange={(v) => run(machines.patch(t.id, { "transition.ease": v }, "transition easing"))} /></div>
            {/if}
            <span class="tsep"></span>
            {#if !t}
              <span class="scope none">No transition: {f.from} → {f.to} snaps</span>
              <Button small variant="outline" onclick={() => run(machines.author("add_transition", { def: view!.def, machine: m.id, from: f.from, to: f.to }))}>Add transition</Button>
            {:else if own(t, f.from, f.to)}
              <span class="scope own" title="Only {f.from} → {f.to} uses this timing">This pair</span>
              {#if sharedFor(m, f.from, f.to, t)}<Button small variant="outline" title="Remove this pair's own timing; it uses the shared one again" onclick={() => run(machines.author("remove_node", { node: t!.id }))}>Use shared</Button>{/if}
            {:else}
              <span class="scope shared" title="Every pair {describe(t)} covers uses these settings: edits here change them all">Shared timing · {describe(t)}</span>
              <Button small variant="outline" title="Give {f.from} → {f.to} its own timing, starting from these settings" onclick={() => run(machines.author("add_transition", { def: view!.def, machine: m.id, from: f.from, to: f.to }))}>Customize this pair</Button>
            {/if}
            {#if t}<span class="hint" title="Scrubbing or playing previews the transition on the main; nothing is recorded">preview only</span>{/if}
          </div>
          {#if t}
            <div class="ruler"><span class="name-col">{Math.round(ms)} ms</span><div class="lane">{#each ticks(ms) as tk (tk)}<span class="tick" style:left="{(tk / ms) * 100}%">{tk >= 1000 ? `${tk / 1000}s` : `${tk}`}</span>{/each}</div></div>
            <div class="rows">
              {#each view.channels as c (c.id)}
                {@const w = t.tweens.find((x) => x.channel === c.name)}
                <div class="row">
                  <span class="name-col chan">
                    <span class="cname" title={c.name}>{c.name}</span>
                    <span class="vals">{fmt(pose(m, f.from, c.name))} → {fmt(pose(m, f.to, c.name))}</span>
                    {#if w}
                      <Popover.Root>
                        <Popover.Trigger class="row-more" aria-label="{c.name}'s slice" title="Its slice: exact times, easing, remove"><Ellipsis size={12} strokeWidth={2} /></Popover.Trigger>
                        <Popover.Portal>
                          <Popover.Content class="property-popover slice-popover" side="top" align="start" sideOffset={6} aria-label="{c.name}'s slice">
                            <div class="slice-body">
                              <div class="slice-title">{c.name} in {f.from} → {f.to}</div>
                              {#if w.shaped}<p class="muted">Shaped by a formula (tween.shape).</p>
                              {:else}
                                <label class="ms">Start<input inputmode="decimal" value={Math.round((w.start ?? 0) * ms)} onchange={(e) => run(machines.patch(w.id, { "tween.start": String(Math.max(0, Math.min(1, Number(e.currentTarget.value) / ms))) }, "slice start"))} /><span>ms</span></label>
                                <label class="ms">End<input inputmode="decimal" value={Math.round((w.end ?? 1) * ms)} onchange={(e) => run(machines.patch(w.id, { "tween.end": String(Math.max(0, Math.min(1, Number(e.currentTarget.value) / ms))) }, "slice end"))} /><span>ms</span></label>
                                <EasingField label="Easing of {c.name}" value={w.ease} onChange={(v) => run(machines.patch(w.id, { "tween.ease": v }, "slice easing"))} />
                              {/if}
                              <Button small variant="outline" onclick={() => run(machines.author("remove_node", { node: w.id }))}>Follow the transition</Button>
                            </div>
                          </Popover.Content>
                        </Popover.Portal>
                      </Popover.Root>
                    {:else}
                      <button class="row-more" aria-label="Give {c.name} its own slice" title="Its own slice and easing in this transition" onclick={() => run(machines.author("add_tween", { transition: t!.id, channel: c.name }))}><Plus size={12} strokeWidth={2} /></button>
                    {/if}
                  </span>
                  <div class="track">
                    {#if w}
                      {@const [a, b] = slice(w)}
                      <!-- svelte-ignore a11y_no_static_element_interactions -->
                      <div class="bar" class:shaped={w.shaped} style:left="{a * 100}%" style:width="{Math.max(b - a, 0.005) * 100}%"
                        title={w.shaped ? "Shaped by a formula" : `${Math.round(a * ms)}–${Math.round(b * ms)} ms`}
                        onpointerdown={(e) => grab(e, w, "both")} onpointermove={moveBar} onpointerup={dropBar} onpointercancel={dropBar}>
                        {#if !w.shaped}
                          <svg class="curve" viewBox="0 0 100 20" preserveAspectRatio="none" aria-hidden="true"><path d={curvePath(w.ease)} /></svg>
                          <!-- svelte-ignore a11y_no_static_element_interactions -->
                          <span class="grip start" onpointerdown={(e) => grab(e, w, "start")} onpointermove={moveBar} onpointerup={dropBar} onpointercancel={dropBar}></span>
                          <!-- svelte-ignore a11y_no_static_element_interactions -->
                          <span class="grip end" onpointerdown={(e) => grab(e, w, "end")} onpointermove={moveBar} onpointerup={dropBar} onpointercancel={dropBar}></span>
                        {:else}<span class="f">ƒ</span>{/if}
                      </div>
                    {:else}
                      <div class="follow"><svg class="curve" viewBox="0 0 100 20" preserveAspectRatio="none" aria-hidden="true"><path d={curvePath(t.ease)} /></svg></div>
                    {/if}
                    {#if h !== null}<span class="playhead" style:left="{h * 100}%"></span>{/if}
                  </div>
                </div>
              {:else}
                <div class="empty-wide">No channels yet: edit a state's look first (pick a state above, then change a part on the canvas).</div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .state-editor { position: absolute; inset: 0; display: flex; flex-direction: column; background: var(--bg); color: var(--fg); font-size: 12px; pointer-events: auto; user-select: none; overflow: hidden; z-index: 3; }
  .head { display: flex; align-items: center; gap: 6px; height: 34px; padding: 0 8px; border-bottom: 1px solid var(--border); flex: none; white-space: nowrap; }
  .crumb { display: inline-flex; align-items: center; gap: 2px; height: 24px; padding: 0 6px 0 2px; border: 0; border-radius: 5px; background: transparent; color: var(--fg-muted); font: inherit; cursor: pointer; }
  .crumb:hover { background: var(--hover); color: var(--fg); }
  .crumb:focus-visible { outline: 1px solid var(--accent); }
  .sep { color: var(--fg-muted); }
  .comp { font-weight: 600; margin-right: 6px; }
  .grow { flex: 1; }
  .error { color: #d9534f; font-size: 11px; max-width: 240px; overflow: hidden; text-overflow: ellipsis; }
  .paused { display: inline-flex; align-items: center; gap: 6px; padding: 2px 4px 2px 8px; border-radius: 12px; background: rgba(255, 153, 51, 0.16); font-size: 11px; }
  .empty { display: flex; align-items: center; gap: 12px; padding: 14px; color: var(--fg-muted); }
  .muted { color: var(--fg-muted); }
  .dot { display: inline-block; width: 7px; height: 7px; border-radius: 50%; background: #ff9933; margin-right: 5px; }

  .pills { display: flex; align-items: center; gap: 2px; min-width: 0; overflow-x: auto; scrollbar-width: none; }
  .pill { display: inline-flex; align-items: center; height: 24px; border: 1px solid var(--border-strong); border-radius: 12px; background: var(--bg-sunken); }
  .pill.on { border-color: var(--accent); }
  .pill.rec { border-color: #ff9933; background: rgba(255, 153, 51, 0.16); }
  .pill-main { display: inline-flex; align-items: center; height: 22px; padding: 0 4px 0 10px; border: 0; background: transparent; color: var(--fg); font: inherit; font-weight: 600; cursor: pointer; }
  .pill-main:focus-visible { outline: 1px solid var(--accent); border-radius: 11px; }
  .def { display: inline-grid; place-items: center; color: var(--fg-muted); margin-left: 4px; }
  :global(.pill-menu) { display: grid; place-items: center; width: 20px; height: 22px; margin-right: 2px; border: 0; border-radius: 10px; background: transparent; color: var(--fg-muted); cursor: pointer; }
  :global(.pill-menu:hover) { background: var(--hover); color: var(--fg); }
  .rename { width: 90px; height: 20px; margin: 0 4px; padding: 0 6px; border: 1px solid var(--accent); border-radius: 10px; background: var(--bg); color: var(--fg); font: inherit; font-weight: 600; outline: none; }
  .gap { display: inline-flex; flex-direction: column; gap: 1px; margin: 0 1px; }
  .arrow { display: grid; place-items: center; min-width: 26px; height: 13px; padding: 0; border: 1px solid transparent; border-radius: 4px; background: transparent; color: var(--fg-muted); cursor: pointer; }
  .arrow.back :global(svg) { transform: scaleX(-1); }
  .arrow.shared { border-style: dashed; border-color: var(--border); }
  .arrow.none { opacity: 0.4; }
  .arrow:hover { background: var(--hover); color: var(--fg); }
  .arrow.on { border-style: solid; border-color: var(--accent); color: var(--accent); }
  .arrow:focus-visible { outline: 1px solid var(--accent); }

  .body { flex: 1; min-height: 0; display: flex; }
  .graph-pane { flex: none; width: 300px; min-width: 200px; max-width: 50%; padding: 2px 6px; border-right: 1px solid var(--border); resize: horizontal; overflow: hidden; display: flex; }
  .graph-pane svg { display: block; flex: 1; width: 100%; height: 100%; }
  .node rect { fill: var(--bg-sunken); stroke: var(--border-strong); }
  .node text { fill: var(--fg); font-size: 11px; font-weight: 600; }
  .node { cursor: pointer; }
  .node:hover rect, .node:focus-visible rect { stroke: var(--accent); }
  .node.rec rect { fill: rgba(255, 153, 51, 0.16); stroke: #ff9933; }
  .node:focus-visible, .edge:focus-visible { outline: none; }
  .edge { cursor: pointer; }
  .edge path { fill: none; }
  .edge .hit { stroke: transparent; stroke-width: 12; }
  .edge .line { stroke: var(--fg-muted); stroke-width: 1.5; }
  .edge.shared .line { stroke-dasharray: 4 3; opacity: 0.7; }
  .edge:hover .line, .edge.on .line, .edge:focus-visible .line { stroke: var(--accent); stroke-width: 2.25; opacity: 1; }
  .edge.on .elabel { fill: var(--accent); font-weight: 600; }
  .elabel { fill: var(--fg-muted); font-size: 9px; }
  .edge:focus, .node:focus { outline: none; }
  .loop { font-size: 10px; padding: 0 2px; white-space: nowrap; }
  .arrowhead { fill: var(--fg-muted); }

  .name-col { width: 200px; flex: none; box-sizing: border-box; padding: 0 8px; }
  .empty-wide { flex: 1; display: flex; align-items: center; gap: 12px; padding: 12px 14px; color: var(--fg-muted); line-height: 1.4; }
  .state-view { flex: 1; min-width: 0; display: flex; }
  .table-wrap { flex: 0 1 auto; min-width: 0; overflow: auto; }
  .poses { border-collapse: collapse; font-variant-numeric: tabular-nums; }
  .poses th, .poses td { height: 26px; padding: 0 12px; border-bottom: 1px solid var(--border); text-align: right; white-space: nowrap; min-width: 64px; }
  .poses .name-col { text-align: left; font-family: var(--font-mono, ui-monospace, monospace); font-size: 11px; color: var(--fg-muted); width: 200px; max-width: 200px; overflow: hidden; text-overflow: ellipsis; }
  .poses thead th { position: sticky; top: 0; background: var(--bg); font-weight: 600; }
  .poses th.name-col { font-family: inherit; font-weight: 400; }
  .poses .col { display: inline-flex; align-items: center; border: 0; background: transparent; color: var(--fg); font: inherit; font-weight: 600; cursor: pointer; padding: 2px 4px; border-radius: 4px; }
  .poses .col:hover { background: var(--hover); }
  .poses .col-on { background: rgba(255, 153, 51, 0.12); }
  .poses td.rest { color: var(--fg-muted); }
  .poses input { width: 64px; height: 20px; border: 1px solid var(--border-strong); border-radius: 4px; background: var(--bg); color: var(--fg); font: inherit; text-align: right; padding: 0 4px; }
  .poses input:focus { outline: none; border-color: var(--accent); }
  .outgoing { flex: none; width: 240px; margin-left: auto; padding: 6px 10px; border-left: 1px solid var(--border); overflow: auto; display: flex; flex-direction: column; gap: 2px; }
  .aside-title { color: var(--fg-muted); font-size: 11px; padding: 2px 6px 4px; }
  .out { display: flex; align-items: center; justify-content: space-between; gap: 8px; height: 26px; padding: 0 8px; border: 0; border-radius: 5px; background: transparent; color: var(--fg); font: inherit; cursor: pointer; text-align: left; }
  .out:hover { background: var(--hover); }
  .out:focus-visible { outline: 1px solid var(--accent); }
  .out-meta { color: var(--fg-muted); font-size: 11px; white-space: nowrap; }

  .tracks { flex: 1; min-width: 0; display: flex; flex-direction: column; overflow: hidden; }
  .toolbar { display: flex; align-items: center; gap: 6px; height: 34px; padding: 0 8px; border-bottom: 1px solid var(--border); flex: none; white-space: nowrap; }
  .pair-pick { width: 170px; flex: none; }
  .pair-pick :global(button) { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ms { display: flex; align-items: center; gap: 3px; height: 26px; padding: 0 6px; border-radius: 5px; background: var(--bg-sunken); color: var(--fg-muted); font-size: 11px; }
  .ms input { width: 44px; border: 0; background: transparent; color: var(--fg); font: inherit; outline: none; text-align: right; }
  .ms:focus-within { box-shadow: inset 0 0 0 1px var(--accent); }
  .toolbar .ease { width: 150px; flex: none; }
  .tsep { width: 1px; height: 18px; margin: 0 4px; background: var(--border); }
  .fx { color: var(--accent); font-style: italic; }
  .scope { font-size: 11px; padding: 2px 8px; border-radius: 9px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .scope.shared { background: rgba(255, 153, 51, 0.16); }
  .scope.own { background: var(--accent-soft); }
  .hint { color: var(--fg-muted); font-size: 11px; margin-left: auto; white-space: nowrap; }
  .ruler { display: flex; height: 20px; border-bottom: 1px solid var(--border); cursor: ew-resize; flex: none; }
  .ruler .name-col { font-size: 10px; color: var(--fg-muted); display: flex; align-items: center; border-right: 1px solid var(--border); cursor: default; }
  .ruler .lane { position: relative; flex: 1; margin: 0 10px; }
  .tick { position: absolute; bottom: 2px; padding-left: 3px; border-left: 1px solid var(--border-strong); line-height: 1; font-size: 10px; color: var(--fg-muted); }
  .rows { flex: 1; overflow: auto; }
  .row { display: flex; align-items: center; height: 26px; border-bottom: 1px solid var(--border); }
  .chan { display: flex; align-items: center; gap: 6px; height: 100%; border-right: 1px solid var(--border); }
  .cname { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; font-family: var(--font-mono, ui-monospace, monospace); font-size: 11px; color: var(--fg-muted); }
  .vals { color: var(--fg-muted); font-size: 10px; font-variant-numeric: tabular-nums; white-space: nowrap; }
  :global(.row-more) { display: grid; place-items: center; flex: none; width: 20px; height: 20px; border: 0; border-radius: 4px; background: transparent; color: var(--fg-muted); cursor: pointer; }
  :global(.row-more:hover) { background: var(--hover); color: var(--fg); }
  :global(.row-more:focus-visible) { outline: 1px solid var(--accent); }
  .track { position: relative; flex: 1; height: 18px; margin: 0 10px; background: var(--bg-sunken); border-radius: 4px; }
  .bar { position: absolute; top: 0; bottom: 0; border-radius: 4px; background: var(--accent-soft); box-shadow: inset 0 0 0 1px var(--accent); cursor: grab; touch-action: none; }
  .bar.shaped { background: var(--hover); box-shadow: inset 0 0 0 1px var(--border-strong); cursor: default; }
  .curve { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; }
  .curve path { fill: none; stroke: var(--accent); stroke-width: 1.5; vector-effect: non-scaling-stroke; }
  .follow { position: absolute; inset: 0; border: 1px dashed var(--border); border-radius: 4px; opacity: 0.55; }
  .bar .f { position: absolute; left: 6px; top: 1px; font-size: 11px; color: var(--fg-muted); }
  .grip { position: absolute; top: 0; bottom: 0; width: 7px; cursor: ew-resize; }
  .grip.start { left: -3px; }
  .grip.end { right: -3px; }
  .grip:hover { background: var(--accent); opacity: 0.5; border-radius: 3px; }
  .playhead { position: absolute; top: -4px; bottom: -4px; width: 2px; margin-left: -1px; background: #4aa3ff; pointer-events: none; }
  :global(.slice-popover) { width: 220px; }
  .slice-body { display: flex; flex-direction: column; gap: 8px; padding: 10px; }
  .slice-title { font-weight: 600; font-size: 12px; }
</style>
