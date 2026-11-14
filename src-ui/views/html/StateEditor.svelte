<script lang="ts">
  // The state editor (plans/specs/machines.md §6): the timeline drilled into
  // one component. A place, entered on purpose (the inspector's Add state or
  // Edit states…, a chip of the canvas's states bar, a double-click on a
  // state lane or a transition band) and left by the breadcrumb, the bar's
  // Close, or selecting something else.
  //
  // Which state is being edited is the canvas's to say (StatesBar). This
  // panel is the machine: its graph, a node per state and an arrow per
  // transition, with the default timing beside it; or, as a table, what
  // each state sets. A click on an arrow opens that transition.
  //
  // One timeline at two levels: the document's says *when* each instance
  // changes state; inside a component, the same timeline says *how* one
  // change happens. So a transition's tracks are not drawn here: the
  // engine's timeline view shows that transition (timeline_view.rs
  // `inside_plan`), and this editor is the header over it: where you are,
  // the transport, the transition's name, length, easing and timing
  // (default or its own).
  import { onDestroy, onMount, tick } from "svelte";
  import { DropdownMenu } from "bits-ui";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Flag from "@lucide/svelte/icons/flag";
  import Network from "@lucide/svelte/icons/network";
  import Table2 from "@lucide/svelte/icons/table-2";
  import Play from "@lucide/svelte/icons/play";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import SkipBack from "@lucide/svelte/icons/skip-back";
  import SkipForward from "@lucide/svelte/icons/skip-forward";
  import Square from "@lucide/svelte/icons/square";
  import Button from "../../lib/ui/Button.svelte";
  import Checkbox from "../../lib/ui/Checkbox.svelte";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import EasingField from "../../lib/ui/EasingField.svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  import { dockApi } from "../workspace";
  import { hueOf, machines, type Channel, type ChannelValue, type Focus, type Machine, type Transition } from "../../lib/machines.svelte";

  type Inside = { from: string; to: string; own: boolean; secs: number; transition: string | null; cursor: number };
  let { zoom = 1, inside = null, panel = "" }: { zoom?: number; inside?: Inside | null; panel?: string } = $props();

  // Room to work: the timeline panel grows to at least 45% of the window
  // while the editor is open, and goes back to its size after.
  let grown: { height: number } | null = null;
  onMount(() => {
    try {
      const g = panel ? dockApi()?.getPanel(panel)?.group : undefined;
      const want = Math.round(window.innerHeight * 0.45);
      if (g && g.height < want) {
        grown = { height: g.height };
        g.api.setSize({ height: want });
      }
    } catch {
      // No dock (a test page): the panel stays as it is.
    }
  });

  const doc = controllerManager.docClient;
  // The document's selection when the editor opened, given back on Done.
  const savedSelection = [...doc.selection];
  const view = $derived(machines.view);
  let error = $state("");
  async function run(p: Promise<{ ok: boolean; error?: string }>) {
    error = "";
    const r = await p;
    if (!r.ok) error = r.error ?? "That did not work";
    return r;
  }
  // The document timeline's view, kept while the editor zooms into
  // transitions and given back when it closes.
  let docAxis: { t0: number; pps: number; y0: number } | null = null;
  void doc.command<{ ok: boolean; t0: number; pps: number; y0: number }>({ op: "timeline_axis", fields: JSON.stringify({ pan: 0 }) }).then((r) => {
    if (r.ok) docAxis = { t0: r.t0, pps: r.pps, y0: r.y0 };
  });
  onDestroy(() => {
    cancelAnimationFrame(playing);
    try {
      const g = panel ? dockApi()?.getPanel(panel)?.group : undefined;
      if (g && grown) g.api.setSize({ height: grown.height });
    } catch {
      // The dock went first.
    }
    const a = docAxis;
    if (a) setTimeout(() => void doc.command({ op: "timeline_axis", fields: JSON.stringify(a) }), 150);
    setTimeout(() => void doc.call("select", [JSON.stringify(savedSelection.map((node) => ({ node }))), "replace"]).catch(() => {}), 200);
  });

  // One machine at a time (the first, or the one chosen).
  let machineId = $state<string | null>(null);
  const m = $derived(view?.machines.find((x) => x.id === machineId) ?? view?.machines[0] ?? null);
  const names = (mm: Machine) => mm.states.map((s) => s.name ?? "").filter(Boolean);
  const focusOf = (mm: Machine): Focus => {
    const f = machines.focus[mm.id];
    const ns = names(mm);
    if (f?.kind === "pair" && ns.includes(f.from) && ns.includes(f.to)) return f;
    if (f?.kind === "state" && (f.state === null || ns.includes(f.state))) return f;
    return { kind: "state", state: mm.pinned && ns.includes(mm.pinned) ? mm.pinned : mm.default ?? ns[0] ?? null };
  };
  const focus = $derived(m ? focusOf(m) : null);
  function setFocus(mm: Machine, f: Focus) {
    machines.focus = { ...machines.focus, [mm.id]: f };
  }
  const transitionOf = (mm: Machine, id: string | null | undefined): Transition | undefined => mm.transitions.find((t) => t.id === id);
  const own = (t: Transition | undefined, from: string, to: string) => !!t && t.from === from && t.to === to;
  const defaultOf = (mm: Machine) => mm.transitions.find((t) => (t.from ?? "*") === "*" && (t.to ?? "*") === "*");
  const pairOf = (mm: Machine, from: string, to: string) => mm.pairs.find((q) => q.from === from && q.to === to);
  const secs = (ms: number | null | undefined) => `${Math.round((ms ?? 0) / 10) / 100} s`;
  /** A transition's own name, when someone gave it one (not "a to b"). */
  const titled = (t: Transition | undefined) => (t?.name && t.name !== `${t.from} to ${t.to}` && t.name !== "change" ? t.name : "");

  // ── States ────────────────────────────────────────────────────────────
  const recording = (mm: Machine, s: string) => machines.pinned?.machine === mm.id && machines.pinned.state === s && !machines.pinned.scrub;
  function editState(_mm: Machine, s: string) {
    stopPlay();
    void run(machines.editState(s) as Promise<{ ok: boolean; error?: string }>);
  }
  async function timePair(mm: Machine, from: string, to: string) {
    if (!view) return;
    stopPlay();
    // Selected bars belong to the transition they were picked in: the
    // tracks' pieces keep their ids across transitions, so a selection
    // carried over would name the old leg's slices.
    // And a part selected while shaping a state gives way, so the
    // Inspector shows the transition.
    if (doc.selection.length) void doc.call("select", ["[]", "replace"]).catch(() => {});
    setFocus(mm, { kind: "pair", from, to });
    // The main shows the transition from its start, and the timeline is
    // inside it; fitted to the panel once the engine has built it.
    await machines.pin(view.def, mm.id, from, { from, to, progress: 0 });
    await tick();
    setTimeout(fit, 120);
  }
  let root: HTMLDivElement | undefined = $state();
  function fit() {
    const w = root?.clientWidth;
    if (w) void doc.command({ op: "timeline_axis", fields: JSON.stringify({ fit: w / zoom }) });
  }

  // A transition retimed (or another opened) refits the tracks to it.
  let fitted = "";
  $effect(() => {
    const k = inside ? `${inside.from}>${inside.to}:${inside.secs}` : "";
    if (k && k !== fitted) {
      fitted = k;
      setTimeout(fit, 60);
    }
  });

  // A transition's name, set in the header.
  function renamePair(mm: Machine, from: string, to: string, name: string) {
    if (!view) return;
    const v = name.trim();
    const t = transitionOf(mm, pairOf(mm, from, to)?.transition);
    if (!v || v === t?.name) return;
    void run(machines.editPair(view.def, mm.id, from, to, { name: v }, "name transition"));
  }
  // ── The look of a state: rows by part ─────────────────────────────────
  type Part = { key: string; name: string; shown: string | null; rows: { c: Channel; label: string }[] };
  const LABELS: Record<string, string> = {
    "transform2d.x": "X", "transform2d.y": "Y", "transform2d.rot": "Rotation", "transform2d.sx": "Scale X", "transform2d.sy": "Scale Y",
    "size.w": "Width", "size.h": "Height", "ellipse.rx": "Radius X", "ellipse.ry": "Radius Y", "appearance.opacity": "Opacity",
    "fill.color": "Fill", "stroke.color": "Stroke", "stroke.width": "Stroke width", "text.content": "Text", "font.size": "Font size",
    "trim.start": "Trim start", "trim.end": "Trim end",
    "corners.tl": "Corner top left", "corners.tr": "Corner top right", "corners.br": "Corner bottom right", "corners.bl": "Corner bottom left",
  };
  const parts = $derived.by(() => {
    const out: Part[] = [];
    for (const c of view?.channels ?? []) {
      const p = c.parts[0];
      const key = p?.node ?? "-";
      let g = out.find((x) => x.key === key);
      if (!g) out.push((g = { key, name: p?.name ?? "Not drawn", shown: p?.shown ?? null, rows: [] }));
      g.rows.push({ c, label: p ? LABELS[p.path] ?? p.path : c.name });
    }
    return out;
  });
  const valueOf = (mm: Machine, s: string, c: string): ChannelValue => mm.states.find((x) => x.name === s)?.pose[c] ?? null;
  // The look table: parts fold, and can be narrowed to the selected ones.
  let folded = $state<Set<string>>(new Set());
  let onlySelected = $state(false);
  const foldPart = (k: string) => {
    const next = new Set(folded);
    if (next.has(k)) next.delete(k);
    else next.add(k);
    folded = next;
  };
  const shownParts = $derived(onlySelected ? parts.filter((g) => !!g.shown && selectedPart.has(g.shown)) : parts);
  // A colour is six digits unless it is see-through.
  const fmt = (v: ChannelValue) => (v === null || v === undefined ? "–" : typeof v === "number" ? (Number.isFinite(v) ? String(Math.round(v * 1000) / 1000) : "–") : /^#[0-9a-f]{6}ff$/i.test(v) ? v.slice(0, 7) : v);
  const selectedPart = $derived(new Set(doc.selection));
  function setValue(mm: Machine, stateId: string, s: string, c: Channel, text: string) {
    const v = text.trim();
    if (!v) return;
    void run(machines.patch(stateId, { [`args.${c.name}`]: c.kind === "color" && !v.startsWith("#") ? `#${v}` : v }, `set ${c.name} in ${s}`));
  }

  // ── Inside a transition: transport, timing, the selected slices ───────
  let playing = 0;
  let isPlaying = $state(false);
  function stopPlay() {
    cancelAnimationFrame(playing);
    isPlaying = false;
  }
  const cursorTo = (t: number) => doc.command({ op: "timeline_axis", fields: JSON.stringify({ cursor: Math.max(0, t) }) });
  function play() {
    if (!inside) return;
    stopPlay();
    isPlaying = true;
    const total = Math.max(inside.secs, 0.001);
    const from0 = inside.cursor >= total - 1e-3 ? 0 : inside.cursor;
    const t0 = performance.now();
    const step = () => {
      const t = Math.min(total, from0 + (performance.now() - t0) / 1000);
      void cursorTo(t);
      if (t < total && isPlaying) playing = requestAnimationFrame(step);
      else isPlaying = false;
    };
    playing = requestAnimationFrame(step);
  }
  // Space plays and stops, as on the document's timeline.
  function onKey(e: KeyboardEvent) {
    if (e.key !== " " || !inside || (e.target as HTMLElement)?.closest("input, textarea, [contenteditable]")) return;
    e.preventDefault();
    if (isPlaying) stopPlay();
    else play();
  }

  /** Edit the focused pair: one step, its own timing made first when it
   *  used the default (and said so). */
  async function editFocused(edit: Parameters<typeof machines.editPair>[4], label: string) {
    if (!view || !m || focus?.kind !== "pair") return;
    await run(machines.editPair(view.def, m.id, focus.from, focus.to, edit, label));
  }
  function lengthFrom(text: string) {
    const t = text.trim().toLowerCase();
    const n = parseFloat(t);
    if (!Number.isFinite(n)) return null;
    return t.endsWith("ms") ? n : n * 1000;
  }

  async function copyTiming(source: string, mirrored: boolean) {
    if (!view || !m || focus?.kind !== "pair") return;
    await run(machines.author("copy_timing", { def: view.def, machine: m.id, from: focus.from, to: focus.to, source, mirrored, label: mirrored ? "copy timing reversed" : "copy timing" }));
  }

  // ── The graph: the whole machine at a glance ──────────────────────────
  const GW = 640, GH = 340;
  /** The transition under the pointer: its arrow and its label light together. */
  let hot = $state<string | null>(null);
  function nodes(mm: Machine) {
    const ns = names(mm);
    const n = ns.length;
    return ns.map((name, i) => {
      const a = n === 2 ? (i === 0 ? Math.PI : 0) : -Math.PI / 2 + (i * 2 * Math.PI) / n;
      return { name, x: GW / 2 + Math.cos(a) * (GW / 2 - 90), y: GH / 2 + Math.sin(a) * (GH / 2 - 60) };
    });
  }
  function edges(mm: Machine) {
    const at = Object.fromEntries(nodes(mm).map((n) => [n.name, n]));
    return mm.pairs
      .filter((q) => at[q.from] && at[q.to])
      .map((q) => {
        const t = transitionOf(mm, q.transition);
        const a = at[q.from], b = at[q.to];
        // Each direction bows to its own side; the label sits near the
        // start, on that side, so a pair's two never meet.
        const dx = b.x - a.x, dy = b.y - a.y, len = Math.hypot(dx, dy) || 1;
        const nx = -dy / len, ny = dx / len;
        // Each direction is a straight line on its own side of the pair's
        // axis, so no two arrows cross; its label sits a third of the way
        // along, outside it.
        const x1 = a.x + (dx / len) * 50 + nx * 12, y1 = a.y + (dy / len) * 22 + ny * 12;
        const x2 = b.x - (dx / len) * 50 + nx * 12, y2 = b.y - (dy / len) * 22 + ny * 12;
        const cx = (x1 + x2) / 2, cy = (y1 + y2) / 2;
        const u = 0.32;
        const mine = own(t, q.from, q.to);
        const label = !t ? "snaps" : `${mine && titled(t) ? titled(t) : `${q.from} → ${q.to}`} · ${secs(q.ms)}`;
        // With the two states' swatches before it, so it says which way
        // it goes without tracing its arrow.
        const w = label.length * 6.2 + 16 + 30;
        // The label sits over its own line, nearer its start, so it reads
        // as that arrow's and a pair's two never meet.
        // Level, and moved sideways from the pair's other arrow (24 away)
        // just far enough to clear it, so it still touches its own.
        const side = Math.abs(nx) > 0.2 ? Math.max(0, w / 2 + 4 - (24 - 9 * Math.abs(ny)) / Math.abs(nx)) : 0;
        const lx = x1 + (x2 - x1) * u + Math.sign(nx) * side, ly = y1 + (y2 - y1) * u;
        return { key: `${q.from}>${q.to}`, from: q.from, to: q.to, own: mine, none: !t, label, d: `M${x1.toFixed(1)},${y1.toFixed(1)} Q${cx.toFixed(1)},${cy.toFixed(1)} ${x2.toFixed(1)},${y2.toFixed(1)}`, lx, ly, w };
      });
  }
  const keyActivate = (e: KeyboardEvent, f: () => void) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      f();
    }
  };
