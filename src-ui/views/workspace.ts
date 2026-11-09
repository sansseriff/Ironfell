/**
 * The workspace: how views are laid out, as dockview tabs and splits
 * (plans/specs/panels-and-session.md, the dock presenter). Layout is workspace
 * state, never in the document or its undo: saved with each scope
 * (`workspace.json`, scopes.md D10 and phase 4), or in browser storage on an
 * unsaved page. The app's own layout holds only what every document has
 * (the Main view, the timeline, the model, library and inspector
 * views; the Document view opens from the add-view menu); a tab onto one of the document's compositions or
 * narratives belongs to the document's layout, so a new scope opens without
 * the demo's, and a scope with no saved layout gets one tab per composition.
 */
import "./dockview.css";
import { createDockview, themeLight, type DockviewApi, type IContentRenderer, type IHeaderActionsRenderer, type ITabRenderer } from 'dockview-core';
import { mount, unmount } from 'svelte';
import ViewPanel from './ViewPanel.svelte';
import ViewTab from './ViewTab.svelte';
import AddViewMenu from './AddViewMenu.svelte';
import ScopeMenu from './ScopeMenu.svelte';
import { dockState } from './dock_state.svelte';
import { LATENCY_VIEW, titleOf, type ViewSpec } from './views';
import { controllerManager } from '../controller-manager.svelte';
import { lastActive, noteActive } from './active_view.svelte';
import type { CompositionEntry, OpenedDocument } from '../runtime/document_client.svelte';

/** The layout every document starts from: the Main view (the `main`
 *  composition and loose nodes; its view id stays `viewer`), the session's
 *  views and the HTML views. The document's own compositions and narratives
 *  are added by `addDocumentViews`. */
function baseLayout(api: DockviewApi) {
  const view = (id: string, title: string, spec: ViewSpec, position?: any, extra: Record<string, unknown> = {}) =>
    api.addPanel({ id, component: 'view', title, params: { spec }, position, ...extra });
  view('viewer', 'Main', { source: 'main', notation: 'composition' });
  // The render benchmark's composition (`?stress=N`), shown in front.
  if (new URLSearchParams(location.search).has('stress')) {
    view('stress', 'Stress', { source: 'stress', notation: 'composition' }, { referencePanel: 'viewer', direction: 'within' });
  }
  // The instance-cache benchmark's (`?meters=N`).
  if (new URLSearchParams(location.search).has('meters')) {
    view('meters', 'Meters', { source: 'meters', notation: 'composition' }, { referencePanel: 'viewer', direction: 'within' });
  }
  // The timeline: the session's generated `timeline` composition
  // (plans/specs/timeline.md), drawn like any other.
  view('timeline', 'Timeline', { source: 'timeline', notation: 'composition' }, { direction: 'below' }, { initialHeight: 220 });
  // The Document view (the document as markup) is not open by default; the
  // add-view menu opens it.
  view('model', 'Model', { source: 'document', notation: 'conversation' }, { referencePanel: 'viewer', direction: 'left' }, { initialWidth: 380 });
  // The standard components (component-authoring.md §6): a view onto the
  // session's library composition; drag one onto a canvas to place it.
  view('library', 'Library', { source: 'library', notation: 'composition' }, { referencePanel: 'model', direction: 'within' }, { inactive: true });
  // The layers panel (plans/specs/layers-panel.md): the front tab on the left,
  // following the composition view used last.
  view('layers', 'Layers', { source: 'follow', notation: 'layers' }, { referencePanel: 'model', direction: 'within', index: 0 });
  view('selection', 'Inspector', { source: 'selection', notation: 'selection' }, { referencePanel: 'viewer', direction: 'right' }, { initialWidth: 280 });
}

/** A readable view id from a name, unique among the open views. */
function freshId(api: DockviewApi, name: string): string {
  const base = name.replace(/[^a-zA-Z0-9_-]/g, '') || 'view';
  if (!api.getPanel(base)) return base;
  for (let i = 2; ; i++) if (!api.getPanel(`${base}-${i}`)) return `${base}-${i}`;
}

function specOf(panel: { params?: unknown }): ViewSpec | undefined {
  return (panel.params as { spec?: ViewSpec } | undefined)?.spec;
}

