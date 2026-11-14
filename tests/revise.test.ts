import { test } from 'node:test';
import assert from 'node:assert/strict';
import { applyEdit } from '../src-ui/model/edit';
import { fakePlanner } from './fixtures/plan';
import { revise, extractRevise } from '../src-ui/model/revise';
import { ask } from '../src-ui/model/agent';
import { failureSummary, streamView } from '../src-ui/chat/conversation';

const schema = {
  facets: {
    name: { text: 'string' },
    transform2d: { x: 'number', y: 'number' },
    size: { w: 'number', h: 'number' },
    port: { signal: 'any', dir: 'enum' },
  },
  types: {
    definition: { facets: [{ kind: 'name', required: false }], children: 'any' },
    port_spec: { facets: [{ kind: 'name', required: false }, { kind: 'port', required: true }], children: 'none' },
    rect: { facets: [{ kind: 'name', required: false }, { kind: 'transform2d', required: true }, { kind: 'size', required: false }], children: 'none' },
  },
};

/** A document API that mints #N1, #N2 … and answers apply with `apply`. */
function fakeApi(apply: (tx: any) => object = () => ({ ok: true, version: 15 })) {
  const applied: any[] = [];
  return {
    applied,
    api: {
      schema: async () => schema,
      version: async () => 14,
      plan: fakePlanner(() => 14),
      apply: async (tx: any) => { applied.push(tx); return apply(tx); },
      view: async () => '<document version="14"/>',
      selection: async () => [],
      netlist: async () => ({ ok: false }),
    } as any,
  };
}

test('store errors read as the model wrote the edit: verbs, aliases, one line per mistake', async () => {
  // A <set> with two leaves writes two ops, so store op 3 is verb 2.
  const { api } = fakeApi(() => ({ ok: false, version: 14, errors: [
    { op: 3, kind: 'bad_id', message: '#N1 already exists' },
    { op: 4, kind: 'bad_id', message: '#N1 already exists' },
    { op: 0, kind: 'bad_id', message: '#N1 already exists' },
  ] }));
  const out = await applyEdit(api, '<edit><definition in="#L" id="$d"/><set id="#P" x="1" y="2"/><rect in="$d" x="0" y="0"/><rect in="#1" x="0" y="0"/></edit>');
  assert.equal(out.ok, false);
  assert.match(out.failure!, /<error ops="0 2 3" kind="bad_id">\$d already exists \(3 times\)<\/error>/);
});

const rejected = `<edit expect-version="14">
  <definition in="#L" id="$d"/>
  <rect in="$d" id="$a" x="0" y="0"/>
  <rect in="$d" x="1" y="{= a > b ? 1 : 2}"/>
  <set id="#P" x="12"/>
</edit>`;

test('revise changes, replaces, drops and inserts verbs by handle, as sent', () => {
  const r = revise(rejected, `<revise expect-version="15">
    <change op="$a" x="5" size.w="9"/>
    <drop op="2"/>
    <insert after="$a"><rect in="$d" id="$b" x="2" y="2"/></insert>
    <insert before="0"><definition in="#L" id="$z"/></insert>
    <replace op="3"><set id="#P" x="13"/>
<set id="#Q" x="1"/></replace>
  </revise>`);
  assert.ok(r.ok, !r.ok ? r.error : '');
  assert.equal(r.edit, `<edit expect-version="15">
  <definition in="#L" id="$z"/>
  <definition in="#L" id="$d"/>
  <rect in="$d" id="$a" x="5" y="0" size.w="9"/>
  <rect in="$d" id="$b" x="2" y="2"/>
  <set id="#P" x="13"/>
<set id="#Q" x="1"/>
</edit>`);
});

test('revise text replaces exact text once, or everywhere with all', () => {
  const once = revise(rejected, '<revise><text old="a &gt; b" new="a &lt; b"/></revise>');
  assert.ok(once.ok);
  assert.match(once.edit, /y="\{= a < b \? 1 : 2\}"/);
  const ambiguous = revise(rejected, '<revise><text old="in=&quot;$d&quot;" new="in=&quot;$e&quot;"/></revise>');
  assert.equal(ambiguous.ok, false);
  assert.match((ambiguous as any).error, /occurs 2 times; quote more of it, or add all="true"/);
  const all = revise(rejected, '<revise><text old="in=&quot;$d&quot;" new="in=&quot;$e&quot;" all="true"/></revise>');
  assert.ok(all.ok);
  assert.equal(all.edit.split('in="$e"').length - 1, 2);
});

