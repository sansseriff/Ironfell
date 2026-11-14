/**
 * Replay model edits on a fresh demo page and save the document: how an agent
 * probe run's applied edits are turned back into a saved document when the run
 * kept none (the probe saves `turn-N-after.json` since 2026-11-13).
 *
 *   URL='http://localhost:5185/Ironfell/?gfx=webgl2&demo' bun replay-edits.ts edits.json out.json
 *
 * `edits.json` is a JSON list of `<edit>` blocks, applied in order with
 * `__iron.edit`; each prints its outcome.
 */
import { chromium } from 'playwright-core';
import { readFile, writeFile } from 'node:fs/promises';

const [editsFile, outFile] = process.argv.slice(2);
if (!editsFile || !outFile) throw new Error('usage: replay-edits.ts edits.json out.json');
const url = process.env.URL ?? 'http://localhost:5185/Ironfell/?gfx=webgl2&demo';
const edits: string[] = JSON.parse(await readFile(editsFile, 'utf8'));
const browser = await chromium.launch({
  executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true, args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist'],
});
try {
  const page = await browser.newPage();
  await page.goto(url);
  await page.waitForSelector('.loading', { state: 'detached', timeout: 60000 });
  await page.waitForFunction(async () => {
    try { return !!(await (window as any).__iron.call('schema')).types; } catch { return false; }
  }, undefined, { timeout: 30000 });
  await page.waitForTimeout(1500);
  for (const [i, xml] of edits.entries()) {
    const r = await page.evaluate(x => (window as any).__iron.edit(x), xml);
    console.log(`edit ${i + 1}: ${JSON.stringify(r).slice(0, 300)}`);
  }
  await page.waitForTimeout(1000);
  await writeFile(outFile, await page.evaluate(() => (window as any).__iron.save() as Promise<string>));
  console.log(`saved ${outFile}`);
} finally {
  await browser.close();
}
