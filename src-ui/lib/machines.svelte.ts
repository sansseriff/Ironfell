// The machine panel's data (machines.md §6), shared by the timeline's
// machine panel and the canvas's "Previewing" chip: the engine's
// `machine_view` for the selection, polled while anything uses it. The
// engine answers without asking for frames, so polling costs no rendering.
import { controllerManager } from "../controller-manager.svelte";

export type Tween = { id: string; channel: string | null; start: number | null; end: number | null; ease: string | null; shaped: boolean };
export type Transition = { id: string; name: string | null; from: string | null; to: string | null; ms: number | null; ease: string | null; tweens: Tween[]; ms_formula?: boolean };
export type Pair = { from: string; to: string; transition: string | null; ms: number | null };
export type Machine = {
  id: string;
  port: string;
  states: { id: string; name: string | null }[];
  transitions: Transition[];
  pairs: Pair[];
  layout: "strip" | "graph";
  /** States in strip order, and whether the last runs back to the first. */
  order: [string[], boolean];
  pinned: string | null;
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
  channels: { id: string; name: string; rest: number }[];
  /** The component's main (for Edit main from an instance). */
  main_of: string | null;
};
export type Pin = { def: string; name: string | null; machine: string; state: string; scrub: { from: string; to: string; progress: number } | null };

class Machines {
  view: MachineView | null = $state(null);
  pinned: Pin | null = $state(null);
  #users = 0;
  #timer: ReturnType<typeof setInterval> | null = null;
  #pending = false;
  /** The main last seen selected, and its definition. */
  #lastMain: { main: string; def: string } | null = null;
  // The panel's session choices, kept while the panel closes and opens.
  layoutChoice: "auto" | "strip" | "graph" = $state("auto");
  chosen: Record<string, { from: string; to: string }> = $state({});

  async read() {
    if (this.#pending) return;
    this.#pending = true;
    try {
      const doc = controllerManager.docClient;
      const r = await doc.command<{ ok: boolean; view: MachineView | null; pinned: Pin | null }>({ op: "machine_view", node: doc.selection[0] ?? "" });
      if (r.ok) {
        if (JSON.stringify(r.view) !== JSON.stringify(this.view)) this.view = r.view;
        if (JSON.stringify(r.pinned) !== JSON.stringify(this.pinned)) this.pinned = r.pinned;
        // Leaving a main lets go of a state previewed on it, so later edits
        // there are not recorded into a state you can no longer see.
        const main = r.view?.main ? { main: r.view.main, def: r.view.def } : null;
        const left = this.#lastMain && this.#lastMain.main !== main?.main ? this.#lastMain : null;
        this.#lastMain = main;
        if (left && r.pinned?.def === left.def) await this.pin(left.def, r.pinned.machine, null);
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

  async author(verb: string, args: Record<string, unknown>) {
    const r = await controllerManager.docClient.command<{ ok: boolean; error?: string; made?: string }>({ op: "author", verb, args: JSON.stringify(args) });
    await this.read();
    return r;
  }

  async patch(node: string, values: Record<string, string>, label: string) {
    const r = await controllerManager.docClient.command<{ ok: boolean; error?: string }>({ op: "inspect_patch", node, patch: JSON.stringify({ values, remove: [], label, replace: false }) });
    await this.read();
    return r;
  }

  /** Pin `state` on the main (or unpin with `null`), optionally showing a moment of a transition. */
  pin(def: string, machine: string, state: string | null, scrub?: { from: string; to: string; progress: number }) {
    return this.author("pin_state", state === null ? { def } : { def, machine, state, ...(scrub ? { scrub } : {}) });
  }
}

export const machines = new Machines();
