/**
 * The registry schema as `schema` returns it: what the manifest lists.
 * Attributes are read against it in the engine (iron_document::markup).
 */
export type Kind = 'number' | 'bool' | 'string' | 'color' | 'vec2' | 'vec3' | 'enum' | 'ref' | 'quantity' | 'record' | 'geometry' | 'series' | 'function' | 'any';

export interface Schema {
  facets: Record<string, Record<string, Kind>>;
  /** The values of each enum leaf that lists them, by `facet.field`. */
  options?: Record<string, string[]>;
  /** What a context's rules may declare (plans/specs/contexts.md). */
  contexts?: { field: string; options: string[]; doc: string }[];
  /** The standard components a model may place as `std:<key>`, with their ports. */
  standard?: { key: string; ports: { name: string; dir: string; kind: string; default: string | null }[] }[];
  types: Record<
    string,
    {
      facets: { kind: string; required: boolean }[];
      default_host: string;
      children: 'none' | 'any' | string[];
    }
  >;
}
