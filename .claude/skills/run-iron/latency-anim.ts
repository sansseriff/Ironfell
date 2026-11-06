/**
 * Smoothness of an animation playing by itself (no pointer input), in headed
 * Chrome: the demo's square, recoloured yellow for the probe, slides on a
 * looping timeline while `latency-probe --watch` records the screen. Each run
 * reports repeated and doubled frames (scripts/latency/judder.py) and the frame
 * hold's counts. Hands off the mouse.
 *
 *   CASES='off,on,always' CHROME_WINDOW=3008,1660 bun run latency-anim.ts
 *
 * CASES: frame hold off | on (armed only by input) | always (holds with no
 * input). Also: REPEAT, RATE (playback rate, default 4), DURATION (s, default 8),
 * EXTRA (more URL parameters, e.g. `&workerTick=raf`), TRACE=1 (a Chrome trace
 * of each run beside its CSV), OUT.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { launchChrome } from './latency-chrome';

const out = process.env.OUT ?? '../../../.agents/latency/anim';
mkdirSync(out, { recursive: true });
const base = process.env.URL ?? 'http://127.0.0.1:5185/Ironfell/?demo';
const cases = (process.env.CASES ?? 'off,on,always').split(',');
const repeat = Number(process.env.REPEAT ?? 2);
const rate = Number(process.env.RATE ?? 4);
const duration = process.env.DURATION ?? '8';
const tag = process.env.TAG ? `-${process.env.TAG}` : '';

for (let n = 0; n < repeat; n++) for (const hold of cases) {
  const label = `anim-${hold}${tag}-${n + 1}`;
  const url = `${base}&frameHold=${hold}${process.env.EXTRA ?? ''}`;
  const { context, close } = await launchChrome();
  try {
    const page = context.pages()[0] ?? await context.newPage();
    await page.goto(url);
    await page.waitForFunction(() => !!(window as any).__iron?.selection, {}, { timeout: 60000 });
    await page.bringToFront();
    await page.waitForTimeout(2000);
    // The demo square: yellow (the probe's colour), playing on a loop over its clip.
    const setup = await page.evaluate(async (rate) => {
      const iron = (window as any).__iron;
      const v: string = await iron.view({ fidelity: 'full' });
      const id = v.match(/<rect id="(#\w+)"[^>]*name="square"/)?.[1] ?? v.match(/id="(#\w+)"[^>]*name="square"/)?.[1];
      if (!id) return 'no square';
      const r = await iron.edit(`<edit><set id="${id}" fill.color="#ffd600ff"/></edit>`);
      if (!r.ok) return JSON.stringify(r);
      await iron.call('clock_set', [JSON.stringify({ loop_in: 0.5, loop_out: 2.5, rate, t: 0.5, playing: true })]);
      return 'ok ' + id;
    }, rate);
    if (!setup.startsWith('ok')) throw new Error(`setup: ${setup}`);
    await page.waitForTimeout(1500);
    await page.evaluate(() => (window as any).__frameHold?.reset());
    let cdp: any = null;
    const events: unknown[] = [];
    if (process.env.TRACE) {
      cdp = await context.newCDPSession(page);
      cdp.on('Tracing.dataCollected', (e: any) => events.push(...e.value));
      await cdp.send('Tracing.start', { traceConfig: { includedCategories: ['viz', 'gpu', 'cc', 'benchmark', 'blink', 'devtools.timeline', 'disabled-by-default-devtools.timeline.frame'], recordMode: 'recordAsMuchAsPossible' }, transferMode: 'ReportEvents' });
    }
    const probe = spawn('../../../scripts/latency/latency-probe', ['--watch=1', '--label', label, '--out', `${out}/${label}.json`, '--csv', `${out}/${label}.csv`, '--duration', duration, '--amplitude', '500'], { stdio: ['ignore', 'ignore', 'pipe'] });
    let err = '';
    probe.stderr.on('data', (d) => { err += d; });
    const code = await new Promise((r) => probe.on('exit', r));
    if (cdp) {
      const done = new Promise<void>((resolve) => cdp.once('Tracing.tracingComplete', () => resolve()));
      await cdp.send('Tracing.end');
      await done;
      writeFileSync(`${out}/${label}-trace.json`, JSON.stringify({ traceEvents: events }));
    }
    if (code !== 0) throw new Error(`probe: ${err.trim()}`);
    const hold_ = await page.evaluate(() => (window as any).__frameHold?.stats() ?? null);
    const fps = await page.evaluate(() => new Promise<number>((resolve) => { let k = 0; const t0 = performance.now(); const f = () => { if (++k === 60) resolve(Math.round(60000 / (performance.now() - t0))); else requestAnimationFrame(f); }; requestAnimationFrame(f); }));
    if (hold_) writeFileSync(`${out}/${label}-hold.json`, JSON.stringify(hold_, null, 2));
    const j = spawnSync('python3', ['../../../scripts/latency/judder.py', `${out}/${label}.csv`], { encoding: 'utf8' }).stdout.trim().split('\n').pop();
    console.log(`${j}  | page rAF ${fps} Hz${hold_ ? ` | hold: held ${hold_.held} of ${hold_.pending}, disarmed ${hold_.disarmed}, p50 ${hold_.holdMs.p50} ms` : ' | no hold'}`);
  } catch (e) {
    console.log(`FAIL ${label}: ${(e as Error).message}`);
  } finally {
    await close();
  }
}
