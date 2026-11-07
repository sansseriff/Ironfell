/**
 * Merging an agent's edit with what changed while it worked.
 *
 * An agent reads the document at version N and may take minutes to answer;
 * the person keeps working meanwhile. Rather than refuse the edit because
 * the version moved, the edit is applied on top of the current document
 * unless it touches what changed: the same leaf set by both, a node the
 * person deleted, moved or reordered, a facet both added or removed. The
 * store still checks everything else (a parent that is gone, a bad id), so
 * those come back as ordinary rejections the agent can revise.
 *
 * What changed is read from the store's log (`scope_log`): each transaction
 * after N, its ops and the solver's derived ops. A conflict is an error at
 * the agent's op with what the person set, so `<edit-failed>` reads it in
 * the agent's terms and a `<revise>` can keep or drop that op; it is then
 * judged against the newer version, which the agent has now been told of.
 */
import type { DocumentApi } from './api';

/** What changed in the document after a version. */
export interface Changes {
  /** Transactions since; 0 when nothing changed. */
  count: number;
  /** The log no longer reaches back that far (compaction): what changed is unknown. */
  gap: boolean;
  /** Node -> leaf path -> its newest slot, for leaves that were set. */
  leaves: Map<string, Map<string, unknown>>;
  /** Node -> facet kinds added or removed. */
  facets: Map<string, Set<string>>;
  /** Nodes moved to another parent or reordered. */
  moved: Set<string>;
  deleted: Set<string>;
  /** Each transaction's label, e.g. "move 1 object". */
  labels: string[];
}

type LoggedOp = { t: string; id?: string; path?: string; slot?: unknown; facet?: Record<string, unknown>; kind?: string };

export async function changesSince(api: DocumentApi, version: number): Promise<Changes> {
  const changes: Changes = { count: 0, gap: false, leaves: new Map(), facets: new Map(), moved: new Set(), deleted: new Set(), labels: [] };
  if (!api.logSince) return { ...changes, gap: true };
  const log = await api.logSince(version);
  if (log.gap) return { ...changes, gap: true };
  for (const line of log.ndjson.split('\n')) {
    if (!line.trim()) continue;
    const tx = JSON.parse(line) as { label?: string; ops?: LoggedOp[]; derived_ops?: LoggedOp[] };
    changes.count += 1;
    if (tx.label) changes.labels.push(tx.label);
    for (const op of [...(tx.ops ?? []), ...(tx.derived_ops ?? [])]) {
      if (!op.id) continue;
      switch (op.t) {
        case 'set': {
          const leaves = changes.leaves.get(op.id) ?? new Map<string, unknown>();
          leaves.set(String(op.path), op.slot);
          changes.leaves.set(op.id, leaves);
          break;
        }
        case 'add_facet':
        case 'remove_facet': {
          const kind = op.t === 'add_facet' ? Object.keys(op.facet ?? {})[0] : op.kind;
          if (kind) changes.facets.set(op.id, (changes.facets.get(op.id) ?? new Set()).add(kind));
          break;
        }
        case 'reparent':
        case 'reorder':
          changes.moved.add(op.id);
          break;
        case 'delete':
          changes.deleted.add(op.id);
          break;
        case 'create':
          changes.deleted.delete(op.id); // a restore (undo of a delete)
          break;
      }
    }
  }
  return changes;
}

/** The agent's ops that touch what changed, as store-style errors at their op index. */
export function conflicts(ops: object[], changes: Changes): { op: number; kind: string; message: string }[] {
  const out: { op: number; kind: string; message: string }[] = [];
  const touched = (id: string) => changes.leaves.has(id) || changes.facets.has(id) || changes.moved.has(id);
  ops.forEach((raw, index) => {
    const op = raw as LoggedOp & { parent?: string | null };
    const say = (message: string) => out.push({ op: index, kind: 'conflict', message });
    const id = op.id ?? '';
    if (op.t !== 'create' && changes.deleted.has(id)) return say(`the person deleted ${id} while you worked`);
    switch (op.t) {
      case 'create':
        if (op.parent && changes.deleted.has(op.parent)) say(`the person deleted ${op.parent}, its parent, while you worked`);
        break;
      case 'set': {
        const path = String(op.path);
        const theirs = changes.leaves.get(id);
        if (theirs?.has(path)) say(`the person set ${id} ${path} to ${show(theirs.get(path))} while you worked`);
        else if (changes.facets.get(id)?.has(path.split('.')[0])) say(`the person added or removed ${id}'s ${path.split('.')[0]} while you worked`);
        break;
      }
      case 'add_facet':
      case 'remove_facet': {
        const kind = op.t === 'add_facet' ? Object.keys(op.facet ?? {})[0] : op.kind ?? '';
        const leaves = [...(changes.leaves.get(id)?.keys() ?? [])].filter((p) => p.startsWith(`${kind}.`));
        if (changes.facets.get(id)?.has(kind) || leaves.length) say(`the person changed ${id}'s ${kind} while you worked${leaves.length ? ` (${leaves.join(', ')})` : ''}`);
        break;
      }
      case 'reparent':
      case 'reorder':
        if (changes.moved.has(id)) say(`the person moved ${id} in the tree while you worked`);
        break;
      case 'delete':
        if (touched(id)) say(`the person changed ${id} while you worked (${describe(changes, id)}); deleting it would discard that`);
        break;
    }
  });
  return out;
}

function describe(changes: Changes, id: string): string {
  const parts = [...(changes.leaves.get(id)?.keys() ?? []), ...[...(changes.facets.get(id) ?? [])].map((k) => `${k} facet`), ...(changes.moved.has(id) ? ['moved'] : [])];
  return parts.slice(0, 4).join(', ') + (parts.length > 4 ? ', …' : '');
}

/** A slot as the view writes it, roughly: a number, a quantity, a colour, a binding. */
function show(slot: unknown): string {
  if (slot && typeof slot === 'object') {
    const s = slot as Record<string, unknown>;
    if ('bind' in s) return `{= ${s.bind}}`;
    if ('q' in s) return String(s.q);
    if ('ref' in s) return String(s.ref);
    if ('enum' in s) return String(s.enum);
  }
  if (typeof slot === 'number') return String(Math.round(slot * 1000) / 1000);
  const text = JSON.stringify(slot);
  return text.length > 60 ? `${text.slice(0, 57)}…` : text;
}

/** For edits the store builds itself (compose, circuit, table, …), which
 *  name nodes but are not ops here: refuse only if one they name is gone. */
export function deletedNamed(markup: string, changes: Changes): string[] {
  return [...new Set(markup.match(/#[\w/]+/g) ?? [])].filter((id) => changes.deleted.has(id));
}
