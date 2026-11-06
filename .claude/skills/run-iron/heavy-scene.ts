/**
 * A heavy scene under interaction, in headed Chrome (real vsync, so the frame
 * modes behave as for a person): the Prussia coat of arms (copy it to
 * .agents/tmp/big.svg) imported into the default layout's main view, fitted,
 * then idle, a hover sweep, a click to select, a drag of the selection, and a
 * wheel pan. For each phase it prints the frame rate and worker tick (cadence
 * probe), the engine's per-phase CPU time (`__iron.timings`, src/perf.rs),
 * the raster tile cache's counts, and the frame mode's counts and changes.
 *
 *   bun run heavy-scene.ts
 *
 * TAB (a dock tab's title: import there instead of the main view),
 * URL (default http://127.0.0.1:5185/Ironfell/?demo), EXTRA (more URL
 * parameters, e.g. `&frameHold=off`), CHROME_WINDOW, OUT (a JSON of everything).
 */
import { writeFileSync } from 'node:fs';
import { launchChrome, powerState } from './latency-chrome';

const url = (process.env.URL ?? 'http://127.0.0.1:5185/Ironfell/?demo') + (process.env.EXTRA ?? '');
const { context, close } = await launchChrome();
const results: Record<string, unknown> = { url, power: powerState() };
try {
  const page = context.pages()[0] ?? await context.newPage();
  page.on('pageerror', (e) => console.log('pageerror', String(e)));
  await page.goto(url);
  await page.waitForFunction(() => !!(window as any).__iron?.selection, {}, { timeout: 60000 });
  await page.bringToFront();
  await page.waitForTimeout(2000);
  results.build = await page.evaluate(async () => (await (window as any).__iron.call('frame_stats', [])) ?? null).catch(() => null);
  // TAB=<title>: import into and interact with that dock tab's view (e.g.
  // Pendulum, Controls) instead of the main view, which shows several
  // compositions through embeds.
  let panelId = 'viewer';
  if (process.env.TAB) {
    await page.locator('.dv-tab', { hasText: new RegExp(`^\\s*${process.env.TAB}\\s*$`) }).first().click();
    await page.waitForTimeout(800);
    panelId = await page.evaluate(() => {
      const panels = [...document.querySelectorAll('[data-panel-id]')].filter((e) => { const r = e.getBoundingClientRect(); return r.width > 100 && r.height > 100; });
      return panels.map((e) => ({ id: e.getAttribute('data-panel-id')!, a: e.getBoundingClientRect().width * e.getBoundingClientRect().height })).sort((a, b) => b.a - a.a)[0]?.id ?? 'viewer';
    });
    console.log(`tab ${process.env.TAB}: panel ${panelId}`);
  }
  const viewer = await page.evaluate((id) => { const r = document.querySelector(`[data-panel-id="${id}"]`)!.getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; }, panelId);
  const at = (fx: number, fy: number) => ({ x: viewer.x + viewer.w * fx, y: viewer.y + viewer.h * fy });
  // Import, as a paste into the main view.
  const c = at(0.5, 0.5);
  await page.mouse.move(c.x, c.y);
  const t0 = Date.now();
  const imported = await page.evaluate(async () => {
    const text = await (await fetch('/Ironfell/.agents/tmp/big.svg')).text();
    const r = await (window as any).__iron.importSvg(text);
    return { ok: r.ok, nodes: r.nodes, definitions: r.definitions, references: r.references };
  });
  console.log(`import ${JSON.stringify(imported)} in ${Date.now() - t0} ms`);
  results.import = { ...imported, ms: Date.now() - t0 };
  await page.waitForTimeout(4000);
  await page.keyboard.press('Escape');
  await page.waitForTimeout(300);
  await page.keyboard.press('Escape');
  await page.waitForTimeout(300);
  await page.keyboard.press('Shift+Digit1');
  await page.waitForTimeout(3000);

  const reset = () => page.evaluate(async () => {
    const w = window as any;
    w.__probeReset?.();
    await w.__iron.timings(true);
    w.__frameHold?.reset();
  });
  const collect = (label: string) => page.evaluate(async (label) => {
    const w = window as any;
    w.__lastProbeStats = null;
    w.__probe();
    for (let i = 0; i < 40 && !w.__lastProbeStats; i++) await new Promise((r) => setTimeout(r, 25));
    const s = w.__lastProbeStats ?? {};
    const t = await w.__iron.timings(true);
    const ph = (p: string) => { const x = t.phases[p]; return x && x.n ? { n: x.n, p50: x.p50, p95: x.p95, max: x.max, total: x.total } : null; };
    const h = w.__frameHold?.stats() ?? null;
    return {
      label,
      fps: s.effectiveFps, tick: s.tickMs, raf: s.rafDeltaMs, rafBuckets: s.rafDeltaBuckets,
      phases: Object.fromEntries(['frame', 'changed_frame', 'sync', 'gesture_reconcile', 'display_list', 'extract', 'encode', 'overlay_encode', 'render', 'bake', 'pick', 'overlay_geometry', 'bounds', 'propagate'].map((p) => [p, ph(p)])),
      counts: { commands: t.commands, culled: t.culled, drawn: t.drawn, bounds_updated: t.bounds_updated, pick_candidates: t.pick_candidates },
      raster: t.raster, 
      hold: h && { mode: h.mode, pending: h.pending, held: h.held, released: h.released, skipped: h.skipped, noRoom: h.noRoom, afterP90: h.afterMs?.p90, events: h.events?.slice(-4), skips: h.skips?.slice(-3) },
      frameMode: document.querySelector('[data-frame-mode]')?.getAttribute('data-frame-mode') ?? null,
    };
  }, label);

  const phases: unknown[] = [];
  const run = async (label: string, act: () => Promise<void>) => {
    await reset();
    await act();
    const r = await collect(label);
    phases.push(r);
    const p = (r as any).phases;
    const f = (x: any) => (x ? `${x.p50}/${x.p95}` : '-');
    console.log(`${label.padEnd(12)} fps ${(r as any).fps} tick p50/p95 ${(r as any).tick?.p50}/${(r as any).tick?.p95} | frame ${f(p.frame)} changed ${f(p.changed_frame)} display_list ${f(p.display_list)} encode ${f(p.encode)} overlay ${f(p.overlay_encode)} bake ${f(p.bake)} render ${f(p.render)} pick ${f(p.pick)} sync ${f(p.sync)} gesture ${f(p.gesture_reconcile)} bounds ${f(p.bounds)} | mode ${(r as any).hold?.mode} held ${(r as any).hold?.held}/${(r as any).hold?.pending}`);
  };
  const sweep = async (ms: number, path: (u: number) => { x: number; y: number }) => {
    const steps = Math.round(ms / 8);
    for (let i = 0; i <= steps; i++) { const p = path(i / steps); await page.mouse.move(p.x, p.y); await page.waitForTimeout(8); }
  };

  await run('idle', async () => { await page.waitForTimeout(2500); });
  await run('hover', async () => sweep(3000, (u) => at(0.2 + 0.6 * u, 0.3 + 0.4 * Math.sin(u * Math.PI * 3) * 0.5 + 0.2)));
  await run('select', async () => {
    const p = at(0.5, 0.45);
    await page.mouse.move(p.x, p.y); await page.waitForTimeout(200);
    await page.mouse.down(); await page.waitForTimeout(120); await page.mouse.up();
    await page.waitForTimeout(1500);
  });
  await run('drag', async () => {
    const p = at(0.5, 0.45);
    await page.mouse.move(p.x, p.y); await page.waitForTimeout(200);
    await page.mouse.down(); await page.waitForTimeout(100);
    await sweep(3000, (u) => ({ x: p.x + 120 * Math.sin(u * Math.PI * 2), y: p.y + 60 * Math.sin(u * Math.PI * 4) }));
    await page.mouse.up(); await page.waitForTimeout(300);
  });
  await run('pan', async () => {
    const p = at(0.5, 0.5);
    await page.mouse.move(p.x, p.y);
    for (let i = 0; i < 120; i++) { await page.mouse.wheel(i < 60 ? 6 : -6, i < 60 ? 4 : -4); await page.waitForTimeout(16); }
    await page.waitForTimeout(300);
  });
  results.phases = phases;
  if (process.env.OUT) writeFileSync(process.env.OUT, JSON.stringify(results, null, 2));
} finally {
  await close();
}
