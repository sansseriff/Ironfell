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
 *
 *   <edit><link-figures a="#l" b="#Q" keep="#v"/></edit>   (alone: the store builds it)
 *   <edit><import-svg to="#4" x="100" y="50"><svg …>…</svg></import-svg></edit>   (alone too)
 */
import { XMLParser } from 'fast-xml-parser';
import type { ApplyResult, DocumentApi, SolverSummary } from './api';
import { coerce, resolveLeaf, type SlotJson } from './schema';

export interface ParsedEdit {
  expectVersion: number;
  exact: boolean;
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
  // exact="true": refuse the edit if the solver cannot honour every value it
  // sets, instead of adjusting and reporting (constraint-system.md §9.2).
  const exact = /^(true|yes|1)$/i.test(String(rootAttrs['exact'] ?? ''));
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
  return { expectVersion, exact, ops, aliases };
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
  /** What the solver did, when constraints were involved. */
  solver?: SolverSummary;
  /** Markup for the model about an applied edit: the solver's outcome (clusters, moved
   *  nodes, unmet requests), or an import's new roots and cuts. */
  summary?: string;
}

/** `<link-figures a b keep?>` (ECM §7.2): the store computes the ops, so it
 *  stands alone in its edit. A merge question comes back as an
 *  `<edit-failed>` naming both entities and asking for `keep`. */
const LINK_FIGURES = /^\s*<edit(?:\s[^>]*)?>\s*<link-figures\s([^>]*?)\/?>\s*(?:<\/link-figures>)?\s*<\/edit>\s*$/;

function linkFigures(api: DocumentApi, attrText: string): EditOutcome {
  const attrs: Record<string, string> = {};
  for (const m of attrText.matchAll(/([\w-]+)="([^"]*)"/g)) attrs[m[1]] = m[2];
  const id = (s: string | undefined) => (s === undefined || s === '' ? '' : s.startsWith('#') ? s : `#${s}`);
  const version = api.version();
  if (!attrs.a || !attrs.b) {
    return { ok: false, failure: failed(version, [{ op: 0, kind: 'parse', message: '<link-figures> needs a="#id" and b="#id"' }]) };
  }
  const r = api.link(id(attrs.a), id(attrs.b), id(attrs.keep), 'model');
  if (r.ok) return { ok: true, version: r.version };
  if (r.question) {
    const q = r.question;
    const entity = (e: typeof q.a) =>
      `    <entity id="${e.entity}"${e.name ? ` name="${escapeXml(e.name)}"` : ''}${e.def ? ` definition="${escapeXml(e.def)}"` : ''}` +
      `${e.params.map(([k, v]) => ` ${k}="${escapeXml(v)}"`).join('')} figures="${e.figures.join(' ')}"` +
      `${e.ports.length ? ` ports="${e.ports.join(' ')}"` : ''}/>`;
    const collisions = q.port_collisions.length ? `\n    <port-collision names="${q.port_collisions.join(' ')}"/>` : '';
    return {
      ok: false,
      failure:
        `<edit-failed applied="none" version="${version}">\n` +
        `  <question kind="merge" a="${id(attrs.a)}" b="${id(attrs.b)}">\n${entity(q.a)}\n${entity(q.b)}${collisions}\n` +
        `    Both already picture an entity. Resend <link-figures> with keep="${q.a.entity}" or keep="${q.b.entity}"; the other entity is deleted and its figures move to the one kept.\n` +
        `  </question>\n</edit-failed>`,
    };
  }
  return { ok: false, failure: failed(version, [{ op: 0, kind: 'link', message: r.error ?? 'link failed' }]) };
}

/** `<import-svg to x y>` with SVG inside (plans/specs/svg-import-export.md):
 *  the SVG's shapes as new nodes under `to`, its origin at `x`, `y`. Alone in
 *  its edit, like `<link-figures>`; what SVG could not carry comes back as
 *  `<cut>` lines in the `<applied>` summary. */
