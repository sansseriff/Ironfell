<script lang="ts">
  // The session's selection (source: selection): what is picked, Link and
  // Show in, and Link's merge question. The seed of the inspector
  // (plans/specs/panels-and-session.md).
  import { selectionProperties, selectionColors } from "../../lib/ui/selection-properties";
  import CompositionSection from "./CompositionSection.svelte";
  import AttachmentTask from "../../lib/AttachmentTask.svelte";
  import { getContext } from "svelte";
  import { machines } from "../../lib/machines.svelte";
  import SliceSection from "./SliceSection.svelte";
  import StateSection from "./StateSection.svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  import { showIn as showPicture } from "../show_in";
  import { commandFailure } from "../../runtime/commands.svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import PropertyField, { type Property } from "../../lib/ui/PropertyField.svelte";
  import ClipboardPaste from "@lucide/svelte/icons/clipboard-paste";
  import PenLine from "@lucide/svelte/icons/pen-line";
  import { columnEdits, parseColumns } from "../../model/columns";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
  import Link from "@lucide/svelte/icons/link";
  import ComponentIcon from "@lucide/svelte/icons/component";
  import SquareDashed from "@lucide/svelte/icons/square-dashed";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import { Popover, DropdownMenu } from "bits-ui";
  import PaintRow from "../../lib/ui/PaintRow.svelte";
  import TextSection from "../../lib/ui/TextSection.svelte";
  import AppearanceIcon from "../../lib/ui/AppearanceIcon.svelte";
  import Scan from "@lucide/svelte/icons/scan";
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import X from "@lucide/svelte/icons/x";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Button from "../../lib/ui/Button.svelte";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import Select from "../../lib/ui/Select.svelte";
  import TextInput from "../../lib/ui/TextInput.svelte";
  import Checkbox from "../../lib/ui/Checkbox.svelte";
  import Disclosure from "../../lib/ui/Disclosure.svelte";
  import EmptyState from "../../lib/ui/EmptyState.svelte";
  import Unlink from "@lucide/svelte/icons/unlink";
  import Waypoints from "@lucide/svelte/icons/waypoints";
  import Section from "../../lib/ui/Section.svelte";
  import type { ViewSpec } from "../views";

  let { spec }: { spec: ViewSpec } = $props();
  void spec;

  const doc = controllerManager.docClient;
  const active = getContext<() => boolean>("iron-view-active") ?? (() => true);

  // Linking and Show in (ECM §7.2). Link takes the two picked pictures
  // (shift-click adds the second); a merge question opens the dialog.
  let linkError = $state("");
  async function link(keep = "") {
    linkError = "";
    const pair = doc.question ? [doc.question.aNode, doc.question.bNode] : doc.selection;
    try {
      const r = await doc.link(pair[0], pair[1], keep);
      if (!r.ok && !r.question) linkError = r.error;
    } catch (e) {
      linkError = String(e);
    }
  }
  // The title's actions are commands (commands.rs), as the canvas menu's and
  // keys' are: Create component (component-model.md §5.5), Wrap in empty. A
  // refusal arrives as a `commandFailed` event and shows here.
  let ranAt = $state(0);
  const commandError = $derived(ranAt > 0 && commandFailure.current && commandFailure.current.at >= ranAt && ["make-component", "wrap"].includes(commandFailure.current.id) ? commandFailure.current.reason : "");
  function command(id: string) {
    ranAt = performance.now();
    void doc.runCommand(id);
  }
  // The attachment task: started from the title's Attach points, it stays
  // mounted (and open) when the selection empties.
  let attach: ReturnType<typeof AttachmentTask> | undefined = $state();
  const showIn = $derived(doc.counterparts.figures.filter((f) => f.node !== doc.selection[0] && f.composition !== null));

  // The inspector (a first, HTML version; plans/specs/panels-and-session.md):
  // the selection's shared fields, each edited through its handle, so a
  // linearly bound value edits its source and a locked one says why.
  type Field = Property;
  type PortInfo = { spec: string; name: string | null; kind: string | null; dir: string | null; connector: string | null; gender: string | null; follow: boolean | null; signal: string | null; bind: string | null; kinds: string[]; editable?: boolean; datatype?: string | null; show?: string | null; group?: string | null; suggest?: string[] };
  type MainInfo = { def: string; name: string | null; instances: number; ports: { spec: string; name: string; dir: string; placed: boolean; port: PortInfo }[]; resizable?: boolean; machines?: MachineInfo[] };
  type MachineInfo = { id: string; port: string; pinned: string | null; states: { id: string; name: string | null }[]; transitions: { id: string; name: string | null; from: string | null; to: string | null; ms: string | null; ease: string | null }[] };
  type Inspected = { node: string; type: string; name: string | null; components: { kind: string; fields: Field[] }[]; addable: string[]; removable: string[]; component: { entity: string; name: string | null; def: string | null; linked: boolean; inputs: Field[]; context: { name: string; from: string | null; provider: string | null; text: string | null }[] } | null; main: MainInfo | null; port: PortInfo | null; table: string | null; sections?: string[]; constraints?: { node: string; horizontal: string; vertical: string } | null };
  // Every section, in the built-in order: what an older engine's inspection
  // (without `sections`) shows.
  const SECTIONS = ["main", "port", "component", "table", "position", "text", "layout", "appearance", "fill", "stroke", "effects", "other", "add"];
  let inspected: Inspected | null = $state(null);
  let setError = $state("");
  let generation = 0;
  let separateCorners = $state(false);
  let strokeAnchor: HTMLDivElement | undefined = $state();
  let inspections: Inspected[] = $state([]);
  const colors = $derived(selectionColors(inspections));
  async function refresh() {
    const nodes = [...doc.selection];
    const request = ++generation, session = doc.generation;
    if (!nodes.length) { inspections = []; inspected = null; return; }
    try {
      const results = await Promise.all(nodes.map(node => doc.command<Inspected>({ op: 'inspect', node })));
      if (request === generation && session === doc.generation && nodes.join() === doc.selection.join()) {
        inspections = results.filter(Boolean); inspected = selectionProperties(inspections);
      }
    } catch (e) { if (request === generation && session === doc.generation) setError = String(e); }
  }
  $effect(() => {
    void doc.generation;
    const nodes = [...doc.selection];
    generation++;
    separateCorners = false; inspected = null; inspections = []; setError = "";
    if (!nodes.length || !active()) return;
    const results = new Map<string, Inspected>();
    // One node's inspection alone can arrive mid-scrub, ahead of the
    // others': merged, the selection would read Mixed for a moment, and
    // the Fill row (with its open picker) would give way to the mixed
    // state. So a multi-selection rereads every node in one batch and
    // applies them together.
    let live = true, reading = false, again = false;
    const readAll = async () => {
      if (reading) { again = true; return; }
      reading = true;
      try {
        do {
          again = false;
          const all = await Promise.all(nodes.map(node => doc.command<Inspected>({ op: 'inspect', node })));
          if (!live || again) continue;
          inspections = all.filter(Boolean);
          inspected = selectionProperties(inspections);
        } while (live && again);
      } catch (e) { if (live) setError = String(e); }
      finally { reading = false; }
    };
    const stops = nodes.map(node => doc.subscribe<Inspected>({ op: 'inspect', node }, r => {
      results.set(node, r);
      if (results.size < nodes.length) return;
      if (nodes.length === 1) { inspections = [r].filter(Boolean); inspected = selectionProperties(inspections); }
      else void readAll();
    }, e => { setError = String(e); }));
    return () => { live = false; for (const stop of stops) stop(); };
  });
  // Timing (a key, a clip) is not a picture: no picture actions in the title.
  const TIMED = ["key", "clip"];
  // A facet with no title of its own, in the inspector's sentence case.
  const sentence = (kind: string) => kind.charAt(0).toUpperCase() + kind.slice(1).replaceAll("_", " ");
  const titles: Record<string, string> = { key: "Key", clip: "Clip", transform2d: "Position", size: "Layout", radius: "Dimensions", appearance: "Appearance", fill: "Fill", gradient: "Gradient", stroke: "Stroke", corners: "Corner radius", arc: "Arc", shadow: "Drop shadow", group_clip: "Clip content", layout: "Auto layout", layout_item: "In layout", table: "Table style", rules: "Lines" };
  const order = ["transform2d", "size", "radius", "layout", "layout_item", "appearance", "corners", "arc", "group_clip", "fill", "gradient", "stroke", "shadow"];
  const components = $derived.by(() => {
    const current = inspected as Inspected | null;
    return current ? [...current.components].sort((a, b) => (order.indexOf(a.kind) < 0 ? 99 : order.indexOf(a.kind)) - (order.indexOf(b.kind) < 0 ? 99 : order.indexOf(b.kind))) : [];
  });
  function fields(kind: string): Field[] { return (inspected as Inspected | null)?.components.find(c => c.kind === kind)?.fields ?? []; }
  function supports(kind: string): boolean { return !!inspected && (inspected.addable.includes(kind) || inspected.components.some(c => c.kind === kind)); }
  function field(kind: string, name: string, text = "0", valueKind = "number"): Field {
    return fields(kind).find(f => f.field === name) ?? { leaf: `${kind}.${name}`, field: name, text, kind: valueKind, nodes: inspections.map(i => i.node), bind: null, edit: "direct", via: null, why: null };
  }
  const cornerLeaves = ["corners.tl", "corners.tr", "corners.br", "corners.bl"];
  const cornerField = $derived.by(() => {
    const corners = ["tl", "tr", "br", "bl"].map(k => field("corners", k));
    const locked = corners.find(f => f.edit === "locked");
    return { ...corners[0], edit: locked ? "locked" : corners[0].edit, why: locked?.why ?? null, bind: corners.find(f => f.bind)?.bind ?? null };
  });
  const mixedCorners = $derived(cornerLeaves.some(k => fields("corners").find(f => f.leaf === k)?.mixed) || ["tr", "br", "bl"].some(k => field("corners", k).text !== field("corners", "tl").text));
  // Facets offered as "add" buttons: auto layout on a group
  // (layout-and-tables.md §2), and how a child sits in one, only where a
  // layout already places it (its position is locked by it).
  const placedByLayout = $derived(fields("transform2d").some(f => f.edit === "locked" && (f.why ?? "").includes("layout")));
  const addKinds = $derived(["arc", "group_clip", "layout", "rules", ...(placedByLayout ? ["layout_item"] : [])]);
  // A component's instance or main says what it is in its own section
  // (Main component, the component's): the `instance` facet's definition id
  // and the `use`'s notation are plumbing there, not properties to set.
  const plumbing = $derived.by(() => { const i = inspected as Inspected | null; return i?.component || i?.main ? ["instance", "use"] : []; });
  const otherComponents = $derived(components.filter(c => !["transform2d", "size", "radius", "appearance", "corners", "fill", "gradient", "stroke", "trim", "constraints", "shadow", "text", "font", "args", ...plumbing].includes(c.kind)));
  // A text node's box is its words' unless it is fixed (or wraps): only
  // then are W and H its own (plans/specs/text.md §2).
  const textSizing = $derived(fields("text").length ? (field("text", "sizing", "auto_width", "enum").text ?? "auto_width") : null);
  // Effects have their own list/presenter. Drop shadow is the currently
  // supported effect; future renderable effect components get entries here.
  const mixedFill = $derived(inspections.length > 1 && [...fields("fill"), ...fields("gradient")].some(f => f.mixed));
  function replaceFill() {
    return patch({ "fill.color": "#808080ff", "fill.opacity": "1" }, ["gradient"], "replace mixed fill", undefined, true);
  }
  const effectTypes = [{ kind: "shadow", label: "Drop shadow" }];
  const effects = $derived(effectTypes.filter(e => fields(e.kind).length));
  async function patch(values: Record<string, string>, remove: string[] = [], label = "edit appearance", target?: string, replace = false, nodes?: string[]) {
    if (!inspected) return;
    const node = target ?? inspected.node;
    setError = "";
    try {
      const r = await doc.command({ op: 'inspect_patch', node: node, patch: JSON.stringify({ values, remove, label, replace, nodes: nodes ?? (target ? [target] : inspections.map(i => i.node)) }) });
      if (!r.ok) setError = r.error;
      await refresh();
    } catch (e) { setError = String(e); }
  }
  function set(f: Field, text: string) { return patch({ [f.leaf]: text }, [], `set ${f.leaf}`, undefined, false, f.nodes); }
  // Detach a linked instance: its picture and ports become authored.
  async function detach() {
    if (!inspected?.component) return;
    setError = "";
    try {
      const r = await doc.command({ op: 'detach', entity: inspected.component.entity });
      if (!r.ok) setError = r.error;
      await refresh();
    } catch (e) { setError = String(e); }
  }
  // A table's structure (layout-and-tables.md §5.1): rows and columns at
  // its end, and merging or splitting the selected cells. One undo each.
  async function tableEdit(verb: string) {
    if (!inspected?.table) return;
    setError = "";
    try {
      const r = await doc.command({ op: 'table_edit', table: inspected.table, verb: verb, cells: JSON.stringify(doc.selection) });
      if (!r.ok) setError = r.error;
      await refresh();
    } catch (e) { setError = String(e); }
  }
  // A component input edits the instance's argument (component-model.md §4).
  function setInput(f: Field, text: string) { return inspected?.component ? patch({ [f.leaf]: text }, [], `set ${f.field}`, inspected.component.entity) : undefined; }
  function inputGroups(inputs: Field[]) {
    const groups: { name: string; inputs: Field[] }[] = [];
    for (const f of inputs) {
      const name = f.group ?? "";
      const g = groups.find((x) => x.name === name) ?? (groups.push({ name, inputs: [] }), groups[groups.length - 1]);
      g.inputs.push(f);
    }
    return groups;
  }
  // "Draw on": Trim End keyed 0 → 1 over a second from the playhead.
  async function drawOn() {
    if (!inspected) return;
    setError = "";
    try {
      const r = await doc.command({ op: 'inspect_animate', node: inspected.node, leaf: "trim.end", action: "draw_on" });
      if (!r.ok) setError = r.error;
      await refresh();
    } catch (e) { setError = String(e); }
  }
  // Data from a spreadsheet or CSV on the clipboard into a component's series
  // inputs (std:plot's xs, ys, …), one undo step (model/columns.ts).
  async function pasteData() {
    if (!inspected?.component) return;
    setError = "";
    let text = "";
    try { text = await navigator.clipboard.readText(); } catch (e) { setError = `Could not read the clipboard: ${String(e)}`; return; }
    const cols = parseColumns(text);
    if (!cols?.columns.length) { setError = "The clipboard holds no columns of numbers (copy cells from a spreadsheet, or CSV)."; return; }
    await patch(columnEdits(cols, inspected.component.inputs), [], "paste data", inspected.component.entity, true);
  }
  function setCorners(_f: Field, text: string) { return patch(Object.fromEntries(cornerLeaves.map(k => [k, text])), [], "round corners"); }
  function paintMode(mode: string) { return mode === "solid" ? patch({}, ["gradient"], "solid fill") : patch({ "gradient.kind": mode }, [], `${mode} fill`); }
  // Component authoring (component-authoring.md): ports, inputs, wrapping,
  // renaming, each one transaction through `document_author`.
  // A key on a state lane: ways into its component's state editor.
  let stateKey = $state<{ state: string; from: string | null } | null>(null);
  $effect(() => {
    const node = inspected?.type === "key" ? inspected.node : null;
    stateKey = null;
    if (node) void doc.command<{ ok: boolean; state: string | null; from: string | null }>({ op: "state_key", key: node }).then((r) => { if (r.ok && r.state && inspected?.node === node) stateKey = { state: r.state, from: r.from }; });
  });
  // The playhead, for the state control's note (choosing a state keys it
  // there when the state is animated): polled while an instance is shown.
  let clockT = $state(0);
  const clockLabel = $derived(`${clockT.toFixed(2)} s`);
  $effect(() => {
    if (!inspected?.component) return;
    const tick = () => void doc.command<{ ok: boolean; t: number }>({ op: "clock_read" }).then((r) => { if (r.ok) clockT = r.t; }).catch(() => {});
    tick();
    const id = setInterval(tick, 300);
    return () => clearInterval(id);
  });
  // Slices selected inside a transition (the state editor's tracks): the
  // Inspector shows what a slice is, not the bar that draws it.
  let sliceSel = $state<{ tween: string; channel: string }[]>([]);
  // Replies can come back out of order (a pick right after switching
  // transitions): only the latest request's answer is shown.
  let sliceAsk = 0;
  $effect(() => {
    const nodes = doc.selection;
    const ask = ++sliceAsk;
    if (!machines.pinned?.scrub || !nodes.length) {
      sliceSel = [];
      return;
    }
    void doc.command<{ ok: boolean; slices?: { tween: string; channel: string }[] }>({ op: "author", verb: "selected_slices", args: JSON.stringify({ nodes }) }).then((r) => {
      if (ask === sliceAsk) sliceSel = r.ok && r.slices && r.slices.length ? r.slices : [];
    });
  });
  // The first states: the editor opens on its naming form (the component's
  // look now, and the new state), which makes them.
  async function addFirstState() {
    const def = inspected?.main?.def;
    if (!def) return;
    const r = await machines.open(def);
    if (r && !r.ok) setError = (r as { error?: string }).error ?? "That did not work";
  }
  async function author(verb: string, args: Record<string, unknown>) {
    setError = "";
    try {
      const r = await doc.command({ op: 'author', verb: verb, args: JSON.stringify(args) });
      if (!r.ok) setError = r.error;
      await refresh();
      return r;
    } catch (e) { setError = String(e); }
  }
  function editPort(spec: string, patch: Record<string, unknown>) { return author("edit_port", { spec, patch }); }
  // The main's setting being edited, if any (a port with no anchor).
  let openSpec: string | null = $state(null);
  const GENDERS = ["any", "male", "female", "neutral"];
  // Optional components (fill, stroke, corners, arc, shadow): one
  // transaction each, "add shadow" or "remove corners".
  async function component(kind: string, add: boolean) {
    if (!inspected) return;
    setError = "";
    try {
      const r = await doc.command({ op: 'inspect_patch', node: inspected.node, patch: JSON.stringify({ nodes: inspections.map(i => i.node), [add ? "add" : "remove"]: [kind], label: `${add ? "add" : "remove"} ${kind}` }) });
      if (!r.ok) setError = `${kind}: ${r.error}`;
      await refresh();
    } catch (e) { setError = String(e); }
  }
