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
  }): MaybePromise<string>;
  version(): MaybePromise<number>;
  nextIds(n: number): MaybePromise<string[]>;
  nextRelationIds(n: number): MaybePromise<string[]>;
  /** Order keys under `parent` ('' = roots) after `after` ('' = front, 'end' = back). */
  place(parent: string, after: string, count: number): MaybePromise<string[]>;
  apply(transaction: object, expectVersion: number): MaybePromise<ApplyResult>;
  /** Link two pictures to one entity as `actor` (ECM §7.2). */
  link(a: string, b: string, keep: string, actor: 'human' | 'model'): MaybePromise<LinkResult>;
  /** What the person has selected, in pick order (the session tier). */
  selection(): MaybePromise<{ node: string; type: string | null; name: string | null }[]>;
  /** Import SVG into `parent` with its origin at `x`, `y` there, as one
   *  transaction (plans/specs/svg-import-export.md). `cuts` lists what it
   *  could not carry. */
  importSvg(text: string, parent: string, x: number, y: number, actor: 'human' | 'model'): MaybePromise<ImportSvgResult>;
  /** Make a table, or with `id` replace one's words, from a JSON spec
   *  (plans/specs/layout-and-tables.md §5.2), as one history entry. */
  table(spec: string): MaybePromise<{ ok: boolean; version?: number; table?: string; error?: string }>;
  /** Write one context's markup into the document (plans/specs/contexts.md
   *  §2.2): it replaces a document context of the same name, or is added. */
  context(markup: string): MaybePromise<{ ok: boolean; version?: number; context?: string; error?: string }>;
  /** Place and wire parts from a `<circuit>` (plans/specs/schematic.md §5),
   *  as one history entry. */
  circuit(markup: string): MaybePromise<{ ok: boolean; version?: number; summary?: string; error?: string }>;
  /** The model's component verbs, `<place>` and `<make-component>`
   *  (`compose::compose_ops`), as one history entry. */
  compose(markup: string): MaybePromise<{ ok: boolean; version?: number; summary?: string; error?: string }>;
  /** A schematic sheet's netlist, SPICE-like, with each net's solved
   *  voltage; `sheet` is a composition's id or name. */
  netlist(sheet: string): MaybePromise<{ ok: boolean; netlist?: string; error?: string }>;
  /** Insert markdown's blocks into a narrative after a block (`''`: first,
   *  `'end'`: last), as one history entry (plans/specs/narrative-panel.md §4). */
  narrativeWrite(source: string, after: string, markdown: string): MaybePromise<{ ok: boolean; version?: number; blocks?: string[]; error?: string }>;
  /** Replace one narrative block with markdown's blocks (none: delete it). */
  narrativeRewrite(block: string, markdown: string): MaybePromise<{ ok: boolean; version?: number; blocks?: string[]; error?: string }>;
}

export interface ImportSvgResult {
  ok: boolean;
  version?: number;
  roots?: string[];
  cuts?: string[];
  error?: string;
}

export interface EntitySummary {
  entity: string;
  name: string | null;
  def: string | null;
  args: [string, string][];
  figures: string[];
  ports: string[];
}

export interface LinkResult {
  ok: boolean;
  version?: number;
  entity?: string | null;
  error?: string;
  /** Both pictures already have entities: say which to keep (ECM I5). */
  question?: { a: EntitySummary; b: EntitySummary; port_collisions: string[] };
}

/** A host supplies only transport; operation names and decoding live here. */
export type DocumentCall = (command: DocumentCommand) => MaybePromise<string>;

export function makeDocumentApi(call: DocumentCall): DocumentApi {
  let schema: Promise<Schema> | null = null;
  const json = async <T>(command: DocumentCommand): Promise<T> => JSON.parse(await call(command));
  return {
    schema: () => (schema ??= json<Schema>({ op: 'schema' }).catch(error => {
      schema = null;
      throw error;
    })),
    view: q => call({ op: 'view', query_json: JSON.stringify({ scope: null, depth: null, ...q }) }),
    version: async () => {
      const text = await call({ op: 'view', query_json: JSON.stringify({ scope: null, fidelity: 'skeleton', depth: 0 }) });
      return Number(text.match(/version="(\d+)"/)?.[1] ?? 0);
    },
    nextIds: n => json({ op: 'next_ids', n }),
    nextRelationIds: n => json({ op: 'next_relation_ids', n }),
    place: (parent, after, count) => json({ op: 'place', parent, after, count }),
    apply: (tx, expect) => json({ op: 'apply', transaction_json: JSON.stringify(tx), expect_version: expect }),
    link: (a, b, keep, actor) => json({ op: 'link', a, b, keep, actor }),
    selection: () => json({ op: 'selection' }),
    importSvg: (text, parent, x, y, actor) => json({ op: 'import_svg_at', text, parent, x, y, actor }),
    table: spec => json({ op: 'table', spec }),
    context: markup => json({ op: 'context', markup }),
    circuit: markup => json({ op: 'circuit', markup }),
    compose: markup => json({ op: 'compose', markup }),
    netlist: sheet => json({ op: 'netlist', sheet }),
    narrativeWrite: (source, after, markdown) => json({ op: 'narrative_write', source, after, markdown, actor: 'model' }),
    narrativeRewrite: (block, markdown) => json({ op: 'narrative_rewrite', block, markdown, actor: 'model' }),
  };
}