const IMPORT_SVG = /^\s*<edit(?:\s[^>]*)?>\s*<import-svg\s([^>]*?)>([\s\S]*)<\/import-svg>\s*<\/edit>\s*$/;

function importSvg(api: DocumentApi, attrText: string, svg: string): EditOutcome {
  const attrs: Record<string, string> = {};
  for (const m of attrText.matchAll(/([\w-]+)="([^"]*)"/g)) attrs[m[1]] = m[2];
  const version = api.version();
  if (!attrs.to) return { ok: false, failure: failed(version, [{ op: 0, kind: 'parse', message: '<import-svg> needs to="#id" (a composition or group)' }]) };
  const text = svg.trim().replace(/^<!\[CDATA\[([\s\S]*)\]\]>$/, '$1');
  const to = attrs.to.startsWith('#') ? attrs.to : `#${attrs.to}`;
  const r = api.importSvg(text, to, Number(attrs.x ?? 0) || 0, Number(attrs.y ?? 0) || 0, 'model');
  if (!r.ok) return { ok: false, failure: failed(version, [{ op: 0, kind: 'import_svg', message: r.error ?? 'import failed' }]) };
  const cuts = (r.cuts ?? []).map((c) => `\n  <cut>${escapeXml(c)}</cut>`).join('');
  return { ok: true, version: r.version, summary: `<applied version="${r.version}">\n  <imported roots="${(r.roots ?? []).join(' ')}"/>${cuts}\n</applied>` };
}

/** Parse and apply one edit as the model, answering in the model's terms. */
export function applyEdit(api: DocumentApi, xml: string, label = 'model edit'): EditOutcome {
  const link = xml.match(LINK_FIGURES);
  if (link) return linkFigures(api, link[1]);
  if (/<link-figures[\s/>]/.test(xml)) {
    return { ok: false, failure: failed(api.version(), [{ op: null, kind: 'parse', message: '<link-figures> must be the only verb in its <edit>' }]) };
  }
  const svg = xml.match(IMPORT_SVG);
  if (svg) return importSvg(api, svg[1], svg[2]);
  if (/<import-svg[\s/>]/.test(xml)) {
    return { ok: false, failure: failed(api.version(), [{ op: null, kind: 'parse', message: '<import-svg> must be the only verb in its <edit>' }]) };
  }
  let parsed: ParsedEdit;
  try {
    parsed = parseEdit(api, xml);
  } catch (e) {
    const err = e instanceof EditError ? e : new EditError(String(e));
    return { ok: false, failure: failed(api.version(), [{ op: err.line ?? null, kind: 'parse', message: err.message }]) };
  }
  const result: ApplyResult = api.apply(
    { label, actor: 'model', ts: Date.now(), ops: parsed.ops, ...(parsed.exact ? { exact: true } : {}) },
    parsed.expectVersion,
  );
  if (result.ok) {
    const out: EditOutcome = { ok: true, version: result.version, aliases: parsed.aliases };
    if (result.solver) {
      out.solver = result.solver;
      out.summary = solverMarkup(result.version ?? 0, result.solver);
    }
    return out;
  }
  return { ok: false, version: result.version, failure: failed(result.version ?? api.version(), result.errors ?? []) };
}

function solverMarkup(version: number, s: SolverSummary): string {
  const unmet = (s.unmet ?? []).map(
    (u) => `\n  <unmet leaf="${u.leaf}" requested="${u.requested}" solved="${u.solved}">${escapeXml(u.reason)}</unmet>`,
  );
  return `<applied version="${version}">\n  <solver clusters="${s.clusters.join(' ')}" moved="${s.moved.join(' ')}"/>${unmet.join('')}\n</applied>`;
}

function failed(version: number, errors: { op: number | null; kind: string; message: string }[]): string {
  const lines = errors.map((e) => `  <error${e.op === null ? '' : ` op="${e.op}"`} kind="${e.kind}">${escapeXml(e.message)}</error>`);
  return `<edit-failed applied="none" version="${version}">\n${lines.join('\n')}\n</edit-failed>`;
}

function escapeXml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
