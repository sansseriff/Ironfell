/**
 * The workspace: how views are laid out, as dockview tabs and splits
 * (plans/specs/panels-and-session.md, the dock presenter). Layout is workspace
 * state: kept across reloads (browser storage for now; the store's workspace
 * root later), never in the document or its undo.
 */
import "./dockview.css";
import { createDockview, themeLight, type DockviewApi, type IContentRenderer, type IHeaderActionsRenderer, type ITabRenderer } from 'dockview-core';
import { mount, unmount } from 'svelte';
import ViewPanel from './ViewPanel.svelte';
import ViewTab from './ViewTab.svelte';
import AddViewMenu from './AddViewMenu.svelte';
import type { ViewSpec } from './views';

const STORAGE_KEY = 'iron.workspace.v6';

/** The layout to start from: the main viewer with each composition in a
 *  tab beside it, HTML views left and right, the timeline across the bottom.
 *  The viewer's view id stays `viewer` (Rust draws loose nodes there). */
function defaultLayout(api: DockviewApi) {
  const view = (id: string, title: string, spec: ViewSpec, position?: any, extra: Record<string, unknown> = {}) =>
    api.addPanel({ id, component: 'view', title, params: { spec }, position, ...extra });
  view('viewer', 'Main', { source: 'main', notation: 'composition' });
  for (const [id, title] of [
    ['scene', 'Scene'],
    ['pendulum', 'Pendulum'],
    ['controls', 'Controls'],
    ['assembly', 'Assembly'],
    ['card', 'Card'],
    ['optics', 'Optics'],
    ['schematic', 'Schematic'],
    ['thermal', 'Thermal'],
    ['piston', 'Piston'],
    ['components', 'Components'],
  ]) {
    view(id, title, { source: id, notation: 'composition' }, { referencePanel: 'viewer', direction: 'within' }, { inactive: true });
  }
  // The render benchmark's composition (`?stress=N`), shown in front.
  if (new URLSearchParams(location.search).has('stress')) {
    view('stress', 'Stress', { source: 'stress', notation: 'composition' }, { referencePanel: 'viewer', direction: 'within' });
  }
  // The instance-cache benchmark's (`?meters=N`).
  if (new URLSearchParams(location.search).has('meters')) {
    view('meters', 'Meters', { source: 'meters', notation: 'composition' }, { referencePanel: 'viewer', direction: 'within' });
  }
  view('timeline', 'Timeline', { source: 'timeline', notation: 'timeline' }, { direction: 'below' }, { initialHeight: 220 });
  // Frame rate, renderer, build and canvas size (free-running.md §4): the
  // session's stats composition, where the FPS overlay used to sit.
  view('stats', 'Stats', { source: 'stats', notation: 'composition' }, { referencePanel: 'timeline', direction: 'right' }, { initialWidth: 360 });
  view('document', 'Document', { source: 'document', notation: 'markup' }, { referencePanel: 'viewer', direction: 'left' }, { initialWidth: 380 });
  view('model', 'Model', { source: 'document', notation: 'conversation' }, { referencePanel: 'document', direction: 'within' }, { inactive: true });
  // The standard components (component-authoring.md §6): a view onto the
  // session's library composition; drag one onto a canvas to place it.
  view('library', 'Library', { source: 'library', notation: 'composition' }, { referencePanel: 'document', direction: 'within' }, { inactive: true });
  view('selection', 'Inspector', { source: 'selection', notation: 'selection' }, { referencePanel: 'viewer', direction: 'right' }, { initialWidth: 280 });
}

let current: DockviewApi | null = null;

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
          const notation = (params.params as { spec?: ViewSpec } | undefined)?.spec?.notation;
          instance = mount(ViewTab, { target: element, props: { api: params.api, notation } });
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

  let restored = false;
  const fresh = new URLSearchParams(location.search).get('layout') === 'default';
  try {
    const saved = fresh ? null : localStorage.getItem(STORAGE_KEY);
    if (saved) {
      api.fromJSON(JSON.parse(saved));
      restored = api.panels.length > 0;
    }
  } catch (e) {
    console.warn('workspace: saved layout unreadable, using the default', e);
  }
  if (!restored) {
    api.clear();
    defaultLayout(api);
  }
  api.onDidLayoutChange(save);
  return api;
}

function save() {
  if (!current) return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(current.toJSON()));
  } catch {
    // storage unavailable: the layout lasts this session only
  }
}

/** Open or close a composition view's render inspector, drawn in the same
 *  tab beside it (plans/specs/render-inspector.md): `share` is the view's
 *  part of the width, `undefined` closes it. Kept with the layout. */
export function setInspecting(panel: string, share: number | undefined) {
  current?.getPanel(panel)?.api.updateParameters({ inspect: share });
  save();
}

/** Close every render inspector (developer mode turned off). */
export function closeRenderInspectors() {
  for (const p of current?.panels ?? []) {
    if ((p.params as { inspect?: number } | undefined)?.inspect !== undefined) setInspecting(p.id, undefined);
  }
}

/** Forget the saved layout and start again from the default. */
export function resetWorkspace() {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // nothing saved
  }
  if (current) {
    current.clear();
    defaultLayout(current);
  }
}
