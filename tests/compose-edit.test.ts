import { test } from 'node:test';
import assert from 'node:assert/strict';
import { documentArguments } from '../src-ui/runtime/protocol.generated';
import { makeDocumentApi } from '../src-ui/model/api';
import { applyEdit } from '../src-ui/model/edit';
import { ask, lookRequest } from '../src-ui/model/agent';
import { manifest, topicRequest, topicsFor } from '../src-ui/model/manifest';

/** A document API whose engine records each call and answers from `answer`. */
function fake(answer: (name: string, args: unknown[]) => unknown = () => ({ ok: true })) {
  const calls: [string, unknown[]][] = [];
  const api = makeDocumentApi(command => {
    const { fn: name, args } = documentArguments(command);
    calls.push([name, args]);
    // A scoped view of any node shows it as a rect.
    const scope = String(args[0]).match(/"scope":"(#[^"]+)"/)?.[1];
    if (name === 'view') return scope ? `<document version="3">\n  <rect id="${scope}"/>\n</document>` : `<document version="3"/>${JSON.stringify(args)}`;
    if (name === 'schema') return JSON.stringify({ types: { rect: { facets: [{ kind: 'transform2d', required: true }], default_host: '2d', children: 'any' } }, facets: { transform2d: { x: 'number', y: 'number' }, on: { x: 'number' } }, standard: [{ key: 'laser', ports: [{ name: 'power', dir: 'in', kind: 'value', default: '10 mW' }] }] });
    if (name === 'selection') return '[]';
    if (name === 'place') return JSON.stringify(Array.from({ length: Number(args[2]) }, (_, i) => `k${i}`));
    if (name === 'next_relation_ids') return '[]';
    return JSON.stringify(answer(name, args));
  });
  return { api, calls };
}

test('component verbs go to the engine as one compose call', async () => {
  const { api, calls } = fake(name => (name === 'compose' ? { ok: true, version: 4, summary: '  <placed entity="#9"/>' } : { ok: true }));
  const markup = '<make-component of="#a #b" name="pump" as="$p"/>\n  <place def="$p" in="#C" x="10" y="20"/>';
  const out = await applyEdit(api, `<edit expect-version="3">\n  ${markup}\n</edit>`);
  assert.equal(out.ok, true);
  assert.match(out.summary ?? '', /<placed entity="#9"\/>/);
  assert.deepEqual(calls.filter(([n]) => n === 'compose'), [['compose', [markup.replace('\n  ', '\n'), '[]']]]);
});

test('component verbs share an edit with plain verbs: their ops go first, one call', async () => {
  const { api, calls } = fake(name => (name === 'compose' ? { ok: true, version: 4, summary: '  <placed entity="#9"/>' } : name === 'next_ids' ? ['#D1', '#R1'] : { ok: true }));
  const out = await applyEdit(api, '<edit><create id="$d" type="rect" x="0" y="0"/><create id="$r" type="rect" parent="$d" x="1" y="1"/><place def="$d" in="#C" x="10" y="20" as="$p"/></edit>');
  assert.equal(out.ok, true, out.failure);
  const [[, [markup, ops]]] = calls.filter(([n]) => n === 'compose');
  assert.equal(markup, '<place def="#D1" in="#C" x="10" y="20" as="$p"/>');
  assert.deepEqual(JSON.parse(String(ops)).map((o: any) => o.id), ['#D1', '#R1']);
  assert.equal(calls.filter(([n]) => n === 'apply').length, 0, 'no separate transaction');
  assert.match(out.summary ?? '', /<created alias="\$d" id="#D1"\/>/);
  assert.match(out.summary ?? '', /<placed entity="#9"\/>/);
});

test('a mixed edit\'s errors count the verbs as the model wrote them', async () => {
  // An op the store refuses: its index among the ops becomes the edit's verb index.
  const op = fake(name => (name === 'compose' ? { ok: false, version: 3, errors: [{ op: 0, kind: 'facet_required', message: 'rect requires transform2d' }] } : name === 'next_ids' ? ['#D1'] : { ok: true }));
  const bad = await applyEdit(op.api, '<edit><place def="std:laser" in="#C"/><create id="$d" type="rect" x="0" y="0"/></edit>');
  assert.equal(bad.ok, false);
  assert.match(bad.failure ?? '', /op="1"/);
  // A component verb the store refuses: "<place> 2" is the edit's third verb.
  const verb = fake(name => (name === 'compose' ? { ok: false, error: '<place> 2: no composition named nowhere' } : name === 'next_ids' ? ['#D1'] : { ok: true }));
  const wrong = await applyEdit(verb.api, '<edit><create id="$d" type="rect" x="0" y="0"/><place def="$d" in="#C"/><place def="$d" in="nowhere"/></edit>');
  assert.match(wrong.failure ?? '', /op="2" kind="compose"/);
});