/** Open a view of the composition `c`. A document composition's view names
 *  it by id, so renaming it (or undoing a rename) keeps the tab; its title
 *  follows the name (`syncTitles`). The session's are named, as fixed. */
export function addCompositionView(api: DockviewApi, c: CompositionEntry, position?: any, inactive = false): string {
  const name = c.name ?? c.id;
  const id = freshId(api, name);
  api.addPanel({ id, component: 'view', title: titleOf(name), params: { spec: { source: c.session ? name : c.id, notation: 'composition' } }, position, inactive });
  return id;
}

/** A tab for each of the document's compositions and narratives that no
 *  view shows yet: a scope's layout before it has saved one. */
async function addDocumentViews(api: DockviewApi) {
  const doc = controllerManager.docClient;
  const shown = new Set(api.panels.map(specOf).filter((v): v is ViewSpec => !!v).map((v) => `${v.notation}:${v.source}`));
  const viewer = api.getPanel('viewer') ? { referencePanel: 'viewer', direction: 'within' } : undefined;
  for (const c of doc.compositions) {
    if (c.session || c.name === 'main') continue;
    if (shown.has(`composition:${c.id}`) || (c.name && shown.has(`composition:${c.name}`))) continue;
    addCompositionView(api, c, viewer, true);
  }
  let narratives: { id: string; name: string }[] = [];
  try {
    const r = await doc.command<{ ok: boolean; narratives: { id: string; name: string }[] }>({ op: 'narrative_list' });
    narratives = r.ok ? r.narratives : [];
  } catch {
    narratives = [];
  }
  const left = api.getPanel('document') ? 'document' : api.getPanel('model') ? 'model' : undefined;
  const beside = left ? { referencePanel: left, direction: 'within' as const } : undefined;
  // The first narrative opens in front: a fresh layout's left group shows
  // the writing view (prose and the chat composer), Model and Library beside it.
  let front = ![...shown].some((k) => k.startsWith('narrative:'));
  for (const n of narratives) {
    const source = n.name || n.id;
    if (shown.has(`narrative:${source}`)) continue;
    // A narrative's view (plans/specs/narrative-panel.md) beside the Model's.
    api.addPanel({ id: freshId(api, source), component: 'view', title: titleOf(source), params: { spec: { source, notation: 'narrative' } }, position: beside && front ? { ...beside, index: 0 } : beside, inactive: !front });
    front = false;
  }
  // Main stays the focused view; the narrative is only its group's front tab.
  api.getPanel('viewer')?.api.setActive();
}

/** Tab titles follow their compositions' names. */
function syncTitles(api: DockviewApi, list: CompositionEntry[]) {
  for (const p of api.panels) {
    const spec = specOf(p);
    if (spec?.notation !== 'composition' || !spec.source.startsWith('#')) continue;
    const c = list.find((e) => e.id === spec.source);
    const title = c?.name ? titleOf(c.name) : null;
    if (title && p.api.title !== title) p.api.setTitle(title);
  }
}

/**
 * `?latency=<layout>` (plans/rendering/latency-view.md): open the Latency view
 * in a fixed layout for repeatable measurements, and never save the layout
 * over the scope's own.
 *   solo    the Latency view alone
 *   panels  the Model view left, the Latency view, the Inspector right
 *   full    the app's usual views, with the Latency view in front of Main
 */
const latencyLayout = new URLSearchParams(location.search).get('latency');

