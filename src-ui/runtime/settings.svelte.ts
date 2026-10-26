import { isNativeHost } from './native_host';
/**
 * App settings: how this browser runs Iron, never document state. Kept in
 * browser storage. The Settings view (views/html/SettingsView.svelte)
 * changes them; the renderer and runtime restart the app, the render scale
 * does not.
 *
 *   renderScale  canvas pixels per CSS pixel. `auto` is the display's own
 *                ratio. `panel4k` assumes a 3840-px-wide panel running a
 *                scaled ("looks like 2560×1440") macOS mode, where the
 *                browser renders at 2× and macOS shrinks the frame: it
 *                renders one canvas pixel per panel pixel instead. A browser
 *                cannot tell such a display from a native 5K one (both report
 *                2560 wide at 2×), so this is a choice, not a detection.
 *                Applies live, like a move to a screen of another density.
 *   renderer     the 2D vector renderer. `classic` needs a WebGPU build with
 *                the `classic` feature; `?bevy=classic` in the URL wins.
 *   runtime      the app in a worker (default) or on the main thread.
 *   appearance   light or dark: the system's (default), or forced
 *                (runtime/theme.svelte.ts).
 *   developer    debugging tools in the UI: the composition settings menu's
 *                render inspector, which splits a composition tab with a view
 *                of how it renders.
 */
import type { RuntimeMode } from './session_adapter';

export type RenderScale = 'auto' | 'panel4k' | '2' | '1.5' | '1';
export type Renderer = 'hybrid' | 'classic';
export type Appearance = 'system' | 'light' | 'dark';

export interface Settings {
  renderScale: RenderScale;
  renderer: Renderer;
  runtime: RuntimeMode;
  appearance: Appearance;
  developer: boolean;
}

const KEY = 'iron.settings.v1';
const DEFAULTS: Settings = { renderScale: 'auto', renderer: 'hybrid', runtime: 'worker', appearance: 'system', developer: false };

function read(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? { ...DEFAULTS, ...JSON.parse(raw) } : { ...DEFAULTS };
  } catch {
    return { ...DEFAULTS };
  }
}

/** The settings as stored; edit through [`saveSettings`]. */
export const settings: Settings = $state(read());

export function saveSettings(next: Partial<Settings>): void {
  Object.assign(settings, next);
  try {
    localStorage.setItem(KEY, JSON.stringify(settings));
  } catch {
    // Private windows and blocked storage: the setting lasts this page only.
  }
}

/** The physical panel width `panel4k` assumes. */
const PANEL_4K = 3840;

/**
 * Canvas pixels per CSS pixel for `scale`. Never above the display's own
 * ratio for `panel4k`: on a screen already at or under 4K density it changes
 * nothing.
 */
export function pixelRatioFor(scale: RenderScale): number {
  const display = window.devicePixelRatio || 1;
  switch (scale) {
    case 'auto':
      return display;
    case 'panel4k':
      return Math.min(display, PANEL_4K / Math.max(1, window.screen.width));
    default:
      return Number(scale);
  }
}

/** Canvas pixels per CSS pixel now: the setting on the screen the window is
 *  on. Read whenever the canvas is sized or a panel measured; the shell sends
 *  it with each canvas resize so the app's pointer mapping follows
 *  (`PanelManager.refreshPixelRatio`). */
export function pixelRatio(): number {
  return isNativeHost() ? (window.devicePixelRatio || 1) : pixelRatioFor(settings.renderScale);
}

/**
 * Call `onChange` whenever the ratio may have changed under the window: the
 * display's ratio (a move to another screen, browser zoom) or the screen.
 * A window resize is watched separately. Returns a function that stops it.
 */
export function watchPixelRatio(onChange: () => void): () => void {
  let query: MediaQueryList | null = null;
  const listen = () => {
    query?.removeEventListener('change', fire);
    // Matches only the current ratio, so it fires once on any change; then
    // listen for the new one.
    query = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    query.addEventListener('change', fire);
  };
  const fire = () => {
    listen();
    onChange();
  };
  listen();
  const screen = window.screen as Screen & EventTarget;
  screen.addEventListener?.('change', onChange);
  return () => {
    query?.removeEventListener('change', fire);
    screen.removeEventListener?.('change', onChange);
  };
}
