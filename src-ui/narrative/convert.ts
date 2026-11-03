/**
 * Blocks between the store's form (`iron_document::narrative::BlockContent`:
 * text plus marks with UTF-16 offsets) and ProseMirror nodes. A value atom
 * is one U+FFFC in the text, as the store keeps it. Plain TypeScript, so it
 * runs under `bun test`.
 */
import { Fragment, type Mark as PMMark, type Node as PMNode, type Schema } from 'prosemirror-model';
import { kindOf, nodeName } from './schema';

export const OBJECT = '￼';

export interface StoredMark {
  type: string;
  start: number;
  end: number;
  target?: string;
  unit?: string;
  digits?: number;
  href?: string;
  /** From `narrative_read`: the target's name, and whether it is gone. */
  label?: string;
  stale?: boolean;
}

export interface BlockContent {
  kind: string;
  level: number;
  show: string;
  focus: string;
  text: string;
  marks: StoredMark[];
}

export interface StoredBlock extends BlockContent {
  id: string;
}

/** A block's content, compared by what is stored (read-only extras dropped). */
export function sameContent(a: BlockContent, b: BlockContent): boolean {
  return canonical(a) === canonical(b);
}

export function canonical(b: BlockContent): string {
  const marks = [...b.marks]
    .map((m) => ({ type: m.type, start: m.start, end: m.end, target: m.target ?? null, unit: m.unit ?? null, digits: m.digits ?? null, href: m.href ?? null }))
    .sort((x, y) => x.start - y.start || y.end - x.end || x.type.localeCompare(y.type) || String(x.target).localeCompare(String(y.target)));
  return JSON.stringify([b.kind, b.kind === 'heading' ? b.level : 1, b.show, b.focus, b.text, marks]);
}

/** The store's content of one top-level editor node. */
export function blockOf(node: PMNode): BlockContent {
  let text = '';
  const marks: StoredMark[] = [];
  const open = new Map<string, StoredMark>();
  const key = (m: PMMark) => `${m.type.name}|${m.attrs.target ?? ''}|${m.attrs.href ?? ''}`;
  const closeExcept = (keep: Set<string>) => {
    for (const [k, m] of open) {
      if (!keep.has(k)) {
        m.end = text.length;
        if (m.end > m.start) marks.push(m);
        open.delete(k);
      }
    }
  };
  node.forEach((child) => {
    const here = new Set(child.marks.map(key));
    closeExcept(here);
    for (const m of child.marks) {
      const k = key(m);
      if (!open.has(k)) {
        const sm: StoredMark = { type: m.type.name, start: text.length, end: text.length };
        if (m.attrs.target) sm.target = m.attrs.target;
        if (m.type.name === 'link') sm.href = m.attrs.href;
        open.set(k, sm);
      }
    }
    if (child.type.name === 'value') {
      const vm: StoredMark = { type: 'value', start: text.length, end: text.length + 1, target: child.attrs.target };
      if (child.attrs.unit) vm.unit = child.attrs.unit;
      if (child.attrs.digits !== null && child.attrs.digits !== undefined) vm.digits = child.attrs.digits;
      marks.push(vm);
      text += OBJECT;
    } else if (child.isText) {
      text += child.text ?? '';
    }
  });
  closeExcept(new Set());
  marks.sort((a, b) => a.start - b.start || b.end - a.end);
  return {
    kind: kindOf(node.type.name),
    level: node.type.name === 'heading' ? node.attrs.level : 1,
    show: node.attrs.show ?? '',
    focus: node.attrs.focus ?? '',
    text,
    marks,
  };
}

/** An editor node for a stored block. */
export function nodeOf(schema: Schema, b: StoredBlock | (BlockContent & { id?: string })): PMNode {
  const type = schema.nodes[nodeName(b.kind)] ?? schema.nodes.paragraph;
  const attrs: Record<string, unknown> = { id: (b as StoredBlock).id ?? null };
  if (type.name === 'heading') attrs.level = Math.min(6, Math.max(1, b.level || 1));
  if (type.name === 'view') {
    attrs.show = b.show;
    attrs.focus = b.focus;
  }
  return type.create(attrs, inline(schema, b, type.name === 'code_block'));
}

