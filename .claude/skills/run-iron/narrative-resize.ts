// Resize the narrative across a view-block intersection, then scroll and click.
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist'] });
const page = await browser.newPage({ viewport: { width: 1680, height: 1000 } });
const errors: string[] = [];
page.on('pageerror', e => errors.push(String(e)));
try {
  await page.goto(process.env.URL ?? 'http://localhost:5173/Ironfell/?gfx=webgl2&demo');
  await page.waitForFunction(() => (window as any).__narrative?.get('lesson')?.status().blocks > 1, { timeout: 30000 });
  const active = () => page.evaluate(() => (window as any).__dock.getPanel('viewer').group.activePanel.id);
  await page.evaluate(() => (window as any).__dock.getPanel('viewer').api.setActive());
  // Narrowing moves the first view block out of the reading zone; widening
  // moves it back in, which used to select Scene without a scroll.
  await page.mouse.move(340, 400); await page.mouse.down(); await page.mouse.move(220, 400, { steps: 12 }); await page.mouse.up();
  await page.waitForTimeout(300);
  await page.mouse.move(220, 400); await page.mouse.down(); await page.mouse.move(340, 400, { steps: 12 }); await page.mouse.up();
  await page.waitForTimeout(300);
  assert.equal(await active(), 'viewer', 'resizing must preserve the central tab');
  const scroll = page.locator('.narrative .scroll:not(.hidden)').first();
  await scroll.evaluate((el: HTMLElement) => { el.scrollTop = 220; });
  await page.waitForTimeout(300);
  assert.equal(await active(), 'pendulum', 'scrolling to a view block opens its composition');
  await page.locator('.nb-view-header').filter({ hasText: 'assembly' }).click();
  assert.equal(await active(), 'assembly', 'clicking a view block opens its composition');
  assert.deepEqual(errors, []);
  console.log('narrative resize, scroll and click: passed; no page errors');
} finally { await browser.close(); }
