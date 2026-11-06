/**
 * Latency view checks (plans/rendering/latency-view.md), in a real Chrome.
 *
 *   URL=http://127.0.0.1:5185/Ironfell/?demo bun run latency-view.ts
 *
 * Per runtime (worker, main) and layout (solo, panels): the view opens from
 * `?latency=`, the square sits at the view's centre, every tracker is centred
 * on a still pointer (pixels in a screenshot, per colour), a drag moves the
 * square without adding to the document's history, and Recentre puts it back.
 * HEADED=1 runs a visible window (real GPU, WebGPU); default is headless
 * WebGL2 on Metal ANGLE. Screenshots go to .agents/latency/view-*.png.
 */
import { chromium } from 'playwright-core';
import { mkdirSync } from 'node:fs';
import assert from 'node:assert/strict';

const out = '../../../.agents/latency';
mkdirSync(out, { recursive: true });
const headed = !!process.env.HEADED;
const browser = await chromium.launch({
  executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: !headed,
  args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--no-sandbox', '--enable-unsafe-webgpu'],
});
const base = new URL(process.env.URL ?? 'http://127.0.0.1:5185/Ironfell/?demo');
if (!headed && !base.searchParams.has('gfx')) base.searchParams.set('gfx', 'webgl2');

// The trackers' colours (LatencyView.svelte TRACKERS) and hue windows.
const TRACKERS = [
  { key: 'square', hue: 50.4, radius: 20 },
  { key: 'engine', hue: 320, radius: 34 },
  { key: 'canvas', hue: 120, radius: 44 },
  { key: 'raf', hue: 28.8, radius: 54 },
  { key: 'svg', hue: 265, radius: 64 },
  { key: 'konva', hue: 0, radius: 74 },
  { key: 'webgpu', hue: 186, radius: 84 },
  { key: 'webgl', hue: 82, radius: 94 },
];
type Centre = { x: number; y: number; n: number };
/** Each tracker's centre in a screenshot (CSS px), from the horizontal and
 *  vertical extent of its colour near the expected point. Decoded in a
 *  blank page, so no image library is needed. */
let decoder: import('playwright-core').Page;
async function centres(png: Buffer, dpr: number, around: { x: number; y: number }): Promise<Record<string, Centre>> {
  return decoder.evaluate(async ({ b64, dpr, around, trackers }) => {
    const img = await createImageBitmap(await (await fetch(`data:image/png;base64,${b64}`)).blob());
    const span = Math.round(115 * dpr), x0 = Math.max(0, Math.round(around.x * dpr) - span), y0 = Math.max(0, Math.round(around.y * dpr) - span);
    const w = Math.min(img.width - x0, 2 * span), h = Math.min(img.height - y0, 2 * span);
    const c = new OffscreenCanvas(w, h).getContext('2d')!;
    c.drawImage(img, -x0, -y0);
    const d = c.getImageData(0, 0, w, h).data;
    const found: Record<string, { x: number; y: number; n: number }> = {};
    const box: Record<string, number[]> = {};
    for (const t of trackers) { found[t.key] = { x: NaN, y: NaN, n: 0 }; box[t.key] = [Infinity, -Infinity, Infinity, -Infinity]; }
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4, r = d[i], g = d[i + 1], b = d[i + 2];
      const max = Math.max(r, g, b), min = Math.min(r, g, b), dd = max - min;
      if (!max || dd / max < 0.65 || max / 255 < 0.55) continue;
      let hue = max === r ? ((g - b) / dd) % 6 : max === g ? (b - r) / dd + 2 : (r - g) / dd + 4;
      hue = (hue * 60 + 360) % 360;
      for (const t of trackers) {
        const dh = Math.min(Math.abs(hue - t.hue), 360 - Math.abs(hue - t.hue));
        if (dh > 9) continue;
        const bx = box[t.key];
        found[t.key].n++;
        bx[0] = Math.min(bx[0], x); bx[1] = Math.max(bx[1], x); bx[2] = Math.min(bx[2], y); bx[3] = Math.max(bx[3], y);
      }
    }
    for (const t of trackers) {
      const bx = box[t.key];
      found[t.key].x = ((bx[0] + bx[1]) / 2 + x0 + 0.5) / dpr;
      found[t.key].y = ((bx[2] + bx[3]) / 2 + y0 + 0.5) / dpr;
    }
    return found;
  }, { b64: png.toString('base64'), dpr, around, trackers: TRACKERS });
}

