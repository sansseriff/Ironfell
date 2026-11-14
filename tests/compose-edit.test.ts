import { test } from 'node:test';
import assert from 'node:assert/strict';
import { documentArguments } from '../src-ui/runtime/protocol.generated';
import { makeDocumentApi } from '../src-ui/model/api';
import { applyEdit } from '../src-ui/model/edit';
import { ask } from '../src-ui/model/agent';
import { manifest, topicsFor } from '../src-ui/model/manifest';
import { fakePlanner, plan as planOf, planned, reader, said } from './fixtures/plan';

/** A document API whose engine records each call and answers from `answer`. */
function fake(answer: (name: string, args: unknown[]) => unknown = () => ({ ok: true })) {
  const calls: [string, unknown[]][] = [];
  const plan = fakePlanner(() => 3);
  const api = makeDocumentApi(command => {
    const { fn: name, args } = documentArguments(command);
    calls.push([name, args]);
    // A scoped view of any node shows it as a rect.
    const scope = String(args[0]).match(/"scope":"(#[^"]+)"/)?.[1];
    if (name === 'view') return scope ? `<document version="3">\n  <rect id="${scope}"/>\n</document>` : `<document version="3"/>${JSON.stringify(args)}`;
    if (name === 'schema') return JSON.stringify({ types: { rect: { facets: [{ kind: 'transform2d', required: true }], default_host: '2d', children: 'any' } }, facets: { transform2d: { x: 'number', y: 'number' }, on: { x: 'number' } }, standard: [{ key: 'laser', ports: [{ name: 'power', dir: 'in', kind: 'value', default: '10 mW' }] }] });
    if (name === 'selection') return '[]';
    if (name === 'version') return JSON.stringify({ version: 3 });
    if (name === 'reply_read') return JSON.stringify(reader.reply(String(args[0])));
    if (name === 'reply_stream') return JSON.stringify(reader.stream(String(args[0])));
    if (name === 'edit_revise') return JSON.stringify(reader.revise(String(args[0]), String(args[1])));
    if (name === 'netlists') return JSON.stringify(reader.netlists());
    if (name === 'edit_plan') {
      // A test's own answer, or the fake planner's.
      const canned = answer(name, args) as object | undefined;
      return JSON.stringify(canned && ('plan' in canned || 'errors' in canned || 'failure' in canned) ? canned : plan(String(args[0])));
    }
    return JSON.stringify(answer(name, args));
  });
  return { api, calls };
}

test('an edit is planned by the engine and applied as one transaction, its summary told', async () => {
  const plan = { ops: [{ t: 'create', id: '#D1' }, { t: 'create', id: '#R1' }], aliases: { $d: '#D1' }, op_verb: [0, 1], verbs: ['create $d', 'place $p'], summary: ['  <created alias="$d" id="#D1"/>', '  <placed entity="#9"/>'], expect_version: 3, exact: false };
  const { api, calls } = fake(name => (name === 'edit_plan' ? { ok: true, version: 3, plan } : name === 'apply' ? { ok: true, version: 4 } : { ok: true }));
  const xml = '<edit expect-version="3"><rect in="#C" id="$d"/><place def="$d" in="#C" id="$p"/></edit>';
  const out = await applyEdit(api, xml);
  assert.equal(out.ok, true);
  assert.deepEqual(calls.filter(([n]) => n === 'edit_plan'), [['edit_plan', [xml]]]);
  const [[, [tx, expect]]] = calls.filter(([n]) => n === 'apply');
  assert.deepEqual(JSON.parse(String(tx)).ops, plan.ops);
  assert.equal(expect, 3);
  assert.equal(out.summary, '<applied version="4">\n  <created alias="$d" id="#D1"/>\n  <placed entity="#9"/>\n</applied>');
});

