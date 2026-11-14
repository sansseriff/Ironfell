/**
 * The model surface shared by browser and native hosts. Transports return
 * serialized engine results; this factory owns their arguments and decoding.
 */
import type { DocumentCommand } from '../runtime/protocol.generated';
import type { Schema } from './schema';

export interface SolverSummary {
  clusters: string[];
  moved: string[];
  unmet?: { leaf: string; requested: number; solved: number; reason: string }[];
}

export interface ApplyResult {
  ok: boolean;
  version?: number;
  errors?: { op: number | null; kind: string; message: string }[];
  /** Causes, not effects (constraint-system.md §9.2): present when constraints re-solved. */
  solver?: SolverSummary;
}

type MaybePromise<T> = T | Promise<T>;

export interface DocumentApi {
  /** A content-only GPU picture of a 2D composition, and the composition's id
   *  (`#…`) when the engine resolved it. */
  picture?(source: string, signal?: AbortSignal): Promise<import('./images').ModelImage & { node?: string }>;
  schema(): MaybePromise<Schema>;
  view(query: {
    scope?: string | null;
    fidelity: 'skeleton' | 'summary' | 'full';
    depth?: number | null;
    /** 'tree' (default), 'model', or { composition: '#4' } (ECM §11). */
    relation?: 'tree' | 'model' | { composition: string };
    /** Instances (an entity, or a use) whose derived insides to show, marked
     *  `derived` (component-model.md §9). */
    expand?: string[];
    /** Leave out constant leaves at their default (what the model reads). */
    omit_defaults?: boolean;
  }): MaybePromise<string>;
  version(): MaybePromise<number>;
  /** Read one `<edit>` into a plan of one transaction, not applied
   *  (iron_document::edit; plans/specs/markup.md). */
  plan(xml: string): MaybePromise<import('./edit').PlanResult>;
  apply(transaction: object, expectVersion: number): MaybePromise<ApplyResult>;
  /** The transactions after `since`, one JSON object a line, or `gap` when the
   *  log no longer reaches back that far (`scope_log`; merge.ts). */
  logSince?(since: number): MaybePromise<{ gap: boolean; ndjson: string; version: number }>;
  /** What the person has selected, in pick order (the session tier). */
  selection(): MaybePromise<{ node: string; type: string | null; name: string | null }[]>;
  /** A schematic sheet's netlist, SPICE-like, with each net's solved
   *  voltage; `sheet` is a composition's id or name. */
  netlist(sheet: string): MaybePromise<{ ok: boolean; netlist?: string; error?: string }>;
}

/** A host supplies only transport; operation names and decoding live here. */
export type DocumentCall = (command: DocumentCommand) => MaybePromise<string>;

export function makeDocumentApi(call: DocumentCall): DocumentApi {
  let schema: Promise<Schema> | null = null;
  const json = async <T>(command: DocumentCommand): Promise<T> => JSON.parse(await call(command));
  return {
    picture: async (source, signal) => {
      const start = await json<{ ok: boolean; ticket?: number; error?: string }>({ op: 'picture', source });
      if (!start.ok || start.ticket === undefined) throw new Error(start.error ?? 'Could not capture composition');
      const ticket = start.ticket;
      try {
        const deadline = Date.now() + 20000;
        while (Date.now() < deadline) {
          signal?.throwIfAborted();
          const result = await json<{ pending?: boolean; ok?: boolean; image?: import('./images').ModelImage; node?: string; error?: string }>({ op: 'picture_poll', ticket, cancel: false });
          if (!result.pending) {
            if (!result.ok || !result.image) throw new Error(result.error ?? 'Could not capture composition');
            return { ...result.image, ...(result.node ? { node: result.node } : {}) };
          }
          await new Promise(resolve => setTimeout(resolve, 50));
        }
        throw new Error('Composition capture timed out');
      } finally { await json({ op: 'picture_poll', ticket, cancel: true }).catch(() => {}); }
    },
    schema: () => (schema ??= json<Schema>({ op: 'schema' }).catch(error => {
      schema = null;
      throw error;
    })),
    view: q => call({ op: 'view', query_json: JSON.stringify({ scope: null, depth: null, ...q }) }),
    version: async () => {
      const text = await call({ op: 'view', query_json: JSON.stringify({ scope: null, fidelity: 'skeleton', depth: 0 }) });
      return Number(text.match(/version="(\d+)"/)?.[1] ?? 0);
    },
    plan: xml => json({ op: 'edit_plan', xml }),
    apply: (tx, expect) => json({ op: 'apply', transaction_json: JSON.stringify(tx), expect_version: expect }),
    logSince: async since => {
      const r = await json<{ pending?: boolean; gap?: boolean; ndjson?: string; version?: number }>({ op: 'scope_log', since });
      return { gap: !!r.pending || !!r.gap, ndjson: r.ndjson ?? '', version: r.version ?? 0 };
    },
    selection: () => json({ op: 'selection' }),
    netlist: sheet => json({ op: 'netlist', sheet }),
  };
}
