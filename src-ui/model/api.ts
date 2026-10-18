/**
 * What the model surface needs from the document, wherever the wasm lives.
 * Every method is a thin wrapper over one FFI call; the worker and the
 * main-thread adapter both implement it with `makeDocumentApi`.
 */
import type { Glue } from '../runtime/glue';
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

export function makeDocumentApi(glue: Glue, handle: bigint): DocumentApi {
  let schema: Schema | null = null;
  return {
    schema: () => (schema ??= JSON.parse(glue.document_schema())),
    view: (q) => glue.document_view(handle, JSON.stringify({ scope: null, depth: null, ...q })),
    version: () => {
      const m = glue.document_view(handle, JSON.stringify({ scope: null, fidelity: 'skeleton', depth: 0 })).match(/version="(\d+)"/);
      return m ? Number(m[1]) : 0;
    },
    nextIds: (n) => JSON.parse(glue.document_next_ids(handle, n)),
    nextRelationIds: (n) => JSON.parse(glue.document_next_relation_ids(handle, n)),
    place: (parent, after, count) => JSON.parse(glue.document_place(handle, parent, after, count)),
    apply: (tx, expect) => JSON.parse(glue.document_apply(handle, JSON.stringify(tx), expect)),
    link: (a, b, keep, actor) => JSON.parse((glue as any).document_link(handle, a, b, keep, actor)),
    selection: () => JSON.parse((glue as any).document_selection(handle)),
    importSvg: (text, parent, x, y, actor) => JSON.parse((glue as any).document_import_svg_at(handle, text, parent, x, y, actor)),
  };
}
