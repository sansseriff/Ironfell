// Components' states (plans/specs/machines.md §6): the state editor's data
// and its mode, shared by the timeline panel (where the editor opens, as a
// drill-in), the inspector (its doorway) and the canvas chip.
//
// The editor is a mode entered on purpose (Add state, Edit states…, a chip
// of the canvas's states bar, a double-click on a lane or a transition band)
// and left by its Close, the breadcrumb, or selecting something else on the
// canvas. While it is open a state is always the one edits go into (or a
// transition is previewed); the canvas's states bar says which. The engine's
// `machine_view` answers without asking for frames, so polling it costs no
// rendering.
import { controllerManager } from "../controller-manager.svelte";

export type Tween = { id: string; channel: string | null; start: number | null; end: number | null; ease: string | null; shaped: boolean };
export type Transition = { id: string; name: string | null; from: string | null; to: string | null; ms: number | null; ease: string | null; tweens: Tween[]; ms_formula?: boolean };
export type Pair = { from: string; to: string; transition: string | null; ms: number | null };
/** A channel's value: a number, a colour (`#rrggbbaa`) or words. */
export type ChannelValue = number | string | null;
export type State = { id: string; name: string | null; pose: Record<string, ChannelValue>; own: string[] };
/** A channel and the parts that read it (the editor's rows are parts). */
export type Channel = { id: string; name: string; rest: ChannelValue; kind: "number" | "color" | "text"; parts: { node: string; shown: string | null; name: string; path: string }[] };
export type Machine = {
  id: string;
  port: string;
  states: State[];
  transitions: Transition[];
  pairs: Pair[];
  layout: "strip" | "graph";
  /** States in strip order, and whether the last runs back to the first. */
  order: [string[], boolean];
  pinned: string | null;
  default: string | null;
  current: { state: string | null; transition: string | null; progress: number | null; from: string | null; to: string | null } | null;
};
export type MachineView = {
  def: string;
  name: string | null;
  /** The main being edited, when the selection is in one. */
  main: string | null;
  /** The instance selected, when it is one. */
  entity: string | null;
  machines: Machine[];
  channels: Channel[];
  /** The component's main. */
  main_of: string | null;
};
export type Pin = { def: string; name: string | null; machine: string; state: string; scrub: { from: string; to: string; progress: number } | null };
/** What the editor shows: a state's look, or a transition's timing. */
export type Focus = { kind: "state"; state: string | null } | { kind: "pair"; from: string; to: string };
/** One channel's slice in a pair edit (machine.rs `Slice`). */
export type SliceEdit = { channel: string; start?: number; end?: number; ease?: string };

type Reply = { ok: boolean; view: MachineView | null; pinned: Pin | null; shared?: number };
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
/** A state's hue, by its place among the machine's states: the colour of
 *  its stretch on the document's timeline (timeline_view.rs `LANE`), and
 *  of its swatch wherever the state is named. Change them together. */
export const STATE_HUES = ["#5c85cc", "#4da88a", "#b880d1", "#d6994d", "#cc6b85", "#809466"];
export const hueOf = (m: Pick<Machine, "states"> | null | undefined, state: string | null | undefined) => {
  const i = m?.states.findIndex((s) => s.name === state) ?? -1;
  return STATE_HUES[(i < 0 ? 0 : i) % STATE_HUES.length];
};
/** The main a node is in (a main's part is `#main/local`). */
export const rootOf = (id: string | undefined) => (id ?? "").split("/")[0];

class Machines {
  /** The component being edited, or null: the timeline shows the clock. */
  editing: { def: string } | null = $state(null);
  /** The edited component's machines (or, not editing, the selection's). */
  view: MachineView | null = $state(null);
  /** The component the selection is in, while editing another. */
  selected: MachineView | null = $state(null);
  pinned: Pin | null = $state(null);
  /** Per machine: what the editor shows. */
  focus: Record<string, Focus> = $state({});
  /** The panel's page for a state: the machine's graph, or the table of
   *  what the states set. */
  page: "graph" | "table" = $state("graph");
  /** When an edit last changed every state instead of the edited one
   *  (a part added, a path's points): the states bar says so briefly. */
  sharedAt = $state(0);
  #shared: number | null = null;
  /** Where the editor was opened from (an instance's lane), for its way back. */
  origin: string | null = $state(null);
  /** The graph pane: shown automatically for a branching machine, or chosen. */
  graph: "auto" | "on" | "off" = $state("auto");
  #users = 0;
  #timer: ReturnType<typeof setInterval> | null = null;
  #pending = false;
  /** The selection when the state was pinned: only a change away pauses. */
  #pinnedWith: string | null = null;

