import type { DocumentCommand } from './protocol.generated';
export interface StateChange {
  version: number; sessionVersion: number; resolvedVersion: number;
  nodes: string[]; authored: boolean; structure: boolean; compositions: boolean; reset: boolean;
  canUndo: boolean; canRedo: boolean;
}
type Listener = { value: (value: any) => void; error?: (error: unknown) => void };
type Query = {
  command: DocumentCommand; listeners: Set<Listener>; dependencies: Set<string>;
  serialized?: string; value?: unknown; generation: number; dirty: boolean; inFlight: boolean;
};
/** One cached read per active query. Consumers own subscriptions; invalidation
 * uses touched/resolved nodes, not broad authored-version rereads. */
export class QuerySubscriptions {
  private queries = new Map<string, Query>();
  private scheduled = false;
  private active = false;
  private epoch = 0;
  private interest = '';
  private contentVersion: number | undefined;
  private reads = new Map<string, number>();
  private settleTimer: ReturnType<typeof setTimeout> | undefined;
  constructor(private request: (command: DocumentCommand) => Promise<string>, private observe: (interest: { all: boolean; compositions: boolean; nodes: string[] }) => void = () => {}) {}
  start() { this.active = true; this.interest = ''; this.publishInterests(); this.schedule(); }
  subscribe<T>(command: DocumentCommand, value: (result: T) => void, error?: (error: unknown) => void): () => void {
    const key = JSON.stringify(command);
    let query = this.queries.get(key);
    if (!query) {
      query = { command, listeners: new Set(), dependencies: dependencies(command), generation: 0, dirty: true, inFlight: false };
      this.queries.set(key, query);
    }
    const listener = { value, error };
    query.listeners.add(listener);
    if (query.serialized !== undefined) value(query.value as T);
    this.publishInterests();
    this.schedule();
    const captured = query;
    return () => {
      captured.listeners.delete(listener);
      if (!captured.listeners.size && this.queries.get(key) === captured) {
        captured.generation++; this.queries.delete(key); this.publishInterests();
      }
    };
  }
  invalidate(change: StateChange) {
    // The whole document's markup (the Document view) is ~70 KB that Blink
    // re-lays out in full, ~4 ms a frame on a large window. It follows
    // commits (authored version, undo, redo, load) at once; live values alone
    // (playback, a drag before release, a simulation) refresh it once they
    // have been quiet for SETTLE_MS (plans/rendering/worker-frame-hold.md §6f).
    const committed = change.reset || change.structure || change.version !== this.contentVersion;
    const live = !committed && change.authored;
    this.contentVersion = change.version;
    if (live && [...this.queries.values()].some(q => wholeDocument(q.command))) {
      clearTimeout(this.settleTimer);
      this.settleTimer = setTimeout(() => {
        this.settleTimer = undefined;
        for (const query of this.queries.values()) if (wholeDocument(query.command)) { query.dirty = true; query.generation++; }
        this.schedule();
      }, SETTLE_MS);
    }
    for (const query of this.queries.values()) {
      const op = query.command.op;
      const relevant = change.reset || (wholeDocument(query.command) ? committed : op === 'compositions' ? change.compositions
        : change.structure || (op === 'composition_info' ? change.compositions
        : !query.dependencies.size || change.nodes.some(node => query.dependencies.has(node))));
      if (relevant) { query.dirty = true; query.generation++; }
    }
    this.schedule();
  }
  stats() { return { active: this.queries.size, reads: Object.fromEntries(this.reads) }; }
  dispose() {
    this.active = false; this.epoch++;
    clearTimeout(this.settleTimer); this.settleTimer = undefined;
    this.contentVersion = undefined;
    for (const query of this.queries.values()) query.generation++;
    this.queries.clear();
  }
  private publishInterests() {
    if (!this.active) return;
    let all = false, compositions = false;
    const nodes = new Set<string>();
    for (const query of this.queries.values()) {
      if (query.command.op === 'compositions' || query.command.op === 'composition_info') compositions = true;
      else if (!query.dependencies.size) all = true;
      for (const node of query.dependencies) nodes.add(node);
    }
    const interest = { all, compositions, nodes: [...nodes].sort() };
    const serialized = JSON.stringify(interest);
    if (serialized !== this.interest) { this.interest = serialized; this.observe(interest); }
  }
  private schedule() {
    if (this.scheduled || !this.active) return;
    this.scheduled = true;
    queueMicrotask(() => {
      this.scheduled = false;
      if (!this.active) return;
      for (const [key, query] of this.queries) if (query.dirty && !query.inFlight) void this.refresh(key, query);
    });
  }
  private async refresh(key: string, query: Query) {
    query.inFlight = true; query.dirty = false;
    const generation = query.generation, epoch = this.epoch;
    const current = () => this.active && epoch === this.epoch && this.queries.get(key) === query && query.generation === generation;
    try {
      this.reads.set(key, (this.reads.get(key) ?? 0) + 1);
      if (this.reads.size > 512) this.reads.delete(this.reads.keys().next().value!);
      const serialized = await this.request(query.command);
      if (!current()) return;
      if (query.serialized !== serialized) {
        const value = query.command.op === 'view' ? serialized : JSON.parse(serialized);
        query.serialized = serialized; query.value = value;
        query.dependencies = wholeDocument(query.command) ? new Set() : new Set([...dependencies(query.command), ...references(serialized)]);
        this.publishInterests();
        for (const listener of query.listeners) listener.value(value);
      }
    } catch (error) {
      if (current()) for (const listener of query.listeners) listener.error?.(error);
    } finally {
      query.inFlight = false;
      if (query.dirty && this.queries.get(key) === query) this.schedule();
    }
  }
}
/** How long live-only changes must pause before the whole document's markup refreshes. */
export const SETTLE_MS = 250;

function references(text: string): Set<string> {
  return new Set(text.match(/#[A-Za-z0-9_]+(?:\/[A-Za-z0-9_~-]+)*/g) ?? []);
}
function dependencies(command: DocumentCommand): Set<string> {
  if ('node' in command) return new Set([command.node]);
  if (command.op === 'view') {
    const scope = JSON.parse(command.query_json).scope;
    return typeof scope === 'string' && scope.startsWith('#') ? new Set([scope]) : new Set();
  }
  return new Set();
}

function wholeDocument(command: DocumentCommand): boolean {
  return command.op === 'view' && JSON.parse(command.query_json).scope == null;
}
