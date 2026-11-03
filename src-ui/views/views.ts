/**
 * Views (plans/specs/panels-and-session.md): every tab or panel in the workspace is
 * a view, a window onto something in the document drawn in some notation.
 *
 *   source    what it is a view of: a composition (id or name; the timeline
 *             is the session's `timeline` composition), the document, the session's selection, a narrative
 *   notation  how it is drawn
 *
 * A notation decides which part of the view the canvas draws (its Rust panel
 * kind) and which part is HTML (a Svelte component). A view can be all
 * canvas, all HTML, or both: a hybrid lays its HTML over its canvas part, and
 * only the HTML's interactive elements take the pointer. Where a view sits
 * (a tab, a split, later a popover or a menu) is the presenter's business,
 * here dockview's; the view does not change with it.
 */
import type { Component } from 'svelte';
import DocumentView from './html/DocumentView.svelte';
import SelectionView from './html/SelectionView.svelte';
import ModelView from './html/ModelView.svelte';
import SettingsView from './html/SettingsView.svelte';
import CompositionTools from './html/CompositionTools.svelte';
import NarrativeView from './html/NarrativeView.svelte';

export type Notation = 'composition' | 'markup' | 'selection' | 'conversation' | 'narrative' | 'settings';

export interface ViewSpec {
  source: string;
  notation: Notation;
}

export interface NotationDef {
  label: string;
  /** The Rust panel kind for the canvas part, if the canvas draws any. */
  canvas?: string;
  /** The HTML part, if any; `panel` is its view's id. */
  html?: Component<{ spec: ViewSpec; panel?: string }>;
}

export const NOTATIONS: Record<Notation, NotationDef> = {
  composition: { label: 'Composition', canvas: 'composition', html: CompositionTools },
  markup: { label: 'Document', html: DocumentView },
  selection: { label: 'Inspector', html: SelectionView },
  conversation: { label: 'Model', html: ModelView },
  // Flowing text over the scope (plans/specs/narrative-panel.md): source is
  // the narrative's name or id.
  narrative: { label: 'Narrative', html: NarrativeView },
  settings: { label: 'Settings', html: SettingsView },
};

/** The app's settings (views/html/SettingsView.svelte): not a document view. */
export const SETTINGS_VIEW: ViewSpec = { source: 'app', notation: 'settings' };

/** Views that are not compositions, for the "open a view" menu. */
export const FIXED_VIEWS: { title: string; spec: ViewSpec }[] = [
  // A session composition generated from the clips (plans/specs/timeline.md).
  { title: 'Timeline', spec: { source: 'timeline', notation: 'composition' } },
  { title: 'Document', spec: { source: 'document', notation: 'markup' } },
  { title: 'Inspector', spec: { source: 'selection', notation: 'selection' } },
  { title: 'Model', spec: { source: 'document', notation: 'conversation' } },
  { title: 'Settings', spec: SETTINGS_VIEW },
];

/** The id a new view gets: readable, and unique among the open ones. */
export function viewId(spec: ViewSpec, taken: (id: string) => boolean): string {
  const base = `${spec.notation === 'composition' ? '' : spec.notation + '-'}${spec.source}`.replace(/[^a-zA-Z0-9_-]/g, '');
  if (!taken(base)) return base;
  for (let i = 2; ; i++) if (!taken(`${base}-${i}`)) return `${base}-${i}`;
}

/** Title-case a composition name for a tab. */
export function titleOf(name: string): string {
  return name ? name[0].toUpperCase() + name.slice(1) : name;
}
