import type { Property } from './PropertyField.svelte';

type Inspection = {
  node: string; type: string; name: string | null;
  components: { kind: string; fields: Property[] }[];
  addable: string[]; removable: string[];
};

/** Only offer properties every selected object supports. Keep the first
 * value as a scrub/picker origin, but never present it as a shared value. */
export function selectionProperties<T extends Inspection>(items: T[]): T | null {
  if (!items.length) return null;
  if (items.length === 1) return items[0];
  const first = items[0];
  const supports = (item: T, kind: string) => item.addable.includes(kind) || item.components.some(c => c.kind === kind);
  const kinds = [...new Set(items.flatMap(i => i.components.map(c => c.kind)))].filter(kind => items.every(i => supports(i, kind)));
  const components = kinds.map(kind => {
    const sources = items.map(i => i.components.find(c => c.kind === kind));
    const template = sources.find(c => c)!;
    return { kind, fields: template.fields.filter(f => sources.every(c => !c || c.fields.some(other => other.leaf === f.leaf && other.kind === f.kind))).map(f => {
      const values = sources.map(c => c?.fields.find(other => other.leaf === f.leaf));
      const locked = values.find(v => v?.edit === 'locked');
      const mixed = values.some(v => !v || (v.full ?? v.text) !== (f.full ?? f.text));
      const sameBinding = values.every(v => v?.bind === f.bind);
      const mixedColor = f.kind === 'color' ? values.some(v => !v || v.text?.slice(0, 7) !== f.text?.slice(0, 7)) : undefined;
      const mixedAlpha = f.kind === 'color' ? values.some(v => !v || v.text?.slice(7) !== f.text?.slice(7)) : undefined;
      return { ...f, nodes: items.map(i => i.node), mixed, mixedColor, mixedAlpha, bind: sameBinding ? f.bind : null,
        edit: locked ? 'locked' : f.edit, why: locked?.why ?? null,
        via: values.every(v => v?.via === f.via) ? f.via : null, key: undefined };
    }) };
  });
  return { ...first, name: `${items.length} selected`, type: 'selection', components,
    addable: [...new Set(items.flatMap(i => i.addable))].filter(k => items.every(i => supports(i, k))),
    removable: first.removable.filter(k => items.every(i => i.removable.includes(k))),
    component: null, main: null, port: null, constraints: null,
    table: items.every(i => (i as T & { table?: string }).table === (first as T & { table?: string }).table) ? (first as T & { table?: string }).table : null,
  };
}

/** Each original paint stays editable without flattening the other colours. */
export function selectionColors(items: Inspection[]): Property[] {
  if (items.length < 2) return [];
  const colors = new Map<string, Property>();
  for (const item of items) for (const component of item.components) for (const f of component.fields) {
    if (f.kind !== 'color' || !f.text) continue;
    const key = `${f.leaf}:${f.text}`;
    const prior = colors.get(key);
    if (prior) {
      prior.nodes!.push(item.node);
      if (f.edit === 'locked') { prior.edit = 'locked'; prior.why = f.why; }
    }
    else colors.set(key, { ...f, nodes: [item.node] });
  }
  return [...colors.values()];
}
