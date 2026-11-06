/**
 * Which views change the DOM, and cost layout, every frame while the timeline
 * plays (plans/rendering/worker-frame-hold.md §6f). Plays the demo square on a
 * loop with no input, then:
 *   1. records DOM mutations for 2 s and groups them by dock panel;
 *   2. reads Chrome's layout and style-recalc time (CDP Performance metrics)
 *      for 3 s with every panel shown, then with each panel's content hidden.
 *
 *   CHROME_WINDOW=3008,1660 bun run latency-churn.ts
 *
 * SELECT=1: select the square first, so the Inspector follows it.
 */
import { launchChrome } from './latency-chrome';

const url = process.env.URL ?? 'http://127.0.0.1:5185/Ironfell/?demo';
const { context, close } = await launchChrome();
try {
  const page = context.pages()[0] ?? await context.newPage();
  await page.goto(url);
  await page.waitForFunction(() => !!(window as any).__iron?.selection, {}, { timeout: 60000 });
  await page.bringToFront();
  await page.waitForTimeout(2000);
  await page.evaluate(async () => {
    const iron = (window as any).__iron;
    await iron.call('clock_set', [JSON.stringify({ loop_in: 0.5, loop_out: 2.5, rate: 4, t: 0.5, playing: true })]);
  });
  if (process.env.SELECT) {
    // Click the demo square (paused first, so it holds still) so the
    // Inspector follows its animated values. Document space is viewer-panel
    // pixels at the default zoom.
    await page.evaluate(() => (window as any).__iron.call('clock_set', [JSON.stringify({ playing: false })]));
    await page.waitForTimeout(300);
    const at = await page.evaluate(async () => {
      const v: string = await (window as any).__iron.view({ fidelity: 'full' });
      const m = v.match(/name="square" x="([-\d.]+)" y="([-\d.]+)"[^>]* w="([\d.]+)" h="([\d.]+)"/);
      const r = document.querySelector('[data-panel-id="viewer"]')!.getBoundingClientRect();
      return m ? { x: r.left + +m[1] + +m[3] / 2, y: r.top + +m[2] + +m[4] / 2 } : null;
    });
    if (at) {
      await page.mouse.move(at.x, at.y); await page.waitForTimeout(150);
      await page.mouse.down(); await page.waitForTimeout(150); await page.mouse.up();
      await page.mouse.move(5, 5);
    }
    await page.waitForTimeout(500);
    console.log(`select: ${JSON.stringify(await page.evaluate(() => (window as any).__iron.selection()))}`);
    await page.evaluate(() => (window as any).__iron.call('clock_set', [JSON.stringify({ playing: true })]));
    await page.waitForTimeout(1000);
  }
  await page.waitForTimeout(1000);

  // 1. Mutations by panel.
  const churn = await page.evaluate(() => new Promise<any>((resolve) => {
    const panelOf = (n: Node | null): string => {
      let e = (n instanceof Element ? n : n?.parentElement) ?? null;
      const content = e?.closest('.dv-content-container');
      if (!content) return 'outside panels: ' + (e?.closest('[class]')?.className.toString().slice(0, 40) ?? '?');
      const group = content.closest('.dv-groupview');
      const tab = group?.querySelector('.dv-tab.dv-active-tab, .dv-tab')?.textContent?.trim() ?? '?';
      return tab;
    };
    const pathOf = (n: Node): string => {
      const parts: string[] = [];
      let e = n instanceof Element ? n : n.parentElement;
      for (let i = 0; e && i < 3; i++, e = e.parentElement) parts.unshift(e.tagName.toLowerCase() + (e.classList[0] ? '.' + e.classList[0] : ''));
      return parts.join('>');
    };
    const byPanel: Record<string, number> = {};
    const byPath: Record<string, number> = {};
    let frames = 0;
    const f = () => { frames++; if (frames < 1e9) requestAnimationFrame(f); };
    requestAnimationFrame(f);
    const obs = new MutationObserver((list) => {
      for (const m of list) {
        const key = panelOf(m.target);
        byPanel[key] = (byPanel[key] ?? 0) + 1;
        const p = `${key} | ${m.type}${m.attributeName ? ':' + m.attributeName : ''} | ${pathOf(m.target)}`;
        byPath[p] = (byPath[p] ?? 0) + 1;
      }
    });
    obs.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    setTimeout(() => {
      obs.disconnect();
      const top = Object.entries(byPath).sort((a, b) => b[1] - a[1]).slice(0, 25);
      resolve({ frames, byPanel, top });
    }, 2000);
  }));
  console.log(`mutations over 2 s (${churn.frames} frames), by panel:`);
  for (const [k, v] of Object.entries(churn.byPanel).sort((a: any, b: any) => b[1] - a[1])) console.log(`  ${String(v).padStart(6)}  ${k}`);
  console.log('top mutation sites:');
  for (const [k, v] of churn.top) console.log(`  ${String(v).padStart(6)}  ${k}`);

  // 2. Layout cost with each panel's content hidden in turn.
  const cdp = await context.newCDPSession(page);
  await cdp.send('Performance.enable');
  const metrics = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m: any) => [m.name, m.value]));
  const measure = async (label: string) => {
    await page.waitForTimeout(500);
    const a = await metrics();
    await page.waitForTimeout(3000);
    const b = await metrics();
    const per = (k: string) => ((b[k] - a[k]) / 3);
    console.log(`${label.padEnd(28)} layout ${(per('LayoutDuration') * 1000).toFixed(0).padStart(4)} ms/s (${per('LayoutCount').toFixed(0)}/s)  style ${(per('RecalcStyleDuration') * 1000).toFixed(0).padStart(4)} ms/s  script ${(per('ScriptDuration') * 1000).toFixed(0).padStart(4)} ms/s`);
  };
  await measure('all panels');
  const groups: string[] = await page.evaluate(() => [...document.querySelectorAll('.dv-groupview')].map((g, i) => {
    g.setAttribute('data-churn', String(i));
    return g.querySelector('.dv-tab.dv-active-tab, .dv-tab')?.textContent?.trim() ?? `group ${i}`;
  }));
  for (let i = 0; i < groups.length; i++) {
    await page.evaluate((i) => { const c = document.querySelector(`[data-churn="${i}"] .dv-content-container`) as HTMLElement | null; if (c) c.style.display = 'none'; }, i);
    await measure(`hide ${groups[i]}`);
    await page.evaluate((i) => { const c = document.querySelector(`[data-churn="${i}"] .dv-content-container`) as HTMLElement | null; if (c) c.style.display = ''; }, i);
  }
} finally {
  await close();
}
