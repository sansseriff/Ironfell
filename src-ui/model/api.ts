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

/** What a model's reply asks for, read by the engine (iron_document::reply::read). */
export interface Reply {
  /** The first `<edit>` block, as written. */
  edit: string | null;
  /** The first `<revise>` block, as written. */
  revise: string | null;
  /** A `<look picture="…"/>`'s composition. */
  picture: string | null;
  /** The instances a `<look expand="…"/>` names, as `#id`s. */
  expand: string[];
  /** What `<topic name="…"/>` asks for. */
  topics: string[];
  /** `<continue/>`: more steps follow this edit. */
  continue: boolean;
  /** The reply without its edit (or revise) and picture looks, trimmed. */
  note: string;
  /** The reply without its picture looks only: what it said when its edit is not acted on. */
  bare: string;
}

/** A verb of an edit being written (or a step of a revise). */
export interface StreamVerb { name: string; attrs: [string, string][]; closed: boolean }

/** A reply as far as it has streamed (iron_document::reply::stream): its
 *  prose (half-written markup held back), the edit or revise forming, and
 *  its verbs whose opening tags have ended. */
export interface StreamView { prose: string; edit: string | null; verbs: StreamVerb[] }

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
  /** Every schematic sheet's netlist, SPICE-like, with each net's solved voltage. */
  netlists(): MaybePromise<{ ok: boolean; netlists?: { sheet: string; netlist: string }[]; error?: string }>;
  /** Read a model's reply (the engine's markup reader; plans/specs/markup.md). */
  reply(text: string): MaybePromise<Reply>;
  /** Read a reply as far as it has streamed. */
  stream(text: string): MaybePromise<StreamView>;
  /** Apply a `<revise>` to the edit the store rejected. */
  revise(rejected: string, patch: string): MaybePromise<{ ok: true; edit: string } | { ok: false; error: string }>;
}

/** A host supplies only transport; operation names and decoding live here. */
export type DocumentCall = (command: DocumentCommand) => MaybePromise<string>;

/** Read a renderer snapshot, releasing its ticket on cancellation or timeout. */
export async function capturePicture(call: DocumentCall, source: string, signal?: AbortSignal): Promise<import('./images').ModelImage & { node?: string }> {
  const json = async <T>(command: DocumentCommand): Promise<T> => JSON.parse(await call(command));
  signal?.throwIfAborted();
  const start = await json<{ ok: boolean; ticket?: number; error?: string }>({ op: 'picture', source });
  if (!start.ok || start.ticket === undefined) throw new Error(start.error ?? 'Could not capture composition');
  const ticket = start.ticket;
  try {
    const deadline = Date.now() + 20000;
    while (Date.now() < deadline) {
      signal?.throwIfAborted();
      const result = await json<{ pending?: boolean; ok?: boolean; image?: import('./images').ModelImage; node?: string; error?: string }>({ op: 'picture_poll', ticket, cancel: false });
      signal?.throwIfAborted();
      if (!result.pending) {
        if (!result.ok || !result.image) throw new Error(result.error ?? 'Could not capture composition');
        return { ...result.image, ...(result.node ? { node: result.node } : {}) };
      }
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw new Error('Composition capture timed out');
  } finally { await json({ op: 'picture_poll', ticket, cancel: true }).catch(() => {}); }
}

export function makeDocumentApi(call: DocumentCall): DocumentApi {
  let schema: Promise<Schema> | null = null;
  const json = async <T>(command: DocumentCommand): Promise<T> => JSON.parse(await call(command));
  return {
    picture: (source, signal) => capturePicture(call, source, signal),
    schema: () => (schema ??= json<Schema>({ op: 'schema' }).catch(error => {
      schema = null;
      throw error;
    })),
    view: q => call({ op: 'view', query_json: JSON.stringify({ scope: null, depth: null, ...q }) }),
    version: async () => (await json<{ version: number }>({ op: 'version' })).version,
    plan: async xml => {
      if (!/<typeset-math[\s/>]/.test(xml)) return json({ op: 'edit_plan', xml });
      const sources = await json<[number, string, string][]>({ op: 'edit_math_sources', xml });
      if (!sources.length) return json({ op: 'edit_plan', xml });
      const equations: Record<number, { source: string; font: string } & ({ svg: string } | { error: string })> = {};
      for (const [index, source, font] of sources) {
        try {
          const { mathSvg } = await import('./math');
          equations[index] = { source, font, svg: await mathSvg(source, font) };
        } catch (error) {
          equations[index] = { source, font, error: error instanceof Error ? error.message : String(error) };
        }
      }
      return json({ op: 'edit_plan_math', xml, equations });
    },
    apply: (tx, expect) => json({ op: 'apply', transaction_json: JSON.stringify(tx), expect_version: expect }),
    logSince: async since => {
      const r = await json<{ pending?: boolean; gap?: boolean; ndjson?: string; version?: number }>({ op: 'scope_log', since });
      return { gap: !!r.pending || !!r.gap, ndjson: r.ndjson ?? '', version: r.version ?? 0 };
    },
    selection: () => json({ op: 'selection' }),
    netlists: () => json({ op: 'netlists' }),
    reply: text => json({ op: 'reply_read', text }),
    stream: text => json({ op: 'reply_stream', text }),
    revise: (rejected, patch) => json({ op: 'edit_revise', rejected, patch }),
  };
}
