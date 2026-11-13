/**
 * The shell's half of the command registry (iron_document `commands.rs`,
 * plans/specs/contexts.md §4.2): which chords each view binds, and running a
 * command by id from a key, a menu or a button.
 *
 * Nothing here names a command's key. The engine resolves each view's
 * `keys.commands` (std:workspace's under the view's own) and reports it in
 * `focus` events; a key press that makes a bound chord goes to the engine as
 * `chord:<chord>`, and the engine picks the command by the keymap of the
 * view under the pointer. The registry tells this side what it must do in
 * the key event itself: start a clipboard write while the event is still a
 * user gesture, and leave the key to the browser when nothing is selected.
 */
import type { UiRequest } from './protocol';
import { beginCopy, lastCopied } from './clipboard';

export interface CommandSpec {
  id: string;
  label: string;
  icon: string;
  /** It acts on the selection. */
  selection: boolean;
  clip: 'none' | 'write' | 'read';
  runs: 'engine' | 'shell';
  doc: string;
}

/** A right-click menu's item, as the engine resolves it (`menu` event). */
export interface MenuItem {
  id: string;
  label: string;
  icon: string;
  /** Its chord in the view's keymap, canonical (`mod+alt+g`), or empty. */
  chord: string;
  enabled: boolean;
  /** Why it is disabled, or empty. */
  reason: string;
  clip: string;
  runs: string;
}

const registry = new Map<string, CommandSpec>();
const byView = new Map<string, Map<string, string>>();

/** The registry, from the engine's `commands` call. */
export function setRegistry(list: CommandSpec[]) {
  registry.clear();
  for (const c of list) registry.set(c.id, c);
}

export function commandSpec(id: string): CommandSpec | undefined {
  return registry.get(id);
}

/** Record a view's command keymap, `mod+alt+g:create-control ]:front …` (canonical
 *  chords, as the engine sends them). */
export function setViewCommands(panel: string, keys: string) {
  const map = new Map<string, string>();
  for (const pair of keys.split(/\s+/).filter(Boolean)) {
    const i = pair.indexOf(':');
    if (i > 0) map.set(pair.slice(0, i), pair.slice(i + 1));
  }
  byView.set(panel, map);
}

/** The commands any view binds `chord` to. */
export function commandsFor(chord: string): CommandSpec[] {
  const out: CommandSpec[] = [];
  for (const map of byView.values()) {
    const id = map.get(chord);
    const spec = id ? registry.get(id) : undefined;
    if (spec && !out.includes(spec)) out.push(spec);
  }
  return out;
}

type Post = (data: UiRequest) => void;
type ShellRun = (at: { x: number; y: number } | null) => void | Promise<void>;
const shellCommands = new Map<string, ShellRun>();

/** The shell's own commands (`runs: shell`) register what they do, where
 *  that lives (the dock shows the model's chat). */
export function registerShellCommand(id: string, run: ShellRun) {
  shellCommands.set(id, run);
}

/** The last command that could not run, and why (a `commandFailed` event). */
export const commandFailure: { current: { id: string; reason: string; at: number } | null } = $state({ current: null });

/**
 * Run a command by id, from a menu or a button, inside the user gesture
 * that asked for it. `at` is the canvas point a right-click was at (CSS px),
 * for Paste here.
 */
export async function runCommand(post: Post, id: string, at: { x: number; y: number } | null = null) {
  const spec = registry.get(id);
  if (!spec) { console.warn(`unknown command ${id}`); return; }
  if (spec.runs === 'shell') {
    if (id === 'paste-here') {
      // A menu click is a user gesture, so the clipboard may be read; if it
      // refuses, the tab's own last copy pastes.
      let text = '';
      try { text = await navigator.clipboard.readText(); } catch { /* refused */ }
      text ||= lastCopied();
      if (!text) { commandFailure.current = { id, reason: 'The clipboard is empty', at: performance.now() }; return; }
      post({ ty: 'paste', text, x: at?.x ?? null, y: at?.y ?? null });
      return;
    }
    const run = shellCommands.get(id);
    if (run) await run(at);
    else console.warn(`no shell handler for ${id}`);
    return;
  }
  if (spec.clip === 'write') beginCopy(() => post({ ty: 'command', id }));
  else post({ ty: 'command', id });
}

/** Session-only control placement and visibility, owned by the engine. */
export const controlState: { placing: string; visible: boolean } = $state({ placing: "", visible: true });