test('errors read as the model wrote the edit: the verb that wrote each op, the names it gave', async () => {
  // The engine refuses to plan: its errors already count verbs.
  const refused = fake(name => (name === 'edit_plan' ? { ok: false, version: 3, errors: [{ op: 1, kind: 'parse', message: '"nope" is not a field of rect', verb: 'set #A' }] } : { ok: true }));
  const bad = await applyEdit(refused.api, '<edit><rect in="#C"/><set id="#A" nope="1"/></edit>');
  assert.equal(bad.failure, '<edit-failed applied="none" version="3">\n  <error op="1" verb="set #A" kind="parse">"nope" is not a field of rect</error>\n</edit-failed>');
  // The store refuses ops: each is placed at its verb, minted ids as names.
  const plan = { ops: [{ t: 'create', id: '#D1' }, { t: 'set', id: '#P' }, { t: 'set', id: '#P' }, { t: 'create', id: '#N2' }], aliases: { $d: '#D1' }, op_verb: [0, 1, 1, 2], verbs: ['create $d', 'set #P', 'create rect'], summary: [], expect_version: 0, exact: false };
  const store = fake(name => (name === 'edit_plan' ? { ok: true, version: 3, plan } : name === 'apply' ? { ok: false, version: 3, errors: [{ op: 3, kind: 'bad_id', message: '#D1 already exists' }, { op: 2, kind: 'bad_id', message: '#D1 already exists' }, { op: 0, kind: 'bad_id', message: '#D1 already exists' }] } : { ok: true }));
  const out = await applyEdit(store.api, '<edit/>');
  assert.match(out.failure ?? '', /<error ops="0 1 2" kind="bad_id">\$d already exists \(3 times\)<\/error>/);
  // A question comes back as the engine wrote it.
  const asked = fake(name => (name === 'edit_plan' ? { ok: false, version: 3, failure: '<edit-failed applied="none" version="3">\n  <question kind="merge"/>\n</edit-failed>' } : { ok: true }));
  assert.match((await applyEdit(asked.api, '<edit><link-figures a="#a" b="#b"/></edit>')).failure ?? '', /<question kind="merge"\/>/);
});

test('a look shows the named instances expanded, and is not a repair attempt', async () => {
  const { api, calls } = fake(name => (name === 'apply' ? { ok: true, version: 4 } : { ok: true }));
  const replies = [said({ expand: ['#u', '#3B'] }), said({ edit: planned('<edit><place entity="#3B" in="#C"/></edit>', planOf([['place #3B', []]])) })];
  const seen: string[] = [];
  const transport = {
    name: 'script',
    complete: async (_system: string, messages: { role: string; content: string }[]) => {
      seen.push(messages[messages.length - 1].content);
      return { text: replies.shift() ?? '' };
    },
  };
  const result = await ask(api, transport as never, 'picture it again');
  assert.equal(result.ok, true);
  assert.equal(result.attempts, 1);
  const looked = calls.filter(([n, a]) => n === 'view' && String(a[0]).includes('"expand"')).map(([, a]) => JSON.parse(String(a[0])));
  assert.deepEqual(looked.map(q => [q.scope, q.expand]), [['#u', ['#u']], ['#3B', ['#3B']]]);
  assert.match(seen[1], /^LOOK\n/);
});