</script>

<svelte:window onkeydown={onKey} />

<div class="state-editor" data-state-editor bind:this={root} style:--top="{32 * zoom}px">
  <!-- The header: where you are, then what the focus offers. -->
  <div class="head canvas-ui">
    <button class="ui-quiet-trigger crumb" onclick={() => machines.close()} title="Back to the document's timeline"><span>{machines.origin ? `${machines.origin}'s timeline` : "Timeline"}</span></button>
    <ChevronRight class="sep" size={12} strokeWidth={1.75} />
    {#if m && focus?.kind === "pair"}
      {@const p = pairOf(m, focus.from, focus.to)}
      {@const t = transitionOf(m, p?.transition)}
      {@const mine = own(t, focus.from, focus.to)}
      <button class="ui-quiet-trigger crumb" data-crumb="component" onclick={() => { machines.page = "graph"; editState(m, focus.from); }} title="All of {view?.name ?? 'the component'}'s states">{view?.name ?? "component"}</button>
      <ChevronRight class="sep" size={12} strokeWidth={1.75} />
      <span class="where pair">
        <button class="state-link" onclick={() => editState(m, focus.from)} title="Edit {focus.from}">{focus.from}</button>→<button class="state-link" onclick={() => editState(m, focus.to)} title="Edit {focus.to}">{focus.to}</button>
      </span>
      {#if t}
        <input class="ui-input tname" aria-label="Name of {focus.from} to {focus.to}" placeholder="Name" value={mine ? titled(t) : ""} onkeydown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }} onchange={(e) => renamePair(m, focus.from, focus.to, e.currentTarget.value)} />
        <span class="tsep"></span>
        <IconButton icon={SkipBack} label="To the start ({focus.from})" onclick={() => { stopPlay(); void cursorTo(0); }} />
        {#if isPlaying}<IconButton icon={Square} label="Stop" title="Stop (Space)" onclick={stopPlay} />
        {:else}<IconButton icon={Play} label="Play {focus.from} to {focus.to}" title="Play (Space)" onclick={play} />{/if}
        <IconButton icon={SkipForward} label="To the end ({focus.to})" onclick={() => { stopPlay(); void cursorTo(inside?.secs ?? 0); }} />
        <span class="readout">{(inside?.cursor ?? 0).toFixed(2)} / {(inside?.secs ?? (p?.ms ?? 0) / 1000).toFixed(2)} s</span>
        <span class="tsep"></span>
        <label class="ui-field len long" title={t.ms_formula ? "Set by a formula" : "Length"}>
          <span>Length</span><input aria-label="Length of {focus.from} to {focus.to}" disabled={t.ms_formula} value={(p?.ms ?? t.ms ?? 0) / 1000} onchange={(e) => { const ms = lengthFrom(e.currentTarget.value); if (ms !== null) void editFocused({ ms }, "transition length"); }} /><span>s</span>
        </label>
        <div class="ease"><EasingField label="Easing of {focus.from} to {focus.to}" value={t.ease} onChange={(v) => editFocused({ ease: v }, "transition easing")} /></div>
        <DropdownMenu.Root>
          <DropdownMenu.Trigger class="ui-select-trigger timing {mine ? 'own' : 'default'}" title={mine ? "This transition has its own timing" : "Shared with every transition that has no timing of its own"}><span class="ui-select-label">{mine ? "Own timing" : "Default timing"}</span><ChevronDown size={12} strokeWidth={1.75} /></DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content class="ui-menu" side="bottom" sideOffset={4} align="start">
              {#if !mine}
                <DropdownMenu.Item class="ui-menu-item" onSelect={() => editFocused({}, "own timing")}>Give it its own timing</DropdownMenu.Item>
              {:else if defaultOf(m)}
                <DropdownMenu.Item class="ui-menu-item" onSelect={() => run(machines.author("remove_node", { node: t.id }))}>Use the default timing</DropdownMenu.Item>
              {/if}
              {@const others = m.transitions.filter((x) => x.id !== t.id && x.from && x.to && x.from !== "*" && x.to !== "*")}
              {#if others.length}
                <DropdownMenu.Separator class="ui-menu-separator" />
                <DropdownMenu.Group>
                  <DropdownMenu.GroupHeading class="ui-menu-heading">Copy timing from</DropdownMenu.GroupHeading>
                  {#each others as o (o.id)}
                    {#if o.from === focus.to && o.to === focus.from}
                      <DropdownMenu.Item class="ui-menu-item" onSelect={() => copyTiming(o.id, true)}>{o.from} → {o.to}, reversed</DropdownMenu.Item>
                    {/if}
                    <DropdownMenu.Item class="ui-menu-item" onSelect={() => copyTiming(o.id, false)}>{o.from} → {o.to}{titled(o) ? ` (${titled(o)})` : ""}</DropdownMenu.Item>
                  {/each}
                </DropdownMenu.Group>
              {/if}
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
      {:else}
        <span class="ui-badge">Snaps</span>
        <Button small variant="outline" onclick={() => editFocused({}, "add transition")}>Add transition</Button>
      {/if}
    {:else}
      <span class="comp">{view?.name ?? "component"}</span>
    {/if}
    <span class="grow"></span>
    {#if error}<span class="error" role="alert" title={error}>{error}</span>{/if}
    {#if machines.selected}
      <Button small variant="outline" onclick={() => machines.open(machines.selected!.def)}>Edit {machines.selected.name ?? "selected"}'s states</Button>
    {/if}
    {#if m && focus?.kind === "state"}
      <Segmented inline iconOnly label="Show the states as" value={machines.page} onChange={(v) => (machines.page = v)} items={[{ value: "graph", label: "Graph", icon: Network }, { value: "table", label: "Table", icon: Table2 }]} />
    {/if}
  </div>

  {#if view && m && focus?.kind === "state"}
    {#if machines.page === "graph"}
      <!-- The whole machine: a node per state, an arrow per transition
           (solid with its own timing, dashed on the default). -->
      <section class="body graph-view canvas-ui" aria-label="States and transitions of {view.name ?? 'the component'}">
        <svg class="graph" viewBox="0 0 {GW} {GH}" preserveAspectRatio="xMidYMid meet" role="group" aria-label="States and transitions">
          <defs><marker id="se-arrow-{m.id}" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L8,4 L0,8 z" class="arrowhead" /></marker></defs>
          <!-- Every arrow, then every label over them: a label is never crossed. -->
          {#each edges(m) as e (e.key)}
            <g class="edge" class:own={e.own} class:none={e.none} class:hot={hot === e.key} aria-hidden="true" onclick={() => timePair(m, e.from, e.to)} onpointerenter={() => (hot = e.key)} onpointerleave={() => (hot = null)}>
              <path d={e.d} class="hit" />
              <path d={e.d} class="line" marker-end="url(#se-arrow-{m.id})" />
            </g>
          {/each}
          {#each edges(m) as e (e.key)}
            <g class="edge" class:hot={hot === e.key} role="button" tabindex="0" aria-label="{e.from} to {e.to}: {e.label}" data-edge="{e.from}>{e.to}" onclick={() => timePair(m, e.from, e.to)} onkeydown={(ev) => keyActivate(ev, () => timePair(m, e.from, e.to))} onpointerenter={() => (hot = e.key)} onpointerleave={() => (hot = null)} onfocus={() => (hot = e.key)} onblur={() => (hot = null)}>
              <title>{e.from} → {e.to}</title>
              <rect class="pill" x={e.lx - e.w / 2} y={e.ly - 9} width={e.w} height="18" rx="9" />
              <rect class="hue" x={e.lx - e.w / 2 + 8} y={e.ly - 4} width="8" height="8" rx="2" style:fill={hueOf(m, e.from)} />
              <ArrowRight class="to" x={e.lx - e.w / 2 + 16.5} y={e.ly - 4.5} size={9} strokeWidth={2} />
              <rect class="hue" x={e.lx - e.w / 2 + 26} y={e.ly - 4} width="8" height="8" rx="2" style:fill={hueOf(m, e.to)} />
              <text class="elabel" x={e.lx + 15} y={e.ly + 4} text-anchor="middle">{e.label}</text>
            </g>
          {/each}
          {#each nodes(m) as n (n.name)}
            <g class="node" class:rec={recording(m, n.name)} style:--hue={hueOf(m, n.name)} role="button" tabindex="0" aria-label="Edit {n.name}" data-node={n.name} onclick={() => editState(m, n.name)} onkeydown={(ev) => keyActivate(ev, () => editState(m, n.name))}>
              <title>{n.name}{m.default === n.name ? " (initial)" : ""}</title>
              <rect x={n.x - 48} y={n.y - 15} width="96" height="30" rx="15" />
              <rect class="hue" x={n.x - 38} y={n.y - 4} width="8" height="8" rx="2" style:fill={hueOf(m, n.name)} />
              <text x={n.x + 6} y={n.y + 4.5} text-anchor="middle">{n.name.length > 10 ? n.name.slice(0, 9) + "…" : n.name}</text>
              {#if m.default === n.name}<Flag class="initial" x={n.x + 28} y={n.y - 6} size={11} strokeWidth={2} />{/if}
            </g>
          {/each}
        </svg>
        {#if defaultOf(m) && edges(m).some((e) => !e.own && !e.none)}
          {@const d = defaultOf(m)!}
          <div class="default" title="The timing of every transition without its own (the dashed arrows)">
            <svg class="dash" viewBox="0 0 22 6" aria-hidden="true"><path d="M1,3 H21" /></svg><span class="muted">Default</span>
            <label class="ui-field len"><input aria-label="Default length" value={(d.ms ?? 300) / 1000} onchange={(e) => { const ms = lengthFrom(e.currentTarget.value); if (ms !== null) void run(machines.patch(d.id, { "transition.ms": String(ms) }, "default length")); }} /><span>s</span></label>
            <div class="ease"><EasingField label="Default easing" value={d.ease} onChange={(v) => run(machines.patch(d.id, { "transition.ease": v }, "default easing"))} /></div>
          </div>
        {/if}
      </section>
    {:else}
      <!-- What each state sets: a row per part's property, a column per state. -->
      <section class="body canvas-ui" aria-label="What each state sets">
        {#if view.channels.length}<div class="table-tools"><Checkbox label="Selected parts only" checked={onlySelected} onChange={(on) => (onlySelected = on)} /></div>{/if}
        <div class="table-wrap">
          <table class="poses" style:--col-hue={hueOf(m, machines.pinned?.state)}>
            <thead>
              <tr>
                <th class="name-col">Part · property</th>
                <th class="base" title="The drawing's own values: what a state that does not set a property shows">Drawing</th>
                {#each m.states as st (st.id)}
                  {@const s = st.name ?? ""}
                  <th class:col-on={recording(m, s)} style:--hue={hueOf(m, s)}><button class="col" onclick={() => editState(m, s)} title="Edit {s}"><span class="hue" style:background={hueOf(m, s)}></span>{s}</button></th>
                {/each}
              </tr>
            </thead>
            <tbody>
              {#each shownParts as g (g.key)}
                {@const open = !folded.has(g.key)}
                <tr class="part" class:sel={!!g.shown && selectedPart.has(g.shown)}>
                  <td class="name-col">
                    <button class="fold" aria-label="{open ? 'Fold' : 'Unfold'} {g.name}" aria-expanded={open} onclick={() => foldPart(g.key)}>{#if open}<ChevronDown size={12} strokeWidth={1.75} />{:else}<ChevronRight size={12} strokeWidth={1.75} />{/if}</button><button class="part-name" onclick={() => machines.selectPart(g.shown)} disabled={!g.shown} title="Select {g.name} on the canvas">{g.name}</button>{#if !open}<span class="count">{g.rows.length}</span>{/if}
                  </td>
                  <td class="base"></td>
                  {#each m.states as st (st.id)}<td class:col-on={recording(m, st.name ?? "")}></td>{/each}
                </tr>
                {#if open}
                  {#each g.rows as r (r.c.id)}
                    <tr class:sel={!!g.shown && selectedPart.has(g.shown)}>
                      <td class="name-col prop" title={r.c.name}>{r.label}</td>
                      <td class="base"><span class="cell">{#if r.c.kind === "color"}<span class="sw" style:background={String(r.c.rest ?? "")}></span>{/if}<span class="val">{fmt(r.c.rest)}</span></span></td>
                      {#each m.states as st (st.id)}
                        {@const s = st.name ?? ""}
                        {@const v = valueOf(m, s, r.c.name)}
                        {@const mineHere = st.own.includes(r.c.name)}
                        <td class:col-on={recording(m, s)} class:rest={!mineHere}>
                          {#if recording(m, s)}
                            <!-- Set here, or empty: the drawing's (shown as the placeholder). -->
                            <span class="cell">
                              {#if r.c.kind === "color"}<span class="sw" style:background={String(v ?? "")}></span>{/if}
                              <input class="ui-input" class:wide={r.c.kind !== "number"} aria-label="{g.name} {r.label} in {s}" placeholder={mineHere ? undefined : fmt(r.c.rest)} value={mineHere ? fmt(v) : ""} onchange={(e) => setValue(m, st.id, s, r.c, e.currentTarget.value)} />
                              {#if mineHere}<IconButton icon={RotateCcw} size={12} class="is-row" label="Use the drawing's {fmt(r.c.rest)} for {g.name} {r.label} in {s}" title="Use the drawing's {fmt(r.c.rest)}" onclick={() => run(machines.author("reset_value", { state: st.id, channel: r.c.name }))} />{:else}<span class="spacer"></span>{/if}
                            </span>
                          {:else}
                            <!-- Not set here: the drawing's value, greyed. -->
                            <span class="cell">{#if r.c.kind === "color"}<span class="sw" style:background={String(v ?? "")}></span>{/if}<span class="val">{fmt(mineHere ? v : r.c.rest)}</span></span>
                          {/if}
                        </td>
                      {/each}
                    </tr>
                  {/each}
                {/if}
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    {/if}
  {/if}
</div>

<style>
  /* The editor frames the engine's timeline (a transition's tracks show
     through), so only its own pieces take the pointer. */
  .state-editor { position: absolute; inset: 0; pointer-events: none; color: var(--fg); font-size: var(--text-md); user-select: none; z-index: 3; }
  .head, .body { pointer-events: auto; position: absolute; background: var(--bg); }
  .head { top: 0; left: 0; right: 0; height: var(--top); display: flex; align-items: center; gap: 6px; padding: 0 8px; border-bottom: 1px solid var(--border); white-space: nowrap; overflow: hidden; box-sizing: border-box; }
  .body { top: var(--top); left: 0; right: 0; bottom: 0; display: flex; flex-direction: column; overflow: hidden; }
  .crumb { flex: none; padding: 0 4px; color: var(--fg-muted); }
  .crumb:hover { color: var(--fg); }
  .head :global(.sep) { flex: none; color: var(--fg-faint); }
  .comp { flex: none; font-weight: 600; }
  .where { flex: none; display: inline-flex; align-items: center; gap: 4px; font-weight: 600; }
  .where.pair { gap: 2px; }
  .state-link { border: 0; background: transparent; color: var(--fg); font: inherit; font-weight: 600; padding: 1px 4px; border-radius: var(--radius-sm); cursor: pointer; }
  .state-link:hover { background: var(--hover); }
  .tname { flex: none; width: 110px; height: 22px; min-height: 22px; }
  .grow { flex: 1; }
  .tsep { width: 1px; height: 18px; margin: 0 2px; background: var(--border); flex: none; }
  .readout { font-size: var(--text-sm); color: var(--fg-muted); font-variant-numeric: tabular-nums; min-width: 82px; }
  .len { flex: none; gap: 4px; padding: 0 8px; width: 64px; color: var(--fg-muted); font-size: var(--text-sm); }
  .len input { width: 100%; min-width: 0; border: 0; background: transparent; color: var(--fg); font: inherit; font-size: var(--text-md); outline: none; text-align: right; }
  .len.long { width: 104px; }
  .ease { width: 128px; flex: none; }
  .head :global(.timing) { flex: none; width: auto; border-color: var(--border); background: transparent; }
  .head :global(.timing.default) { color: var(--fg-muted); }
  .error { color: var(--danger); font-size: var(--text-sm); max-width: 240px; overflow: hidden; text-overflow: ellipsis; }
  .head :global(.ui-segmented) { flex: none; }
  .muted { color: var(--fg-muted); }
  .poses .hue { display: inline-block; flex: none; width: 8px; height: 8px; border-radius: 2px; margin-right: 6px; }

  .graph { flex: 1; width: 100%; min-height: 0; }
  .graph-view { background: var(--bg); }
  /* A transition's label is its button: raised, as buttons are. */
  .pill { fill: var(--bg); stroke: var(--border-strong); filter: drop-shadow(0 1px 1px rgb(0 0 0 / 0.12)); }
  .node :global(.initial) { color: var(--fg-muted); }
  .default { position: absolute; left: 10px; bottom: 8px; display: flex; align-items: center; gap: 6px; font-size: var(--text-sm); }
  .dash { width: 22px; height: 6px; }
  .dash path { stroke: var(--fg-muted); stroke-width: 1.25; stroke-dasharray: 4 4; fill: none; }
  .table-tools { display: flex; align-items: center; gap: 12px; padding: 4px 10px; flex: none; font-size: var(--text-sm); }
  .poses .base { color: var(--fg-muted); border-right: 1px solid var(--border); }
  .poses th.base { font-weight: 400; font-size: var(--text-sm); }
  .fold { display: inline-grid; place-items: center; width: 16px; height: 16px; margin-right: 2px; border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--fg-muted); cursor: pointer; vertical-align: middle; }
  .fold:hover { background: var(--hover); color: var(--fg); }
  .count { margin-left: 6px; font-size: var(--text-xs); color: var(--fg-muted); }
  .spacer { display: inline-block; width: 20px; }
  .cell input::placeholder { color: var(--fg-faint); font-style: italic; }
  .node rect { fill: var(--bg); stroke: var(--border-strong); }
  .node text { fill: var(--fg); font-size: 12px; font-weight: 600; }
  .node { cursor: pointer; }
  .node:hover rect { stroke: var(--accent); }
  .node.rec > rect:first-of-type { fill: color-mix(in srgb, var(--hue) 18%, var(--bg)); stroke: var(--hue); stroke-width: 2; }
  .edge { cursor: pointer; }
  .edge path { fill: none; }
  .edge :global(.to) { color: var(--fg-muted); }
  .edge .hit { stroke: transparent; stroke-width: 12; }
  .edge .line { stroke: var(--fg-muted); stroke-width: 1.25; stroke-dasharray: 4 4; opacity: 0.7; }
  .edge.own .line { stroke-dasharray: none; opacity: 1; }
  .edge.none .line { opacity: 0.25; }
  .edge.hot .pill { fill: color-mix(in srgb, var(--accent) 14%, var(--bg)); stroke: var(--accent); }
  .edge.hot .elabel { fill: var(--accent); }
  .edge.hot .line { stroke: var(--accent); stroke-width: 2; opacity: 1; }
  .elabel { fill: var(--fg); font-size: 10.5px; font-weight: 500; }
  .node:focus, .edge:focus { outline: none; }
  .arrowhead { fill: var(--fg-muted); }

  /* What each state sets. */
  .table-wrap { flex: 1; min-height: 0; overflow: auto; }
  .poses { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
  .poses th, .poses td { height: 24px; padding: 0 10px; border-bottom: 1px solid var(--border); text-align: right; white-space: nowrap; min-width: 72px; }
  .poses thead th { position: sticky; top: 0; z-index: 1; background: var(--bg); font-weight: 600; }
  .poses .name-col { text-align: left; width: 180px; max-width: 180px; overflow: hidden; text-overflow: ellipsis; }
  .poses th.name-col { font-weight: 400; color: var(--fg-muted); font-size: var(--text-sm); }
  .poses tr.part td { border-bottom: 0; height: 22px; padding-top: 4px; }
  .poses .part-name { border: 0; background: transparent; color: var(--fg); font: inherit; font-weight: 600; padding: 1px 4px; margin-left: -4px; border-radius: var(--radius-sm); cursor: pointer; }
  .poses .part-name:hover:not(:disabled) { background: var(--hover); }
  .poses td.prop { padding-left: 22px; color: var(--fg-muted); }
  .poses tr.sel td.name-col { color: var(--accent); }
  .poses .col { display: inline-flex; align-items: center; border: 0; background: transparent; color: var(--fg); font: inherit; font-weight: 600; cursor: pointer; padding: 2px 4px; border-radius: var(--radius); }
  .poses .col:hover { background: var(--hover); }
  .poses .col-on { background: color-mix(in srgb, var(--col-hue, var(--accent)) 10%, transparent); }
  .poses td.rest .val { color: var(--fg-faint); font-style: italic; }
  .cell { display: inline-flex; align-items: center; justify-content: flex-end; gap: 4px; }
  .cell input { width: 64px; min-height: 22px; height: 22px; padding: 0 6px; text-align: right; }
  .cell input.wide { width: 96px; text-align: left; }
  .sw { width: 12px; height: 12px; border-radius: 3px; box-shadow: inset 0 0 0 1px var(--border-strong); flex: none; }
</style>