test('revise errors say which handle or step is wrong', () => {
  const err = (markup: string) => { const r = revise(rejected, markup); assert.equal(r.ok, false); return (r as any).error as string; };
  assert.match(err('<revise><drop op="$nope"/></revise>'), /no op of the rejected edit has id "\$nope"/);
  assert.match(err('<revise><drop op="9"/></revise>'), /there is no op 9; the edit had ops 0 to 3/);
  assert.match(err('<revise><drop op="1"/><change op="1" x="1"/></revise>'), /op 1 was dropped earlier/);
  assert.match(err('<revise><text old="missing" new="x"/></revise>'), /"missing" is not in the rejected edit/);
  assert.match(err('<revise><frob op="1"/></revise>'), /unknown revise step <frob>/);
  assert.deepEqual(revise(rejected, '<revise/>'), { ok: true, edit: rejected });
  assert.equal(extractRevise('Fixing it.\n<revise><drop op="1"/></revise>'), '<revise><drop op="1"/></revise>');
});

test('a reply with a <revise> patches the rejected edit and applies it whole', async () => {
  const { api, applied } = fakeApi((tx) => tx.ops.some((op: any) => op.id === '#P')
    ? { ok: false, version: 14, errors: [{ op: tx.ops.length - 1, kind: 'not_found', message: '#P does not exist' }] }
    : { ok: true, version: 15 });
  const replies = [`Building it.\n${rejected}`, 'Dropping the stray set.\n<revise><drop op="3"/></revise>'];
  const seen: string[] = [];
  const transport = { name: 'script', complete: async (_s: string, messages: { content: string }[]) => { seen.push(messages[messages.length - 1].content); return { text: replies.shift() ?? '' }; } };
  const result = await ask(api, transport as never, 'build it');
  assert.equal(result.ok, true);
  assert.equal(result.attempts, 2);
  assert.match(seen[1], /<error op="3" verb="set #P" kind="not_found">/);
  assert.match(seen[1], /<revise>/);
  assert.doesNotMatch(result.edit!, /#P/);
  assert.match(result.edit!, /^<edit expect-version="14">\n  <definition in="#L" id="\$d"/);
  assert.equal(result.note, 'Dropping the stray set.');
  assert.deepEqual(result.trace!.map((s) => [s.kind, (s as any).ok, (s as any).revised]), [['edit', false, undefined], ['edit', true, true]]);
  assert.equal(applied.length, 2);
});

test('a <revise> with a <look picture> beside it is applied, then the picture taken', async () => {
  const { api, applied } = fakeApi((tx) => tx.ops.some((op: any) => op.id === '#P')
    ? { ok: false, version: 14, errors: [{ op: tx.ops.length - 1, kind: 'not_found', message: '#P does not exist' }] }
    : { ok: true, version: 15 });
  const shots: string[] = [];
  api.picture = async (source: string) => { shots.push(source); return { name: source, mediaType: 'image/png', data: 'iVBORw0KGgo=' }; };
  const replies = [`Building it.\n${rejected}`, 'Dropping it.\n<revise><drop op="3"/></revise>\n<look picture="scene"/>', 'Looks right.'];
  const transport = { name: 'script', complete: async () => ({ text: replies.shift() ?? '' }) };
  const result = await ask(api, transport as never, 'build it');
  assert.equal(result.ok, true);
  assert.equal(applied.length, 2);
  assert.deepEqual(shots, ['scene']);
  assert.deepEqual(result.trace!.map((s) => [s.kind, (s as any).ok]), [['edit', false], ['edit', true], ['picture', true], ['prose', undefined]]);
});

test('a revise that cannot apply is reported, and the model may try again', async () => {
  const { api } = fakeApi((tx) => tx.ops.some((op: any) => op.id === '#P')
    ? { ok: false, version: 14, errors: [{ op: 3, kind: 'not_found', message: '#P does not exist' }] }
    : { ok: true, version: 15 });
  const replies = [rejected, '<revise><drop op="$p"/></revise>', '<revise><drop op="3"/></revise>'];
  const seen: string[] = [];
  const transport = { name: 'script', complete: async (_s: string, messages: { content: string }[]) => { seen.push(messages[messages.length - 1].content); return { text: replies.shift() ?? '' }; } };
  const result = await ask(api, transport as never, 'build it');
  assert.equal(result.ok, true);
  assert.equal(result.attempts, 3);
  assert.match(seen[2], /<revise-failed>revise step 0 &lt;drop&gt;: no op of the rejected edit has id "\$p"<\/revise-failed>/);
  assert.match(failureSummary('<revise-failed>no op 9</revise-failed>'), /the revise could not be applied: no op 9/);
});

test('a streaming revise forms like an edit, not as prose', () => {
  const view = streamView('Fixing the binding.\n<revise>\n  <change op="$a" x="1"/>\n  <drop op="4"/>\n</revise>');
  assert.equal(view.prose.trim(), 'Fixing the binding.');
  assert.deepEqual(view.verbs, ['Change $a · x', 'Drop 4']);
  assert.equal(streamView('Fixing.\n<revi').prose, 'Fixing.\n');
});
