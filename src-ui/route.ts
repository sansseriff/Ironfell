/**
 * The app's two pages (plans/specs/scopes.md §10.2): the **scopes page**,
 * which lists the stored scopes and needs only browser storage, and the
 * **app**, the engine and its dock, opened on one scope. One static SPA, so
 * the route is in the query: `?scope=<id>` (or `?new=empty|demo`, or
 * `?scope=last`) is the app; no scope is the scopes page. Pages that are not
 * saved (`?demo`, the benchmarks, the native build, no OPFS) are always the
 * app, as before scopes had a page. Other parameters (`gfx`, `bevy`,
 * `showcase`, …) carry over between the pages.
 */
import { isNativeHost } from './runtime/native_host';
import { opfsAvailable } from './runtime/scope_storage';

/** Parameters that say which scope the app opens, dropped between pages. */
const ROUTE_KEYS = ['scope', 'new', 'export'];

/** Whether this page keeps its work in a scope (OPFS). */
export function persistent(): boolean {
  const q = new URLSearchParams(location.search);
  if (q.has('demo') || Number(q.get('stress')) > 0 || Number(q.get('meters')) > 0) return false;
  return !isNativeHost() && opfsAvailable();
}

export type Page = 'scopes' | 'app';

export function page(): Page {
  const q = new URLSearchParams(location.search);
  if (!persistent() || q.get('latencyFilm') === '1') return 'app';
  return q.has('scope') || q.has('new') ? 'app' : 'scopes';
}

/** What the app opens, from its URL. */
export type ScopeTarget = { open: string } | { create: 'empty' | 'demo' } | { last: true };

export function target(): ScopeTarget {
  const q = new URLSearchParams(location.search);
  const kind = q.get('new');
  if (kind === 'empty' || kind === 'demo') return { create: kind };
  const id = q.get('scope');
  return id && id !== 'last' ? { open: id } : { last: true };
}

function url(set: Record<string, string>): string {
  const q = new URLSearchParams(location.search);
  for (const k of ROUTE_KEYS) q.delete(k);
  for (const [k, v] of Object.entries(set)) q.set(k, v);
  const s = q.toString().replace(/=(&|$)/g, '$1');
  return `${location.pathname}${s ? `?${s}` : ''}`;
}

/** The scopes page. */
export const scopesUrl = () => url({});
/** The app on a stored scope; `exporting` downloads it once open. */
export const scopeUrl = (id: string, exporting = false) => url(exporting ? { scope: id, export: '' } : { scope: id });
/** The app making a new scope. */
export const newScopeUrl = (kind: 'empty' | 'demo') => url({ new: kind });

/** Point the address bar at the open scope without a navigation. */
export function showScope(id: string) {
  const next = scopeUrl(id);
  if (`${location.pathname}${location.search}` !== next) history.replaceState(history.state, '', next);
}
