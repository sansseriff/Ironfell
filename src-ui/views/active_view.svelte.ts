/**
 * Which composition view was used last (plans/specs/layers-panel.md §4): the
 * layers panel follows it. The dock sets it when a composition tab becomes
 * active (workspace.ts); a selection made in a view sets it too
 * (LayersView), since clicking a canvas does not activate its tab.
 */
export const activeView: { panel: string | null; source: string | null } = $state({ panel: null, source: null });

/** When each view was last active, for "the most recent tab of this
 *  composition" (layers-panel.md §5). */
export const lastActive = new Map<string, number>();
let tick = 0;

export function noteActive(panel: string, source: string | null) {
  lastActive.set(panel, ++tick);
  if (source !== null) {
    activeView.panel = panel;
    activeView.source = source;
  }
}
