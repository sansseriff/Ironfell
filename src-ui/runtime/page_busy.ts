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

/**
 * `?pageLayout=a-b` (dev, §6g): heavy, uneven layout instead of script time.
 * Off-screen paragraphs of text (laid out, never painted) change width each
 * frame in rAF and are laid out there, a random a–b ms (measured each frame
 * and steered), as a busy view (a chat transcript) would cost.
 */
export function startPageLayout(spec: string | null): (() => void) | null {
  if (!spec) return null;
  const [a, b] = spec.split('-').map(Number);
  const lo = Number.isFinite(a) ? a : 0;
  const hi = Number.isFinite(b) ? b : lo;
  if (!(hi > 0)) return null;
  const box = document.createElement('div');
  box.setAttribute('aria-hidden', 'true');
  box.style.cssText = 'position:fixed;left:-30000px;top:0;width:340px;pointer-events:none;font:13px/1.4 system-ui,sans-serif';
  const words = 'frame layout worker vsync compositor paragraph measure shaping canvas latency interaction pointer timeline document inspector'.split(' ');
  const paras: HTMLElement[] = [];
  for (let i = 0; i < 1500; i++) {
    const p = document.createElement('p');
    let t = '';
    for (let k = 0; k < 60; k++) t += words[(i * 7 + k * 13) % words.length] + (k % 9 === 8 ? '. ' : ' ');
    p.textContent = t;
    p.style.margin = '0';
    // Each paragraph in its own contained box, so a change re-lays out that
    // paragraph only and the cost scales with how many change.
    const cell = document.createElement('div');
    cell.style.cssText = 'contain:strict;width:340px;height:120px;overflow:hidden';
    cell.appendChild(p);
    box.appendChild(cell);
    paras.push(p);
  }
  document.body.appendChild(box);
  let flip = false;
  // Closed loop: the layout is forced and timed here, and the number of
  // paragraphs changed next frame follows the target. It runs in this rAF
  // callback: after the hold in the old order (the hold's callback runs
  // first), before it in the work-first order (the hold's runs last).
  let n = 50;
  let raf = 0;
  const frame = () => {
    const target = lo + Math.random() * (hi - lo);
    flip = !flip;
    const w = flip ? '339px' : '340px';
    const count = Math.min(paras.length, Math.max(1, Math.round(n)));
    const t0 = performance.now();
    for (let i = 0; i < count; i++) paras[i].style.width = w;
    void box.offsetHeight;
    const took = Math.max(0.05, performance.now() - t0);
    n = Math.min(paras.length, Math.max(1, n * Math.min(2, Math.max(0.5, target / took))));
    raf = requestAnimationFrame(frame);
  };
  raf = requestAnimationFrame(frame);
  return () => { cancelAnimationFrame(raf); box.remove(); };
}
