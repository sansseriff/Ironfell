/**
 * Screenshot every composition tab of the demo (plans/specs/demo.md), each
 * clipped to its dock group, in light and dark at 1× and 2×.
 *
 *   URL='http://localhost:5185/Ironfell/?gfx=webgl2&demo' bun demo-shots.ts [title …]
 *
 * Writes `.agents/demo-shots/<scheme>-<dpr>x/<title>.png` in the worktree and
 * prints each file. With titles, only those tabs. `SCHEMES=light` and
 * `DPRS=1` narrow the matrix; `SIZE=1680x1000` sets the window.
 */
import { chromium } from 'playwright-core';
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';

const url = process.env.URL ?? 'http://localhost:5185/Ironfell/?gfx=webgl2&demo';
const [w, h] = (process.env.SIZE ?? '1680x1000').split('x').map(Number);
const schemes = (process.env.SCHEMES ?? 'light,dark').split(',') as ('light' | 'dark')[];
const dprs = (process.env.DPRS ?? '1,2').split(',').map(Number);
const only = process.argv.slice(2);
const root = join(import.meta.dir, '../../../.agents/demo-shots');
const browser = await chromium.launch({
  executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true, args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist'],
});
try {
  for (const dpr of dprs) {
    for (const scheme of schemes) {
      const page = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: dpr, colorScheme: scheme });
      const errors: string[] = [];
      page.on('pageerror', (e) => errors.push(String(e)));
      await page.goto(url);
      await page.waitForSelector('.loading', { state: 'detached', timeout: 60000 });
      await page.waitForFunction(() => ((window as any).__dock?.panels.length ?? 0) > 3, undefined, { timeout: 30000 });
      // The showcase's tabs arrive after load; its circuit is the last.
      await page.waitForFunction(() => (window as any).__dock.panels.some((p: any) => p.id === 'circuit'), undefined, { timeout: 30000 }).catch(() => console.log('no showcase tabs'));
      await page.waitForTimeout(1500);
      const titles: string[] = await page.evaluate(() =>
        (window as any).__dock.panels.filter((p: any) => p.params?.spec?.notation === 'composition' && (p.id === 'viewer' || p.params.spec.source.startsWith('#'))).map((p: any) => p.title));
      const dir = join(root, `${scheme}-${dpr}x`);
      await mkdir(dir, { recursive: true });
      for (const title of titles) {
        if (only.length && !only.includes(title)) continue;
        const rect = await page.evaluate((t) => {
          const p = (window as any).__dock.panels.find((x: any) => x.title === t);
          p.api.setActive();
          const r = p.group.element.getBoundingClientRect();
          return { x: r.x, y: r.y, width: r.width, height: r.height };
        }, title);
        await page.waitForTimeout(1500);
        const file = join(dir, `${title.replace(/[^\w.-]+/g, '-')}.png`);
        await page.screenshot({ path: file, clip: rect });
        console.log(file);
      }
      if (errors.length) console.log(`page errors (${scheme} ${dpr}x): ${errors.join(' | ')}`);
      await page.close();
    }
  }
} finally {
  await browser.close();
}
