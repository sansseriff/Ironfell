import { readdir, readFile, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import type { LatencySample, TrialResult } from '../shared/protocol';

const root = resolve(import.meta.dirname, '../..');
const manualDir = resolve(root, 'manual_results');

interface AppManualAnalysis {
  file: string;
  app: string;
  durationMs: number;
  dragSamples: number;
  eventSamples: number;
  renderSamples: number;
  event: Stats;
  render: Stats;
  movingEvent05: Stats;
  movingEvent1: Stats;
  inputAge: Stats;
  renderGap: Stats;
  bursts100: Burst[];
  timeSeries: Array<{ t: number; eventP95: number | null; renderP95: number | null; eventMax: number | null }>;
}

interface Stats {
  n: number;
  p50: number | null;
  p95: number | null;
  max: number | null;
  mean: number | null;
}

interface Burst {
  startMs: number;
  durationMs: number;
  count: number;
  maxErrorPx: number;
}

async function main() {
  if (!existsSync(manualDir)) {
    throw new Error(`Missing ${manualDir}`);
  }

  const files = (await readdir(manualDir)).filter((file) => file.endsWith('.json')).sort();
  const analyses: AppManualAnalysis[] = [];
  for (const file of files) {
    const parsed = JSON.parse(await readFile(resolve(manualDir, file), 'utf8'));
    for (const { label, result } of unpackResults(file, parsed)) {
      analyses.push(analyze(label, result));
    }
  }

  await writeFile(resolve(manualDir, 'analysis.json'), JSON.stringify({ generatedAt: new Date().toISOString(), analyses }, null, 2));
  await writeFile(resolve(manualDir, 'analysis.html'), renderHtml(analyses));
  console.log(`Wrote ${analyses.length} app analyses to manual_results/analysis.html`);
  printConsole(analyses);
}

function analyze(file: string, result: TrialResult): AppManualAnalysis {
  const samples = result.samples;
  const drag = samples.filter((sample) => sample.phase === 'dragging');
  const events = drag.filter((sample) => sample.frameKind === 'event' && typeof sample.relativeErrorPx === 'number');
  const renders = drag.filter((sample) => sample.frameKind !== 'event' && typeof sample.relativeErrorPx === 'number');
  const t0 = samples[0]?.t ?? 0;
  const t1 = samples.at(-1)?.t ?? t0;

  return {
    file,
    app: result.app,
    durationMs: t1 - t0,
    dragSamples: drag.length,
    eventSamples: events.length,
    renderSamples: renders.length,
    event: stats(events.map((sample) => sample.relativeErrorPx)),
    render: stats(renders.map((sample) => sample.relativeErrorPx)),
    movingEvent05: latencyStats(events.filter((sample) => (sample.referenceSpeedPxPerMs ?? 0) >= 0.5)),
    movingEvent1: latencyStats(events.filter((sample) => (sample.referenceSpeedPxPerMs ?? 0) >= 1),
    ),
    inputAge: inputAgeStats(result, 'dragging'),
    renderGap: renderGapStats(renders),
    bursts100: bursts(events, 100, 100, t0).slice(0, 12),
    timeSeries: timeSeries(events, renders, t0, t1, 250),
  };
}

function unpackResults(file: string, parsed: unknown): Array<{ label: string; result: TrialResult }> {
  if (isManualPackage(parsed)) {
    return parsed.results.map((result, index) => ({
      label: `${file}#${index + 1}_${result.app}`,
      result,
    }));
  }
  return [{ label: file, result: parsed as TrialResult }];
}

function isManualPackage(value: unknown): value is { results: TrialResult[] } {
  return Boolean(
    value
    && typeof value === 'object'
    && Array.isArray((value as { results?: unknown }).results),
  );
}

function stats(values: Array<number | null | undefined>): Stats {
  const filtered = values.filter((value): value is number => value !== null && value !== undefined && Number.isFinite(value));
  return {
    n: filtered.length,
    p50: percentile(filtered, 0.5),
    p95: percentile(filtered, 0.95),
    max: filtered.length ? Math.max(...filtered) : null,
    mean: filtered.length ? filtered.reduce((sum, value) => sum + value, 0) / filtered.length : null,
  };
}

function latencyStats(samples: LatencySample[]) {
  return stats(samples.map((sample) => sample.relativeLatencyMs));
}

function inputAgeStats(result: TrialResult, phase: LatencySample['phase']) {
  const pointers = result.timings.filter((timing) => timing.ty === 'pointer').sort((a, b) => a.t - b.t);
  const ages: number[] = [];
  let pointerIndex = 0;
  for (const sample of result.samples) {
    while (pointerIndex + 1 < pointers.length && pointers[pointerIndex + 1].t <= sample.t) {
      pointerIndex++;
    }
    const pointer = pointers[pointerIndex];
    if (sample.phase === phase && pointer && pointer.t <= sample.t) {
      ages.push(sample.t - pointer.t);
    }
  }
  return stats(ages);
}

function renderGapStats(renderSamples: LatencySample[]) {
  const gaps: number[] = [];
  for (let i = 1; i < renderSamples.length; i++) {
    gaps.push(renderSamples[i].t - renderSamples[i - 1].t);
  }
  return stats(gaps);
}

function bursts(samples: LatencySample[], threshold: number, maxGapMs: number, t0: number): Burst[] {
  const high = samples
    .filter((sample) => (sample.relativeErrorPx ?? 0) >= threshold)
    .sort((a, b) => a.t - b.t);
  const output: Burst[] = [];
  let current: { start: number; end: number; count: number; maxErrorPx: number } | null = null;

  for (const sample of high) {
    if (!current || sample.t - current.end > maxGapMs) {
      if (current) output.push(toBurst(current, t0));
      current = { start: sample.t, end: sample.t, count: 1, maxErrorPx: sample.relativeErrorPx ?? 0 };
    } else {
      current.end = sample.t;
      current.count++;
      current.maxErrorPx = Math.max(current.maxErrorPx, sample.relativeErrorPx ?? 0);
    }
  }
  if (current) output.push(toBurst(current, t0));
  return output.sort((a, b) => b.durationMs - a.durationMs);
}

function toBurst(current: { start: number; end: number; count: number; maxErrorPx: number }, t0: number): Burst {
  return {
    startMs: current.start - t0,
    durationMs: current.end - current.start,
    count: current.count,
    maxErrorPx: current.maxErrorPx,
  };
}

function timeSeries(events: LatencySample[], renders: LatencySample[], t0: number, t1: number, bucketMs: number) {
  const rows: Array<{ t: number; eventP95: number | null; renderP95: number | null; eventMax: number | null }> = [];
  for (let start = t0; start <= t1; start += bucketMs) {
    const end = start + bucketMs;
    const eventValues = events.filter((sample) => sample.t >= start && sample.t < end).map((sample) => sample.relativeErrorPx);
    const renderValues = renders.filter((sample) => sample.t >= start && sample.t < end).map((sample) => sample.relativeErrorPx);
    const validEventValues = eventValues.filter((value): value is number => value !== undefined && Number.isFinite(value));
    rows.push({
      t: start - t0,
      eventP95: percentile(validEventValues, 0.95),
      renderP95: percentile(renderValues.filter((value): value is number => value !== undefined && Number.isFinite(value)), 0.95),
      eventMax: validEventValues.length ? Math.max(...validEventValues) : null,
    });
  }
  return rows;
}

function percentile(values: number[], p: number): number | null {
  const filtered = values.filter(Number.isFinite).sort((a, b) => a - b);
  if (filtered.length === 0) return null;
  return filtered[Math.min(filtered.length - 1, Math.max(0, Math.floor((filtered.length - 1) * p)))];
}

function renderHtml(analyses: AppManualAnalysis[]) {
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Manual Latency Analysis</title>
  <style>
    body { margin: 0; padding: 28px; background: #f2f4f7; color: #17202c; font: 14px system-ui, -apple-system, Segoe UI, sans-serif; }
    h1, h2, p { margin-top: 0; }
    section { margin: 0 0 22px; padding: 16px; border: 1px solid #d8dee6; border-radius: 8px; background: white; }
    table { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
    th, td { padding: 7px 8px; border-bottom: 1px solid #e5e9ef; text-align: left; white-space: nowrap; }
    th { color: #586474; }
    .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(420px, 1fr)); gap: 16px; }
    svg { width: 100%; height: 170px; overflow: visible; background: #fbfcfd; border: 1px solid #e2e7ee; border-radius: 6px; }
    .event { fill: none; stroke: #e53935; stroke-width: 2; }
    .render { fill: none; stroke: #0877ff; stroke-width: 2; }
    .axis { stroke: #9aa5b1; stroke-width: 1; }
    .caption { color: #586474; font-size: 12px; }
  </style>
</head>
<body>
  <h1>Manual Latency Analysis</h1>
  <p>Event line is immediate green-reference separation before the tested renderer catches up. Render line is separation at render samples.</p>
  <section>
    <h2>Summary</h2>
    ${summaryTable(analyses)}
  </section>
  <section>
    <h2>Time Series</h2>
    <div class="grid">${analyses.map(chartCard).join('')}</div>
  </section>
  <section>
    <h2>Largest High-Error Bursts (&gt; 100px)</h2>
    ${burstTable(analyses)}
  </section>
</body>
</html>`;
}

function summaryTable(analyses: AppManualAnalysis[]) {
  return `<table><thead><tr>
    <th>App</th><th>Duration</th><th>Event Err p50</th><th>Event Err p95</th><th>Event Max</th>
    <th>Event Lat p50 >= .5</th><th>Event Lat p95 >= .5</th><th>Render Gap p95</th><th>Input Age p95</th>
  </tr></thead><tbody>
  ${analyses.map((a) => `<tr>
    <td>${a.app}</td><td>${fmt(a.durationMs / 1000)}s</td><td>${fmt(a.event.p50)}px</td><td>${fmt(a.event.p95)}px</td><td>${fmt(a.event.max)}px</td>
    <td>${fmt(a.movingEvent05.p50)}ms</td><td>${fmt(a.movingEvent05.p95)}ms</td><td>${fmt(a.renderGap.p95)}ms</td><td>${fmt(a.inputAge.p95)}ms</td>
  </tr>`).join('')}
  </tbody></table>`;
}

function chartCard(analysis: AppManualAnalysis) {
  const maxY = Math.max(50, ...analysis.timeSeries.flatMap((row) => [row.eventP95 ?? 0, row.renderP95 ?? 0, row.eventMax ?? 0]));
  const maxX = Math.max(1, analysis.durationMs);
  const width = 560;
  const height = 150;
  const point = (row: { t: number }, value: number | null) => {
    const x = (row.t / maxX) * width;
    const y = height - ((value ?? 0) / maxY) * height;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  };
  const eventLine = analysis.timeSeries.filter((row) => row.eventP95 !== null).map((row) => point(row, row.eventP95)).join(' ');
  const renderLine = analysis.timeSeries.filter((row) => row.renderP95 !== null).map((row) => point(row, row.renderP95)).join(' ');
  return `<article>
    <h3>${analysis.app}</h3>
    <svg viewBox="0 0 ${width} ${height}" role="img" aria-label="${analysis.app} time series">
      <line class="axis" x1="0" y1="${height}" x2="${width}" y2="${height}"></line>
      <line class="axis" x1="0" y1="0" x2="0" y2="${height}"></line>
      <polyline class="event" points="${eventLine}"></polyline>
      <polyline class="render" points="${renderLine}"></polyline>
    </svg>
    <p class="caption">Red: event p95 relative error. Blue: render p95 relative error. Y max ${fmt(maxY)}px.</p>
  </article>`;
}

function burstTable(analyses: AppManualAnalysis[]) {
  const rows = analyses.flatMap((analysis) => analysis.bursts100.map((burst) => ({ app: analysis.app, ...burst })))
    .sort((a, b) => b.durationMs - a.durationMs)
    .slice(0, 40);
  return `<table><thead><tr><th>App</th><th>Start</th><th>Duration</th><th>Samples</th><th>Max Error</th></tr></thead><tbody>
    ${rows.map((row) => `<tr><td>${row.app}</td><td>${fmt(row.startMs / 1000)}s</td><td>${fmt(row.durationMs)}ms</td><td>${row.count}</td><td>${fmt(row.maxErrorPx)}px</td></tr>`).join('')}
  </tbody></table>`;
}

function printConsole(analyses: AppManualAnalysis[]) {
  console.log('app\\teventErrP50\\teventErrP95\\teventLatP50>=.5\\teventLatP95>=.5\\trenderGapP95\\tlongestBurstMs');
  for (const a of analyses) {
    console.log([
      a.app,
      fmt(a.event.p50),
      fmt(a.event.p95),
      fmt(a.movingEvent05.p50),
      fmt(a.movingEvent05.p95),
      fmt(a.renderGap.p95),
      fmt(a.bursts100[0]?.durationMs ?? null),
    ].join('\\t'));
  }
}

function fmt(value: number | null | undefined) {
  return value === null || value === undefined || !Number.isFinite(value) ? 'n/a' : value.toFixed(2);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
