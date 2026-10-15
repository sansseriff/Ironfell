/**
 * Light or dark, for the whole app: the system's appearance (so day/night
 * switching follows it), unless the Settings view forces one. The shell's
 * colours are CSS tokens (app.css) keyed on the same media query and on
 * `data-theme` on <html>; the canvas, which CSS cannot reach, is told
 * through `onThemeChange` (PanelManager sends it to the app).
 */
import { saveSettings, settings, type Appearance } from './settings.svelte';

export const theme = $state({ dark: false });

const listeners = new Set<(dark: boolean) => void>();
let query: MediaQueryList | null = null;

function apply(): void {
  const forced = settings.appearance;
  const dark = forced === 'dark' || (forced === 'system' && !!query?.matches);
  if (forced === 'system') document.documentElement.removeAttribute('data-theme');
  else document.documentElement.setAttribute('data-theme', forced);
  if (theme.dark !== dark) theme.dark = dark;
  for (const l of listeners) l(dark);
}

/** Follow the system appearance; call once at startup. */
export function startTheme(): void {
  query = window.matchMedia('(prefers-color-scheme: dark)');
  query.addEventListener('change', apply);
  apply();
}

/** Force light or dark, or follow the system again. */
export function setAppearance(appearance: Appearance): void {
  saveSettings({ appearance });
  apply();
}

/** Call `fn` now and whenever the theme changes. Returns an unsubscribe. */
export function onThemeChange(fn: (dark: boolean) => void): () => void {
  listeners.add(fn);
  fn(theme.dark);
  return () => listeners.delete(fn);
}