  async read() {
    if (this.#pending) return;
    this.#pending = true;
    try {
      const doc = controllerManager.docClient;
      const sel = doc.selection[0] ?? "";
      const r = await doc.command<Reply>({ op: "machine_view", node: this.editing?.def ?? sel });
      if (!r.ok) return;
      if (!same(r.view, this.view)) this.view = r.view;
      if (!same(r.pinned, this.pinned)) this.pinned = r.pinned;
      if (r.shared !== undefined) {
        if (this.#shared !== null && r.shared > this.#shared && this.editing) this.sharedAt = Date.now();
        this.#shared = r.shared;
      }
      if (this.editing) {
        const s = sel ? await doc.command<Reply>({ op: "machine_view", node: sel }) : null;
        const other = s?.ok && s.view && s.view.def !== this.editing.def ? s.view : null;
        if (!same(other, this.selected)) this.selected = other;
        // The editor is a place: selecting something else on the canvas
        // leaves it.
        const main = r.view?.main_of;
        const inMain = !!main && rootOf(sel) === main;
        if (inMain) this.#pinnedWith = sel;
        // Nothing selected is not leaving: the selection empties when the
        // timeline's pieces it held go (a transition closed).
        if (r.pinned && !r.pinned.scrub && r.pinned.def === this.editing.def && sel && !inMain && sel !== this.#pinnedWith) {
          this.#pending = false;
          await this.close();
          return;
        }
      }
    } catch {
      // No engine session yet.
    } finally {
      this.#pending = false;
    }
  }

  /** Start polling while a component uses this; the returned function stops. */
  use(): () => void {
    if (this.#users++ === 0) {
      void this.read();
      this.#timer = setInterval(() => void this.read(), 200);
    }
    return () => {
      if (--this.#users === 0 && this.#timer) {
        clearInterval(this.#timer);
        this.#timer = null;
      }
    };
  }

  /** Open the editor on a component, showing `focus` of its first machine
   *  (by default its initial state). A component with no states gets its
   *  first two: its look now, and a new one to shape. */
  async open(def: string, focus?: Focus, machine?: string, origin: string | null = null) {
    this.editing = { def };
    this.origin = origin;
    this.selected = null;
    this.page = "graph";
    await this.read();
    if (this.view && !this.view.machines.length) {
      const r = await this.author("add_state", { def, first: "state_1" });
      if (!r.ok) {
        this.editing = null;
        return r;
      }
      focus = r.state ? { kind: "state", state: r.state } : undefined;
    }
    const mm = this.view?.machines.find((x) => x.id === machine) ?? this.view?.machines[0];
    if (!mm) return { ok: true };
    const f: Focus = focus ?? { kind: "state", state: mm.default ?? mm.states[0]?.name ?? null };
    this.focus = { ...this.focus, [mm.id]: f };
    if (f.kind === "state" && f.state) await this.pin(def, mm.id, f.state);
    else if (f.kind === "pair") await this.pin(def, mm.id, f.from, { from: f.from, to: f.to, progress: 0 });
    // Inside the component a click selects a part; the component itself
    // selected would be dragged whole instead.
    const doc = controllerManager.docClient;
    if (this.view?.main_of && doc.selection.length === 1 && doc.selection[0] === this.view.main_of) await doc.call("select", ["[]", "replace"]).catch(() => {});
    return { ok: true };
  }

  /** The machine the editor shows. */
  get machine(): Machine | null {
    return this.view?.machines[0] ?? null;
  }

  /** Edit a state's look: the main shows it, and edits go into it. */
  async editState(state: string) {
    const m = this.machine;
    if (!this.view || !m) return { ok: false };
    if (!this.editing) return this.open(this.view.def, { kind: "state", state });
    this.focus = { ...this.focus, [m.id]: { kind: "state", state } };
    return this.pin(this.view.def, m.id, state);
  }

  /** Add a state (named by the engine) and edit its look. */
  async addState() {
    if (!this.view) return { ok: false };
    const def = this.view.def;
    if (!this.editing) this.editing = { def };
    const r = await this.author("add_state", { def });
    const m = this.machine;
    if (r.ok && m && r.state) this.focus = { ...this.focus, [m.id]: { kind: "state", state: r.state } };
    return r;
  }

  /** Leave the editor: preview off, the clock back. */
  async close() {
    const p = this.pinned;
    this.editing = null;
    this.selected = null;
    if (p) await this.pin(p.def, p.machine, null);
    await this.read();
  }

  async author(verb: string, args: Record<string, unknown>) {
    const r = await controllerManager.docClient.command<{ ok: boolean; error?: string; made?: string; state?: string }>({ op: "author", verb, args: JSON.stringify(args) });
    await this.read();
    return r;
  }

  /** Edit one pair's timing as one step; a pair on the default timing
   *  gets its own first (machine.rs `edit_pair_ops`). */
  editPair(def: string, machine: string, from: string, to: string, edit: { ms?: number; ease?: string; name?: string; slices?: SliceEdit[]; follow?: string[] }, label: string) {
    return this.author("edit_pair", { def, machine, from, to, ...edit, label });
  }

  /** Select a part of the main on the canvas (the editor's rows). */
  selectPart(shown: string | null, add = false) {
    if (!shown) return;
    const doc = controllerManager.docClient;
    const nodes = add ? [...new Set([...doc.selection, shown])] : [shown];
    void doc.call("select", [JSON.stringify(nodes.map((node) => ({ node }))), "replace"]).catch(() => {});
  }

  async patch(node: string, values: Record<string, string>, label: string) {
    const r = await controllerManager.docClient.command<{ ok: boolean; error?: string }>({ op: "inspect_patch", node, patch: JSON.stringify({ values, remove: [], label, replace: false }) });
    await this.read();
    return r;
  }

  /** Pin `state` on the main (or unpin with `null`), optionally showing a moment of a transition. */
  pin(def: string, machine: string, state: string | null, scrub?: { from: string; to: string; progress: number }) {
    this.#pinnedWith = controllerManager.docClient.selection[0] ?? "";
    return this.author("pin_state", state === null ? { def } : { def, machine, state, ...(scrub ? { scrub } : {}) });
  }

  /** What a double-click on the timeline opened: a state key's band (its
   *  transition) or stretch (its state). */
  async openKey(key: string, part: "band" | "lane") {
    const r = await controllerManager.docClient.command<{ ok: boolean; def: string; machine: string; state: string | null; from: string | null; entity_name?: string | null }>({ op: "state_key", key });
    if (!r.ok || !r.state) return;
    const origin = r.entity_name ?? null;
    if (part === "band" && r.from) await this.open(r.def, { kind: "pair", from: r.from, to: r.state }, r.machine, origin);
    else await this.open(r.def, { kind: "state", state: r.state }, r.machine, origin);
  }
}

export const machines = new Machines();
