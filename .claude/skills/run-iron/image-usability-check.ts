// The hidden-tab pill stays under the pointer; SVG and HEIC become bounded
// raster attachments before they reach the chat. The 499-byte HEIC fixture is
// libheif's fuzzing/data/corpus/colors-no-alpha.heic.
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { fileURLToPath } from 'node:url';

const url = process.env.URL ?? 'http://localhost:5173/Ironfell/?gfx=webgl2&demo';
const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--no-sandbox'] });
const page = await browser.newPage({ viewport: { width: 1680, height: 1000 }, deviceScaleFactor: Number(process.env.DPR ?? 1) });
const errors: string[] = [];
page.on('pageerror', error => errors.push(String(error)));
try {
  await page.goto(url);
  await page.locator('.loading').waitFor({ state: 'detached', timeout: 30000 });
  const pill = page.locator('.grab-pill').first();
  await pill.waitFor();
  const idleScale = await pill.evaluate(element => getComputedStyle(element, '::before').transform);
  assert.match(idleScale, /0\.45/);
  // At rest the pill is not painted; the pointer in the group's top strip shows it.
  assert.equal(await pill.evaluate(element => getComputedStyle(element, '::before').opacity), '0');
  // The collapsed hit area is the top 10 px: enter it, then move down the grown pill.
  await page.mouse.move(170, 4);
  await page.waitForTimeout(100);
  await page.mouse.move(170, 16);
  await page.waitForTimeout(100);
  const stable = await page.evaluate(async () => {
    for (let i = 0; i < 12; i++) {
      const pill = document.querySelector('.grab-pill')!;
      const top = document.elementFromPoint(170, 16)!;
      const rect = pill.getBoundingClientRect();
      if (!pill.matches(':hover') || !pill.contains(top) || rect.width !== 40 || rect.height !== 22 || getComputedStyle(top).cursor !== 'grab') return false;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    return true;
  });
  assert(stable, 'grab pill hover and cursor must remain stable');
  await pill.click();
  await page.getByRole('menu').waitFor();
  await page.keyboard.press('Escape');
  console.log('grab pill: stable hover and menu');

  await page.getByRole('button', { name: 'Model (the agent)' }).click();
  const composer = page.locator('.composer:visible').first();
  await composer.waitFor();
  const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="5000" height="2500" viewBox="0 0 5000 2500"><rect width="5000" height="2500" fill="navy"/><circle cx="2500" cy="1250" r="900" fill="orange"/><text x="100" y="400" font-size="200" fill="white">Hello SVG</text></svg>';
  await composer.evaluate((element, source) => {
    const transfer = new DataTransfer();
    transfer.items.add(new File([source], 'reference.svg', { type: 'image/svg+xml' }));
    element.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
  }, svg);
  const svgImage = composer.locator('.attachment img').first();
  await svgImage.waitFor({ timeout: 30000 });
  const svgResult = await svgImage.evaluate(async (image: HTMLImageElement) => { await image.decode(); return { name: image.alt, type: image.src.slice(0, 22), width: image.naturalWidth, height: image.naturalHeight }; });
  assert.deepEqual(svgResult, { name: 'reference.svg', type: 'data:image/png;base64,', width: 1568, height: 784 });
  const textPixels = await svgImage.evaluate((image: HTMLImageElement) => {
    const canvas = document.createElement('canvas'); canvas.width = 400; canvas.height = 150;
    const ctx = canvas.getContext('2d')!; ctx.drawImage(image, 0, 0);
    const pixels = ctx.getImageData(0, 0, 400, 150).data;
    let white = 0;
    for (let i = 0; i < pixels.length; i += 4) if (pixels[i] > 230 && pixels[i + 1] > 230 && pixels[i + 2] > 230) white++;
    return white;
  });
  assert(textPixels > 1000, 'SVG text must be rasterized');
  console.log('SVG: 1568×784 PNG with text');

  await composer.locator('input[type=file]').setInputFiles(fileURLToPath(new URL('../../../tests/fixtures/chat-colors.heic', import.meta.url)));
  const heicImage = composer.locator('.attachment img').nth(1);
  await heicImage.waitFor({ timeout: 30000 });
  const heicResult = await heicImage.evaluate(async (image: HTMLImageElement) => { await image.decode(); return { name: image.alt, type: image.src.slice(0, 22), width: image.naturalWidth, height: image.naturalHeight }; });
  assert.deepEqual(heicResult, { name: 'chat-colors.heic', type: 'data:image/png;base64,', width: 64, height: 64 });
  console.log('HEIC: 64×64 PNG');

  if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT });

  await composer.getByRole('button', { name: 'Remove reference.svg' }).click();
  assert.equal(await composer.locator('.attachment').count(), 1);
  assert.deepEqual(errors, []);
  console.log('remove: one attachment remains; no page errors');
} finally { await browser.close(); }
