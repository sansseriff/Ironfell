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
      await page.evaluate(() => (window as any).__frameHold?.reset());
      const result = measure(label, { browser: 'chrome', runtime, layout, continuous: !!process.env.CONTINUOUS, build, ua, power: powerState(), url: page.url() });
      // The page's own frame rate during the run (the view's readout): 60 here, or Chrome throttled it.
      // The frame hold's own counts (?frameHold=on): held, released, timed out, skipped.
      const hold = await page.evaluate(() => (window as any).__frameHold?.stats() ?? null);
      if (result && hold) {
        writeFileSync(`${out}/${label}-hold.json`, JSON.stringify(hold, null, 2));
        console.log(`  frame hold${hold.adaptive ? '' : ' (fixed cap)'}: ${hold.pending} of ${hold.frames} frames with a tick running at rAF; held ${hold.held} (released ${hold.released}, timed out ${hold.timedOut}); not held: skipped ${hold.skipped}, no room ${hold.noRoom}, backed off ${hold.backedOff}, disarmed ${hold.disarmed}; hold p50 ${hold.holdMs.p50} / p90 ${hold.holdMs.p90} ms, total ${hold.holdMs.total} ms; missed frames ${hold.missed} (${hold.missedAfterHold} after a hold), backoffs ${hold.backoffs}; limit ${hold.limitMs} ms, page work after p50 ${hold.afterMs.p50} / p90 ${hold.afterMs.p90} ms`);
      }
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
  // Safari restores its last session's tabs on launch, so each run uses a
  // fresh Safari: open the page, measure, then close every window and quit, so
  // the next launch restores nothing (and a person's next Safari does not
  // reopen test pages). Key presses only while Safari is frontmost
  // (latency-key --only): scripting Safari would raise an Automation prompt.
  const wasRunning = spawnSync('pgrep', ['-x', 'Safari']).status === 0;
  if (wasRunning) console.log('  Safari is already running: runs open tabs in it, and it is left running');
  const key = (...args: string[]) => spawnSync('../../../scripts/latency/latency-key', ['--only', 'com.apple.Safari', ...args]).status === 0;
  for (let n = 0; n < repeat; n++) for (const runtime of runtimes) for (const layout of layouts) {
    spawnSync('open', ['-a', 'Safari', url(runtime, layout)]);
    // The probe waits for the square to appear, then lets the page settle.
    await Bun.sleep(4000);
    measure(`safari-${runtime}-${layout}${tag}${repeat > 1 ? `-${n + 1}` : ''}`, { browser: 'safari', runtime, layout, url: url(runtime, layout) }, 40);
    if (!wasRunning) {
      if (!key('13', 'cmd', 'alt', 'shift')) console.log('  WARNING Safari was not frontmost; its window was left open');
      await Bun.sleep(800);
      key('12', 'cmd');
      for (let i = 0; i < 20 && spawnSync('pgrep', ['-x', 'Safari']).status === 0; i++) await Bun.sleep(250);
      if (spawnSync('pgrep', ['-x', 'Safari']).status === 0) spawnSync('killall', ['Safari']);
      await Bun.sleep(1000);
    }
  }
} else if (browserName === 'firefox') {
  // A separate Firefox instance with its own temporary profile per run
  // (-no-remote -new-instance), so a person's Firefox and its session are
  // untouched. Launched through `open` (LaunchServices): spawned directly from
  // a sandboxed shell it cannot load the profile. The profile lives in
  // /private/tmp: under ~/Documents macOS asks for folder access.
  const { mkdirSync, rmSync } = await import('node:fs');
  for (let n = 0; n < repeat; n++) for (const runtime of runtimes) for (const layout of layouts) {
    const dir = `/private/tmp/iron-latency-firefox-${process.pid}-${n}-${runtime}-${layout}`;
    mkdirSync(dir, { recursive: true });
    writeFileSync(`${dir}/user.js`, [
      'user_pref("browser.shell.checkDefaultBrowser", false);',
      'user_pref("browser.aboutwelcome.enabled", false);',
      'user_pref("browser.startup.homepage_override.mstone", "ignore");',
      'user_pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true);',
      'user_pref("browser.sessionstore.resume_from_crash", false);',
      'user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);',
    ].join('\n'));
    spawnSync('open', ['-na', process.env.FIREFOX_APP ?? 'Firefox', '--args', '-no-remote', '-new-instance', '-profile', dir, url(runtime, layout)]);
    try {
      await Bun.sleep(6000);
      measure(`firefox-${runtime}-${layout}${tag}${repeat > 1 ? `-${n + 1}` : ''}`, { browser: 'firefox', runtime, layout, url: url(runtime, layout) }, 40);
    } finally {
      spawnSync('pkill', ['-f', dir]);
      await Bun.sleep(1500);
      rmSync(dir, { recursive: true, force: true });
    }
  }
} else if (browserName === 'native') {
  // The Tauri host's dev binary (bun run native:ui, then cargo build -p
  // iron-native --profile dev-opt), launched directly with the Latency view
  // in front. MODES picks presets: baseline (pipelined, display pacing),
  // adaptive (the native default, what bun run native:run uses), late-fifo.
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
