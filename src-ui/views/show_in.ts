/**
 * Show in (plans/specs/layers-panel.md §5): go to a picture in its
 * composition. Brings the composition's tab to the front, or opens one;
 * selects the picture there, with its chain, as the select tool would; and
 * frames it. The layers panel's chips and the Inspector's Show in both call
 * this.
 */
import { controllerManager } from '../controller-manager.svelte';
import { showComposition } from './workspace';

export async function showIn(target: { node: string; composition: string | null; chain?: string }) {
  const doc = controllerManager.docClient;
  const panel = target.composition ? showComposition(target.composition) : null;
  const items = [{ node: target.node, panel: panel ?? undefined, chain: target.chain }];
  await doc.command({ op: 'select', items_json: JSON.stringify(items), mode: 'replace' });
  await doc.command({ op: 'focus', node: target.node });
  // A tab opened just now has no view in the engine until its canvas posts
  // a rectangle; frame it again once it has one.
  setTimeout(() => void doc.command({ op: 'focus', node: target.node }).catch(() => {}), 300);
}
