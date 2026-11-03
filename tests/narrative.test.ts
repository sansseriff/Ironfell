// plans/specs/narrative-panel.md phase 2: stored blocks to editor nodes and
// back, and the block edits that turn the store's list into the editor's.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { schema } from '../src-ui/narrative/schema';
import { blockOf, diff, nodeOf, OBJECT, sameContent, type StoredBlock } from '../src-ui/narrative/convert';

const block = (id: string, text: string, extra: Partial<StoredBlock> = {}): StoredBlock => ({ id, kind: 'paragraph', level: 1, show: '', focus: '', text, marks: [], ...extra });

test('a block survives the editor unchanged', () => {
  const samples: StoredBlock[] = [
    block('#1', `The box is ${OBJECT} wide, and **bold**.`, {
      marks: [
        { type: 'mention', start: 4, end: 7, target: '#4' },
        { type: 'value', start: 11, end: 12, target: '#4.size.w', digits: 0 },
        { type: 'strong', start: 23, end: 31 },
        { type: 'em', start: 0, end: 3 },
      ],
    }),
    block('#2', 'A heading', { kind: 'heading', level: 2 }),
    block('#3', 'The bench', { kind: 'view', show: 'bench', focus: '#9' }),
    block('#4', 'let x = 1;\nlet y = 2;', { kind: 'code' }),
    block('#5', 'µ😀 overlapping', { marks: [{ type: 'strong', start: 0, end: 7 }, { type: 'link', start: 4, end: 15, href: 'https://x' }] }),
  ];
  for (const b of samples) {
    const node = nodeOf(schema, b);
    assert.equal(node.attrs.id, b.id);
    const back = blockOf(node);
    assert.ok(sameContent(back, b), `${JSON.stringify(b)}\n→ ${JSON.stringify(back)}`);
  }
});

test('the diff: set, insert after a split, delete, move', () => {
  const stored = [block('#a', 'one'), block('#b', 'two'), block('#c', 'three'), block('#d', 'four')];
  const content = (text: string) => ({ ...block('', text) });
  // #b was split (the second half keeps #b's id), #c deleted, #a moved to the end.
  const editor = [
    { id: '#b', content: content('tw') },
    { id: '#b', content: content('o') },
    { id: '#d', content: content('four') },
    { id: '#a', content: content('one') },
  ];
  const { edits, keys } = diff(stored, editor);
  assert.deepEqual(edits.map((e) => [e.op, e.id ?? e.key, e.after ?? null]), [
    ['delete', '#c', null],
    ['set', '#b', null],
    ['insert', 'n1', '#b'],
    ['move', '#a', '#d'],
  ]);
  assert.equal(keys.get(1), 'n1');
  assert.deepEqual(diff(stored, stored.map((b) => ({ id: b.id, content: b }))).edits, [], 'no change, no edits');
});