function arrangeLatency(api: DockviewApi, layout: string) {
  const view = (id: string, title: string, spec: ViewSpec, position?: any, extra: Record<string, unknown> = {}) =>
    api.addPanel({ id, component: 'view', title, params: { spec }, position, ...extra });
  if (layout === 'full') {
    if (!api.getPanel('latency')) view('latency', 'Latency', LATENCY_VIEW, api.getPanel('viewer') ? { referencePanel: 'viewer', direction: 'within' } : undefined);
    api.getPanel('latency')?.api.setActive();
    // `&without=timeline,library,…`: close some of them (latency-view.md §7).
    const without = (new URLSearchParams(location.search).get('without') ?? '').split(',').filter(Boolean);
    // `without=all`: every other tab, closed one by one (not `api.clear()`).
    for (const p of [...api.panels]) if (p.id !== 'latency' && (without.includes('all') || without.includes(p.id))) p.api.close();
    return;
  }
  api.clear();
  view('latency', 'Latency', LATENCY_VIEW);
  // `&with=timeline,library,…`: add some of the app's views beside it, to find
  // which of them changes the measurement (latency-view.md §7).
  const extras: Record<string, [string, ViewSpec]> = {
    viewer: ['Main', { source: 'main', notation: 'composition' }],
    timeline: ['Timeline', { source: 'timeline', notation: 'composition' }],
    document: ['Document', { source: 'document', notation: 'markup' }],
    library: ['Library', { source: 'library', notation: 'composition' }],
  };
  for (const key of (new URLSearchParams(location.search).get('with') ?? '').split(',').filter((k) => k in extras)) {
    const [title, spec] = extras[key];
    view(key, title, spec, { referencePanel: 'latency', direction: 'below' }, { initialHeight: 200 });
  }
  if (layout === 'panels') {
    view('model', 'Model', { source: 'document', notation: 'conversation' }, { referencePanel: 'latency', direction: 'left' }, { initialWidth: 380 });
    view('selection', 'Inspector', { source: 'selection', notation: 'selection' }, { referencePanel: 'latency', direction: 'right' }, { initialWidth: 300 });
  }
}

let current: DockviewApi | null = null;
/** Whose layout the dock shows: a scope's id, `page` for an unsaved page,
 *  null until the first document is open. */
let openedKey: string | null = null;
/** Laying out programmatically: not the person's change, not saved yet. */
let applying = 0;
let saveTimer: ReturnType<typeof setTimeout> | null = null;
let unwatch: (() => void)[] = [];

export function createWorkspace(el: HTMLElement): DockviewApi {
  const api = createDockview(el, {
    theme: themeLight,
    // Pointer events drive every drag: HTML5 drag-and-drop is unreliable in
    // Safari and embedded webviews, and in headless test browsers.
    dndStrategy: 'pointer',
    // Every tab is a ViewTab: icon, title, close (VS Code's look).
    defaultTabComponent: 'view',
    createTabComponent: (): ITabRenderer => {
      const element = document.createElement('div');
      element.style.height = '100%';
      let instance: Record<string, any> | null = null;
      return {
        element,
        init(params) {
          const spec = (params.params as { spec?: ViewSpec } | undefined)?.spec;
          instance = mount(ViewTab, { target: element, props: { api: params.api, notation: spec?.notation, spec } });
        },
        dispose() {
          if (instance) unmount(instance);
          instance = null;
        },
      };
    },
    createComponent: (): IContentRenderer => {
      const element = document.createElement('div');
      element.style.width = '100%';
      element.style.height = '100%';
      let instance: Record<string, any> | null = null;
      return {
        element,
        init(params) {
          instance = mount(ViewPanel, {
            target: element,
            props: { id: params.api.id, spec: (params.params as { spec: ViewSpec }).spec, api: params.api },
          });
        },
        dispose() {
          if (instance) unmount(instance);
          instance = null;
        },
      };
    },
    // The scope menu (scopes.md §6): offered to every group, shown by the
    // one at the top left.
    createPrefixHeaderActionComponent: (group): IHeaderActionsRenderer => {
      const element = document.createElement('div');
      element.style.height = '100%';
      let instance: Record<string, any> | null = null;
      return {
        element,
        init() {
          instance = mount(ScopeMenu, { target: element, props: { api, group } });
        },
        dispose() {
          if (instance) unmount(instance);
          instance = null;
        },
      };
    },
    createRightHeaderActionComponent: (group): IHeaderActionsRenderer => {
      const element = document.createElement('div');
      let instance: Record<string, any> | null = null;
      return {
        element,
        init() {
          instance = mount(AddViewMenu, { target: element, props: { api, group } });
        },
        dispose() {
          if (instance) unmount(instance);
          instance = null;
        },
      };
    },
  });
  current = api;

  // The one layout every scope used to share, before layouts were per scope.
  try { localStorage.removeItem('iron.workspace.v8'); } catch { /* nothing saved */ }
  // The app's views at once; the document's once it is open (`open`).
  applying++;
  baseLayout(api);
  applying--;
  api.onDidLayoutChange(scheduleSave);
  // A restored layout may hide some groups' tab bars.
  api.onDidLayoutChange(syncHiddenHeaders);
  // The layers panel follows the composition view used last (layers-panel.md §4).
  api.onDidActivePanelChange(({ panel }) => {
    if (!panel) return;
    const spec = specOf(panel);
    noteActive(panel.id, spec?.notation === 'composition' ? spec.source : null);
  });
  const doc = controllerManager.docClient;
  unwatch = [
    doc.onDocumentOpened((o) => void open(api, o)),
    doc.watchCompositions((list) => syncTitles(api, list)),
  ];
  return api;
}

