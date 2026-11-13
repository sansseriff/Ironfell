<script lang="ts">
  // The state editor (plans/specs/machines.md §6): the timeline drilled into
  // one component. A mode, entered on purpose (the inspector's Edit states…,
  // a double-click on a state lane or a transition band) and left with Done
  // or the breadcrumb; selection never switches it.
  //
  // One timeline at two levels: the document's says *when* each instance
  // changes state; inside a component, the same timeline says *how* one
  // change happens. So a transition's tracks are not drawn here: the
  // engine's timeline view shows that transition (timeline_view.rs
  // `inside_plan`), with the same rows, bars, diamonds, ruler and playhead,
  // and this editor is the frame around it:
  // - the header: where you are, the transport, the transition's length,
  //   easing and timing (default or its own), and what acts on the slices
  //   selected in the tracks (easing, stagger);
  // - the navigator at the left: each state, the transitions out of it
  //   under it, and the default timing; or the same as a graph;
  // - for a state, its look: a row per part's property, a column per state,
  //   the edited state's editable; edits on the canvas record into it.
  import { onDestroy, onMount, tick } from "svelte";
  import { DropdownMenu, Popover } from "bits-ui";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import X from "@lucide/svelte/icons/x";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import ListTree from "@lucide/svelte/icons/list-tree";
  import Network from "@lucide/svelte/icons/network";
  import Play from "@lucide/svelte/icons/play";
  import Plus from "@lucide/svelte/icons/plus";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import SkipBack from "@lucide/svelte/icons/skip-back";
  import SkipForward from "@lucide/svelte/icons/skip-forward";
  import Square from "@lucide/svelte/icons/square";
  import Button from "../../lib/ui/Button.svelte";
  import Checkbox from "../../lib/ui/Checkbox.svelte";
  import EmptyState from "../../lib/ui/EmptyState.svelte";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import EasingField, { CURVES } from "../../lib/ui/EasingField.svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  import { dockApi } from "../workspace";
  import { machines, type Channel, type ChannelValue, type Focus, type Machine, type Transition } from "../../lib/machines.svelte";

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
  let note = $state("");
  let noteTimer: ReturnType<typeof setTimeout> | undefined;
  function say(text: string) {
    note = text;
    clearTimeout(noteTimer);
    noteTimer = setTimeout(() => (note = ""), 4000);
  }
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
    clearTimeout(noteTimer);
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
    if (f?.kind === "default") return f;
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
  const easeLabel = (e: string | null | undefined) => CURVES.find((c) => c.value === e)?.label ?? (e ? "Custom" : "Easy Ease");
  /** A transition's own name, when someone gave it one (not "a to b"). */
  const titled = (t: Transition | undefined) => (t?.name && t.name !== `${t.from} to ${t.to}` && t.name !== "change" ? t.name : "");

  // ── States ────────────────────────────────────────────────────────────
  const recording = (mm: Machine, s: string) => machines.pinned?.machine === mm.id && machines.pinned.state === s && !machines.pinned.scrub;
  function editState(mm: Machine, s: string) {
    if (!view) return;
    stopPlay();
    setFocus(mm, { kind: "state", state: s });
    machines.paused = null;
    void run(machines.pin(view.def, mm.id, s));
  }
  async function timePair(mm: Machine, from: string, to: string) {
    if (!view) return;
    stopPlay();
    setFocus(mm, { kind: "pair", from, to });
    // The main shows the transition from its start, and the timeline is
    // inside it; fitted to the panel once the engine has built it.
    await machines.pin(view.def, mm.id, from, { from, to, progress: 0 });
    await tick();
    setTimeout(fit, 120);
  }
  function showDefault(mm: Machine) {
    if (!view) return;
    stopPlay();
    setFocus(mm, { kind: "default" });
    if (machines.pinned) void machines.pin(view.def, mm.id, null);
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

  // Naming: a state's or a transition's name, in place.
  let renaming = $state<string | null>(null);
  function renameState(id: string, from: string, to: string) {
    renaming = null;
    const v = to.trim();
    if (v && v !== from) void run(machines.author("rename_state", { state: id, name: v }));
  }
  function renamePair(mm: Machine, from: string, to: string, name: string) {
    renaming = null;
    if (!view) return;
    const v = name.trim();
    const t = transitionOf(mm, pairOf(mm, from, to)?.transition);
    if (!v || v === t?.name) return;
    void run(machines.editPair(view.def, mm.id, from, to, { name: v }, "name transition"));
  }
  const focusInput = (el: HTMLInputElement) => {
    el.focus();
    el.select();
  };

  // Adding: the first time, both states are named; after, the new one.
  let firstName = $state("state_1");
  let secondName = $state("state_2");
  async function createStates() {
    if (!view) return;
    const r = await run(machines.author("add_state", { def: view.def, first: firstName, name: secondName }));
    const mm = machines.view?.machines[0];
    if (r.ok && mm) setFocus(mm, { kind: "state", state: secondName.trim().replace(/ /g, "_") });
  }
  let addOpen = $state(false);
  let newName = $state("");
  async function addState() {
    if (!view) return;
    const name = newName.trim();
    const r = await run(machines.author("add_state", { def: view.def, ...(name ? { name } : {}) }));
    addOpen = false;
    newName = "";
    const mm = machines.view?.machines.find((x) => x.id === m?.id) ?? machines.view?.machines[0];
    const made = (r as { state?: string }).state;
    if (r.ok && mm && made) setFocus(mm, { kind: "state", state: made });
  }

  // ── The look of a state: rows by part ─────────────────────────────────
  type Part = { key: string; name: string; shown: string | null; rows: { c: Channel; label: string }[] };
  const LABELS: Record<string, string> = {
    "transform2d.x": "X", "transform2d.y": "Y", "transform2d.rot": "Rotation", "transform2d.sx": "Scale X", "transform2d.sy": "Scale Y",
    "size.w": "Width", "size.h": "Height", "ellipse.rx": "Radius X", "ellipse.ry": "Radius Y", "appearance.opacity": "Opacity",
    "fill.color": "Fill", "stroke.color": "Stroke", "stroke.width": "Stroke width", "text.content": "Text", "font.size": "Font size",
    "trim.start": "Trim start", "trim.end": "Trim end",
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
  const fmt = (v: ChannelValue) => (v === null || v === undefined ? "–" : typeof v === "number" ? (Number.isFinite(v) ? String(Math.round(v * 1000) / 1000) : "–") : v);
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
    const before = transitionOf(m, pairOf(m, focus.from, focus.to)?.transition);
    const wasOwn = own(before, focus.from, focus.to);
    const r = await run(machines.editPair(view.def, m.id, focus.from, focus.to, edit, label));
    if (r.ok && !wasOwn) say(`${focus.from} → ${focus.to} now has its own timing; the default is unchanged`);
  }
  function lengthFrom(text: string) {
    const t = text.trim().toLowerCase();
    const n = parseFloat(t);
    if (!Number.isFinite(n)) return null;
    return t.endsWith("ms") ? n : n * 1000;
  }

  // The slices selected in the tracks (their bars or diamonds).
  type Picked = { tween: string; channel: string };
  let slices = $state<Picked[]>([]);
  $effect(() => {
    const nodes = doc.selection;
    if (!inside || !nodes.length) {
      slices = [];
      return;
    }
    void machines.author("selected_slices", { nodes }).then((r) => {
      slices = ((r as unknown as { slices?: Picked[] }).slices ?? []).filter((s) => s.channel);
    });
  });
  const tweenOf = (t: Transition | undefined, channel: string) => t?.tweens.find((w) => w.channel === channel);
  async function copyTiming(source: string, mirrored: boolean) {
    if (!view || !m || focus?.kind !== "pair") return;
    await run(machines.author("copy_timing", { def: view.def, machine: m.id, from: focus.from, to: focus.to, source, mirrored, label: mirrored ? "copy timing reversed" : "copy timing" }));
  }

  // ── The navigator: a state's transitions show under it when it is the
  // one in focus (or opened by hand), so the tree stays short.
  let expanded = $state<Set<string>>(new Set());
  const showsPairs = (s: string) => expanded.has(s) || (focus?.kind === "state" && focus.state === s) || (focus?.kind === "pair" && focus.from === s);
  function toggleState(s: string) {
    const next = new Set(expanded);
    if (next.has(s)) next.delete(s);
    else next.add(s);
    expanded = next;
  }

  // ── The graph: the whole machine at a glance, over the tracks ─────────
  let graph = $state(false);
  const GW = 560, GH = 300;
  function nodes(mm: Machine) {
    const ns = names(mm);
    const n = ns.length;
    return ns.map((name, i) => {
      const a = n === 2 ? (i === 0 ? Math.PI : 0) : -Math.PI / 2 + (i * 2 * Math.PI) / n;
      return { name, x: GW / 2 + Math.cos(a) * (GW / 2 - 90), y: GH / 2 + Math.sin(a) * (GH / 2 - 36) };
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
        const x1 = a.x + (dx / len) * 46 + nx * 8, y1 = a.y + (dy / len) * 18 + ny * 8;
        const x2 = b.x - (dx / len) * 46 + nx * 8, y2 = b.y - (dy / len) * 18 + ny * 8;
        const cx = (x1 + x2) / 2 + nx * 30, cy = (y1 + y2) / 2 + ny * 30;
        // The label on its own curve's bulge, so a pair's two never meet.
        const lx = (x1 + x2) / 4 + cx / 2, ly = (y1 + y2) / 4 + cy / 2;
        const mine = own(t, q.from, q.to);
        return { key: `${q.from}>${q.to}`, from: q.from, to: q.to, own: mine, none: !t, label: !t ? "snaps" : mine ? `${titled(t) ? titled(t) + " · " : ""}${secs(q.ms)}` : "default", d: `M${x1.toFixed(1)},${y1.toFixed(1)} Q${cx.toFixed(1)},${cy.toFixed(1)} ${x2.toFixed(1)},${y2.toFixed(1)}`, lx, ly };
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

<div class="state-editor" data-state-editor bind:this={root} style:--top="{32 * zoom}px" style:--nav="{220 * zoom}px">
  <!-- The header: where you are, then what the focus offers. -->
  <div class="head canvas-ui">
    <button class="ui-quiet-trigger crumb" onclick={() => machines.close()} title="Back to the document's timeline, its playhead and selection as they were (Done)"><ChevronLeft size={14} strokeWidth={1.75} /><span>{machines.origin ? `${machines.origin}'s timeline` : "Timeline"}</span></button>
    <ChevronRight class="sep" size={12} strokeWidth={1.75} />
    <span class="comp" title="Editing this component's states: changes affect every instance">{view?.name ?? "component"}</span>
    {#if focus}<span class="mode" class:anim={focus.kind !== "state"} title="For every instance of {view?.name ?? 'the component'}: these are the component's states and transitions, not one instance's">{focus.kind === "state" ? "State look" : focus.kind === "pair" ? "Transition animation" : "Default timing"}</span>{/if}
    {#if m && focus}
      <ChevronRight class="sep" size={12} strokeWidth={1.75} />
      {#if focus.kind === "state"}
        <span class="where">{#if focus.state && recording(m, focus.state)}<span class="dot"></span>{/if}{focus.state ?? "—"}</span>
      {:else if focus.kind === "default"}
        <span class="where">Default timing</span>
      {:else}
        {@const p = pairOf(m, focus.from, focus.to)}
        {@const t = transitionOf(m, p?.transition)}
        {@const mine = own(t, focus.from, focus.to)}
        <span class="where pair">
          <button class="state-link" onclick={() => editState(m, focus.from)} title="Edit {focus.from}'s look">{focus.from}</button>→<button class="state-link" onclick={() => editState(m, focus.to)} title="Edit {focus.to}'s look">{focus.to}</button>
          {#if titled(t) && mine}<span class="tname">{titled(t)}</span>{/if}
        </span>
        {#if t}
          <span class="tsep"></span>
          <IconButton icon={SkipBack} label="To the start ({focus.from})" onclick={() => { stopPlay(); void cursorTo(0); }} />
          {#if isPlaying}<IconButton icon={Square} label="Stop" title="Stop (Space)" onclick={stopPlay} />
          {:else}<IconButton icon={Play} label="Play {focus.from} to {focus.to}" title="Play on the main (Space)" onclick={play} />{/if}
          <IconButton icon={SkipForward} label="To the end ({focus.to})" onclick={() => { stopPlay(); void cursorTo(inside?.secs ?? 0); }} />
          <span class="readout" title="Where the main is in the transition">{(inside?.cursor ?? 0).toFixed(2)} / {(inside?.secs ?? (p?.ms ?? 0) / 1000).toFixed(2)} s</span>
          <span class="tsep"></span>
          <label class="ui-field len" title={t.ms_formula ? "Set by a formula: edit it in the inspector" : "How long it takes (s, or ms with “ms”)"}>
            <input aria-label="Length of {focus.from} to {focus.to}" disabled={t.ms_formula} value={(p?.ms ?? t.ms ?? 0) / 1000} onchange={(e) => { const ms = lengthFrom(e.currentTarget.value); if (ms !== null) void editFocused({ ms }, "transition length"); }} /><span>s</span>
          </label>
          <div class="ease"><EasingField label="Easing of {focus.from} to {focus.to}" value={t.ease} onChange={(v) => editFocused({ ease: v }, "transition easing")} /></div>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger class="ui-select-trigger timing {mine ? 'own' : 'default'}" title={mine ? "This transition has timing of its own" : "It uses the default timing, shared with every transition that has none of its own. Editing it here gives it its own."}>{mine ? "Own timing" : "Default timing"}</DropdownMenu.Trigger>
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
                {#if defaultOf(m)}
                  <DropdownMenu.Separator class="ui-menu-separator" />
                  <DropdownMenu.Item class="ui-menu-item" onSelect={() => showDefault(m)}>Edit the default timing…</DropdownMenu.Item>
                {/if}
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
        {:else}
          <span class="scope none">No transition: it snaps</span>
          <Button small variant="outline" onclick={() => editFocused({}, "add transition")}>Add a transition</Button>
        {/if}
      {/if}
    {/if}
    <span class="grow"></span>

    {#if error}<span class="error" role="alert" title={error}>{error}</span>{/if}
    {#if machines.paused}
      <span class="paused" role="status">Recording into <strong>{machines.paused.state}</strong> paused <Button small variant="outline" onclick={() => machines.resume()}>Return to main</Button></span>
    {:else if machines.selected}
      <Button small variant="outline" onclick={() => machines.open(machines.selected!.def)} title="Edit that component's states instead">Edit {machines.selected.name ?? "selected"}'s states</Button>
    {/if}
    {#if focus?.kind !== "pair"}<span class="scope-note" title="These are the component's states and transitions, not one instance's">every instance</span>{/if}
    <Button small variant="primary" onclick={() => machines.close()}>Done</Button>
  </div>

  {#if note}<div class="toast canvas-ui" role="status">{note}</div>{/if}
  {#if !view}
    <div class="body full canvas-ui"><EmptyState inline>Loading…</EmptyState></div>
  {:else if !m}
    <!-- The first states, named on the way in. -->
    <div class="body full canvas-ui">
      <form class="first" onsubmit={(e) => { e.preventDefault(); void createStates(); }}>
        <p><strong>{view.name ?? "This component"}</strong> has no states yet. A state is a look the component can be in; its instances move between them, and you choose how.</p>
        <div class="first-fields">
          <label class="stack"><span>Its look now</span><input class="ui-input" aria-label="The first state's name" bind:value={firstName} use:focusInput /></label>
          <label class="stack"><span>A new state</span><input class="ui-input" aria-label="The new state's name" bind:value={secondName} /></label>
          <Button small variant="primary" type="submit"><Plus size={14} strokeWidth={1.75} />Make the states</Button>
        </div>
        <p class="muted">Then change the parts on the canvas: the edits go into the new state.</p>
      </form>
    </div>
  {:else if focus}
    <!-- The navigator: each state with the transitions out of it, or the graph. -->
    <nav class="nav canvas-ui" aria-label="States and transitions of {view.name ?? 'the component'}">
      <div class="nav-head">
        <span>States</span>
        <span class="grow"></span>
        <Popover.Root bind:open={addOpen}>
          <Popover.Trigger class="ui-icon-button" aria-label="Add a state" title="Add a state"><Plus size={14} strokeWidth={1.75} /></Popover.Trigger>
          <Popover.Portal>
            <Popover.Content class="ui-popover add-popover" side="right" align="start" sideOffset={6}>
              <form class="ui-popover-body" onsubmit={(e) => { e.preventDefault(); void addState(); }}>
                <label class="stack"><span>Name</span><input class="ui-input" aria-label="The new state's name" placeholder="state_{names(m).length + 1}" bind:value={newName} use:focusInput /></label>
                <Button small variant="primary" type="submit">Add, and edit its look</Button>
              </form>
            </Popover.Content>
          </Popover.Portal>
        </Popover.Root>
        <IconButton icon={graph ? ListTree : Network} label={graph ? "Show as a list" : "Show as a graph"} pressed={graph} onPressedChange={(on) => (graph = on)} />
      </div>
        <div class="tree" role="tree">
          {#each m.states as st (st.id)}
            {@const s = st.name ?? ""}
            <div class="item state" class:on={focus.kind === "state" && focus.state === s} class:rec={recording(m, s)} role="treeitem" aria-selected={focus.kind === "state" && focus.state === s}>
              {#if renaming === st.id}
                <input class="ui-input rename" aria-label="Rename {s}" value={s} use:focusInput onkeydown={(e) => { if (e.key === "Escape") renaming = null; else if (e.key === "Enter") renameState(st.id, s, e.currentTarget.value); }} onblur={(e) => renameState(st.id, s, e.currentTarget.value)} />
              {:else}
                <button class="label" onclick={() => editState(m, s)} ondblclick={() => (renaming = st.id)} title={recording(m, s) ? `Editing ${s}: edits to the parts go into it` : `Edit ${s}'s look (double-click to rename)`}>
                  {#if recording(m, s)}<span class="dot"></span>{/if}<span class="nm">{s}</span>{#if m.default === s}<span class="ui-badge" title="Instances start in it">initial</span>{/if}
                </button>
              {/if}
              <DropdownMenu.Root>
                <DropdownMenu.Trigger class="ui-icon-button more" aria-label="{s} options" title="Rename, copy a look, make initial, delete"><Ellipsis size={14} strokeWidth={1.75} /></DropdownMenu.Trigger>
                <DropdownMenu.Portal>
                  <DropdownMenu.Content class="ui-menu" side="right" sideOffset={6} align="start">
                    <DropdownMenu.Item class="ui-menu-item" onSelect={() => (renaming = st.id)}>Rename</DropdownMenu.Item>
                    <DropdownMenu.Item class="ui-menu-item" disabled={m.default === s} onSelect={() => run(machines.author("set_default_state", { state: st.id }))}>Make initial</DropdownMenu.Item>
                    {#if m.states.length > 1}
                      <DropdownMenu.Separator class="ui-menu-separator" />
                      <DropdownMenu.Group>
                        <DropdownMenu.GroupHeading class="ui-menu-heading">Copy the look of</DropdownMenu.GroupHeading>
                        {#each m.states.filter((x) => x.id !== st.id) as o (o.id)}
                          <DropdownMenu.Item class="ui-menu-item" onSelect={() => run(machines.author("copy_pose", { state: st.id, from: o.id }))}>{o.name}</DropdownMenu.Item>
                        {/each}
                      </DropdownMenu.Group>
                    {/if}
                    <DropdownMenu.Separator class="ui-menu-separator" />
                    <DropdownMenu.Item class="ui-menu-item" disabled={m.states.length < 2} onSelect={() => run(machines.author("delete_state", { state: st.id }))}>Delete {s}</DropdownMenu.Item>
                  </DropdownMenu.Content>
                </DropdownMenu.Portal>
              </DropdownMenu.Root>
            </div>
            {#if !showsPairs(s)}
              {@const n = m.pairs.filter((q) => q.from === s && q.transition).length}
              <button class="item more-pairs" onclick={() => toggleState(s)} title="Show the transitions from {s}"><ChevronRight size={12} strokeWidth={1.75} /><span>{n} transition{n === 1 ? "" : "s"} from {s}</span></button>
            {/if}
            {#each m.pairs.filter((q) => q.from === s && showsPairs(s)) as q (q.to)}
              {@const t = transitionOf(m, q.transition)}
              {@const mine = own(t, q.from, q.to)}
              {@const on = focus.kind === "pair" && focus.from === q.from && focus.to === q.to}
              <div class="item pair" class:on role="treeitem" aria-selected={on}>
                {#if renaming === `${q.from}>${q.to}`}
                  <input class="ui-input rename" aria-label="Name {q.from} to {q.to}" value={titled(t)} placeholder="melting" use:focusInput onkeydown={(e) => { if (e.key === "Escape") renaming = null; else if (e.key === "Enter") renamePair(m, q.from, q.to, e.currentTarget.value); }} onblur={(e) => renamePair(m, q.from, q.to, e.currentTarget.value)} />
                {:else}
                  <button class="label" onclick={() => timePair(m, q.from, q.to)} ondblclick={() => (renaming = `${q.from}>${q.to}`)} title="{q.from} → {q.to}: {t ? (mine ? 'its own timing' : 'the default timing') : 'snaps'} (double-click to name it)">
                    <span class="to">→ {q.to}</span>{#if mine && titled(t)}<span class="tname">{titled(t)}</span>{/if}
                    <span class="meta" class:muted={!mine}>{t ? (mine ? secs(q.ms) : "default") : "snaps"}</span>
                  </button>
                {/if}
              </div>
            {/each}
          {/each}
        </div>
      {#if defaultOf(m)}
            {@const d = defaultOf(m)!}
            <div class="item default pinned" class:on={focus.kind === "default"}>
              <button class="label" onclick={() => showDefault(m)} title="The timing of every transition without its own">
                <span class="nm">Default timing</span><span class="meta">{secs(d.ms)} · {easeLabel(d.ease)}</span>
              </button>
            </div>
          {/if}
    </nav>

    {#if graph}
      <!-- The whole machine: every transition labelled (its name and length,
           or "default"), solid when it has its own timing. A click opens it. -->
      <section class="body graph-view canvas-ui" aria-label="States as a graph">
        <div class="toolbar"><span class="muted">Click a state to edit its look, an arrow to time its transition. Dashed arrows use the default timing.</span><span class="grow"></span><IconButton icon={X} label="Close the graph" onclick={() => (graph = false)} /></div>
        <svg class="graph" viewBox="0 0 {GW} {GH}" preserveAspectRatio="xMidYMid meet" role="group" aria-label="States and transitions">
          <defs><marker id="se-arrow-{m.id}" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L8,4 L0,8 z" class="arrowhead" /></marker></defs>
          {#each edges(m) as e (e.key)}
            {@const on = focus.kind === "pair" && focus.from === e.from && focus.to === e.to}
            {@const w = e.label.length * 6.2 + 14}
            <g class="edge" class:on class:own={e.own} class:none={e.none} role="button" tabindex="0" aria-label="{e.from} to {e.to}: {e.label}" onclick={() => { graph = false; void timePair(m, e.from, e.to); }} onkeydown={(ev) => keyActivate(ev, () => { graph = false; void timePair(m, e.from, e.to); })}>
              <title>{e.from} → {e.to}: {e.label}</title>
              <path d={e.d} class="hit" />
              <path d={e.d} class="line" marker-end="url(#se-arrow-{m.id})" />
              <rect class="pill" x={e.lx - w / 2} y={e.ly - 9} width={w} height="18" rx="9" />
              <text class="elabel" x={e.lx} y={e.ly + 4} text-anchor="middle">{e.label}</text>
            </g>
          {/each}
          {#each nodes(m) as n (n.name)}
            <g class="node" class:rec={recording(m, n.name)} class:on={focus.kind === "state" && focus.state === n.name} role="button" tabindex="0" aria-label="Edit {n.name}" onclick={() => { graph = false; editState(m, n.name); }} onkeydown={(ev) => keyActivate(ev, () => { graph = false; editState(m, n.name); })}>
              <title>{n.name}{m.default === n.name ? " (initial)" : ""}</title>
              <rect x={n.x - 44} y={n.y - 15} width="88" height="30" rx="15" />
              <text x={n.x} y={n.y + 5} text-anchor="middle">{n.name.length > 12 ? n.name.slice(0, 11) + "…" : n.name}</text>
              {#if m.default === n.name}<text class="initial" x={n.x} y={n.y + 28} text-anchor="middle">initial</text>{/if}
            </g>
          {/each}
        </svg>
      </section>
    {:else if focus.kind === "state"}
      <!-- A state's look: a row per part's property, a column per state. -->
      <section class="body canvas-ui" aria-label="The look of {focus.state}">
        <div class="toolbar">
          {#if focus.state && recording(m, focus.state)}
            <span class="dot"></span><span class="rec-note">Editing <strong>{focus.state}</strong>: change the parts on the canvas, or the values here. Positions, sizes, colours, opacity and text go into {focus.state}.</span>
          {:else if focus.state}
            <span class="muted">Showing <strong>{focus.state}</strong>.</span><Button small variant="outline" onclick={() => editState(m, focus.state!)}>Edit {focus.state}</Button>
          {/if}
        </div>
        {#if !view.channels.length}
          <div class="empty-wide">
            {#if machines.paused}
              <EmptyState inline>Recording into <strong>{machines.paused.state}</strong> is paused: the selection left the component.
                {#snippet actions()}<Button small variant="outline" onclick={() => machines.resume()}>Return to main</Button>{/snippet}</EmptyState>
            {:else}
              <EmptyState inline>Nothing differs between the states yet. Select a part of <strong>{view.name ?? "the component"}</strong> on the canvas and change it (move it, its colour, its opacity, its text): the change goes into <strong>{focus.state}</strong>, and a row appears here.</EmptyState>
            {/if}
          </div>
        {:else}
          <div class="table-tools">
            <Checkbox label="Only the parts selected on the canvas" checked={onlySelected} onChange={(on) => (onlySelected = on)} />
          </div>
          <div class="table-wrap">
            <table class="poses">
              <thead>
                <tr>
                  <th class="name-col">Part · property</th>
                  <th class="base" title="The drawing's own values: what a state that does not set a property shows">Drawing</th>
                  {#each m.states as st (st.id)}
                    {@const s = st.name ?? ""}
                    <th class:col-on={recording(m, s)}><button class="col" onclick={() => editState(m, s)} title="Edit {s}'s look">{#if recording(m, s)}<span class="dot"></span>{/if}{s}</button></th>
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
                                <input class="ui-input" class:wide={r.c.kind !== "number"} aria-label="{g.name} {r.label} in {s}" placeholder={mineHere ? undefined : fmt(r.c.rest)} title={mineHere ? `${s}'s own` : `Not set in ${s}: it shows the drawing's ${fmt(r.c.rest)}. Type a value to set it for ${s}.`} value={mineHere ? fmt(v) : ""} onchange={(e) => setValue(m, st.id, s, r.c, e.currentTarget.value)} />
                                {#if mineHere}<IconButton icon={RotateCcw} size={12} class="is-row" label="Use the drawing's {fmt(r.c.rest)} for {g.name} {r.label} in {s}" title="Use the drawing's {fmt(r.c.rest)} instead" onclick={() => run(machines.author("reset_value", { state: st.id, channel: r.c.name }))} />{:else}<span class="spacer"></span>{/if}
                              </span>
                            {:else}
                              <span class="cell" title={mineHere ? undefined : "The drawing's: this state does not set it"}>{#if r.c.kind === "color"}<span class="sw" style:background={String(v ?? "")}></span>{/if}<span class="val">{mineHere ? fmt(v) : "–"}</span></span>
                            {/if}
                          </td>
                        {/each}
                      </tr>
                    {/each}
                  {/if}
                {:else}
                  <tr><td class="name-col muted" colspan={m.states.length + 2}>No selected part reads a state's values.</td></tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </section>
    {:else if focus.kind === "default"}
      {@const d = defaultOf(m)}
      <section class="body canvas-ui" aria-label="Default timing">
        {#if d}
          <div class="toolbar">
            <span class="muted">Every transition without timing of its own uses this.</span>
            <label class="ui-field len"><input aria-label="Default length" value={(d.ms ?? 300) / 1000} onchange={(e) => { const ms = lengthFrom(e.currentTarget.value); if (ms !== null) void run(machines.patch(d.id, { "transition.ms": String(ms) }, "default length")); }} /><span>s</span></label>
            <div class="ease"><EasingField label="Default easing" value={d.ease} onChange={(v) => run(machines.patch(d.id, { "transition.ease": v }, "default easing"))} /></div>
          </div>
          {@const users = m.pairs.filter((q) => q.transition === d.id)}
          <div class="users">
            <div class="aside-title">Used by {users.length} transition{users.length === 1 ? "" : "s"}</div>
            {#each users as q (q.from + q.to)}
              <button class="out" onclick={() => timePair(m, q.from, q.to)}>{q.from} → {q.to}</button>
            {:else}
              <span class="muted">None: every transition has its own timing.</span>
            {/each}
          </div>
        {:else}
          <div class="empty-wide"><EmptyState inline>There is no default timing: transitions without their own snap.</EmptyState></div>
        {/if}
      </section>
    {/if}
  {/if}
</div>

<style>
  /* The editor frames the engine's timeline (a transition's tracks show
     through), so only its own pieces take the pointer. */
  .state-editor { position: absolute; inset: 0; pointer-events: none; color: var(--fg); font-size: var(--text-md); user-select: none; z-index: 3; }
  .head, .nav, .body { pointer-events: auto; position: absolute; background: var(--bg); }
  .head { top: 0; left: 0; right: 0; height: var(--top); display: flex; align-items: center; gap: 6px; padding: 0 8px; border-bottom: 1px solid var(--border); white-space: nowrap; overflow: hidden; box-sizing: border-box; }
  .nav { top: var(--top); left: 0; bottom: 0; width: var(--nav); border-right: 1px solid var(--border); display: flex; flex-direction: column; overflow: hidden; box-sizing: border-box; background: var(--bg-sunken); }
  .body { top: var(--top); left: var(--nav); right: 0; bottom: 0; display: flex; flex-direction: column; overflow: hidden; }
  .body.full { left: 0; padding: 12px 14px; }
  .crumb { flex: none; gap: 2px; padding-left: 2px; color: var(--fg-muted); }
  .head :global(.sep) { flex: none; color: var(--fg-faint); }
  .comp { flex: none; font-weight: 600; }
  .mode { flex: none; padding: 1px 7px; border-radius: var(--radius-full); font-size: var(--text-xs); font-weight: 600; background: color-mix(in srgb, var(--record) 18%, transparent); }
  .mode.anim { background: var(--accent-soft); color: var(--accent); }
  .scope-note { flex: none; font-size: var(--text-xs); color: var(--fg-muted); }
  .where { flex: none; display: inline-flex; align-items: center; gap: 4px; font-weight: 600; }
  .where.pair { gap: 2px; }
  .state-link { border: 0; background: transparent; color: var(--fg); font: inherit; font-weight: 600; padding: 1px 4px; border-radius: var(--radius-sm); cursor: pointer; }
  .state-link:hover { background: var(--hover); }
  .tname { color: var(--fg-muted); font-weight: 400; margin-left: 4px; overflow: hidden; text-overflow: ellipsis; }
  .grow { flex: 1; }
  .tsep { width: 1px; height: 18px; margin: 0 2px; background: var(--border); flex: none; }
  .readout { font-size: var(--text-sm); color: var(--fg-muted); font-variant-numeric: tabular-nums; min-width: 82px; }
  .len { flex: none; gap: 4px; padding: 0 8px; width: 64px; color: var(--fg-muted); font-size: var(--text-sm); }
  .len input { width: 100%; min-width: 0; border: 0; background: transparent; color: var(--fg); font: inherit; font-size: var(--text-md); outline: none; text-align: right; }
  .ease { width: 128px; flex: none; }
  .head :global(.timing) { flex: none; width: auto; padding: 0 8px; }
  .head :global(.timing.default) { color: var(--fg-muted); }
  .toast { pointer-events: auto; position: absolute; top: calc(var(--top) + 6px); right: 12px; z-index: 2; max-width: 420px; padding: 6px 10px; border: 1px solid var(--border-raised, var(--border)); border-radius: var(--radius); background: var(--bg-raised, var(--bg)); box-shadow: var(--shadow-popover, 0 4px 16px rgb(0 0 0 / 0.15)); font-size: var(--text-sm); }
  .error { color: var(--danger); font-size: var(--text-sm); max-width: 240px; overflow: hidden; text-overflow: ellipsis; }
  .paused { display: inline-flex; align-items: center; gap: 6px; padding: 2px 4px 2px 8px; border-radius: var(--radius-full); background: color-mix(in srgb, var(--record) 16%, transparent); font-size: var(--text-sm); }
  .scope.none { color: var(--fg-muted); font-size: var(--text-sm); }
  .muted { color: var(--fg-muted); }
  .dot { display: inline-block; flex: none; width: 7px; height: 7px; border-radius: 50%; background: var(--record); margin-right: 5px; }

  /* The navigator. */
  .nav-head { display: flex; align-items: center; gap: 2px; height: 26px; padding: 0 4px 0 10px; font-size: var(--text-sm); color: var(--fg-muted); flex: none; }
  .tree { flex: 1; overflow: auto; padding: 0 4px 6px; }
  .item { display: flex; align-items: center; min-height: 24px; border-radius: var(--radius); }
  .item:hover { background: var(--hover); }
  .item.on { background: var(--accent-soft); }
  .item.state.rec { background: color-mix(in srgb, var(--record) 14%, transparent); }
  .item .label { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; height: 24px; padding: 0 6px; border: 0; background: transparent; color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .item.state .label { font-weight: 600; }
  .item.pair .label { padding-left: 18px; color: var(--fg); }
  .item.default { margin-top: 6px; border-top: 1px solid var(--border); border-radius: 0; }
  .nm, .to { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .meta { margin-left: auto; font-size: var(--text-sm); color: var(--fg-muted); white-space: nowrap; font-variant-numeric: tabular-nums; }
  .meta.muted { font-style: italic; }
  .item :global(.more) { opacity: 0; }
  .item:hover :global(.more), .item.on :global(.more) { opacity: 1; }
  .rename { flex: 1; min-width: 0; height: 22px; min-height: 22px; margin: 1px 2px; }
  .graph { flex: 1; width: 100%; min-height: 0; }
  .graph-view { background: var(--bg); }
  .pill { fill: var(--bg); stroke: var(--border); }
  .edge.own .pill { stroke: var(--border-strong); }
  .edge.on .pill { stroke: var(--accent); }
  .initial { fill: var(--fg-muted); font-size: 10px; }
  .item.more-pairs { width: 100%; gap: 4px; padding: 0 6px 0 16px; border: 0; background: transparent; color: var(--fg-muted); font: inherit; font-size: var(--text-sm); cursor: pointer; }
  .item.default.pinned { flex: none; margin: 0 4px 4px; border-top: 1px solid var(--border); border-radius: 0; }
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
  .node:hover rect, .node.on rect { stroke: var(--accent); }
  .node.rec rect { fill: color-mix(in srgb, var(--record) 16%, transparent); stroke: var(--record); }
  .edge { cursor: pointer; }
  .edge path { fill: none; }
  .edge .hit { stroke: transparent; stroke-width: 12; }
  .edge .line { stroke: var(--fg-muted); stroke-width: 1.25; stroke-dasharray: 4 4; opacity: 0.7; }
  .edge.own .line { stroke-dasharray: none; opacity: 1; }
  .edge.none .line { opacity: 0.25; }
  .edge:hover .line, .edge.on .line { stroke: var(--accent); stroke-width: 2; opacity: 1; }
  .elabel { fill: var(--fg-muted); font-size: 10.5px; }
  .edge.on .elabel { fill: var(--accent); font-weight: 600; }
  .node:focus, .edge:focus { outline: none; }
  .arrowhead { fill: var(--fg-muted); }

  /* A state's look. */
  .toolbar { display: flex; align-items: center; gap: 8px; min-height: 30px; padding: 0 10px; border-bottom: 1px solid var(--border); flex: none; white-space: nowrap; overflow: hidden; }
  .rec-note { color: var(--fg-muted); font-size: var(--text-sm); overflow: hidden; text-overflow: ellipsis; }
  .toolbar .dot { margin-right: -2px; }
  .empty-wide { flex: 1; padding: 12px 14px; }
  .table-wrap { flex: 1; min-height: 0; overflow: auto; }
  .poses { border-collapse: collapse; font-variant-numeric: tabular-nums; }
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
  .poses .col-on { background: color-mix(in srgb, var(--record) 10%, transparent); }
  .poses td.rest .val { color: var(--fg-muted); }
  .cell { display: inline-flex; align-items: center; justify-content: flex-end; gap: 4px; }
  .cell input { width: 64px; min-height: 22px; height: 22px; padding: 0 6px; text-align: right; }
  .cell input.wide { width: 96px; text-align: left; }
  .sw { width: 12px; height: 12px; border-radius: 3px; box-shadow: inset 0 0 0 1px var(--border-strong); flex: none; }
  .users { padding: 8px 10px; display: flex; flex-direction: column; align-items: flex-start; gap: 2px; overflow: auto; }
  .aside-title { color: var(--fg-muted); font-size: var(--text-sm); padding-bottom: 4px; }
  .out { border: 0; border-radius: var(--radius); background: transparent; color: var(--fg); font: inherit; cursor: pointer; padding: 2px 8px; }
  .out:hover { background: var(--hover); }
  .first { display: flex; flex-direction: column; gap: 10px; max-width: 640px; }
  .first p { margin: 0; line-height: 1.5; }
  .first-fields { display: flex; align-items: flex-end; gap: 10px; }
  .stack { display: flex; flex-direction: column; gap: 4px; font-size: var(--text-sm); color: var(--fg-muted); }
  .first-fields .stack { width: 180px; }
  :global(.stagger-popover) { width: 240px; }
  :global(.add-popover) { width: 220px; }
</style>
