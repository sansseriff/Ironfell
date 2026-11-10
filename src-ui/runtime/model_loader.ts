/**
 * The web host's Model service (plans/specs/reactivity.md §8, tier 1 of
 * plans/specs/model-interop.md): the engine asks (`EngineEvent` `queries`),
 * and this carries each request out and answers with the `query_result`
 * command. Nothing waits on it: the query shows `pending` until then.
 *
 * A Model is a URL. The request is a POST of the query's input as JSON; the
 * answer is the response's JSON, whatever its shape (an array of numbers
 * becomes a series, an object a record). A newer request for the same query
 * aborts the one in flight. Runs where the engine runs (the worker), so
 * Model data never crosses the UI thread.
 */

export type QueryRequest = { node: string; seq: number; model: string; input: unknown };

/** How long a Model may take before the query fails. */
const TIMEOUT_MS = 60_000;

export class ModelLoader {
  private flights = new Map<string, AbortController>();

  constructor(private answer: (node: string, seq: number, ok: boolean, payload: unknown) => void) {}

  request(requests: QueryRequest[]) {
    for (const r of requests) void this.run(r);
  }

  /** Abort everything in flight (the engine is going away). */
  dispose() {
    for (const c of this.flights.values()) c.abort();
    this.flights.clear();
  }

  private async run(r: QueryRequest) {
    this.flights.get(r.node)?.abort();
    const controller = new AbortController();
    this.flights.set(r.node, controller);
    const timer = setTimeout(() => controller.abort(new Error('timed out')), TIMEOUT_MS);
    try {
      const response = await fetch(r.model, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(r.input),
        signal: controller.signal,
      });
      if (!response.ok) throw new Error(`${response.status} ${response.statusText}`.trim());
      const payload = await response.json();
      this.answer(r.node, r.seq, true, payload);
    } catch (e) {
      // Overtaken by a newer request: the engine has dropped this one already.
      if (controller.signal.aborted && this.flights.get(r.node) !== controller) return;
      this.answer(r.node, r.seq, false, e instanceof Error ? e.message : String(e));
    } finally {
      clearTimeout(timer);
      if (this.flights.get(r.node) === controller) this.flights.delete(r.node);
    }
  }
}