/** The dock is going away (Workspace.svelte). */
export function destroyWorkspace() {
  for (const stop of unwatch) stop();
  unwatch = [];
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = null;
  current = null;
  openedKey = null;
}

/** Lay out the document just opened: its saved layout, or the app's views
 *  plus a tab for each of its compositions and narratives. A settings
 *  restart reopening the same scope leaves the dock alone. */
async function open(api: DockviewApi, o: OpenedDocument) {
  if (o.key === openedKey) return;
  const first = openedKey === null;
  openedKey = o.key;
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = null;
  applying++;
  try {
    let restored = false;
    if (o.saved) {
      try {
        api.fromJSON(JSON.parse(o.saved));
        restored = api.panels.length > 0;
      } catch (e) {
        console.warn('workspace: saved layout unreadable, using the default', e);
      }
    }
    if (!restored) {
      if (!first || o.saved) {
        api.clear();
        baseLayout(api);
      }
      await addDocumentViews(api);
    }
    syncTitles(api, controllerManager.docClient.compositions);
    if (latencyLayout && first) arrangeLatency(api, latencyLayout);
  } finally {
    applying--;
  }
  if (!o.saved) saveNow();
}

function scheduleSave() {
  if (applying || openedKey === null) return;
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(saveNow, 300);
}

function saveNow() {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = null;
  if (!current || openedKey === null || latencyLayout) return;
  controllerManager.docClient.writeWorkspace(openedKey, JSON.stringify(current.toJSON()));
}

/** Open or close a composition view's render inspector, drawn in the same
 *  tab beside it (plans/specs/render-inspector.md): `share` is the view's
 *  part of the width, `undefined` closes it. Kept with the layout. */
export function setInspecting(panel: string, share: number | undefined) {
  current?.getPanel(panel)?.api.updateParameters({ inspect: share });
  scheduleSave();
}

/** Show or hide a composition view's FPS readout (its lower right corner,
 *  off by default). Kept with the layout. */
export function setShowFps(panel: string, on: boolean) {
  current?.getPanel(panel)?.api.updateParameters({ fps: on || undefined });
  scheduleSave();
}

/** Close every render inspector (developer mode turned off). */
export function closeRenderInspectors() {
  for (const p of current?.panels ?? []) {
    if ((p.params as { inspect?: number } | undefined)?.inspect !== undefined) setInspecting(p.id, undefined);
  }
}

/** Forget the open document's layout and start again: the app's views
 *  and a tab for each of the document's compositions and narratives. */
export async function resetWorkspace() {
  if (!current || openedKey === null) return;
  const api = current;
  controllerManager.docClient.forgetWorkspace(openedKey);
  applying++;
  try {
    api.clear();
    baseLayout(api);
    await addDocumentViews(api);
    syncTitles(api, controllerManager.docClient.compositions);
  } finally {
    applying--;
  }
  saveNow();
}

/** Show the composition `source` (a name or id): its open view becomes the
 *  active tab, or a new view opens beside the main viewer. A narrative's
 *  view block does this (plans/specs/narrative-panel.md §1). Returns the
 *  view's id, or null with no workspace. */