test('the prompt comes in topics: a core, the relevant ones, an index of the rest', () => {
  const schema = { types: {}, facets: {}, standard: [{ key: 'laser', ports: [{ name: 'power', dir: 'in', kind: 'value', default: '10 mW' }] }] };
  const core = manifest(schema, false, []);
  // The core keeps what every edit needs, and lists every topic it leaves out.
  assert.match(core, /EDITS, inside <edit/);
  assert.match(core, /MORE TOPICS/);
  assert.match(core, /\n  tables\s+tables: rows of words/);
  assert.doesNotMatch(core, /std:laser/);
  // A topic included is shown, and leaves the index.
  const withTables = manifest(schema, false, ['tables']);
  assert.match(withTables, /preset \(light: hairlines/);
  assert.doesNotMatch(withTables, /\n  tables\s+tables: rows of words/);
  // Chosen by the document and the request.
  assert.deepEqual(topicsFor('<document><rect id="#1"/></document>', 'make it red'), []);
  assert.deepEqual(topicsFor('<composition id="#c" name="sheet" notation="schematic">', 'make it red'), ['schematic']);
  assert.deepEqual(topicsFor('<document/>', 'add a table of the results'), ['tables']);
});

test('a topic request is answered with that topic, and is not a repair attempt', async () => {
  const { api } = fake(name => (name === 'apply' ? { ok: true, version: 4 } : { ok: true }));
  const replies = [said({ topics: ['tables'] }), said({ edit: planned('<edit><table in="#C" x="0" y="0">a | b</table></edit>', planOf([['table', []]])) })];
  const seen: string[] = [];
  let system = '';
  const transport = {
    name: 'script',
    complete: async (s: string, messages: { role: string; content: string }[]) => {
      system = s;
      seen.push(messages[messages.length - 1].content);
      return { text: replies.shift() ?? '' };
    },
  };
  const result = await ask(api, transport as never, 'make it red');
  assert.equal(result.attempts, 1);
  assert.doesNotMatch(system, /preset \(light: hairlines/, 'tables was not in the prompt');
  assert.match(seen[1], /^TOPIC\nTABLES/);
  assert.deepEqual(result.trace?.find((s) => s.kind === 'topic'), { attempt: 1, kind: 'topic', names: ['tables'] });
});

test('the manifest lists the standard components from the schema', () => {
  const text = manifest({ types: {}, facets: {}, standard: [{ key: 'laser', ports: [{ name: 'power', dir: 'in', kind: 'value', default: '10 mW' }, { name: 'out', dir: 'out', kind: 'optical_power', default: null }] }] });
  assert.match(text, /std:laser\s+in power value 10 mW; out out optical_power/);
  assert.match(text, /<look expand="#use"\/>/);
  assert.doesNotMatch(text, /only when asked/);
});

/** One step of several: a set, and <continue/>. */
const stepEdit = planned('<edit><set id="#N1" x="5"/></edit>', planOf([['set #N1', [{ t: 'set', id: '#N1', path: 'transform2d.x', slot: 5 }]]]));
const step = said({ edit: stepEdit }, { continue: true });

/** A transport that answers from a list, recording what it was sent last. */
function scripted(replies: string[] | (() => string)) {
  const seen: string[] = [];
  return {
    seen,
    transport: { name: 'script', complete: async (_s: string, messages: { role: string; content: string }[]) => {
      seen.push(messages[messages.length - 1].content);
      return { text: typeof replies === 'function' ? replies() : replies.shift() ?? '' };
    } } as never,
  };
}

test('<continue/> after an applied edit asks for the next step, told what the first made', async () => {
  let version = 3;
  const { api, calls } = fake(name => (name === 'apply' ? { ok: true, version: ++version } : { ok: true }));
  const { seen, transport } = scripted([
    said({ edit: planned('<edit><rect in="#1" id="$a" x="0" y="0"/></edit>', planOf([['create $a', [{ t: 'create', id: '#N1', ty: 'rect', parent: '#1', order: 'a0', facets: [] }]]], { $a: '#N1' })) }, { continue: true }),
    said({ edit: stepEdit }),
  ]);
  const result = await ask(api, transport, 'two steps');
  assert.equal(result.ok, true);
  assert.equal(calls.filter(([n]) => n === 'apply').length, 2);
  assert.match(seen[1], /<created alias="\$a" id="#N1"\/>/);
  assert.match(seen[1], /^<applied version="4">/);
});

test('repairs in a row are bounded, and a stuck turn says the earlier steps are applied', async () => {
  let applies = 0;
  const { api } = fake(name => {
    if (name !== 'apply') return { ok: true };
    applies++;
    return applies === 1 ? { ok: true, version: 4 } : { ok: false, version: 4, errors: [{ op: 0, kind: 'bad', message: 'no' }] };
  });
  const { transport } = scripted(() => step);
  const result = await ask(api, transport, 'stuck');
  assert.equal(result.ok, false);
  assert.equal(applies, 1 + 4, 'one applied step, then an edit and three repairs');
  assert.match(result.error ?? '', /rejected 4 edits in a row; the steps before them are applied/);
});

test('every model call counts against the turn\'s budget', async () => {
  let version = 3;
  const { api, calls } = fake(name => (name === 'apply' ? { ok: true, version: ++version } : { ok: true }));
  const { transport } = scripted(() => step);
  const result = await ask(api, transport, 'forever');
  assert.equal(calls.filter(([n]) => n === 'apply').length, 20);
  assert.equal(result.ok, true, 'the last step applied');
});

test('a plan made before the document moved is made again, not applied stale', async () => {
  // Planned at 2; the person's change made it 3 (the fake's view) before the apply.
  let version = 2;
  const plan = { ops: [{ t: 'create', id: '#N1' }], aliases: {}, op_verb: [0], verbs: ['create rect'], summary: [], expect_version: 0, exact: false };
  const { api, calls } = fake(name => {
    if (name === 'edit_plan') return { ok: true, version, plan };
    if (name === 'apply') return calls.filter(([n]) => n === 'apply').length === 1 ? (version = 3, { ok: false, version: 3, errors: [{ op: null, kind: 'stale', message: 'stale' }] }) : { ok: true, version: 4 };
    return { ok: true };
  });
  const out = await applyEdit(api, '<edit><rect in="#C"/></edit>');
  assert.equal(out.ok, true);
  assert.equal(calls.filter(([n]) => n === 'edit_plan').length, 2);
});
