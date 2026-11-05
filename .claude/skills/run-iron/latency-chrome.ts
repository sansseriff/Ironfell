/**
 * Headed Chrome for latency measurements (latency-measure.ts, latency-trace.ts):
 * its own temporary profile, so the person's Chrome is untouched. On battery
 * at or below 20% Chrome's default Energy Saver throttles pages to 30 fps
 * (found 2026-11-05: BeginFrameArgs interval 33 ms,
 * `frames_throttled_since_last: 1`), which silently changes every result. The
 * Preferences key below did not stop that in testing: run on AC power, and
 * read `powerState()` and the page's rAF rate with each result.
 */
import { chromium, type BrowserContext } from 'playwright-core';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

export async function launchChrome(): Promise<{ context: BrowserContext; close: () => Promise<void> }> {
  const dir = mkdtempSync(join(tmpdir(), 'iron-latency-chrome-'));
  mkdirSync(join(dir, 'Default'), { recursive: true });
  // performance_tuning.battery_saver_mode.state 0: Energy Saver off (not effective in testing; see above).
  writeFileSync(join(dir, 'Default', 'Preferences'), JSON.stringify({ performance_tuning: { battery_saver_mode: { state: 0 } } }));
  // PLAIN_CHROME=1: without Playwright's default switches (field trials off,
  // automation, background throttling off, forced sRGB, …), closer to a
  // person's Chrome. Playwright still needs its debugging pipe.
  const plain = !!process.env.PLAIN_CHROME;
  const context = await chromium.launchPersistentContext(dir, {
    ...(plain ? { ignoreDefaultArgs: true } : {}),
    executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    headless: false,
    viewport: null,
    // CHROME_WINDOW=w,h (points; default 1710,1080, the built-in display's usable area).
    args: ['--window-position=0,0', `--window-size=${process.env.CHROME_WINDOW ?? '1710,1080'}`, '--enable-gpu', '--ignore-gpu-blocklist', '--disable-features=Translate', '--no-first-run', ...(plain ? ['--remote-debugging-pipe', `--user-data-dir=${dir}`, '--no-default-browser-check'] : [])],
  });
  return {
    context,
    close: async () => {
      await context.close();
      rmSync(dir, { recursive: true, force: true });
    },
  };
}

/** `pmset -g batt`'s first lines: on battery or AC, and the charge. */
export function powerState(): string {
  return spawnSync('pmset', ['-g', 'batt'], { encoding: 'utf8' }).stdout.split('\n').slice(0, 2).join(' ').replace(/\s+/g, ' ').trim();
}
