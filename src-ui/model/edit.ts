/**
 * The write surface: `<edit>` markup in, one transaction out (doc 04 §9.2).
 *
 * Correspondence is declared, never inferred: every verb names its node by
 * id. `$aliases` name nodes created in the same edit. Attributes on
 * `<create>` and `<set>` are leaves, resolved against the registry schema
 * and coerced by kind; a `{= expr}` value is a binding.
 *
 *   <edit expect-version="7">
 *     <create id="$b" type="rect" parent="#1" after="#M" x="10" y="20" w="30" h="30" color="#ff0000"/>
 *     <set id="#P" x="12" size.h="{= #O.slider.value * 200}"/>
 *     <add id="#N" stroke.width="2"/>
 *     <delete id="#3"/>
 *     <reparent id="#4" to="#1" first="true"/>
 *     <link from="#S" to="#Q" rel="animates" offset="0.5"/>
 *     <unlink id="r1"/>
 *   </edit>
 */
import { XMLParser } from 'fast-xml-parser';
import type { ApplyResult, DocumentApi } from './api';
import { coerce, resolveLeaf, type SlotJson } from './schema';

export interface ParsedEdit {
  expectVersion: number;
  ops: object[];
  /** alias -> minted id */
  aliases: Record<string, string>;
}

export class EditError extends Error {
  constructor(
    message: string,
    readonly line?: number,
  ) {
    super(message);
  }
}

const VERBS = ['create', 'set', 'add', 'delete', 'reparent', 'link', 'unlink'] as const;

/** Pull the `<edit>…</edit>` block out of a model reply that may say more. */
export function extractEdit(text: string): string | null {
  const m = text.match(/<edit[\s>][\s\S]*?<\/edit>|<edit\s*\/>/);
  return m ? m[0] : null;
}

