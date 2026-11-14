import { test } from 'node:test';
import assert from 'node:assert/strict';
import { documentArguments } from '../src-ui/runtime/protocol.generated';
import { makeDocumentApi } from '../src-ui/model/api';

for (const asynchronous of [false, true]) {
  test(`model API over ${asynchronous ? 'asynchronous native' : 'synchronous wasm'} transport`, async () => {
    const calls: [string, unknown[]][] = [];
    const api = makeDocumentApi(command => {
      const { fn: name, args } = documentArguments(command);
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
    await api.plan('<edit/>');
    assert.deepEqual(await api.apply({ ops: [] }, 12), { ok: true });
    await api.selection();
    await api.netlist('circuit');
    assert.deepEqual(calls, [
      ['schema', []],
      ['view', ['{"scope":null,"fidelity":"skeleton","depth":0}']],
      ['view', ['{"scope":null,"depth":null,"fidelity":"full","relation":"model"}']],
      ['edit_plan', ['<edit/>']],
      ['apply', ['{"ops":[]}', 12]],
      ['selection', []], ['netlist', ['circuit']],
    ]);
  });
}

test('schema failure can be retried; transport errors and transaction rejection survive decoding', async () => {
  let attempts = 0;
  const api = makeDocumentApi(command => {
    const name = command.op;
    if (name === 'schema' && ++attempts === 1) throw new Error('disconnected');
    if (name === 'selection') return Promise.reject(new Error('session disposed'));
    return JSON.stringify(name === 'schema' ? { types: [] } : { ok: false, errors: [{ kind: 'stale' }] });
  });
  await assert.rejects(async () => api.schema(), /disconnected/);
  assert.deepEqual(await api.schema(), { types: [] });
  await assert.rejects(async () => api.selection(), /session disposed/);
  assert.deepEqual(await api.apply({}, 99), { ok: false, errors: [{ kind: 'stale' }] });
});
