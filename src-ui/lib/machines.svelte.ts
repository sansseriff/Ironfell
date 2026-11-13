// Components' states (plans/specs/machines.md §6): the state editor's data
// and its mode, shared by the timeline panel (where the editor opens, as a
// drill-in), the inspector (its doorway) and the canvas chip.
//
// The editor is a mode entered on purpose (Edit states…, a double-click on
// a lane or a transition band) and left on purpose (Done, the breadcrumb).
// It is bound to one component; selection never switches it. The engine's
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
export type Focus = { kind: "state"; state: string | null } | { kind: "pair"; from: string; to: string } | { kind: "default" };
/** One channel's slice in a pair edit (machine.rs `Slice`). */
export type SliceEdit = { channel: string; start?: number; end?: number; ease?: string };

type Reply = { ok: boolean; view: MachineView | null; pinned: Pin | null };
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
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
  /** A state whose recording paused when the selection left its main. */
  paused: { def: string; machine: string; state: string } | null = $state(null);
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
      if (this.editing) {
        const s = sel ? await doc.command<Reply>({ op: "machine_view", node: sel }) : null;
        const other = s?.ok && s.view && s.view.def !== this.editing.def ? s.view : null;
        if (!same(other, this.selected)) this.selected = other;
        // Recording follows the main: selecting outside it pauses a
        // recording state (said in the editor; resuming is explicit).
        const main = r.view?.main_of;
        const inMain = !!main && rootOf(sel) === main;
        if (inMain) this.#pinnedWith = sel;
        // Nothing selected is not leaving: the selection empties when the
        // timeline's pieces it held go (a transition closed).
        if (r.pinned && !r.pinned.scrub && r.pinned.def === this.editing.def && sel && !inMain && sel !== this.#pinnedWith) {
          this.paused = { def: r.pinned.def, machine: r.pinned.machine, state: r.pinned.state };
          await this.pin(r.pinned.def, r.pinned.machine, null);
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

  /** Open the editor on a component, showing `focus` of its first machine. */
  async open(def: string, focus?: Focus, machine?: string, origin: string | null = null) {
    this.editing = { def };
    this.origin = origin;
    this.paused = null;
    this.selected = null;
    await this.read();
    const m = machine ?? this.view?.machines[0]?.id;
    if (m && focus) {
      this.focus = { ...this.focus, [m]: focus };
      if (focus.kind === "state" && focus.state) await this.pin(def, m, focus.state);
    }
  }

  /** Leave the editor: preview off, the clock back. */
  async close() {
    const p = this.pinned;
    this.editing = null;
    this.paused = null;
    this.selected = null;
    if (p) await this.pin(p.def, p.machine, null);
    await this.read();
  }

  /** Back to the paused state: select its main and record into it again. */
  async resume() {
    const p = this.paused;
    const main = this.view?.main_of;
    if (!p) return;
    if (main) await controllerManager.docClient.command({ op: "narrative_probe", node: main, action: "select" });
    this.paused = null;
    await this.pin(p.def, p.machine, p.state);
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