export function parseEdit(api: DocumentApi, xml: string): ParsedEdit {
  const parser = new XMLParser({
    ignoreAttributes: false,
    attributeNamePrefix: '',
    allowBooleanAttributes: true,
    preserveOrder: true,
    trimValues: true,
  });
  let tree: any[];
  try {
    tree = parser.parse(xml);
  } catch (e) {
    throw new EditError(`not well-formed: ${e instanceof Error ? e.message : e}`);
  }
  const root = tree.find((n) => n.edit !== undefined);
  if (!root) throw new EditError('no <edit> element');
  const rootAttrs = root[':@'] ?? {};
  const expectVersion = Number(rootAttrs['expect-version'] ?? 0) || 0;
  const schema = api.schema();

  // Pass 1: verbs in order, with their attributes.
  type Verb = { verb: (typeof VERBS)[number]; attrs: Record<string, string>; index: number };
  const verbs: Verb[] = [];
  for (const child of root.edit as any[]) {
    const verb = Object.keys(child).find((k) => k !== ':@');
    if (!verb) continue;
    if (!(VERBS as readonly string[]).includes(verb)) throw new EditError(`unknown verb <${verb}>; use ${VERBS.join(', ')}`);
    verbs.push({ verb: verb as Verb['verb'], attrs: { ...(child[':@'] ?? {}) }, index: verbs.length });
  }

  // Pass 2: mint ids for aliases, in order of creation.
  const aliasNames = verbs.filter((v) => v.verb === 'create').map((v) => v.attrs.id).filter((id) => id?.startsWith('$'));
  const minted = api.nextIds(aliasNames.length);
  const aliases: Record<string, string> = {};
  aliasNames.forEach((a, i) => (aliases[a] = minted[i]));
  const resolveId = (raw: string | undefined, what: string, index: number): string => {
    if (!raw) throw new EditError(`op ${index}: missing ${what}`, index);
    if (raw.startsWith('$')) {
      const id = aliases[raw];
      if (!id) throw new EditError(`op ${index}: alias ${raw} is not created in this edit`, index);
      return id;
    }
    return raw.startsWith('#') || raw.startsWith('r') ? raw : `#${raw}`;
  };

  // Pass 3: placements, batched per (parent, after) so keys are distinct.
  const placementKeys = new Map<string, string[]>();
  const wantPlacement: { key: string; index: number }[] = [];
  const placementOf = (v: Verb): { parent: string; after: string } => {
    const parentRaw = v.verb === 'reparent' ? v.attrs.to : v.attrs.parent;
    const parent = parentRaw ? resolveId(parentRaw, 'parent', v.index) : '';
    const after = v.attrs.first !== undefined ? '' : v.attrs.after ? resolveId(v.attrs.after, 'after', v.index) : 'end';
    return { parent, after };
  };
  for (const v of verbs) {
    if (v.verb === 'create' || v.verb === 'reparent') {
      const p = placementOf(v);
      const key = `${p.parent}|${p.after}`;
      wantPlacement.push({ key, index: v.index });
    }
  }
  for (const key of new Set(wantPlacement.map((w) => w.key))) {
    const count = wantPlacement.filter((w) => w.key === key).length;
    const [parent, after] = key.split('|');
    // A parent created in this same edit has no children yet; keys start fresh.
    const keys = minted.includes(parent) ? freshKeys(count) : api.place(parent, after, count);
    placementKeys.set(key, keys);
  }
  const takeKey = (v: Verb) => {
    const p = placementOf(v);
    const keys = placementKeys.get(`${p.parent}|${p.after}`)!;
    return { parent: p.parent || null, order: keys.shift()! };
  };

  // Pass 4: ops.
  const relIds = api.nextRelationIds(verbs.filter((v) => v.verb === 'link').length);
  let relIndex = 0;
  const ops: object[] = [];
  const skip = new Set(['id', 'type', 'parent', 'after', 'first', 'to', 'from', 'rel', 'path', 'value', 'bind']);
  for (const v of verbs) {
    const a = v.attrs;
    const at = (msg: string) => new EditError(`op ${v.index} <${v.verb}>: ${msg}`, v.index);
    switch (v.verb) {
      case 'create': {
        const type = a.type;
        if (!type) throw at('missing type');
        if (!schema.types[type]) throw at(`unknown type "${type}"; types: ${Object.keys(schema.types).join(', ')}`);
        const id = a.id?.startsWith('$') ? aliases[a.id] : resolveId(a.id ?? minted[0], 'id', v.index);
        const { parent, order } = takeKey(v);
        const comps: Record<string, Record<string, SlotJson>> = {};
        for (const [k, raw] of Object.entries(a)) {
          if (skip.has(k)) continue;
          let leaf;
          try {
            leaf = resolveLeaf(schema, type, k);
            comps[leaf.component] ??= {};
            comps[leaf.component][leaf.field] = coerce(leaf.kind, raw);
          } catch (e) {
            throw at(e instanceof Error ? e.message : String(e));
          }
        }
        for (const c of schema.types[type].components) if (c.required) comps[c.kind] ??= {};
        ops.push({ t: 'create', id, ty: type, parent, order, components: Object.entries(comps).map(([k, f]) => ({ [k]: f })) });
        break;
      }
      case 'set':
      case 'add': {
        const id = resolveId(a.id, 'id', v.index);
        const type = a.type; // optional hint for bare attribute names
        const pairs: [string, string][] = [];
        if (a.path) pairs.push([a.path, a.bind !== undefined ? `{= ${a.bind}}` : (a.value ?? '')]);
        for (const [k, raw] of Object.entries(a)) if (!skip.has(k)) pairs.push([k, raw]);
        if (pairs.length === 0) throw at('nothing to set; give leaf attributes such as x="10" or path="size.h" value="10"');
        const byComp: Record<string, Record<string, SlotJson>> = {};
        for (const [k, raw] of pairs) {
          let leaf;
          try {
            leaf = k.includes('.') || !type ? resolveLeafAnyType(schema, k) : resolveLeaf(schema, type, k);
            const slot = coerce(leaf.kind, raw);
            if (v.verb === 'set') ops.push({ t: 'set', id, path: `${leaf.component}.${leaf.field}`, slot });
            else (byComp[leaf.component] ??= {})[leaf.field] = slot;
          } catch (e) {
            throw at(e instanceof Error ? e.message : String(e));
          }
        }
        for (const [comp, fields] of Object.entries(byComp)) ops.push({ t: 'add_comp', id, comp: { [comp]: fields } });
        break;
      }
      case 'delete':
        ops.push({ t: 'delete', id: resolveId(a.id, 'id', v.index) });
        break;
      case 'reparent': {
        const id = resolveId(a.id, 'id', v.index);
        const { parent, order } = takeKey(v);
        ops.push({ t: 'reparent', id, parent, order });
        break;
      }
      case 'link': {
        const data: Record<string, SlotJson> = {};
        for (const [k, raw] of Object.entries(a)) if (!skip.has(k)) data[k] = coerce('any', raw);
        ops.push({
          t: 'link',
          rel: {
            id: relIds[relIndex++],
            from: resolveId(a.from, 'from', v.index),
            to: resolveId(a.to, 'to', v.index),
            rel: a.rel ?? 'references',
            ...(Object.keys(data).length ? { data } : {}),
          },
        });
        break;
      }
      case 'unlink':
        ops.push({ t: 'unlink', id: resolveId(a.id, 'id', v.index) });
        break;
    }
  }
  return { expectVersion, ops, aliases };
}

