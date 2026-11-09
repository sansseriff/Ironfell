// Screenshots of the chat composer: empty, a few lines, and past its cap,
// in light and dark, at 1x and 2x. Prints each field's measurements.
//   URL=http://127.0.0.1:5185/Ironfell/?gfx=webgl2&demo OUT=/tmp/composer bun run composer-shots.ts
import { chromium } from 'playwright-core';
import { mkdir } from 'node:fs/promises';

const url = process.env.URL ?? 'http://127.0.0.1:5175/Ironfell/?gfx=webgl2&demo';
const out = process.env.OUT ?? '/tmp/composer';
await mkdir(out, { recursive: true });
const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--no-sandbox'] });
try {
  for (const scale of [1, 2]) for (const scheme of ['light', 'dark'] as const) {
    const page = await browser.newPage({ viewport: { width: 1400, height: 900 }, deviceScaleFactor: scale, colorScheme: scheme });
    page.on('pageerror', e => console.error('pageerror', String(e)));
    await page.goto(url);
    await page.getByRole('tab', { name: 'Model', exact: true }).click({ timeout: 20000 });
    const field = page.getByRole('textbox', { name: 'Agent instruction' }).first();
    await field.waitFor({ state: 'visible', timeout: 15000 });
    const surface = page.locator('.composer .surface').first();
    const measure = () => field.evaluate((el: HTMLTextAreaElement) => {
      const s = getComputedStyle(el);
      return { h: el.offsetHeight, scroll: el.scrollHeight, client: el.clientHeight, overflowY: s.overflowY, scrollbar: el.offsetWidth - el.clientWidth };
    });
    const shot = async (name: string) => {
      const box = (await surface.boundingBox())!;
      await page.screenshot({ path: `${out}/${name}-${scheme}@${scale}.png`, clip: { x: box.x - 16, y: box.y - 16, width: box.width + 32, height: box.height + 32 } });
      console.log(name, scheme, `@${scale}`, JSON.stringify(await measure()));
    };
    await shot('empty');
    await field.click();
    await shot('focused');
    await field.fill('First line of an instruction\nsecond line\nthird line');
    await shot('lines');
    await field.fill(Array.from({ length: 14 }, (_, i) => `Line ${i + 1} of a long instruction that keeps going`).join('\n'));
    await shot('overflow');
    await field.fill('');
    await shot('cleared');
    await page.close();
  }
} finally {
  await browser.close();
}
