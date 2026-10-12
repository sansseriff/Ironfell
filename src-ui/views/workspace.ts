/**
 * The workspace: how views are laid out, as dockview tabs and splits
 * (plans/panels-and-session.md, the dock presenter). Layout is workspace
 * state: kept across reloads (browser storage for now; the store's workspace
 * root later), never in the document or its undo.
 */
import "./dockview.css";
import { createDockview, themeLight, type DockviewApi, type IContentRenderer, type IHeaderActionsRenderer } from 'dockview-core';
import { mount, unmount } from 'svelte';
import ViewPanel from './ViewPanel.svelte';
import AddViewMenu from './AddViewMenu.svelte';
import Toolbar from './Toolbar.svelte';
import type { ViewSpec } from './views';

const STORAGE_KEY = 'iron.workspace.v3';

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
  ]) {
    view(id, title, { source: id, notation: 'composition' }, { referencePanel: 'viewer', direction: 'within' }, { inactive: true });
  }
  view('timeline', 'Timeline', { source: 'timeline', notation: 'timeline' }, { direction: 'below' }, { initialHeight: 220 });
  view('document', 'Document', { source: 'document', notation: 'markup' }, { referencePanel: 'viewer', direction: 'left' }, { initialWidth: 380 });
  view('model', 'Model', { source: 'document', notation: 'conversation' }, { referencePanel: 'document', direction: 'within' }, { inactive: true });
  view('selection', 'Inspector', { source: 'selection', notation: 'selection' }, { referencePanel: 'viewer', direction: 'right' }, { initialWidth: 200 });
}

let current: DockviewApi | null = null;

export function createWorkspace(el: HTMLElement): DockviewApi {
  const api = createDockview(el, {
    theme: themeLight,
    // Pointer events drive every drag: HTML5 drag-and-drop is unreliable in
    // Safari and embedded webviews, and in headless test browsers.
    dndStrategy: 'pointer',
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
    createLeftHeaderActionComponent: (group): IHeaderActionsRenderer => {
      const element = document.createElement('div');
      element.style.height = '100%';
      let instance: Record<string, any> | null = null;
      return {
        element,
        init() {
          instance = mount(Toolbar, { target: element, props: { group } });
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
  api.onDidLayoutChange(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(api.toJSON()));
    } catch {
      // storage unavailable: the layout lasts this session only
    }
  });
  return api;
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