/** A leaf written as `component.field`, or a bare field unique across all components. */
function resolveLeafAnyType(schema: ReturnType<DocumentApi['schema']>, attr: string) {
  if (attr === 'name') return { component: 'name', field: 'text', kind: 'string' as const };
  if (attr.includes('.')) {
    const [component, field] = attr.split('.', 2);
    const kind = schema.components[component]?.[field] ?? schema.components[component]?.['*'];
    if (!kind) throw new Error(`unknown leaf "${attr}"`);
    return { component, field, kind };
  }
  // Bare names never match a run-time field (`*`): parameters are written params.<name>.
  const hits = Object.entries(schema.components).filter(([, fields]) => fields[attr] !== undefined && attr !== '*');
  if (hits.length === 1) return { component: hits[0][0], field: attr, kind: hits[0][1][attr] };
  if (hits.length === 0) throw new Error(`unknown leaf "${attr}"`);
  throw new Error(`"${attr}" is ambiguous; write one of ${hits.map(([c]) => `${c}.${attr}`).join(', ')}`);
}

/** Order keys for children of a node that does not exist yet: a0, a1, a2 … */
function freshKeys(count: number): string[] {
  const digits = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';
  return Array.from({ length: count }, (_, i) => (i < 62 ? `a${digits[i]}` : `b${digits[Math.floor(i / 62)]}${digits[i % 62]}`));
}

export interface EditOutcome {
  ok: boolean;
  version?: number;
  aliases?: Record<string, string>;
  /** Markup the model can act on (doc 04 §9.5). Present when `ok` is false. */
  failure?: string;
}

/** Parse and apply one edit as the model, answering in the model's terms. */
export function applyEdit(api: DocumentApi, xml: string, label = 'model edit'): EditOutcome {
  let parsed: ParsedEdit;
  try {
    parsed = parseEdit(api, xml);
  } catch (e) {
    const err = e instanceof EditError ? e : new EditError(String(e));
    return { ok: false, failure: failed(api.version(), [{ op: err.line ?? null, kind: 'parse', message: err.message }]) };
  }
  const result: ApplyResult = api.apply(
    { label, actor: 'model', ts: Date.now(), ops: parsed.ops },
    parsed.expectVersion,
  );
  if (result.ok) return { ok: true, version: result.version, aliases: parsed.aliases };
  return { ok: false, version: result.version, failure: failed(result.version ?? api.version(), result.errors ?? []) };
}

function failed(version: number, errors: { op: number | null; kind: string; message: string }[]): string {
  const lines = errors.map((e) => `  <error${e.op === null ? '' : ` op="${e.op}"`} kind="${e.kind}">${escapeXml(e.message)}</error>`);
  return `<edit-failed applied="none" version="${version}">\n${lines.join('\n')}\n</edit-failed>`;
}

function escapeXml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
