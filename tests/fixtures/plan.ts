/**
 * A stand-in for the engine's edit planner (iron_document::edit, the
 * `edit_plan` call) in tests of the agent loop, merging and revising. The
 * real planner is tested in Rust (src/iron_document/tests/edit.rs); this one
 * makes just enough of a plan: a top-level node is a create (ids #N1, #N2 …,
 * its `id="$x"` an alias), a <set>'s attributes are sets (a bare x, y read as
 * transform2d, w, h as size), a <delete> a delete; other verbs write nothing.
 */
import type { Plan, PlanResult } from '../../src-ui/model/edit';

const VERBS = ['set', 'add', 'delete', 'reparent', 'link', 'unlink', 'place', 'make-component', 'circuit', 'table', 'context', 'import-svg', 'link-figures', 'write', 'rewrite'];
/** Node types the fake knows; any other element is refused, as the engine refuses one. */
const TYPES = ['group', 'rect', 'ellipse', 'text', 'path', 'slider', 'let', 'composition', 'definition', 'template', 'port_spec', 'library', 'model', 'timeline', 'clip', 'key', 'narrative', 'embed', 'empty', 'mesh'];
const BARE: Record<string, string> = { x: 'transform2d.x', y: 'transform2d.y', w: 'size.w', h: 'size.h', name: 'name.text' };

/** The edit's top-level elements, with their attributes. */
function verbs(xml: string): { name: string; attrs: Record<string, string> }[] {
  const out: { name: string; attrs: Record<string, string> }[] = [];
  let depth = 0;
  for (const m of xml.matchAll(/<(\/?)([\w:-]+)((?:\s+[\w.:-]+(?:\s*=\s*(?:"[^"]*"|'[^']*'))?)*)\s*(\/?)>/g)) {
    const [, close, name, attrText, self] = m;
    if (close) { depth--; continue; }
    if (depth === 1) {
      const attrs: Record<string, string> = {};
      for (const a of attrText.matchAll(/([\w.:-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g)) attrs[a[1]] = a[2] ?? a[3];
      out.push({ name, attrs });
    }
    if (!self) depth++;
  }
  return out;
}

function slot(raw: string): unknown {
  const bind = raw.trim().match(/^\{=([\s\S]*)\}$/);
  if (bind) return { bind: bind[1].trim() };
  const n = Number(raw);
  return raw.trim() !== '' && !Number.isNaN(n) ? n : raw;
}

/** A planner minting ids (#N1, #N2 …, or `mint`'s): call it with an edit's markup. */
export function fakePlanner(version: () => number = () => 0, mint?: () => string) {
  let next = 0;
  const fresh = mint ?? (() => `#N${++next}`);
  return (xml: string): PlanResult => {
    if (!/<edit[\s>/]/.test(xml)) return { ok: false, version: version(), errors: [{ op: null, kind: 'parse', message: 'expected one <edit> element' }] };
    const expect = Number(xml.match(/<edit[^>]*expect-version="(\d+)"/)?.[1] ?? 0);
    const plan: Plan = { ops: [], aliases: {}, op_verb: [], verbs: [], summary: [], expect_version: expect, exact: /<edit[^>]*exact="true"/.test(xml) };
    const els = verbs(xml);
    const unknown = els.findIndex((el) => !VERBS.includes(el.name) && !TYPES.includes(el.name));
    if (unknown >= 0) return { ok: false, version: version(), errors: [{ op: unknown, kind: 'parse', message: `<${els[unknown].name}> is not a verb or a node type`, verb: `create ${els[unknown].name}` }] };
    // Ids first, so any verb may name a node the edit makes.
    const minted = els.map((el) => (VERBS.includes(el.name) ? null : fresh()));
    els.forEach((el, i) => { if (minted[i] && el.attrs.id?.startsWith('$')) plan.aliases[el.attrs.id] = minted[i]!; });
    const id = (raw: string) => plan.aliases[raw] ?? raw;
    els.forEach((el, i) => {
      const push = (op: object) => { plan.ops.push(op); plan.op_verb.push(i); };
      if (minted[i]) {
        plan.verbs.push(el.attrs.id ? `create ${el.attrs.id}` : `create ${el.name}${el.attrs.in ? ` in ${el.attrs.in}` : ''}`);
        push({ t: 'create', id: minted[i], ty: el.name, parent: el.attrs.in ? id(el.attrs.in) : null, order: `a${i}`, facets: [] });
        return;
      }
      plan.verbs.push(`${el.name} ${el.attrs.id ?? el.attrs.in ?? ''}`.trim());
      if (el.name === 'set') {
        for (const [k, v] of Object.entries(el.attrs)) if (k !== 'id') push({ t: 'set', id: id(el.attrs.id), path: k.includes('.') ? k : BARE[k] ?? k, slot: slot(v) });
      } else if (el.name === 'delete') {
        push({ t: 'delete', id: id(el.attrs.id) });
      }
    });
    plan.summary = Object.entries(plan.aliases).map(([a, n]) => `  <created alias="${a}" id="${n}"/>`);
    return { ok: true, version: version(), plan };
  };
}
