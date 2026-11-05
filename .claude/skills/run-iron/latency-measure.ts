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
import { mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { launchChrome, powerState } from './latency-chrome';

const out = process.env.OUT ?? '../../../.agents/latency/measure';
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
  if ((j as any).interference) console.log(`  WARNING ${label}: the cursor was moved by something else during the sweep; discard this run`);
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
    const { context, close } = await launchChrome();
    try {
      const page = context.pages()[0] ?? await context.newPage();
      // RENDER_SCALE=auto|panel4k|2|1.5|1: the Settings view's render scale (canvas px per CSS px).
      if (process.env.RENDER_SCALE) await context.addInitScript((renderScale) => localStorage.setItem('iron.settings.v1', JSON.stringify({ renderScale })), process.env.RENDER_SCALE);
      await page.goto(url(runtime, layout));
      await page.waitForFunction(() => document.querySelector('[data-latency-view]')?.getAttribute('data-ready') === 'true', {}, { timeout: 60000 });
      const build = await page.evaluate(async () => { try { return JSON.parse(await (window as any).__iron.call('build_info', [])); } catch { return null; } });
      // CONTINUOUS=1: the view's composition renders every frame.
      if (process.env.CONTINUOUS) await page.getByRole('switch', { name: 'Continuous rendering' }).click();
      await page.bringToFront();
      await page.waitForTimeout(1500);
      // SESSION=k: k runs in this one browser session (does a state persist?).
      for (let k = 0; k < Number(process.env.SESSION ?? 1); k++) {
      const label = `chrome-${runtime}-${layout}${process.env.CONTINUOUS ? '-continuous' : ''}${tag}${repeat > 1 ? `-${n + 1}` : ''}${process.env.SESSION ? `-s${k + 1}` : ''}`;
      const ua = await page.evaluate(() => navigator.userAgent);
      // The engine's own frame timing over the run (CadenceProbe): when each
      // tick started and ended after its rAF timestamp.
      await page.evaluate(() => (window as any).__probeReset?.());
      const result = measure(label, { browser: 'chrome', runtime, layout, continuous: !!process.env.CONTINUOUS, build, ua, power: powerState(), url: page.url() });
      // The page's own frame rate during the run (the view's readout): 60 here, or Chrome throttled it.
      const rafHz = await page.evaluate(() => Number(document.querySelector('[data-latency-stats]')?.textContent?.match(/rAF (\d+)/)?.[1] ?? 0));
      if (rafHz && rafHz < 55) console.log(`  WARNING page rAF ${rafHz} Hz: the page was throttled; discard this run`);
      await page.evaluate(() => { (window as any).__lastProbeStats = null; (window as any).__probe?.(); });
      await page.waitForFunction(() => !!(window as any).__lastProbeStats, {}, { timeout: 5000 }).catch(() => {});
      const cadence = await page.evaluate(() => (window as any).__lastProbeStats);
      if (result && cadence) {
        writeFileSync(`${out}/${label}-cadence.json`, JSON.stringify(cadence, null, 2));
        console.log(`  engine tick: start p50 ${cadence.startMs?.p50} ms, end p50 ${cadence.endMs?.p50} / p95 ${cadence.endMs?.p95} ms after rAF; tick p50 ${cadence.tickMs?.p50} ms; ${cadence.effectiveFps} fps${cadence.pointerAgeMs ? `; pointer age at tick p50 ${cadence.pointerAgeMs.p50} ms (waited in worker ${cadence.pointerWaitMs.p50} ms)` : ''}`);
      }
      }
    } catch (e) {
      console.log(`FAIL chrome-${runtime}-${layout}: ${(e as Error).message}`);
    } finally {
      await close();
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
console.log('\nlabel | square | engine | canvas | raf | svg | konva | webgpu | webgl  (p50 / p90 ms)');
for (const r of results) {
  const cell = (k: string) => `${r.lag_ms[k]?.p50?.toFixed(1) ?? '–'} / ${r.lag_ms[k]?.p90?.toFixed(1) ?? '–'}`;
  console.log(`${r.label} | ${['square', 'engine', 'canvas', 'raf', 'svg', 'konva', 'webgpu', 'webgl'].map(cell).join(' | ')}`);
}
