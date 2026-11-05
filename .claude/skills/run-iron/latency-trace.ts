/**
 * A Chrome trace (compositor, GPU, canvas) of one Latency view measurement
 * (plans/rendering/latency-view.md §7): headed Chrome, the probe drives the OS
 * pointer while CDP tracing runs. Hands off the mouse.
 *
 *   URL='http://127.0.0.1:5185/Ironfell/?demo&latency=solo&runtime=worker' NAME=solo bun run latency-trace.ts
 *
 * Writes .agents/latency/trace/<NAME>.json (Chrome trace format) and the
 * probe's JSON beside it.
 */
import { launchChrome } from './latency-chrome';
import { mkdirSync, writeFileSync } from 'node:fs';
import { spawn } from 'node:child_process';

const out = '../../../.agents/latency/trace';
mkdirSync(out, { recursive: true });
const name = process.env.NAME ?? 'trace';
const { context, close } = await launchChrome();
try {
  const page = context.pages()[0] ?? await context.newPage();
  await page.goto(process.env.URL!);
  await page.waitForFunction(() => document.querySelector('[data-latency-view]')?.getAttribute('data-ready') === 'true', {}, { timeout: 60000 });
  await page.bringToFront();
  await page.waitForTimeout(1500);
  const cdp = await context.newCDPSession(page);
  const categories = (process.env.CATEGORIES ?? 'viz,gpu,cc,benchmark,blink,devtools.timeline,disabled-by-default-devtools.timeline.frame,disabled-by-default-viz.surface_id_flow,disabled-by-default-viz.surface_lifetime,disabled-by-default-cc.debug.scheduler.frames,input').split(',');
  const events: unknown[] = [];
  cdp.on('Tracing.dataCollected', (e: any) => events.push(...e.value));
  const done = new Promise<void>((resolve) => cdp.once('Tracing.tracingComplete', () => resolve()));
  await cdp.send('Tracing.start', { traceConfig: { includedCategories: categories, recordMode: 'recordAsMuchAsPossible' }, transferMode: 'ReportEvents' });
  const probe = spawn('../../../scripts/latency/latency-probe', ['--label', name, '--out', `${out}/${name}-probe.json`, '--csv', `${out}/${name}-probe.csv`, '--duration', process.env.DURATION ?? '3'], { stdio: ['ignore', 'ignore', 'inherit'] });
  await new Promise((r) => probe.on('exit', r));
  await cdp.send('Tracing.end');
  await done;
  writeFileSync(`${out}/${name}.json`, JSON.stringify({ traceEvents: events }));
  console.log(`${name}: ${events.length} trace events`);
} finally {
  await close();
}
