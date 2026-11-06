/**
 * `?pageBusy=a-b` (dev, plans/rendering/worker-frame-hold.md §6g): a stand-in
 * for heavy, uneven UI work on the main thread. Each frame spins for a random
 * a–b ms in rAF, delaying the page's frame as a busy view's work would.
 * `?pageBusy=N` is a fixed N ms. Weak-hardware tests: with `?workerTickBusy=N`
 * for a slow worker.
 *
 * It changes nothing on screen: an earlier version also changed a 1 px
 * element each frame, and that alone moved Chrome to a pipeline one to two
 * frames later for the whole page (measured 2026-11-06, §6g).
 */
export function startPageBusy(spec: string | null): (() => void) | null {
  if (!spec) return null;
  const [a, b] = spec.split('-').map(Number);
  const lo = Number.isFinite(a) ? a : 0;
  const hi = Number.isFinite(b) ? b : lo;
  if (!(hi > 0)) return null;
  let raf = 0;
  const frame = () => {
    const until = performance.now() + lo + Math.random() * (hi - lo);
    while (performance.now() < until) { /* spin */ }
    raf = requestAnimationFrame(frame);
  };
  raf = requestAnimationFrame(frame);
  return () => cancelAnimationFrame(raf);
}
