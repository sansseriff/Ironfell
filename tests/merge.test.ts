import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ask } from '../src-ui/model/agent';
import { changesSince, conflicts } from '../src-ui/model/merge';
import { fakePlanner } from './fixtures/plan';

const schema = {
  facets: { name: { text: 'string' }, transform2d: { x: 'number', y: 'number' }, size: { w: 'number', h: 'number' } },
  types: { rect: { facets: [{ kind: 'name', required: false }, { kind: 'transform2d', required: true }, { kind: 'size', required: false }], children: 'none' } },
};

/** A document at version 14 whose person keeps working: `during` runs while
 *  the model is thinking, adding logged transactions. */
function world() {
  let version = 14;
  const log: { version: number; label: string; ops: object[] }[] = [];
  const applied: { tx: any; expect: number }[] = [];
  let staleOnce = false;
  const person = (label: string, ...ops: object[]) => { version += 1; log.push({ version, label, ops }); };
  const api = {
    schema: async () => schema,
    version: async () => version,
    view: async () => `<document version="${version}"/>`,
    selection: async () => [],
    netlist: async () => ({ ok: false }),
    plan: fakePlanner(() => version),
    logSince: async (since: number) => ({ gap: false, version, ndjson: log.filter((t) => t.version > since).map((t) => JSON.stringify(t) + '\n').join('') }),
    apply: async (tx: any, expect: number) => {
      applied.push({ tx, expect });
      if (staleOnce) { staleOnce = false; person('nudge', { t: 'set', id: '#Z', path: 'transform2d.x', slot: 1 }); return { ok: false, version, errors: [{ op: null, kind: 'stale', message: 'expected' }] }; }
      if (expect !== version) return { ok: false, version, errors: [{ op: null, kind: 'stale', message: 'expected' }] };
      version += 1;
      return { ok: true, version };
    },
  } as any;
  return { api, applied, person, get version() { return version; }, set version(v: number) { version = v; }, raceOnce: () => { staleOnce = true; } };
}

function scripted(replies: string[], during: ((call: number) => void) = () => {}) {
  const seen: string[] = [];
  let call = 0;
  return {
    seen,
    transport: { name: 'script', complete: async (_s: string, messages: { content: string }[]) => { during(call++); seen.push(messages[messages.length - 1].content); return { text: replies.shift() ?? '' }; } },
  };
}

const edit = '<edit expect-version="14"><rect in="#1" id="$r" x="1" y="2"/><set id="#B" x="40"/></edit>';

test('an edit that touches nothing the person changed is applied on top of their changes', async () => {
  const w = world();
  const { transport } = scripted([edit], () => {
    w.person('draw a rectangle', { t: 'create', id: '#U', ty: 'rect', parent: '#1', order: 'a0', facets: [] });
    w.person('move 1 object', { t: 'set', id: '#A', path: 'transform2d.x', slot: 200 }, { t: 'set', id: '#A', path: 'transform2d.y', slot: 90 });
  });
  const result = await ask(w.api, transport as never, 'add a box');
  assert.equal(result.ok, true);
  assert.equal(result.merged, 2);
  assert.equal(w.applied.length, 1);
  assert.equal(w.applied[0].expect, 16);
  assert.equal((result.trace!.at(-1) as any).merged, 2);
});

test('a leaf both changed is a conflict at the agent\'s op; a revise drops it, or an empty one keeps it', async () => {
  for (const [repair, keeps] of [['<revise><drop op="1"/></revise>', false], ['<revise/>', true]] as const) {
    const w = world();
    const { transport, seen } = scripted([edit, repair], (call) => { if (call === 0) w.person('move 1 object', { t: 'set', id: '#B', path: 'transform2d.x', slot: 123.4567 }); });
    const result = await ask(w.api, transport as never, 'add a box');
    assert.match(seen[1], /<error op="1" verb="set #B" kind="conflict">the person set #B transform2d.x to 123.457 while you worked<\/error>/);
    assert.equal(result.ok, true, result.outcome?.failure);
    assert.equal(result.attempts, 2);
    assert.equal(result.merged, 1);
    const sets = w.applied.at(-1)!.tx.ops.filter((op: any) => op.t === 'set');
    assert.equal(sets.length, keeps ? 1 : 0);
  }
});

test('deleting or building under what the person deleted or changed is a conflict', () => {
  const changes = { count: 2, gap: false, leaves: new Map([['#A', new Map([['fill.color', { rgba: [1, 0, 0, 1] }]])]]), facets: new Map(), moved: new Set(['#M']), deleted: new Set(['#P']), labels: [] };
  const found = conflicts([
    { t: 'create', id: '#N1', parent: '#P' },
    { t: 'delete', id: '#A' },
    { t: 'set', id: '#P', path: 'transform2d.x' },
    { t: 'reparent', id: '#M', parent: '#1' },
    { t: 'set', id: '#A', path: 'transform2d.x' },
    { t: 'remove_facet', id: '#A', kind: 'fill' },
  ], changes as any);
  assert.deepEqual(found.map((f) => f.op), [0, 1, 2, 3, 5]);
  assert.match(found[1].message, /the person changed #A while you worked \(fill.color\); deleting it would discard that/);
});

test('the log reads sets, facets, moves, deletes and restores, solver ops included', async () => {
  const w = world();
  w.person('drag', { t: 'set', id: '#A', path: 'transform2d.x', slot: 1 });
  w.api.logSince = async () => ({ gap: false, version: 16, ndjson: [
    JSON.stringify({ label: 'drag', ops: [{ t: 'set', id: '#A', path: 'transform2d.x', slot: 1 }], derived_ops: [{ t: 'set', id: '#S', path: 'transform2d.y', slot: 2 }] }),
    JSON.stringify({ label: 'tidy', ops: [{ t: 'delete', id: '#D' }, { t: 'reorder', id: '#R', order: 'b' }, { t: 'add_facet', id: '#A', facet: { stroke: {} } }, { t: 'delete', id: '#E' }, { t: 'create', id: '#E' }] }),
  ].join('\n') });
  const c = await changesSince(w.api, 14);
  assert.equal(c.count, 2);
  assert.deepEqual([...c.leaves.keys()], ['#A', '#S']);
  assert.deepEqual([...c.deleted], ['#D']);
  assert.deepEqual([...c.moved], ['#R']);
  assert.deepEqual([...c.facets.get('#A')!], ['stroke']);
});

test('a change that lands between the check and the apply is checked again; a log too short refuses', async () => {
  const w = world();
  w.raceOnce();
  const { transport } = scripted([edit]);
  const result = await ask(w.api, transport as never, 'add a box');
  assert.equal(result.ok, true);
  assert.equal(w.applied.length, 2);
  assert.equal(result.merged, 1);

  const far = world();
  far.api.logSince = async () => ({ gap: true, version: far.version, ndjson: '' });
  const { transport: t2 } = scripted([edit], () => far.person('a lot'));
  const refused = await ask(far.api, t2 as never, 'add a box');
  assert.equal(refused.ok, false);
  assert.match(refused.error!, /too much of that is gone from history to merge/);
  assert.equal(refused.attempts, 1);
  assert.equal(far.applied.length, 0);
});