function inline(schema: Schema, b: BlockContent, plain: boolean): Fragment {
  const text = b.text;
  if (plain) return text ? Fragment.from(schema.text(text.replaceAll(OBJECT, ''))) : Fragment.empty;
  const values = new Map(b.marks.filter((m) => m.type === 'value').map((m) => [m.start, m]));
  const spans = b.marks.filter((m) => m.type !== 'value' && schema.marks[m.type]);
  // Cut the text at every mark boundary and every value atom.
  const cuts = new Set<number>([0, text.length]);
  for (const m of spans) {
    cuts.add(Math.min(m.start, text.length));
    cuts.add(Math.min(m.end, text.length));
  }
  for (const s of values.keys()) {
    cuts.add(s);
    cuts.add(s + 1);
  }
  const points = [...cuts].sort((a, b) => a - b);
  const out: PMNode[] = [];
  for (let i = 0; i + 1 < points.length; i++) {
    const [a, z] = [points[i], points[i + 1]];
    if (a >= z) continue;
    const v = values.get(a);
    if (v && z === a + 1 && text[a] === OBJECT) {
      out.push(schema.nodes.value.create({ target: v.target ?? '', unit: v.unit ?? null, digits: v.digits ?? null }));
      continue;
    }
    const piece = text.slice(a, z).replaceAll(OBJECT, '');
    if (!piece) continue;
    const ms = spans
      .filter((m) => m.start <= a && m.end >= z)
      .map((m) => schema.marks[m.type].create(m.type === 'link' ? { href: m.href ?? '' } : m.target ? { target: m.target } : {}));
    out.push(schema.text(piece, ms));
  }
  return Fragment.from(out);
}

export interface Edit {
  op: 'insert' | 'set' | 'delete' | 'move';
  key?: string;
  id?: string;
  after?: string | null;
  block?: BlockContent;
}

/**
 * The block edits that turn `stored` (in store order) into `editor` (each
 * top-level node's id, or null for a new one, with its content). A repeated
 * id (ProseMirror copies attributes when a block splits) is new after its
 * first use, as is a temporary id (`~…`, which is then its key). Returns the
 * edits and, per new block's index, its key.
 */
export function diff(stored: StoredBlock[], editor: { id: string | null; content: BlockContent }[]): { edits: Edit[]; keys: Map<number, string> } {
  const known = new Map(stored.map((b, i) => [b.id, { block: b, index: i }]));
  const seen = new Set<string>();
  const ids: (string | null)[] = editor.map((e) => {
    if (e.id && !isTemp(e.id) && known.has(e.id) && !seen.has(e.id)) {
      seen.add(e.id);
      return e.id;
    }
    return null;
  });
  const edits: Edit[] = [];
  const keys = new Map<number, string>();
  // Kept blocks whose order is the longest increasing run of store indices
  // stay; the others move.
  const kept = ids.map((id, i) => ({ id, i })).filter((x): x is { id: string; i: number } => x.id !== null);
  const stay = lis(kept.map((k) => known.get(k.id)!.index));
  const staying = new Set(stay.map((j) => kept[j].id));
  for (const b of stored) if (!seen.has(b.id)) edits.push({ op: 'delete', id: b.id });
  let prev: string | null = null;
  editor.forEach((e, i) => {
    const id = ids[i];
    if (id === null) {
      const key = e.id && isTemp(e.id) ? e.id : `n${i}`;
      keys.set(i, key);
      edits.push({ op: 'insert', key, after: prev, block: e.content });
      prev = key;
      return;
    }
    if (!staying.has(id)) edits.push({ op: 'move', id, after: prev });
    if (!sameContent(known.get(id)!.block, e.content)) edits.push({ op: 'set', id, block: e.content });
    prev = id;
  });
  return { edits, keys };
}

/** A block the editor made and the store has not given an id yet. */
export const isTemp = (id: string) => id.startsWith('~');

/** The expression a value atom shows (`iron_document::narrative::value_expr`). */
export function valueExpr(a: { target: string; unit?: string | null; digits?: number | null }): string {
  const t = a.target;
  const d = a.digits ?? null;
  if (a.unit && d !== null) return `fmt(${t}, "${a.unit}", ${d})`;
  if (a.unit) return `fmt(${t}, "${a.unit}")`;
  if (d !== null) return `fmt(${t}, ${d})`;
  return `fmt(${t})`;
}

/** Indices (into `xs`) of a longest strictly increasing subsequence. */
function lis(xs: number[]): number[] {
  const tails: number[] = [];
  const prevOf: number[] = new Array(xs.length).fill(-1);
  for (let i = 0; i < xs.length; i++) {
    let lo = 0, hi = tails.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (xs[tails[mid]] < xs[i]) lo = mid + 1;
      else hi = mid;
    }
    if (lo > 0) prevOf[i] = tails[lo - 1];
    tails[lo] = i;
  }
  const out: number[] = [];
  for (let k = tails.length ? tails[tails.length - 1] : -1; k >= 0; k = prevOf[k]) out.push(k);
  return out.reverse();
}
