/**
 * `<revise>`: patch the edit the store just rejected instead of resending it
 * whole. The harness keeps the rejected `<edit>` as written; a revise names
 * its verbs by handle (an `$alias` it created, an `as=` it named, or its op
 * number, counting verbs from 0 as `<edit-failed>` does) and changes only
 * those. Handles always mean the rejected edit as sent, never as partly
 * revised, so the order of a revise's own verbs does not move them. The
 * revised edit is then applied whole, one transaction, like any edit.
 *
 *   <revise>
 *     <change op="$sinv" port.signal="{= $sin.port.signal * 2}"/>   set or add attributes on one verb
 *     <replace op="14"><create …/></replace>                         swap one verb for any number
 *     <drop op="$bfill"/>
 *     <insert after="$ta"><create …/></insert>                       or before="…"
 *     <text old="$spump.port" new="$sp.port" all="true"/>            exact text, once unless all
 *   </revise>
 *
 * `<revise expect-version="N">` (or `exact=`) changes the edit's own
 * attribute. `<text>` runs last, over the revised markup, in order.
 */

export type ReviseResult = { ok: true; edit: string } | { ok: false; error: string };

/** Pull the `<revise>…</revise>` block out of a model reply that may say more. */
export function extractRevise(text: string): string | null {
  const m = text.match(/<revise[\s>][\s\S]*?<\/revise>|<revise\s*\/>/);
  return m ? m[0] : null;
}

/** An element at the top level of a body: its source span, name and attributes. */
interface Item {
  /** Text between the previous element (or the body's start) and this one. */
  gap: string;
  src: string;
  name: string;
  attrs: Record<string, string>;
  /** The opening tag's span within `src`, and what follows it, for elements with a body. */
  openEnd: number;
  inner: string;
}

/** Split `<root …>body</root>` into its opening tag, top-level elements and closing tail. */
function split(xml: string, root: string): { head: string; items: Item[]; tail: string } {
  const start = xml.search(new RegExp(`<${root}[\\s>/]`));
  if (start < 0) throw new Error(`no <${root}> element`);
  const open = tagEnd(xml, start);
  const head = xml.slice(start, open.end);
  if (open.selfClosing) return { head: head.replace(/\s*\/>$/, '>'), items: [], tail: `</${root}>` };
  const close = xml.lastIndexOf(`</${root}>`);
  if (close < open.end) throw new Error(`<${root}> is not closed`);
  const body = xml.slice(open.end, close);
  const items: Item[] = [];
  let pos = 0;
  let gapStart = 0;
  while (pos < body.length) {
    const lt = body.indexOf('<', pos);
    if (lt < 0) break;
    if (body.startsWith('<!--', lt)) {
      const end = body.indexOf('-->', lt);
      pos = end < 0 ? body.length : end + 3;
      continue;
    }
    const name = body.slice(lt + 1).match(/^[A-Za-z][\w-]*/)?.[0];
    if (!name) throw new Error(`unexpected "${body.slice(lt, lt + 20)}" in <${root}>`);
    const t = tagEnd(body, lt);
    let end = t.end;
    if (!t.selfClosing) end = closeOf(body, t.end, name);
    const src = body.slice(lt, end);
    items.push({ gap: body.slice(gapStart, lt), src, name, attrs: attributes(body.slice(lt, t.end)), openEnd: t.end - lt, inner: t.selfClosing ? '' : body.slice(t.end, end - `</${name}>`.length) });
    pos = gapStart = end;
  }
  return { head, items, tail: body.slice(gapStart) + `</${root}>` };
}

/** Where the tag opening at `lt` ends (after its `>`), quote-aware. */
function tagEnd(s: string, lt: number): { end: number; selfClosing: boolean } {
  let quote = '';
  for (let i = lt + 1; i < s.length; i++) {
    const c = s[i];
    if (quote) { if (c === quote) quote = ''; continue; }
    if (c === '"' || c === "'") quote = c;
    else if (c === '>') return { end: i + 1, selfClosing: s[i - 1] === '/' };
  }
  throw new Error(`tag "${s.slice(lt, lt + 40)}…" is not closed`);
}

/** The end of `</name>` matching an element opened just before `from`. */
function closeOf(s: string, from: number, name: string): number {
  let depth = 1;
  let pos = from;
  while (pos < s.length) {
    const lt = s.indexOf('<', pos);
    if (lt < 0) break;
    if (s.startsWith(`</${name}`, lt) && /[\s>]/.test(s[lt + name.length + 2] ?? '')) {
      depth -= 1;
      const gt = s.indexOf('>', lt);
      if (depth === 0) return gt + 1;
      pos = gt + 1;
      continue;
    }
    if (s.startsWith('<!--', lt)) {
      const end = s.indexOf('-->', lt);
      pos = end < 0 ? s.length : end + 3;
      continue;
    }
    const t = s.slice(lt + 1).match(/^[A-Za-z][\w-]*/);
    if (!t) { pos = lt + 1; continue; }
    const tag = tagEnd(s, lt);
    if (t[0] === name && !tag.selfClosing) depth += 1;
    pos = tag.end;
  }
  throw new Error(`<${name}> is not closed`);
}