export function showComposition(source: string): string | null {
  if (!current || !source) return null;
  const c = controllerManager.docClient.compositions.find((e) => e.id === source || e.name === source);
  // With two tabs on one composition, the one used last (layers-panel.md §5).
  const existing = current.panels.filter((p) => {
    const spec = specOf(p);
    return spec?.notation === 'composition' && (spec.source === source || (!!c && (spec.source === c.id || spec.source === c.name)));
  }).sort((a, b) => (lastActive.get(b.id) ?? 0) - (lastActive.get(a.id) ?? 0))[0];
  if (existing) {
    existing.api.setActive();
    return existing.id;
  }
  const viewer = current.getPanel('viewer') ? { referencePanel: 'viewer', direction: 'within' } : undefined;
  if (c) return addCompositionView(current, c, viewer);
  const id = freshId(current, source);
  current.addPanel({ id, component: 'view', title: titleOf(source), params: { spec: { source, notation: 'composition' } }, position: viewer });
  return id;
}

/** The views onto narratives (their sources), for the driver. */
export function openNarratives(): string[] {
  return (current?.panels ?? []).map((p) => (p.params as { spec?: ViewSpec } | undefined)?.spec).filter((s): s is ViewSpec => s?.notation === 'narrative').map((s) => s.source);
}

/** A view's spec as the dock holds it now (a pinned layers view rewrites its
 *  own), or undefined for no such view. */
export function viewSpec(panel: string): ViewSpec | undefined {
  const p = current?.getPanel(panel);
  return p ? specOf(p) : undefined;
}

/** Replace a view's spec, kept with the layout: the layers panel's pin
 *  (layers-panel.md §4). */
export function setViewSpec(panel: string, spec: ViewSpec) {
  current?.getPanel(panel)?.api.updateParameters({ spec });
  scheduleSave();
}

/** The open view of a composition (id or name) used last, if any. */
export function viewShowing(source: string): string | null {
  if (!current) return null;
  const c = controllerManager.docClient.compositions.find((e) => e.id === source || e.name === source);
  const views = current.panels.filter((p) => {
    const spec = specOf(p);
    return spec?.notation === 'composition' && (spec.source === source || (!!c && (spec.source === c.id || spec.source === c.name)));
  }).sort((a, b) => (lastActive.get(b.id) ?? 0) - (lastActive.get(a.id) ?? 0));
  return views[0]?.id ?? null;
}

/** Hide or show a group's tab bar (a person's choice, saved with the
 *  layout as dockview's `hideHeader`): a hidden one leaves the group's
 *  views a grab handle to switch, drag and show the bar again. */
export function setTabBarHidden(groupId: string, hidden: boolean) {
  const g = current?.groups.find((x) => x.id === groupId);
  if (!g) return;
  g.header.hidden = hidden;
  syncHiddenHeaders();
  scheduleSave();
}

/** Which groups' tab bars are hidden, for views to react to. */
export function syncHiddenHeaders() {
  dockState.hidden = (current?.groups ?? []).filter((g) => g.header.hidden).map((g) => g.id);
}

/** The dock, for views that change what it shows (the view strip). */
export function dockApi(): DockviewApi | null {
  return current;
}

/**
 * Show a strip item's view (panel-primitives.md §5) from the panel `from`:
 * in `from`'s group, opening it there if it is not open and moving it there
 * if it is open but hidden elsewhere; a view already showing in another
 * group is focused where it is. `split` opens it in a split below `from`'s
 * group instead, so two show at once.
 */
export function showStripView(from: { group: { id: string } }, spec: ViewSpec, id: string | undefined, title: string, split: boolean) {
  const api = current;
  if (!api) return;
  const group = api.groups.find((g) => g.id === from.group.id);
  if (!group) return;
  const existing = api.panels.find((p) => {
    const s = specOf(p);
    return s?.notation === spec.notation && s.source === spec.source;
  });
  const freshPanel = (position: any) =>
    api.addPanel({ id: id && !api.getPanel(id) ? id : freshId(api, title), component: 'view', title, params: { spec }, position });
  if (existing && existing.group.id !== group.id && existing.api.isVisible) {
    existing.api.setActive();
    return;
  }
  if (split) {
    if (existing) existing.api.moveTo({ group, position: 'bottom' });
    else freshPanel({ referenceGroup: group, direction: 'below' });
    return;
  }
  if (existing) {
    if (existing.group.id !== group.id) existing.api.moveTo({ group });
    existing.api.setActive();
  } else {
    freshPanel({ referenceGroup: group, direction: 'within' });
  }
}
