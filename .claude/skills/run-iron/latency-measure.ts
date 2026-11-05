/**
 * Measured input latency of the Latency view (plans/rendering/latency-view.md
 * §4), in a real browser window on the main display, with the OS pointer.
 * Takes over the mouse for ~10 s per run: hands off.
 *
 *   ../../../scripts/latency/build.sh
 *   BROWSER=chrome RUNTIMES=worker,main LAYOUTS=solo,panels bun run latency-measure.ts
 *   BROWSER=safari RUNTIMES=worker LAYOUTS=solo bun run latency-measure.ts
 *   BROWSER=native MODES=baseline,adaptive LAYOUTS=solo,panels bun run latency-measure.ts
 *
 * Chrome: a separate instance (its own temporary profile) launched headed by
 * Playwright, with the real GPU (WebGPU on macOS). Safari: opened with
 * `open -a Safari`, quit after (only when it was not already running).
 * Each run's JSON goes to .agents/latency/measure/<label>.json, and a table
 * of p50 lags (ms) is printed at the end.
 */
import { chromium } from 'playwright-core';
import { mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';

const out = '../../../.agents/latency/measure';
mkdirSync(out, { recursive: true });
const probe = '../../../scripts/latency/latency-probe';
const browserName = process.env.BROWSER ?? 'chrome';
const runtimes = (process.env.RUNTIMES ?? 'worker,main').split(',');
const layouts = (process.env.LAYOUTS ?? 'solo,panels').split(',');
const repeat = Number(process.env.REPEAT ?? 1);
const base = process.env.URL ?? 'http://127.0.0.1:5185/Ironfell/?demo';
const probeArgs = (process.env.PROBE_ARGS ?? '').split(' ').filter(Boolean);
const tag = process.env.TAG ? `-${process.env.TAG}` : '';

type Result = { label: string; lag_ms: Record<string, { p50?: number; p90?: number; n: number }>; frame_interval_ms: { p50?: number }; rest_offset_pt: Record<string, number> };
const results: Result[] = [];

function measure(label: string, meta: Record<string, unknown>, wait = 0): Result | null {
  const r = spawnSync(probe, ['--label', label, '--out', `${out}/${label}.json`, '--csv', `${out}/${label}.csv`, '--meta', JSON.stringify(meta), '--wait', String(wait), '--debug', `${out}/${label}-still.png`, ...probeArgs], { encoding: 'utf8', timeout: 120_000 });
  if (r.status !== 0) {
    console.log(`FAIL ${label}: ${r.stderr.trim() || r.error}`);
    return null;
  }
  const j = JSON.parse(r.stdout) as Result;
  results.push(j);
  const lag = Object.entries(j.lag_ms).map(([k, v]) => `${k} ${v.p50?.toFixed(1) ?? '–'}`).join(' · ');
  console.log(`${label}: p50 ms ${lag} · frame ${j.frame_interval_ms.p50?.toFixed(1)} ms`);
  return j;
}

function url(runtime: string, layout: string) {
  const u = new URL(base);
  u.searchParams.set('latency', layout);
  u.searchParams.set('runtime', runtime);
  return u.toString();
}

if (browserName === 'chrome') {
  for (let n = 0; n < repeat; n++) for (const runtime of runtimes) for (const layout of layouts) {
    const browser = await chromium.launch({
      executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
      headless: false,
      args: ['--window-position=0,0', '--window-size=1710,1080', '--enable-gpu', '--ignore-gpu-blocklist', '--disable-features=Translate', '--no-first-run'],
    });
    try {
      const context = await browser.newContext({ viewport: null });
      const page = await context.newPage();
      await page.goto(url(runtime, layout));
      await page.waitForFunction(() => document.querySelector('[data-latency-view]')?.getAttribute('data-ready') === 'true', {}, { timeout: 60000 });
      const build = await page.evaluate(async () => { try { return JSON.parse(await (window as any).__iron.call('build_info', [])); } catch { return null; } });
      // CONTINUOUS=1: the view's composition renders every frame.
      if (process.env.CONTINUOUS) await page.getByRole('switch', { name: 'Continuous rendering' }).click();
      await page.bringToFront();
      await page.waitForTimeout(1500);
      measure(`chrome-${runtime}-${layout}${process.env.CONTINUOUS ? '-continuous' : ''}${tag}${repeat > 1 ? `-${n + 1}` : ''}`, { browser: 'chrome', runtime, layout, continuous: !!process.env.CONTINUOUS, build, ua: await page.evaluate(() => navigator.userAgent) });
    } catch (e) {
      console.log(`FAIL chrome-${runtime}-${layout}: ${(e as Error).message}`);
    } finally {
      await browser.close();
    }
  }
} else if (browserName === 'safari') {
  const wasRunning = spawnSync('pgrep', ['-x', 'Safari']).status === 0;
  for (let n = 0; n < repeat; n++) for (const runtime of runtimes) for (const layout of layouts) {
    spawnSync('open', ['-a', 'Safari', url(runtime, layout)]);
    // The probe waits for the square to appear, then lets the page settle.
    await Bun.sleep(4000);
    measure(`safari-${runtime}-${layout}${tag}${repeat > 1 ? `-${n + 1}` : ''}`, { browser: 'safari', runtime, layout }, 40);
  }
  if (!wasRunning) spawnSync('killall', ['Safari']);
} else if (browserName === 'native') {
  // The Tauri host's dev binary (bun run native:ui, then cargo build -p
  // iron-native --profile dev-opt), launched directly with the Latency view
  // in front. MODES picks presets from package.json: baseline (pipelined,
  // display pacing), adaptive (the recommended native:adaptive).
  const presets: Record<string, Record<string, string>> = {
    baseline: { IRON_NATIVE_RENDER_MODE: 'pipelined', IRON_NATIVE_PACING: 'display' },
    adaptive: { IRON_NATIVE_RENDER_MODE: 'sync', IRON_NATIVE_PACING: 'adaptive', IRON_NATIVE_FRAME_LATENCY: '2' },
    'late-fifo': { IRON_NATIVE_RENDER_MODE: 'sync', IRON_NATIVE_PACING: 'late-fifo', IRON_NATIVE_FRAME_LATENCY: '1' },
  };
  const bin = process.env.NATIVE_BIN ?? '../../../target/aarch64-apple-darwin/dev-opt/iron-native';
  for (let n = 0; n < repeat; n++) for (const mode of (process.env.MODES ?? 'baseline,adaptive').split(',')) for (const layout of layouts) {
    const child = Bun.spawn([bin], { env: { ...process.env, ...presets[mode], IRON_NATIVE_LATENCY: layout }, stdout: 'ignore', stderr: 'ignore' });
    try {
      await Bun.sleep(5000);
      measure(`native-${mode}-${layout}${tag}${repeat > 1 ? `-${n + 1}` : ''}`, { browser: 'native', mode, layout, env: presets[mode] }, 60);
    } finally {
      child.kill();
      await child.exited;
      await Bun.sleep(1000);
    }
  }
}

writeFileSync(`${out}/summary-${browserName}${tag}.json`, JSON.stringify(results, null, 2));
console.log('\nlabel | square | engine | canvas | raf | svg  (p50 / p90 ms)');
for (const r of results) {
  const cell = (k: string) => `${r.lag_ms[k]?.p50?.toFixed(1) ?? '–'} / ${r.lag_ms[k]?.p90?.toFixed(1) ?? '–'}`;
  console.log(`${r.label} | ${['square', 'engine', 'canvas', 'raf', 'svg'].map(cell).join(' | ')}`);
}