const ATTR = /([\w.:$-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g;

function attributes(tag: string): Record<string, string> {
  const a: Record<string, string> = {};
  for (const m of tag.replace(/^<[\w-]+/, '').matchAll(ATTR)) a[m[1]] = m[2] ?? m[3];
  return a;
}

/** Set or add `name="value"` on an opening tag, keeping the rest as written. */
function setAttr(tag: string, name: string, value: string): string {
  const quoted = `"${value.replace(/"/g, '&quot;')}"`;
  const esc = name.replace(/[.$]/g, '\\$&');
  const existing = new RegExp(`(\\s${esc}\\s*=\\s*)(?:"[^"]*"|'[^']*')`);
  if (existing.test(tag)) return tag.replace(existing, (_m, lead) => `${lead}${quoted}`);
  return tag.replace(/\s*(\/?>)$/, (_m, end) => ` ${name}=${quoted}${end === '/>' ? '/>' : '>'}`);
}

function unescapeXml(s: string): string {
  return s.replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');
}

/** Apply a `<revise>` to the rejected `<edit>`; the error, if any, reads as the model wrote it. */
export function revise(rejected: string, markup: string): ReviseResult {
  try {
    return { ok: true, edit: reviseOrThrow(rejected, markup) };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}

function reviseOrThrow(rejected: string, markup: string): string {
  const draft = split(rejected, 'edit');
  const patch = split(markup, 'revise');
  if (!patch.items.length) throw new Error('<revise> is empty; give <change>, <replace>, <drop>, <insert> or <text>');

  // Each verb of the rejected edit, by handle, as sent.
  const handle = (raw: string | undefined, where: string): number => {
    if (raw === undefined || raw === '') throw new Error(`${where} needs op="$alias" or op="N"`);
    if (/^\d+$/.test(raw)) {
      const n = Number(raw);
      if (n >= draft.items.length) throw new Error(`${where}: there is no op ${n}; the edit had ops 0 to ${draft.items.length - 1}`);
      return n;
    }
    const hits = draft.items.flatMap((item, i) => (item.attrs.id === raw || item.attrs.as === raw ? [i] : []));
    if (hits.length === 1) return hits[0];
    if (hits.length === 0) throw new Error(`${where}: no op of the rejected edit has id or as "${raw}"`);
    throw new Error(`${where}: "${raw}" names ops ${hits.join(' and ')}; use op="N"`);
  };

  type Slot = { src: string | null; gap: string; before: string[]; after: string[] };
  const slots: Slot[] = draft.items.map((item) => ({ src: item.src, gap: item.gap, before: [], after: [] }));
  let head = draft.head;
  for (const [k, v] of Object.entries(attributes(patch.head))) head = setAttr(head, k, unescapeXml(v));
  const texts: Item[] = [];

  for (const [i, step] of patch.items.entries()) {
    const where = `revise step ${i} <${step.name}>`;
    const live = (n: number) => {
      const slot = slots[n];
      if (slot.src === null) throw new Error(`${where}: op ${n} was dropped earlier in this revise`);
      return slot;
    };
    switch (step.name) {
      case 'change': {
        const slot = live(handle(step.attrs.op, where));
        const changes = Object.entries(step.attrs).filter(([k]) => k !== 'op');
        if (!changes.length) throw new Error(`${where}: give the attributes to set, e.g. x="10"`);
        const t = tagEnd(slot.src!, 0);
        let tag = slot.src!.slice(0, t.end);
        for (const [k, v] of changes) tag = setAttr(tag, k, unescapeXml(v));
        slot.src = tag + slot.src!.slice(t.end);
        break;
      }
      case 'replace':
        live(handle(step.attrs.op, where)).src = step.inner.trim();
        break;
      case 'drop':
        live(handle(step.attrs.op, where)).src = null;
        break;
      case 'insert': {
        const anchor = step.attrs.after ?? step.attrs.before;
        if (anchor === undefined) throw new Error(`${where} needs after="$alias|N" or before="$alias|N"`);
        if (!step.inner.trim()) throw new Error(`${where}: put the verbs to insert inside it`);
        const slot = slots[handle(anchor, where)];
        (step.attrs.after !== undefined ? slot.after : slot.before).push(step.inner.trim());
        break;
      }
      case 'text':
        texts.push(step);
        break;
      default:
        throw new Error(`unknown revise step <${step.name}>; use change, replace, drop, insert or text`);
    }
  }

  const indent = draft.items[0]?.gap.match(/\n([ \t]*)$/)?.[1] ?? '  ';
  let body = '';
  for (const slot of slots) {
    for (const verbs of slot.before) body += `\n${indent}${verbs}`;
    if (slot.src !== null) body += slot.src === '' ? '' : (slot.gap || `\n${indent}`) + slot.src;
    for (const verbs of slot.after) body += `\n${indent}${verbs}`;
  }
  let edit = head + body + draft.tail;

  for (const [i, step] of texts.entries()) {
    const where = `revise <text> ${i}`;
    if (step.attrs.old === undefined || step.attrs.new === undefined) throw new Error(`${where} needs old="…" and new="…"`);
    const all = /^(true|yes|1)$/i.test(step.attrs.all ?? '');
    // The text as written in the edit's markup, or with its entities read
    // (old="&quot;mW&quot;" finds "mW" inside a bare attribute too).
    const forms: [string, string][] = [[step.attrs.old, step.attrs.new], [unescapeXml(step.attrs.old), unescapeXml(step.attrs.new)]];
    const hit = forms.find(([old]) => old !== '' && edit.includes(old));
    if (!hit) throw new Error(`${where}: "${step.attrs.old}" is not in the rejected edit`);
    const [old, replacement] = hit;
    const count = edit.split(old).length - 1;
    if (count > 1 && !all) throw new Error(`${where}: "${old}" occurs ${count} times; quote more of it, or add all="true"`);
    edit = edit.split(old).join(replacement);
  }
  return edit;
}