</script>

<section class="html-view properties inspector" data-selection={inspections.map(i => i.node).join(" ")}>
  <!-- The composition's settings while nothing is selected, as Figma shows
       the page's: a selection gets the whole panel. -->
  {#if !doc.selection.length && !(machines.editing && machines.pinned)}<CompositionSection />{/if}

  <!-- The object: its name, its kind (the id in the tooltip), and what to
       do with the selection, one row of icon buttons. -->
  {#if sliceSel.length}
    <SliceSection slices={sliceSel} />
  {:else if inspected}
    <!-- One line: scripts read the title's text ("name kind"). -->
    <header class="ui-section-header title"><span class="title-name">{inspected.name ?? inspected.type}</span>{#if inspected.name}{" "}<span class="ui-section-meta" title="{inspected.type} {inspected.node}">{inspected.type}</span>{/if}{#if !TIMED.includes(inspected.type)}<span class="ui-section-actions">{#if doc.selection.length === 2}<IconButton icon={Link} label="Link" title="Link the two pictures: one entity with both as figures" onclick={() => link()} />{/if}{#if inspected.main || inspected.component}<IconButton icon={ComponentIcon} label="Make component" title="Make a component holding this one" onclick={() => command("make-component")} />{:else}<Button small variant="outline" onclick={() => command("make-component")} title="Make a component of the selection"><ComponentIcon size={14} strokeWidth={1.75} />Make component</Button>{/if}<IconButton icon={SquareDashed} label="Wrap in empty" title="Wrap the selection in an empty at its centre (⌘⌥G)" onclick={() => command("wrap")} />{#if !attach?.isOpen()}<span class="attach" data-attachment-task><IconButton icon={Waypoints} label="Attach points to control" title="Attach points to control: select a main component or an empty in its main" onclick={() => attach?.begin()} /></span>{/if}</span>{/if}</header>
  {/if}
  <AttachmentTask bind:this={attach} />
  {#if showIn.length}
    <div class="show-in">
      {#each showIn as f (f.node)}
        <Button small onclick={() => void showPicture({ node: f.node, composition: f.composition })}>Show in {f.name ?? f.composition}<ChevronRight size={14} strokeWidth={1.75} /></Button>
      {/each}
    </div>
  {/if}
  {#if linkError}<p class="error">{linkError}</p>{/if}
  {#if commandError}<p class="error">{commandError}</p>{/if}
  {#if doc.question}
    <div class="question" role="dialog" aria-label="Merge entities">
      <p>Both pictures already stand for an entity. Which one should remain?</p>
      {#each [doc.question.a, doc.question.b] as e (e.entity)}
        <div class="candidate">
          <strong>{e.name ?? e.entity}</strong> <span class="mono">{e.entity}</span> · {e.def ?? "no definition"}
          <div class="mono">args: {e.args.length ? e.args.map((p: [string, string]) => `${p[0]}=${p[1]}`).join(", ") : "none"}</div>
          <div class="mono">figures: {e.figures.join(" ")}{e.ports.length ? ` · ports: ${e.ports.join(", ")}` : ""}</div>
          <Button small variant="outline" onclick={() => link(e.entity)}>Keep {e.name ?? e.entity}</Button>
        </div>
      {/each}
      {#if doc.question.port_collisions.length}<p class="error">Ports on both: {doc.question.port_collisions.join(", ")}</p>{/if}
      <Button small onclick={() => (doc.question = null)}>Cancel</Button>
    </div>
  {/if}

  {#if sliceSel.length}
    <!-- Slices: the section above. -->
  {:else if inspected}
    <div class="sections">
      {#if stateKey}
        <div class="state-key">
          {#if stateKey.from}<Button small variant="outline" onclick={() => machines.openKey(inspected!.node, "band")} title="Its length, easing and slices are the component's: every instance's {stateKey.from} → {stateKey.state} changes">Edit transition {stateKey.from} → {stateKey.state}…</Button>{/if}
          <Button small variant="outline" onclick={() => machines.openKey(inspected!.node, "lane")} title="Edit the look of {stateKey.state} (every instance's)">Edit state {stateKey.state}…</Button>
        </div>
      {/if}
      {#if setError}<p class="error">{setError}</p>{/if}
      {#if doc.selection.length > 1}<p class="selection-note">Edits apply to all selected objects. Differing values show Mixed.</p>{/if}
      {#snippet portEditor(p: PortInfo, editable: boolean)}
        <div class="property-grid port-grid" data-section="port-editor">
          <label class="port-field">Name<TextInput value={p.name ?? ""} disabled={!editable} onchange={(e: Event) => editPort(p.spec, { name: (e.currentTarget as HTMLInputElement).value })} /></label>
          <div class="port-field"><span>Kind</span><Select label="Kind" value={p.kind ?? "value"} disabled={!editable} items={p.kinds.map((k) => ({ value: k, label: k }))} onChange={(v) => editPort(p.spec, { kind: v })} /></div>
          <div class="port-field"><span>Direction</span><Select label="Direction" value={p.dir ?? "in"} disabled={!editable} items={["in", "out", "pair"].map((d) => ({ value: d, label: d }))} onChange={(v) => editPort(p.spec, { dir: v })} /></div>
          <label class="port-field">Connector<TextInput value={p.connector ?? ""} placeholder="any" disabled={!editable} onchange={(e: Event) => editPort(p.spec, { connector: (e.currentTarget as HTMLInputElement).value })} /></label>
          <div class="port-field"><span>Gender</span><Select label="Gender" value={p.gender ?? "any"} disabled={!editable} items={GENDERS.map((g) => ({ value: g, label: g }))} onChange={(v) => editPort(p.spec, { gender: v })} /></div>
          {#if p.follow !== null}<div class="port-field check"><Checkbox label="Loose end" checked={!!p.follow} disabled={!editable} onChange={(on) => editPort(p.spec, { follow: on })} /></div>{/if}
          <!-- Its type (types-and-units.md): empty is loose, connecting to
               anything; the suggestions come from its value and from typed
               ports its instances are connected to (§3.5). -->
          <label class="port-field wide">Data type<TextInput class="mono" value={p.datatype ?? ""} placeholder="any (loose)" disabled={!editable} title="nm, length, choice(a|b), bool, color, text, series, fn(1), like(input); add | auto. Empty: loose, it connects to anything." onchange={(e: Event) => editPort(p.spec, { datatype: (e.currentTarget as HTMLInputElement).value })} /></label>
          {#if editable && !p.datatype && p.suggest?.length}
            <div class="wide type-suggest"><span class="hint">Use</span>{#each p.suggest as t (t)}<Button small variant="outline" onclick={() => editPort(p.spec, { datatype: t })} title="Declare this type">{t}</Button>{/each}</div>
          {/if}
          {#if p.dir !== "out"}
            <div class="port-field"><span>Show as pin</span><Select label="Show as pin" value={p.show ?? "auto"} disabled={!editable} items={[{ value: "auto", label: "Auto" }, { value: "pin", label: "Always" }, { value: "settings", label: "In settings only" }]} onChange={(v) => editPort(p.spec, { show: v })} /></div>
            <label class="port-field">Group<TextInput value={p.group ?? ""} placeholder="none" disabled={!editable} onchange={(e: Event) => editPort(p.spec, { group: (e.currentTarget as HTMLInputElement).value })} /></label>
          {/if}
          <label class="port-field wide">{p.dir === "out" ? "Value" : "Default"}<TextInput class="mono" value={p.bind ? `{= ${p.bind}}` : (p.signal ?? "")} disabled={!editable} title="A number or quantity (10 mW), or an expression in braces: {'{'}= #power.port.signal{'}'}" onchange={(e: Event) => editPort(p.spec, { signal: (e.currentTarget as HTMLInputElement).value })} /></label>
        </div>
      {/snippet}
      <!-- Sections in the order the selection's view resolves (`inspector.sections`,
           contexts.md §2.7); each shows only where its node has what it edits. -->
      {#each inspected.sections ?? SECTIONS as section (section)}
        {#if section === "main" && (inspected.main)}
          <Section title="Main component" class="property-section" headerClass="component" data-section="main" meta="{inspected.main.instances} instance{inspected.main.instances === 1 ? '' : 's'}">
            {#snippet heading()}<h3 class="main-chip"><ComponentIcon size={14} strokeWidth={1.75} />Main component</h3>{/snippet}
            <label class="port-field">Name<TextInput value={inspected.main.name ?? ""} onchange={(e: Event) => author("rename", { node: inspected!.main!.def, name: (e.currentTarget as HTMLInputElement).value })} /></label>
            <div class="ports" data-section="ports">
              {#each inspected.main.ports as p (p.spec)}
                <button class="port-row" class:open={openSpec === p.spec} onclick={() => (openSpec = openSpec === p.spec ? null : p.spec)} title={p.placed ? "A port placed on the picture" : "A setting: no place on the picture"}>
                  <span class="dir {p.dir}">{p.dir}</span><span>{p.name}</span><span class="mono">{p.port?.kind}</span>{#if !p.placed}<span class="setting">setting</span>{/if}
                </button>
                {#if openSpec === p.spec && p.port}{@render portEditor(p.port, true)}{/if}
              {/each}
            </div>
            <div class="inline-row"><Button small variant="outline" onclick={() => author("add_input", { def: inspected!.main!.def, name: "input", default: "0" })} title="Add a setting: an input with no place on the picture"><Plus size={14} strokeWidth={1.75} />Input</Button><span class="hint">Ports: the port tool, on the main</span></div>
            <!-- States (machines.md §6): a doorway into the state editor, which
                 opens in the timeline panel; the inspector stays for values. -->
            <div class="states-door" data-section="states">
              {#if inspected.main.machines?.length}
                {@const m0 = inspected.main.machines[0]}
                <span class="door-label">States</span><span class="door-names" title={m0.states.map((s) => s.name).join(", ")}>{m0.states.map((s) => s.name).join(" · ")}</span>
                <Button small variant="outline" onclick={() => machines.open(inspected!.main!.def)} >Edit states…</Button>
              {:else}
                <span class="door-label">States</span><span class="door-names hint">none</span>
                <Button small variant="outline" onclick={addFirstState}><Plus size={14} strokeWidth={1.75} />Add state</Button>
              {/if}
            </div>
            <!-- Figma's "Resizing" (frame-constraints.md §4): the picture gets a box
                 of its parts' bounds; each part says how it follows the box. -->
            <div class="inline-row resizable" title="Each picture of this component gets its own size; drag a corner to resize it. Set how each part follows in its Constraints.">
              <Checkbox label="Resizable" checked={!!inspected.main.resizable} onChange={(on) => author("resizable", { def: inspected!.main!.def, on })} />
              <span class="hint">{inspected.main.resizable ? "parts follow their constraints" : "pictures keep the main's size"}</span>
            </div>
          </Section>
        {:else if section === "port" && (inspected.port)}
          <Section title="Port" class="property-section" headerClass="component" data-section="port" meta={inspected.port.editable ? undefined : "edit on the main"}>
            {@render portEditor(inspected.port, !!inspected.port.editable)}
          </Section>
        {:else if section === "component" && (inspected.component)}
          <Section title={inspected.component.def ?? "Component"} aria-label="Component" class="property-section" headerClass="component" data-section="component" meta="{inspected.component.name ?? inspected.component.entity}{inspected.component.linked ? '' : ' · detached'}">
            {#snippet actions()}{#if inspected?.component?.linked}<IconButton icon={Unlink} label="Detach instance" title="Detach: make this instance's picture and ports plain shapes that no longer follow the definition" onclick={detach} />{/if}{/snippet}
            {@const hasStates = machines.view?.entity === inspected.component.entity && !!machines.view?.machines.length}
            {#if inspected.component.inputs.some((f: Field) => f.kind === "series")}
              <div class="inline-row"><Button small variant="outline" onclick={pasteData} title="Paste columns copied from a spreadsheet or CSV: the first into the first series input (xs), the next into ys, … A header like “λ (nm)” sets the unit and the axis label. Replaces their formulas."><ClipboardPaste size={14} strokeWidth={1.75} />Paste data</Button><span class="hint">columns from a spreadsheet or CSV</span></div>
            {/if}
            <!-- Inputs by their author's groups (types-and-units.md §6), in order. -->
            {#each inputGroups(inspected.component.inputs) as g (g.name)}
              {#if g.name === "States" && hasStates}
                <!-- One heading for the states: the control sets this
                     instance's (and plays the transition); the editor changes
                     every instance's. -->
                {@const run = machines.view?.machines[0]?.current}
                {@const stateField = g.inputs.find((f) => f.group === "States")}
                {@const keyed = !!stateField?.key && stateField.key !== "none"}
                <div class="input-group states-head"><span class="ui-subheading">States{#if keyed}{" "}<span class="ui-badge" title="Choosing a state adds a key at {clockLabel}">keyed</span>{/if}</span><Button small variant="outline" onclick={() => machines.open(machines.view!.def, undefined, undefined, inspected?.component?.name ?? null)}>Edit states…</Button></div>
                {#if run?.transition && run.from && run.to}<div class="hint running" role="status">{run.from} → {run.to} · {Math.round((run.progress ?? 0) * 100)}%</div>{/if}
              {:else if g.name}<div class="ui-subheading input-group">{g.name}</div>{/if}
              <div class="property-grid">{#each g.inputs as f (f.leaf)}<PropertyField field={f} node={inspected.component.entity} onSet={setInput} onRefresh={refresh} onError={(e) => setError = e} />{/each}</div>
            {/each}
            <!-- What this picture reads from where it is placed (reactivity.md §6). -->
            {#if inspected.component.context.length}
              <div class="ui-subheading input-group" title="Values this picture takes from the group it is placed in, not from its inputs">From context</div>
              <div class="context-reads" data-section="context">
                {#each inspected.component.context as c (c.name)}
                  <div class="context-read" title={c.from ? `@${c.name} is provided by ${c.from}` : `Nothing above provides @${c.name}: the component's own default`}>
                    <span class="mono">@{c.name}</span><span class="context-value">{c.text ?? "—"}</span><span class="door-label">{c.from ? `from ${c.from}` : "default"}</span>
                  </div>
                {/each}
              </div>
            {/if}
          </Section>
        {:else if section === "table" && inspected.table && fields("transform2d").length}
            <Section title="Table" class="property-section" headerClass="component" data-section="table">
              <div class="button-row">
                <Button small variant="outline" onclick={() => tableEdit("add_row")} title="Add a row at the bottom"><Plus size={14} strokeWidth={1.75} />Row</Button>
                <Button small variant="outline" onclick={() => tableEdit("remove_row")} title="Remove the bottom row"><Minus size={14} strokeWidth={1.75} />Row</Button>
                <Button small variant="outline" onclick={() => tableEdit("add_column")} title="Add a column at the right"><Plus size={14} strokeWidth={1.75} />Column</Button>
                <Button small variant="outline" onclick={() => tableEdit("remove_column")} title="Remove the right column"><Minus size={14} strokeWidth={1.75} />Column</Button>
                <Button small variant="outline" onclick={() => tableEdit("preset:light")} title="Hairlines between rows, a muted label column">Light</Button>
                <Button small variant="outline" onclick={() => tableEdit("preset:boxed")} title="Every cell boxed, a filled header">Boxed</Button>
                {#if inspected.table !== inspected.node}
                  <Button small variant="outline" onclick={() => tableEdit("merge")} disabled={doc.selection.length < 2} title="Merge the selected cells into one">Merge</Button>
                  <Button small variant="outline" onclick={() => tableEdit("split")} disabled={doc.selection.length !== 1} title="Split a merged cell into single cells">Split</Button>
                {/if}
              </div>
            </Section>
        {:else if section === "position" && fields("transform2d").length}
          <Section title="Position" class="property-section" headerClass="component" data-section="position">
            <div class="property-grid">
              {#each ["x", "y"] as axis}<PropertyField field={field("transform2d", axis)} node={inspected.node} compact prefix={axis.toUpperCase()} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />{/each}
              <PropertyField field={field("transform2d", "rot")} node={inspected.node} compact label="Rotation" onSet={set} onRefresh={refresh} onError={(e) => setError = e}>
                {#snippet icon()}<RotateCw size={14} strokeWidth={1.75} />{/snippet}
              </PropertyField>
            </div>
            {#if inspected.constraints}
              {@const c = inspected.constraints}
              <!-- Constraints (frame-constraints.md §3): how this part follows a
                   resized picture of its component. -->
              <div class="ui-subheading constraints-head">Constraints</div>
              <div class="constraints">
                <div class="constraints-box" aria-hidden="true">
                  <span class="bar left" class:on={c.horizontal === "left" || c.horizontal === "left_right"}></span>
                  <span class="bar right" class:on={c.horizontal === "right" || c.horizontal === "left_right"}></span>
                  <span class="bar top" class:on={c.vertical === "top" || c.vertical === "top_bottom"}></span>
                  <span class="bar bottom" class:on={c.vertical === "bottom" || c.vertical === "top_bottom"}></span>
                  <span class="bar midh" class:on={c.horizontal === "center"}></span>
                  <span class="bar midv" class:on={c.vertical === "center"}></span>
                  <span class="core"></span>
                </div>
                <div class="constraints-selects">
                  <Select label="Horizontal" value={c.horizontal} items={[{ value: "left", label: "Left" }, { value: "right", label: "Right" }, { value: "left_right", label: "Left & right" }, { value: "center", label: "Center" }, { value: "scale", label: "Scale" }]} onChange={(v) => patch({ "constraints.horizontal": v }, [], "constraints", c.node)} />
                  <Select label="Vertical" value={c.vertical} items={[{ value: "top", label: "Top" }, { value: "bottom", label: "Bottom" }, { value: "top_bottom", label: "Top & bottom" }, { value: "center", label: "Center" }, { value: "scale", label: "Scale" }]} onChange={(v) => patch({ "constraints.vertical": v }, [], "constraints", c.node)} />
                </div>
              </div>
            {/if}
            <Disclosure label="Scale">
              <div class="property-grid disclosed">
                {#each ["sx", "sy"] as axis}<PropertyField field={field("transform2d", axis, "1")} node={inspected.node} compact label={axis === "sx" ? "Scale X" : "Scale Y"} prefix={axis === "sx" ? "X" : "Y"} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />{/each}
              </div>
            </Disclosure>
          </Section>
        {:else if section === "text" && (fields("text").length)}
          <TextSection nodes={inspections.map(i => i.node)} node={inspected.node} {field} {patch} {refresh} onError={(e) => setError = e} />
        {:else if section === "layout" && ((fields("size").length && textSizing !== "auto_width") || fields("radius").length)}
          <Section title="Layout" class="property-section" headerClass="component" data-section="layout">
            <div class="ui-subheading">{fields("size").length ? "Dimensions" : "Radii"}</div>
            <div class="property-grid">
              {#each (fields("size").length ? fields("size").filter(f => textSizing !== "auto_height" || f.field === "w") : fields("radius")) as f (f.leaf)}
                <PropertyField field={f} node={inspected.node} compact prefix={f.field.toUpperCase()} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />
              {/each}
            </div>
          </Section>
        {:else if section === "appearance" && (supports("appearance") || supports("corners"))}
          <Section title="Appearance" class="property-section" headerClass="component" data-section="appearance">
            <div class="property-grid appearance-grid" class:has-corners={supports("corners")}>
              {#if supports("appearance")}<PropertyField field={field("appearance", "opacity", "1")} node={inspected.node} compact label="Opacity" onSet={set} onRefresh={refresh} onError={(e) => setError = e}>
                {#snippet icon()}<AppearanceIcon kind="opacity" />{/snippet}
              </PropertyField>{/if}
              {#if supports("corners")}
                <PropertyField field={cornerField} node={inspected.node} compact label="Corner radius" mixed={mixedCorners} scrubLeaves={cornerLeaves} onSet={setCorners} onRefresh={refresh} onError={(e) => setError = e}>
                  {#snippet icon()}<AppearanceIcon kind="radius" />{/snippet}
                </PropertyField>
                <IconButton icon={Scan} label="Individual corners" pressed={separateCorners} onPressedChange={(on) => separateCorners = on} />
              {/if}
            </div>
            {#if separateCorners}
              <div class="property-grid individual-corners">
                {#each [{ key: "tl", label: "Top left" }, { key: "tr", label: "Top right" }, { key: "bl", label: "Bottom left" }, { key: "br", label: "Bottom right" }] as const as corner}
                  <PropertyField field={field("corners", corner.key)} node={inspected.node} compact label={corner.label} onSet={set} onRefresh={refresh} onError={(e) => setError = e}>
                    {#snippet icon()}<AppearanceIcon kind={corner.key} />{/snippet}
                  </PropertyField>
                {/each}
              </div>
            {/if}
          </Section>
        {:else if section === "fill" && (supports("fill") || fields("gradient").length)}
          <Section title="Fill" class="property-section" headerClass="component" data-section="fill">
            {#snippet actions()}{#if mixedFill}<IconButton icon={Plus} label="Replace mixed fill" title="Replace every selected fill with one solid fill" onclick={replaceFill} />{:else if !fields("fill").length && !fields("gradient").length}<IconButton icon={Plus} label="Add fill" onclick={() => component("fill", true)} />{/if}{/snippet}
            {#if mixedFill}
              <EmptyState inline>Click + to replace mixed content</EmptyState>
            {:else if fields("fill").length || fields("gradient").length}
              <PaintRow node={inspected.node} label="Fill" color={field("fill", "color", "#808080ff", "color")} opacity={field("fill", "opacity", "1")} gradient={fields("gradient")} onSet={set} onRefresh={refresh} onError={(e) => setError = e} onMode={paintMode} onRemove={() => patch({}, ["fill", "gradient"], "remove fill")} />
            {:else}<EmptyState inline>No fill</EmptyState>{/if}
          </Section>
        {:else if section === "stroke" && (supports("stroke"))}
          <Section title="Stroke" class="property-section" headerClass="component" data-section="stroke">
            {#snippet actions()}{#if !fields("stroke").length}<IconButton icon={Plus} label="Add stroke" onclick={() => component("stroke", true)} />{/if}{/snippet}
            {#if fields("stroke").length}
              <PaintRow node={inspected.node} label="Stroke" color={field("stroke", "color", "#000000ff", "color")} onSet={set} onRefresh={refresh} onError={(e) => setError = e} onRemove={() => component("stroke", false)} />
              <div class="stroke-options" bind:this={strokeAnchor}>
                <div class="readout-field"><span class="ui-subheading">Position</span><div class="ui-field readout" title="Strokes currently render centred on the outline">Centre</div></div>
                <PropertyField field={field("stroke", "width", "1")} node={inspected.node} label="Weight" onSet={set} onRefresh={refresh} onError={(e) => setError = e} />
                <Popover.Root>
                  <Popover.Trigger class="ui-icon-button" aria-label="Stroke settings" title="Stroke settings"><SlidersHorizontal size={14} strokeWidth={1.75} /></Popover.Trigger>
                  <Popover.Portal><Popover.Content class="ui-popover" side="left" align="center" sideOffset={12} customAnchor={strokeAnchor} aria-label="Stroke settings">
                    <header><strong>Stroke settings</strong><Popover.Close class="ui-icon-button" aria-label="Close stroke settings"><X size={14} strokeWidth={1.75} /></Popover.Close></header>
                    <div class="ui-popover-body">
                      <div class="ui-subheading">Basic</div>
                      <div class="property-grid">
                        {#each fields("stroke").filter(f => ["join", "cap", "miter", "dash", "dash_offset", "path_length"].includes(f.field)) as f (f.leaf)}
                          <PropertyField field={f} node={inspected.node} label={{ join: "Join", cap: "Cap", miter: "Miter limit", dash: "Dash pattern", dash_offset: "Dash offset", path_length: "Path length" }[f.field]} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />
                        {/each}
                      </div>
                      <!-- Trim Paths (plans/specs/wires-and-formulas.md §5): the part of the
                           outline that is stroked; "draw on" animates End from 0 to 1. -->
                      {#if supports("trim")}
                        <div class="ui-subheading trim-heading"><span>Trim</span>
                          {#if fields("trim").length}
                            <IconButton icon={Minus} label="Remove trim" onclick={() => component("trim", false)} />
                          {:else}
                            <IconButton icon={Plus} label="Add trim" onclick={() => component("trim", true)} />
                          {/if}
                        </div>
                        <div><Button small variant="outline" onclick={drawOn} title="Key Trim End from 0 at the playhead to 100% a second later, with Easy Ease (adds Trim if needed)"><PenLine size={14} strokeWidth={1.75} />Draw on</Button></div>
                        {#if fields("trim").length}
                          <div class="property-grid">
                            {#each fields("trim") as f (f.leaf)}
                              <PropertyField field={f} node={inspected.node} label={{ start: "Start", end: "End", offset: "Offset", mode: "Pieces" }[f.field]} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />
                            {/each}
                          </div>
                        {/if}
                      {/if}
                    </div>
                  </Popover.Content></Popover.Portal>
                </Popover.Root>
              </div>
            {:else}<EmptyState inline>No stroke</EmptyState>{/if}
          </Section>
        {:else if section === "effects" && (supports("shadow"))}
          <Section title="Effects" class="property-section" headerClass="component" data-section="effects">
            {#snippet actions()}
              <DropdownMenu.Root>
                <DropdownMenu.Trigger class="ui-icon-button" aria-label="Add effect" title="Add effect"><Plus size={14} strokeWidth={1.75} /></DropdownMenu.Trigger>
                <DropdownMenu.Portal><DropdownMenu.Content class="ui-menu" side="left" align="start" sideOffset={10}>
                  {#each effectTypes as effect}<DropdownMenu.Item class="ui-menu-item" data-effect-type={effect.kind} disabled={fields(effect.kind).length > 0} onSelect={() => component(effect.kind, true)}>{effect.label}</DropdownMenu.Item>{/each}
                </DropdownMenu.Content></DropdownMenu.Portal>
              </DropdownMenu.Root>
            {/snippet}
            {#each effects as effect (effect.kind)}
              <div class="effect-row">
                <Popover.Root>
                  <Popover.Trigger class="ui-field effect-trigger" aria-label="{effect.label} settings"><Square size={14} strokeWidth={1.75} /><span>{effect.label}</span><SlidersHorizontal size={14} strokeWidth={1.75} /></Popover.Trigger>
                  <Popover.Portal><Popover.Content class="ui-popover" side="left" align="start" sideOffset={12} aria-label="{effect.label} settings">
                    <header><strong>{effect.label}</strong><Popover.Close class="ui-icon-button" aria-label="Close effect settings"><X size={14} strokeWidth={1.75} /></Popover.Close></header>
                    <div class="ui-popover-body">
                      <PaintRow node={inspected.node} label="Shadow" color={field(effect.kind, "color", "#00000040", "color")} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />
                      <div class="property-grid">
                        {#each fields(effect.kind).filter(f => f.kind === "number") as f (f.leaf)}<PropertyField field={f} node={inspected.node} label={{ dx: "X", dy: "Y", blur: "Blur" }[f.field]} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />{/each}
                      </div>
                    </div>
                  </Popover.Content></Popover.Portal>
                </Popover.Root>
                <IconButton icon={Minus} label="Remove {effect.kind}" title="Remove {effect.label.toLowerCase()}" onclick={() => component(effect.kind, false)} />
              </div>
            {:else}<EmptyState inline>No effects</EmptyState>{/each}
          </Section>
        {:else if section === "other"}
        {#each otherComponents as c (c.kind)}
          <Section title={titles[c.kind] ?? sentence(c.kind)} class="property-section" headerClass="component" data-kind={c.kind}>
            {#snippet actions()}{#if inspected?.removable.includes(c.kind)}<IconButton icon={Minus} label="Remove {c.kind}" onclick={() => component(c.kind, false)} />{/if}{/snippet}
            <div class="property-grid">{#each c.fields as f (f.leaf)}<PropertyField field={f} node={inspected.node} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />{/each}</div>
          </Section>
        {/each}
        {:else if section === "add" && (inspected.addable.some(k => addKinds.includes(k)))}
          <!-- A facet not yet added is its section's title with a + (Figma's
               "Auto layout +"), as Stroke and Effects are when empty. -->
          {#each inspected.addable.filter(k => addKinds.includes(k)) as kind (kind)}
            <Section title={titles[kind] ?? sentence(kind)} class="property-section is-addable" data-add={kind}>
              {#snippet actions()}<IconButton icon={Plus} label="Add {kind}" title="Add {(titles[kind] ?? sentence(kind)).toLowerCase()}" onclick={() => component(kind, true)} />{/snippet}
            </Section>
          {/each}
        {/if}
      {/each}
      {#if colors.length}
        <Section title="Selection colors" class="property-section" data-section="selection-colors">
          {#each colors as color (`${color.leaf}:${color.nodes!.join()}`)}
            <PaintRow node={color.nodes![0]} label={`${titles[color.leaf.split(".")[0]] ?? color.field} ${color.text!.slice(0, 7).toUpperCase()}`} {color} onSet={set} onRefresh={refresh} onError={(e) => setError = e} />
          {/each}
        </Section>
      {/if}
    </div>
  {:else if machines.editing && machines.pinned}
    <StateSection />
  {:else}
    <EmptyState icon={MousePointer2} title="Make a selection" />
  {/if}
  {#if inspected && !sliceSel.length}<footer>Alt + drag a number to adjust <span>⇧ Precision</span></footer>{/if}
</section>

<style>
  .properties { padding: 0 !important; gap: 0 !important; font-size: var(--text-md); }
  .show-in { display: flex; flex-wrap: wrap; gap: 4px; padding: 8px calc(var(--panel-pad) - 8px); border-bottom: 1px solid var(--border); }
  .attach { display: contents; }
  .button-row, .inline-row { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; }
  .button-row { gap: 4px; }
  .sections { display: flex; flex-direction: column; }
  .title { padding: 0 var(--panel-pad); min-height: var(--panel-header-h); border-bottom: 1px solid var(--border); }
  .title-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mono { font-family: var(--font-mono); font-size: var(--text-xs); color: var(--fg-faint); }
  .hint { font-size: var(--text-sm); color: var(--fg-muted); }
  .error { padding: 8px var(--panel-pad) 0; overflow-wrap: anywhere; }
  .selection-note { padding: 8px var(--panel-pad) 0; color: var(--fg-muted); font-size: var(--text-sm); }
  footer { display: flex; justify-content: space-between; padding: 12px var(--panel-pad); font-size: var(--text-sm); color: var(--fg-muted); }
  .state-key { display: flex; flex-wrap: wrap; gap: 4px; padding: 8px var(--panel-pad) 0; }
  .question { display: flex; flex-direction: column; gap: 8px; margin: 8px var(--panel-pad); padding: 12px; border: 1px solid var(--danger); border-radius: var(--radius-lg); }
  .context-reads { display: flex; flex-direction: column; gap: 4px; }
  .context-read { display: flex; align-items: baseline; gap: 8px; min-height: 22px; }
  .context-value { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .states-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .running { color: var(--accent); font-variant-numeric: tabular-nums; }
  .states-door { display: flex; align-items: center; gap: 8px; min-height: var(--control-h); }
  .door-label { color: var(--fg-muted); font-size: var(--text-sm); }
  .door-names { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .input-group { margin-top: 4px; }
  .constraints-head { margin-top: 4px; }
  .constraints { display: flex; gap: 10px; align-items: center; }
  .constraints-box { position: relative; flex: 0 0 64px; height: 64px; border-radius: var(--radius); background: var(--bg-sunken); }
  .constraints-box .core { position: absolute; inset: 22px; border: 1px solid var(--border-strong); border-radius: 2px; }
  .constraints-box .bar { position: absolute; background: var(--border-strong); border-radius: 1px; }
  .constraints-box .bar.on { background: var(--accent); }
  .constraints-box .left { left: 6px; top: 31px; width: 12px; height: 2px; }
  .constraints-box .right { right: 6px; top: 31px; width: 12px; height: 2px; }
  .constraints-box .top { top: 6px; left: 31px; width: 2px; height: 12px; }
  .constraints-box .bottom { bottom: 6px; left: 31px; width: 2px; height: 12px; }
  .constraints-box .midh { left: 26px; top: 31px; width: 12px; height: 2px; opacity: 0; }
  .constraints-box .midv { top: 26px; left: 31px; width: 2px; height: 12px; opacity: 0; }
  .constraints-box .midh.on, .constraints-box .midv.on { opacity: 1; }
  .constraints-selects { display: flex; flex-direction: column; gap: 8px; flex: 1; min-width: 0; }
  .disclosed { margin-top: 8px; }
  .appearance-grid.has-corners { grid-template-columns: repeat(2, minmax(0, 1fr)) 24px; align-items: center; }
  /* Figma's column: every row of fields keeps the 24 px action column at the
     right (and its 8 px gap), empty or not, so the right edges of X/Y, W/H,
     opacity and corners line up. A grid with its own action column fills it. */
  .inspector :global(.property-grid:not(.has-corners)) { padding-right: calc(var(--control-h-sm) + 8px); }
  .stroke-options { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 24px; gap: 8px; align-items: end; }
  .stroke-options > :global(.ui-icon-button) { margin-bottom: 2px; }
  .readout-field { display: flex; flex-direction: column; gap: 4px; }
  .readout { padding: 0 8px; color: var(--fg-muted); }
  .effect-row { display: flex; align-items: center; gap: 8px; }
  .effect-row :global(.effect-trigger) { flex: 1; gap: 8px; padding: 0 8px; color: var(--fg); cursor: pointer; }
  .effect-row :global(.effect-trigger span) { flex: 1; text-align: left; }
  .effect-row :global(.effect-trigger svg) { flex: none; color: var(--fg-muted); }
  .trim-heading { display: flex; justify-content: space-between; align-items: center; margin-top: 4px; }
  .type-suggest { display: flex; flex-wrap: wrap; gap: 4px; align-items: center; }
  /* Component authoring: a main's violet, ports' teal (component-authoring.md §0). */
  .main-chip { display: inline-flex; align-items: center; gap: 6px; margin: 0; font: inherit; color: var(--component); }
  .port-field { display: flex; flex-direction: column; gap: 4px; min-width: 0; font-size: var(--text-sm); color: var(--fg-muted); }
  .port-field.wide, .port-grid .wide { grid-column: 1 / -1; }
  .port-field.check { justify-content: flex-end; min-height: var(--control-h); }
  .port-field :global(.ui-input) { width: 100%; }
  .port-field :global(.ui-input.mono) { font-family: var(--font-mono); font-size: var(--text-sm); color: var(--fg); }
  .port-grid { margin: 4px 0 8px; }
  .ports { display: flex; flex-direction: column; gap: 2px; }
  .port-row { display: flex; gap: 8px; align-items: center; min-height: var(--control-h-sm); padding: 0 6px; border: 0; border-radius: var(--radius); background: transparent; color: var(--fg); text-align: left; font-size: var(--text-md); cursor: pointer; }
  .port-row:hover, .port-row.open { background: var(--hover); }
  .port-row .dir { width: 28px; font-size: var(--text-xs); color: var(--port); font-weight: 600; text-transform: uppercase; }
  .port-row .setting { margin-left: auto; font-size: var(--text-xs); color: var(--fg-faint); }
</style>