const runtimes = (process.env.RUNTIMES ?? 'worker,main').split(',');
const layouts = (process.env.LAYOUTS ?? 'solo,panels').split(',');
const dpr = Number(process.env.DPR ?? 2);
let failures = 0;
decoder = await (await browser.newContext()).newPage();
try {
  for (const runtime of runtimes) for (const layout of layouts) {
    const label = `${runtime}/${layout}/dpr${dpr}`;
    const context = await browser.newContext({ viewport: { width: 1400, height: 880 }, deviceScaleFactor: dpr });
    await context.addInitScript((runtime) => localStorage.setItem('iron.settings.v1', JSON.stringify({ runtime, renderScale: 'auto' })), runtime);
    const page = await context.newPage();
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(String(e)));
    page.on('console', (m) => { if (m.text().includes('panicked at')) errors.push(m.text()); });
    const url = new URL(base); url.searchParams.set('latency', layout);
    try {
      await page.goto(url.toString());
      await page.waitForFunction(() => document.querySelector('[data-latency-view]')?.getAttribute('data-ready') === 'true', {}, { timeout: 45000 });
      const tabs = await page.locator('.dv-tab').allInnerTexts();
      if (layout === 'panels') for (const t of ['Latency', 'Model', 'Inspector']) assert.ok(tabs.some((x) => x.includes(t)), `${t} tab in ${tabs}`);
      const stage = (await page.locator('[data-latency-view]').boundingBox())!;
      const centre = { x: stage.x + Math.round(stage.width / 2), y: stage.y + Math.round(stage.height / 2) };
      const history = () => page.evaluate(async () => JSON.parse(await (window as any).__iron.save()).version as number);
      const v0 = await history();

      // Pointer still over the square's centre: every tracker shares it.
      await page.mouse.move(centre.x, centre.y);
      await page.waitForTimeout(400);
      let c = await centres(await page.screenshot({ path: `${out}/view-${runtime}-${layout}-still.png` }), dpr, centre);
      // Headless Chrome may have no WebGPU: its control ring is then absent.
      const drawn = TRACKERS.filter((t) => t.key !== 'webgpu' || c.webgpu.n > 0);
      for (const t of drawn) {
        assert.ok(c[t.key].n > 20, `${label}: ${t.key} drawn (${c[t.key].n} px)`);
        assert.ok(Math.hypot(c[t.key].x - centre.x, c[t.key].y - centre.y) <= 1.5, `${label}: ${t.key} centred at rest: ${JSON.stringify(c[t.key])} vs ${JSON.stringify(centre)}`);
      }

      // Drag 150 px right and hold still: the square follows, all concentric.
      await page.mouse.down();
      for (let i = 1; i <= 15; i++) { await page.mouse.move(centre.x + i * 10, centre.y); await page.waitForTimeout(16); }
      await page.waitForTimeout(400);
      const held = { x: centre.x + 150, y: centre.y };
      c = await centres(await page.screenshot({ path: `${out}/view-${runtime}-${layout}-held.png` }), dpr, held);
      for (const t of drawn) assert.ok(Math.hypot(c[t.key].x - held.x, c[t.key].y - held.y) <= 1.5, `${label}: ${t.key} centred while held: ${JSON.stringify(c[t.key])}`);
      await page.mouse.up();
      await page.waitForTimeout(200);
      assert.equal(await history(), v0, `${label}: dragging the square is not a document edit`);

      // Recentre brings it back.
      await page.mouse.move(stage.x + 30, stage.y + stage.height - 30);
      await page.getByRole('button', { name: 'Recentre' }).click();
      await page.mouse.move(centre.x, centre.y);
      await page.waitForTimeout(400);
      c = await centres(await page.screenshot(), dpr, centre);
      assert.ok(Math.hypot(c.square.x - centre.x, c.square.y - centre.y) <= 1.5, `${label}: recentred square ${JSON.stringify(c.square)}`);
      const alerts = await page.locator('[data-latency-view] [role=alert]').allInnerTexts();
      assert.deepEqual(alerts, [], `${label}: no error shown`);
      assert.deepEqual(errors, [], `${label}: page errors`);
      console.log(`PASS LATENCY VIEW ${label}: opens, concentric at rest and held, drag outside history, recentre; no page errors`);
    } catch (e) {
      failures++;
      console.log(`FAIL LATENCY VIEW ${label}: ${(e as Error).message}`);
      await page.screenshot({ path: `${out}/view-${runtime}-${layout}-fail.png` }).catch(() => {});
    }
    await context.close();
  }
  // Developer mode offers the view in the "+" menu; without it, it is absent.
  for (const developer of [true, false]) {
    const label = `menu/developer-${developer ? 'on' : 'off'}`;
    const context = await browser.newContext({ viewport: { width: 1400, height: 880 } });
    await context.addInitScript((developer) => localStorage.setItem('iron.settings.v1', JSON.stringify({ developer })), developer);
    const page = await context.newPage();
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(String(e)));
    try {
      await page.goto(base.toString());
      await page.waitForFunction(() => !!(window as any).__iron?.selection, {}, { timeout: 45000 });
      await page.getByRole('button', { name: 'Open a view' }).first().click();
      const item = page.getByRole('menuitem', { name: /^Latency/ });
      if (developer) {
        await item.click();
        await page.waitForFunction(() => document.querySelector('[data-latency-view]')?.getAttribute('data-ready') === 'true', {}, { timeout: 30000 });
        assert.ok((await page.locator('.dv-tab').allInnerTexts()).some((t) => t.includes('Latency')), 'a Latency tab');
      } else {
        await page.getByRole('menuitem', { name: 'Inspector' }).waitFor();
        assert.equal(await item.count(), 0, 'no Latency item without developer mode');
      }
      assert.deepEqual(errors, [], 'page errors');
      console.log(`PASS LATENCY VIEW ${label}: ${developer ? 'opens from the + menu' : 'not offered'}; no page errors`);
    } catch (e) {
      failures++;
      console.log(`FAIL LATENCY VIEW ${label}: ${(e as Error).message}`);
      await page.screenshot({ path: `${out}/view-${label.replace('/', '-')}-fail.png` }).catch(() => {});
    }
    await context.close();
  }
} finally {
  await browser.close();
}
process.exit(failures ? 1 : 0);
