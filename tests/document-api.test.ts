import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeDocumentApi } from '../src-ui/model/api';

for (const asynchronous of [false, true]) {
  test(`model API over ${asynchronous ? 'asynchronous native' : 'synchronous wasm'} transport`, async () => {
    const calls: [string, unknown[]][] = [];
    const api = makeDocumentApi((name, args) => {
      calls.push([name, args]);
      const result = name === 'view' ? '<document version="12"/>'
        : JSON.stringify(name === 'schema' ? { types: [] } : { ok: true });
      return asynchronous ? Promise.resolve(result) : result;
    });
    const [schema, again] = await Promise.all([api.schema(), api.schema()]);
    assert.equal(schema, again);
    assert.equal(calls.filter(([name]) => name === 'schema').length, 1);
    assert.equal(await api.version(), 12);
    assert.equal(await api.view({ fidelity: 'full', relation: 'model' }), '<document version="12"/>');
    await api.nextIds(2);
    await api.nextRelationIds(3);
    await api.place('#1', 'end', 2);
    assert.deepEqual(await api.apply({ ops: [] }, 12), { ok: true });
    await api.link('#a', '#b', '#a', 'model');
    await api.selection();
    await api.importSvg('<svg/>', '#1', 10, 20, 'human');
    await api.table('{"rows":[]}');
    assert.deepEqual(calls, [
      ['schema', []],
      ['view', ['{"scope":null,"fidelity":"skeleton","depth":0}']],
      ['view', ['{"scope":null,"depth":null,"fidelity":"full","relation":"model"}']],
      ['next_ids', [2]], ['next_relation_ids', [3]], ['place', ['#1', 'end', 2]],
      ['apply', ['{"ops":[]}', 12]], ['link', ['#a', '#b', '#a', 'model']],
      ['selection', []], ['import_svg_at', ['<svg/>', '#1', 10, 20, 'human']],
      ['table', ['{"rows":[]}']],
    ]);
  });
}

test('schema failure can be retried; transport errors and transaction rejection survive decoding', async () => {
  let attempts = 0;
  const api = makeDocumentApi((name) => {
    if (name === 'schema' && ++attempts === 1) throw new Error('disconnected');
    if (name === 'selection') return Promise.reject(new Error('session disposed'));
    return JSON.stringify(name === 'schema' ? { types: [] } : { ok: false, errors: [{ kind: 'stale' }] });
  });
  await assert.rejects(async () => api.schema(), /disconnected/);
  assert.deepEqual(await api.schema(), { types: [] });
  await assert.rejects(async () => api.selection(), /session disposed/);
  assert.deepEqual(await api.apply({}, 99), { ok: false, errors: [{ kind: 'stale' }] });
});
