/**
 * The registry schema as `document_schema()` returns it: the one source for
 * the manifest, attribute resolution, and value coercion.
 */
export type Kind = 'number' | 'bool' | 'string' | 'color' | 'vec2' | 'vec3' | 'enum' | 'ref' | 'quantity' | 'any';

export interface Schema {
  components: Record<string, Record<string, Kind>>;
  types: Record<
    string,
    {
      components: { kind: string; required: boolean }[];
      default_host: string;
      children: 'none' | 'any' | string[];
    }
  >;
}

/**
 * Resolve an attribute name on a node type to `component.field`. A bare
 * field name resolves to the first component, in the type's declared order,
 * that has it (so `color` on a rect is `fill.color`, and `stroke.color` must
 * be written in full); `name` is the Name component's text.
 */
export function resolveLeaf(schema: Schema, type: string, attr: string): { component: string; field: string; kind: Kind } {
  if (attr.includes('.')) {
    const [component, field] = attr.split('.', 2);
    // A `*` entry means the component's fields are run-time (params).
    const kind = schema.components[component]?.[field] ?? schema.components[component]?.['*'];
    if (!kind) throw new Error(`unknown leaf "${attr}"`);
    return { component, field, kind };
  }
  if (attr === 'name') return { component: 'name', field: 'text', kind: 'string' };
  const spec = schema.types[type];
  if (!spec) throw new Error(`unknown node type "${type}"`);
  const hits = spec.components
    .map((c) => c.kind)
    .filter((c) => schema.components[c]?.[attr] !== undefined);
  if (hits.length >= 1) return { component: hits[0], field: attr, kind: schema.components[hits[0]][attr] };
  const all = spec.components.flatMap((c) => Object.keys(schema.components[c.kind] ?? {}).map((f) => `${c.kind}.${f}`));
  throw new Error(`"${attr}" is not a field of ${type}; available: ${all.join(', ')}`);
}

/** The JSON a slot takes in an op: a value, or `{bind: expr}`. */
export type SlotJson =
  | number
  | boolean
  | string
  | number[]
  | { rgba: number[] }
  | { enum: string }
  | { ref: string }
  | { q: string }
  | { bind: string };

/** `9.58 mW`, `1550nm`, `3 kg*m2/s3`: a magnitude followed by a unit. The unit table lives in Rust. */
const QUANTITY = /^[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?\s*[A-Za-zµμ°Ω][A-Za-z0-9µμ°Ω*·/^-]*$/;

/**
 * Coerce liberally (doc 04 §9.4): `12`, `"12"`, `12px` are all 12; colours
 * take `#rrggbb[aa]` or `rgba(...)`; vectors take `1,2` or `[1, 2]`; any
 * attribute may instead carry a binding as `{= expr}`.
 */
export function coerce(kind: Kind, raw: string): SlotJson {
  const text = raw.trim();
  const bind = text.match(/^\{=\s*([\s\S]*?)\s*\}$/);
  if (bind) return { bind: bind[1] };
  switch (kind) {
    case 'number': {
      const n = Number(text.replace(/(px|s|deg)$/, ''));
      if (Number.isNaN(n)) throw new Error(`"${raw}" is not a number`);
      return n;
    }
    case 'bool':
      if (/^(true|yes|on|1)$/i.test(text)) return true;
      if (/^(false|no|off|0)$/i.test(text)) return false;
      throw new Error(`"${raw}" is not a bool`);
    case 'string':
      return raw;
    case 'enum':
      return { enum: text };
    case 'ref':
      return { ref: text.startsWith('#') ? text : `#${text}` };
    case 'quantity':
      if (!QUANTITY.test(text)) throw new Error(`"${raw}" is not a quantity (write a magnitude and a unit, e.g. 9.58 mW)`);
      return { q: text };
    case 'color':
      return { rgba: parseColor(text) };
    case 'vec2':
    case 'vec3': {
      const v = parseVector(text);
      const n = kind === 'vec2' ? 2 : 3;
      if (v.length !== n) throw new Error(`"${raw}" needs ${n} numbers`);
      return v;
    }
    case 'any': {
      if (QUANTITY.test(text) && !/^(true|false)$/i.test(text)) return { q: text };
      if (/^-?\d/.test(text) && !text.includes(',')) return Number(text);
      if (/^[\[(]?\s*-?\d/.test(text) && text.includes(',')) return parseVector(text);
      if (text.startsWith('#') && text.length >= 7) return { rgba: parseColor(text) };
      if (/^(true|false)$/i.test(text)) return text.toLowerCase() === 'true';
      return raw;
    }
  }
}

function parseVector(text: string): number[] {
  const parts = text.replace(/^[\[(]|[\])]$/g, '').split(/[,\s]+/).filter(Boolean).map(Number);
  if (parts.some(Number.isNaN)) throw new Error(`"${text}" is not a vector`);
  return parts;
}

function parseColor(text: string): number[] {
  const hex = text.match(/^#([0-9a-f]{6})([0-9a-f]{2})?$/i);
  if (hex) {
    const n = parseInt(hex[1], 16);
    const a = hex[2] ? parseInt(hex[2], 16) / 255 : 1;
    return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255, a];
  }
  const fn = text.match(/^rgba?\(([^)]*)\)$/i);
  if (fn) {
    const parts = fn[1].split(/[,\s]+/).filter(Boolean).map(Number);
    if (parts.length >= 3 && !parts.some(Number.isNaN)) {
      const scale = parts.slice(0, 3).some((c) => c > 1) ? 255 : 1;
      return [parts[0] / scale, parts[1] / scale, parts[2] / scale, parts[3] ?? 1];
    }
  }
  throw new Error(`"${text}" is not a colour (use #rrggbb, #rrggbbaa, or rgba(r, g, b, a))`);
}
