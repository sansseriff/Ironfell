import { expect, test } from 'bun:test';
import { selectionColors, selectionProperties } from '../src-ui/lib/ui/selection-properties';
import type { Property } from '../src-ui/lib/ui/PropertyField.svelte';
const field = (leaf: string, text: string, kind = 'number'): Property => ({ leaf, field: leaf.split('.')[1], text, kind, bind: null, edit: 'direct', via: null, why: null });
const item = (node: string, fields: Property[], addable: string[] = []) => ({ node, name: null, type: 'rect', components: [...new Set(fields.map(f => f.leaf.split('.')[0]))].map(kind => ({ kind, fields: fields.filter(f => f.leaf.startsWith(`${kind}.`)) })), addable, removable: [] });
test('mixed fields retain an origin, include all targets, and clear misleading binding metadata', () => {
  const a = item('#a', [{ ...field('stroke.width', '2'), bind: '#source.slider.value', via: '#source' }]);
  const b = item('#b', [field('stroke.width', '4')]);
  const f = selectionProperties([a, b])!.components[0].fields[0];
  expect(f).toMatchObject({ text: '2', mixed: true, nodes: ['#a', '#b'], bind: null, via: null });
  b.components[0].fields[0].text = '2';
  expect(selectionProperties([a, b])!.components[0].fields[0].mixed).toBe(false);
});
test('only supported properties are offered, including absent optional facets', () => {
  const a = item('#a', [field('size.w', '100'), field('stroke.width', '2')]);
  const b = item('#b', [field('radius.rx', '50')], ['stroke']);
  const result = selectionProperties([a, b])!;
  expect(result.components.map(c => c.kind)).toEqual(['stroke']);
  expect(result.components[0].fields[0].mixed).toBe(true);
});
test('any locked member locks the shared field', () => {
  const a = item('#a', [field('size.w', '100')]);
  const b = item('#b', [{ ...field('size.w', '100'), edit: 'locked', why: 'layout' }]);
  expect(selectionProperties([a, b])!.components[0].fields[0]).toMatchObject({ edit: 'locked', why: 'layout' });
});
test('selection paints target only the objects with that original colour', () => {
  const items = ['#a', '#b', '#c'].map((id, i) => item(id, [field('fill.color', i === 2 ? '#ff0000ff' : '#000000ff', 'color')]));
  expect(selectionColors(items).map(f => [f.text, f.nodes])).toEqual([['#000000ff', ['#a', '#b']], ['#ff0000ff', ['#c']]]);
});

test('color and opacity report mixed values independently', () => {
  const a = item('#a', [field('stroke.color', '#ff0000ff', 'color')]);
  const b = item('#b', [field('stroke.color', '#0000ffff', 'color')]);
  const color = () => selectionProperties([a, b])!.components[0].fields[0];
  expect(color()).toMatchObject({ mixed: true, mixedColor: true, mixedAlpha: false });
  b.components[0].fields[0].text = '#ff000080';
  expect(color()).toMatchObject({ mixed: true, mixedColor: false, mixedAlpha: true });
});
