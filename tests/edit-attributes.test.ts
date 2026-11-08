import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseEdit } from '../src-ui/model/edit';

// The 2026-11-07 agent log: a slider's value="{= …}" on <create> was dropped
// without a word, <set value="…"> was refused unless path= named the leaf, and
// x.bind="…" (the form the view prints) was not accepted.
const schema = {
  facets: {
    transform2d: { x: 'number', y: 'number' },
    slider: { min: 'number', max: 'number', value: 'number' },
    key: { t: 'number', value: 'any' },
  },
  types: {
    slider: { facets: [{ kind: 'transform2d', required: true }, { kind: 'slider', required: true }], children: 'none' },
    key: { facets: [{ kind: 'key', required: true }], children: 'none' },
  },
};

function api() {
  let next = 0;
  return {
    schema: async () => schema,
    version: async () => 3,
    nextIds: async (n: number) => Array.from({ length: n }, () => `#N${++next}`),
    nextRelationIds: async () => [],
    place: async (_p: string, _a: string, n: number) => Array.from({ length: n }, (_, i) => `k${i}`),
  } as any;
}

test('create keeps a value attribute, bound or constant', async () => {
  const { ops } = await parseEdit(api(), '<edit><create type="slider" x="1" value="{= #A.size.w}" max="9"/><create type="key" t="0" value="3"/></edit>');
  const facets = (op: any) => Object.assign({}, ...op.facets);
  assert.deepEqual(facets(ops[0]).slider.value, { bind: '#A.size.w' });
  assert.equal(facets(ops[0]).slider.max, 9);
  assert.notEqual(facets(ops[1]).key.value, undefined);
});

test('set takes value as a leaf without path=, and path= value= still works', async () => {
  const { ops } = await parseEdit(api(), '<edit><set id="#S" type="slider" value="4"/><set id="#S" path="slider.min" value="1"/></edit>');
  assert.deepEqual(ops.map((o: any) => [o.path, o.slot]), [['slider.value', 4], ['slider.min', 1]]);
});

test('x.bind="expr" is accepted as a binding', async () => {
  const { ops } = await parseEdit(api(), '<edit><set id="#S" x.bind="#A.transform2d.y + 1"/></edit>');
  assert.deepEqual(ops.map((o: any) => [o.path, o.slot]), [['transform2d.x', { bind: '#A.transform2d.y + 1' }]]);
});

test('a run of numbers is a series; two or three with commas stay a vector', async () => {
  const { coerce } = await import('../src-ui/model/schema');
  assert.deepEqual(coerce('any', '400 450 500 nm'), { ser: '400 450 500 nm' });
  assert.deepEqual(coerce('any', '0 0.4 0.9 1.6'), { ser: '0 0.4 0.9 1.6' });
  assert.deepEqual(coerce('any', '1, 2, 3, 4'), { ser: '1, 2, 3, 4' });
  assert.deepEqual(coerce('any', '1,2'), [1, 2]);
  assert.deepEqual(coerce('any', '9.58 mW'), { q: '9.58 mW' });
  assert.equal(coerce('any', '12'), 12);
  assert.equal(coerce('geometry', 'M 0 0 L 10 5'), 'M 0 0 L 10 5');
});
