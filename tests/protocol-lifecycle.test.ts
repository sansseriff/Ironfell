import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PendingRequests } from '../src-ui/runtime/pending_requests';
import { QuerySubscriptions, type StateChange } from '../src-ui/runtime/query_subscriptions';
const tick = () => new Promise(resolve => setTimeout(resolve, 0));
const change = (nodes: string[], extra = {}): StateChange => ({ version: 1, sessionVersion: 0, resolvedVersion: 1, nodes, authored: true, structure: false, compositions: false, reset: false, canUndo: true, canRedo: false, ...extra });
test('replacement, failed sends, timeouts and disposal settle promises; stale sessions cannot reply', async () => {
  const requests = new PendingRequests(); requests.attach(1);
  const replaced = requests.request(() => {}); const check = assert.rejects(replaced, /session replaced/);
  requests.attach(2); await check;
  let id = 0;
  const current = requests.request((next) => { id = next; });
  requests.settle(id, 1, 'stale'); assert.equal(requests.size, 1);
  requests.settle(id, 2, 'current'); assert.equal(await current, 'current');
  await assert.rejects(requests.request(() => { throw new Error('send failed'); }), /send failed/);
  await assert.rejects(requests.request(() => {}, 1), /timed out/);
  const pending = requests.request(() => {}); const disposed = assert.rejects(pending, /disposed/);
  requests.dispose(); await disposed; assert.equal(requests.size, 0);
  await assert.rejects(requests.request(() => {}), /no session/);
});
test('scoped reads coalesce, ignore unrelated changes, keep identity, and observe resolved-only changes', async () => {
  let value = 1; const output: unknown[] = []; const counts: Record<string, number> = {};
  const queries = new QuerySubscriptions(async command => {
    const key = JSON.stringify(command); counts[key] = (counts[key] ?? 0) + 1;
    return JSON.stringify(command.op === 'compositions' ? [] : { node: '#a', value });
  });
  const inspect = { op: 'inspect', node: '#a' } as const;
  const stop = queries.subscribe(inspect, data => output.push(data));
  const other = queries.subscribe(inspect, () => {});
  const stopCompositions = queries.subscribe({ op: 'compositions' }, () => {});
  queries.start(); await tick();
  assert.equal(counts[JSON.stringify(inspect)], 1);
  queries.invalidate(change(['#b'])); await tick(); assert.equal(counts[JSON.stringify(inspect)], 1);
  queries.invalidate(change(['#a'])); queries.invalidate(change(['#a'])); await tick();
  assert.equal(counts[JSON.stringify(inspect)], 2); assert.equal(output.length, 1);
  value = 2; queries.invalidate(change(['#a'], { resolvedVersion: 2 })); await tick();
  assert.deepEqual(output, [{ node: '#a', value: 1 }, { node: '#a', value: 2 }]);
  assert.equal(counts['{"op":"compositions"}'], 1);
  stop(); other(); stopCompositions(); assert.equal(queries.stats().active, 0);
});
test('unmounted or superseded asynchronous queries cannot publish old results', async () => {
  const resolve: ((value: string) => void)[] = []; const values: unknown[] = [];
  const queries = new QuerySubscriptions(() => new Promise(done => resolve.push(done)));
  const stop = queries.subscribe({ op: 'inspect', node: '#a' }, value => values.push(value));
  queries.start(); await tick();
  queries.invalidate(change(['#a'])); resolve.shift()!('{"old":true}'); await tick();
  assert.deepEqual(values, []);
  resolve.shift()!('{"current":true}'); await tick(); assert.deepEqual(values, [{ current: true }]);
  queries.invalidate(change(['#a'])); await tick(); stop(); resolve.shift()!('{"unmounted":true}'); await tick();
  assert.equal(values.length, 1);
  queries.subscribe({ op: 'inspect', node: '#b' }, value => values.push(value)); await tick();
  queries.dispose(); resolve.shift()!('{"disposed":true}'); await tick(); assert.equal(values.length, 1);
});

test('whole-document readers watch authored changes without mirroring every node id or following Stats', async () => {
  let reads = 0; const interests: unknown[] = [];
  const queries = new QuerySubscriptions(async () => { reads++; return '<document><rect id="#a"/></document>'; }, interest => interests.push(interest));
  queries.subscribe({ op: 'view', query_json: '{"scope":null,"fidelity":"full"}' }, () => {});
  queries.start(); await tick();
  queries.invalidate(change([], { reset: true })); await tick();
  const before = reads;
  queries.invalidate(change(['#session'], { authored: false, sessionVersion: 2, resolvedVersion: 9 })); await tick();
  assert.equal(reads, before);
  assert.deepEqual(interests.at(-1), { all: true, compositions: false, nodes: [] });
  queries.invalidate(change(['#a'], { authored: true, resolvedVersion: 10 })); await tick();
  assert.equal(reads, before + 1);
});
