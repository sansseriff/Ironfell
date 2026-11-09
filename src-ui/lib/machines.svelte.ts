// The machine panel's data (machines.md §6), shared by the timeline's
// machine panel and the canvas's "Previewing" chip: the engine's
// `machine_view` for the selection, polled while anything uses it. The
// engine answers without asking for frames, so polling costs no rendering.
import { controllerManager } from "../controller-manager.svelte";

export type Tween = { id: string; channel: string | null; start: number | null; end: number | null; ease: string | null; shaped: boolean };
export type Transition = { id: string; name: string | null; from: string | null; to: string | null; ms: number | null; ease: string | null; tweens: Tween[] };
export type Pair = { from: string; to: string; transition: string | null; ms: number | null };
export type Machine = {
  id: string;
  port: string;
  states: { id: string; name: string | null }[];
  transitions: Transition[];
  pairs: Pair[];
  layout: "strip" | "graph";
  pinned: string | null;
  current: { state: string | null; transition: string | null; progress: number | null } | null;
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
};
export type Pin = { def: string; name: string | null; machine: string; state: string; scrub: { from: string; to: string; progress: number } | null };

class Machines {
  view: MachineView | null = $state(null);
  pinned: Pin | null = $state(null);
  #users = 0;
  #timer: ReturnType<typeof setInterval> | null = null;
  #pending = false;

  async read() {
    if (this.#pending) return;
    this.#pending = true;
    try {
      const doc = controllerManager.docClient;
      const r = await doc.command<{ ok: boolean; view: MachineView | null; pinned: Pin | null }>({ op: "machine_view", node: doc.selection[0] ?? "" });
      if (r.ok) {
        if (JSON.stringify(r.view) !== JSON.stringify(this.view)) this.view = r.view;
        if (JSON.stringify(r.pinned) !== JSON.stringify(this.pinned)) this.pinned = r.pinned;
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
