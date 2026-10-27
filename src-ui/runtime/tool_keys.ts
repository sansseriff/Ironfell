/**
 * The letters each view's keymap binds to tools (plans/specs/contexts.md
 * §2.7), as the engine reports them in `focus` events. A plain key bound in
 * any view goes to the engine as a tool key; the engine picks the tool by the
 * keymap of the view under the pointer.
 */
const byView = new Map<string, Set<string>>();

/** Record a view's keymap, `v:select r:rect …`. */
export function setViewKeys(panel: string, keys: string) {
  byView.set(panel, new Set(keys.split(/\s+/).filter(Boolean).map((p) => p.split(':')[0])));
}

export function isToolLetter(key: string): boolean {
  for (const keys of byView.values()) if (keys.has(key)) return true;
  return false;
}