test('component verbs refuse a table beside them, and a schematic place without its circuit', async () => {
  const { api, calls } = fake();
  const mixed = await applyEdit(api, '<edit><place def="#d" in="#C"/><table to="#C" x="0" y="0">a | b</table></edit>');
  assert.equal(mixed.ok, false);
  assert.match(mixed.failure ?? '', /only verb in their &lt;edit&gt;/);
  const part = await applyEdit(api, '<edit><place part="resistor" x="10" y="10"></place></edit>');
  assert.equal(part.ok, false);
  assert.match(part.failure ?? '', /not part=/);
  assert.equal(calls.filter(([n]) => n === 'compose' || n === 'apply').length, 0);
});

test('a look shows the named instances expanded, and is not a repair attempt', async () => {
  const { api, calls } = fake(name => (name === 'compose' ? { ok: true, version: 4, summary: '' } : { ok: true }));
  const replies = ['<look expand="#u #3B"/>', '<edit><place entity="#3B" in="#C"/></edit>'];
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
  assert.deepEqual(lookRequest('<look expand="u7"/>'), ['#u7']);
  assert.equal(lookRequest('no look here'), null);
});

test('the prompt comes in topics: a core, the relevant ones, an index of the rest', () => {
  const schema = { types: {}, facets: {}, standard: [{ key: 'laser', ports: [{ name: 'power', dir: 'in', kind: 'value', default: '10 mW' }] }] };
  const core = manifest(schema, false, []);
  // The core keeps what every edit needs, and lists every topic it leaves out.
  assert.match(core, /VERBS, inside <edit/);
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
  assert.deepEqual(topicRequest('<topic name="tables layout"/>'), ['tables', 'layout']);
  assert.equal(topicRequest('nothing'), null);
});

test('a topic request is answered with that topic, and is not a repair attempt', async () => {
  const { api } = fake(name => (name === 'compose' ? { ok: true, version: 4, summary: '' } : { ok: true }));
  const replies = ['<topic name="tables"/>', '<edit><table to="#C" x="0" y="0">a | b</table></edit>'];
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

test('a ref leaf may name a node created in the same edit by its $alias', async () => {
  const schema = {
    facets: { name: { text: 'string' }, instance: { def: 'ref', mode: 'enum' } },
    types: {
      definition: { facets: [{ kind: 'name', required: false }], default_host: 'library', children: 'any' },
      entity: { facets: [{ kind: 'instance', required: true }], default_host: 'model', children: 'any' },
    },
  };
  const api = {
    schema: async () => schema,
    nextIds: async (n: number) => ['#D1', '#E1', '#E2'].slice(0, n),
    nextRelationIds: async () => [],
    place: async (_p: string, _a: string, n: number) => Array.from({ length: n }, (_, i) => `k${i}`),
  } as any;
  const { parseEdit } = await import('../src-ui/model/edit');
  const parsed = await parseEdit(api, '<edit><create id="$def" type="definition" parent="#L" name="level"/><create id="$e" type="entity" parent="#M" instance.def="$def"/><set id="$e" instance.def="$def"/></edit>');
  const create = parsed.ops[1] as any;
  assert.deepEqual(create.facets.find((f: any) => f.instance).instance.def, { ref: '#D1' });
  assert.deepEqual((parsed.ops[2] as any).slot, { ref: '#D1' });
  await assert.rejects(parseEdit(api, '<edit><create id="$e" type="entity" parent="#M" instance.def="$nope"/></edit>'), /alias \$nope is not created in this edit/);
});

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
  const { api, calls } = fake(name => (name === 'apply' ? { ok: true, version: ++version } : name === 'next_ids' ? ['#N1'] : { ok: true }));
  const { seen, transport } = scripted([
    '<edit><create id="$a" type="rect" x="0" y="0"/></edit><continue/>',
    '<edit><set id="#N1" x="5"/></edit>',
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
    if (name !== 'apply') return name === 'next_ids' ? ['#N1'] : { ok: true };
    applies++;
    return applies === 1 ? { ok: true, version: 4 } : { ok: false, version: 4, errors: [{ op: 0, kind: 'bad', message: 'no' }] };
  });
  const { transport } = scripted(() => '<edit><set id="#N1" x="5"/></edit><continue/>');
  const result = await ask(api, transport, 'stuck');
  assert.equal(result.ok, false);
  assert.equal(applies, 1 + 4, 'one applied step, then an edit and three repairs');
  assert.match(result.error ?? '', /rejected 4 edits in a row; the steps before them are applied/);
});

test('every model call counts against the turn\'s budget', async () => {
  let version = 3;
  const { api, calls } = fake(name => (name === 'apply' ? { ok: true, version: ++version } : { ok: true }));
  const { transport } = scripted(() => '<edit><set id="#N1" x="5"/></edit><continue/>');
  const result = await ask(api, transport, 'forever');
  assert.equal(calls.filter(([n]) => n === 'apply').length, 20);
  assert.equal(result.ok, true, 'the last step applied');
});

test('a bare field two facets share is read against the node\'s own type', async () => {
  const { api, calls } = fake(name => (name === 'apply' ? { ok: true, version: 4 } : { ok: true }));
  const out = await applyEdit(api, '<edit><set id="#6f" x="240"/></edit>');
  assert.equal(out.ok, true, out.failure);
  const [[, [tx]]] = calls.filter(([n]) => n === 'apply');
  assert.equal(JSON.parse(String(tx)).ops[0].path, 'transform2d.x');
});
