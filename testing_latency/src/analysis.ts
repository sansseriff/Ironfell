import {
  ANALYSIS_GENERATED_AT,
  ANALYSIS_RESULTS,
  type CompactAnalysisResult,
} from './generated/results-data';

const SMOOTH_PATHS = new Set(['constant-horizontal', 'sine-horizontal']);

export function renderAnalysisPage(container: HTMLElement) {
  const results = ANALYSIS_RESULTS;
  if (results.length === 0) {
    container.innerHTML = `
      <main class="page">
        <h1>Latency Analysis</h1>
        <p>No generated results are available yet.</p>
        <pre>npm run run
npm run analyze
npm run dev</pre>
      </main>
    `;
    return;
  }

  const smooth = results.filter((result) => SMOOTH_PATHS.has(result.path));
  const jitter = results.filter((result) => result.path === 'micro-jitter');
  const steps = results.filter((result) => result.path === 'step-horizontal');

  container.innerHTML = `
    <main class="page analysis-page">
      <header class="analysis-header">
        <div>
          <a href="/" class="back-link">Latency tests</a>
          <h1>Latency Analysis</h1>
          <p class="lede">Generated ${formatDate(ANALYSIS_GENERATED_AT)} from ${results.length} automated trial JSON files.</p>
        </div>
      </header>

      <section class="panel">
        <h2>Smooth Path Overview</h2>
        <p class="note">Constant and sine paths are the most reliable visual-latency estimates because pointer speed is continuous.</p>
        ${renderSmoothOverview(smooth)}
      </section>

      <section class="panel">
        <h2>Tail Behavior</h2>
        <p class="note">Micro-jitter stresses coalescing, event timing, and frame handoff. P95 and max matter more than p50 here.</p>
        ${renderJitterOverview(jitter)}
      </section>

      <section class="panel">
        <h2>Step Response</h2>
        <p class="note">The step path has long zero-speed periods, so latency percentiles can be misleading. Max position error is the better signal.</p>
        ${renderStepOverview(steps)}
      </section>

      <section class="panel">
        <h2>Latency Histograms</h2>
        <div class="hist-grid">
          ${results.map(renderHistogramCard).join('')}
        </div>
      </section>

      <section class="panel">
        <h2>All Results</h2>
        ${renderResultTable(results)}
      </section>
    </main>
  `;
}

function renderSmoothOverview(results: CompactAnalysisResult[]) {
  const byApp = groupBy(results, (result) => result.app);
  const rows = [...byApp.entries()]
    .map(([app, appResults]) => {
      const p50 = mean(appResults.map((result) => result.summary.p50LatencyMs));
      const p95 = mean(appResults.map((result) => result.summary.p95LatencyMs));
      const inputAge = mean(appResults.flatMap((result) => result.inputAgeMs));
      const frameP95 = percentile(appResults.flatMap((result) => result.frameIntervalsMs), 0.95);
      return { app, p50, p95, inputAge, frameP95 };
    })
    .sort((a, b) => safe(a.p50) - safe(b.p50));

  const max = Math.max(...rows.map((row) => safe(row.p95)), 1);
  return `
    <div class="bar-list">
      ${rows.map((row) => renderBarRow(row.app, row.p50, row.p95, max)).join('')}
    </div>
    <table>
      <thead><tr><th>App</th><th>Mean p50</th><th>Mean p95</th><th>Input Age p50-ish</th><th>Frame Interval p95</th></tr></thead>
      <tbody>
        ${rows.map((row) => `
          <tr>
            <td>${row.app}</td>
            <td>${fmt(row.p50)} ms</td>
            <td>${fmt(row.p95)} ms</td>
            <td>${fmt(row.inputAge)} ms</td>
            <td>${fmt(row.frameP95)} ms</td>
          </tr>
        `).join('')}
      </tbody>
    </table>
  `;
}

function renderJitterOverview(results: CompactAnalysisResult[]) {
  const rows = results
    .map((result) => ({
      app: result.app,
      p50: result.summary.p50LatencyMs,
      p95: result.summary.p95LatencyMs,
      max: result.summary.maxLatencyMs,
      error: result.summary.meanErrorPx,
    }))
    .sort((a, b) => safe(a.p95) - safe(b.p95));
  const max = Math.max(...rows.map((row) => safe(row.p95)), 1);
  return `
    <div class="bar-list">
      ${rows.map((row) => renderBarRow(row.app, row.p50, row.p95, max)).join('')}
    </div>
    <table>
      <thead><tr><th>App</th><th>p50</th><th>p95</th><th>Max</th><th>Mean Error</th></tr></thead>
      <tbody>
        ${rows.map((row) => `
          <tr>
            <td>${row.app}</td>
            <td>${fmt(row.p50)} ms</td>
            <td>${fmt(row.p95)} ms</td>
            <td>${fmt(row.max)} ms</td>
            <td>${fmt(row.error)} px</td>
          </tr>
        `).join('')}
      </tbody>
    </table>
  `;
}

function renderStepOverview(results: CompactAnalysisResult[]) {
  const rows = results
    .map((result) => ({
      app: result.app,
      meanError: result.summary.meanErrorPx,
      maxError: result.summary.maxErrorPx,
      p50: result.summary.p50LatencyMs,
    }))
    .sort((a, b) => safe(a.meanError) - safe(b.meanError));
  return `
    <table>
      <thead><tr><th>App</th><th>Mean Error</th><th>Max Error</th><th>Reported p50</th></tr></thead>
      <tbody>
        ${rows.map((row) => `
          <tr>
            <td>${row.app}</td>
            <td>${fmt(row.meanError)} px</td>
            <td>${fmt(row.maxError)} px</td>
            <td>${fmt(row.p50)} ms</td>
          </tr>
        `).join('')}
      </tbody>
    </table>
  `;
}

function renderHistogramCard(result: CompactAnalysisResult) {
  const bins = histogram(result.latenciesMs, [0, 5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240]);
  const max = Math.max(...bins.map((bin) => bin.count), 1);
  return `
    <article class="hist-card">
      <header>
        <strong>${result.app}</strong>
        <span>${result.path}</span>
      </header>
      <svg viewBox="0 0 260 120" role="img" aria-label="Latency histogram for ${result.app} ${result.path}">
        ${bins.map((bin, index) => {
          const barWidth = 260 / bins.length;
          const height = (bin.count / max) * 92;
          const x = index * barWidth + 2;
          const y = 104 - height;
          return `<rect x="${x}" y="${y}" width="${Math.max(2, barWidth - 4)}" height="${height}" rx="2"></rect>`;
        }).join('')}
        <line x1="0" y1="104" x2="260" y2="104"></line>
      </svg>
      <div class="hist-meta">
        <span>p50 ${fmt(result.summary.p50LatencyMs)} ms</span>
        <span>p95 ${fmt(result.summary.p95LatencyMs)} ms</span>
      </div>
    </article>
  `;
}

function renderResultTable(results: CompactAnalysisResult[]) {
  const rows = [...results].sort((a, b) => a.app.localeCompare(b.app) || a.path.localeCompare(b.path));
  return `
    <table>
      <thead>
        <tr>
          <th>App</th>
          <th>Path</th>
          <th>p50</th>
          <th>p95</th>
          <th>Drag p50</th>
          <th>Drag p95</th>
          <th>Rel Drag p50</th>
          <th>Rel Drag p95</th>
          <th>Mean Error</th>
          <th>Drag Err p95</th>
          <th>Input Age p50</th>
          <th>Frame p95</th>
          <th>Samples</th>
        </tr>
      </thead>
      <tbody>
        ${rows.map((result) => `
          <tr>
            <td>${result.app}</td>
            <td>${result.path}</td>
            <td>${fmt(result.summary.p50LatencyMs)}</td>
            <td>${fmt(result.summary.p95LatencyMs)}</td>
            <td>${fmt(percentile(result.draggingLatenciesMs || [], 0.5))}</td>
            <td>${fmt(percentile(result.draggingLatenciesMs || [], 0.95))}</td>
            <td>${fmt(percentile(result.draggingRelativeLatenciesMs || [], 0.5))}</td>
            <td>${fmt(percentile(result.draggingRelativeLatenciesMs || [], 0.95))}</td>
            <td>${fmt(result.summary.meanErrorPx)} px</td>
            <td>${fmt(percentile(result.draggingErrorsPx || [], 0.95))} px</td>
            <td>${fmt(percentile(result.inputAgeMs, 0.5))}</td>
            <td>${fmt(percentile(result.frameIntervalsMs, 0.95))}</td>
            <td>${result.sampleCount}</td>
          </tr>
        `).join('')}
      </tbody>
    </table>
  `;
}

function renderBarRow(label: string, p50: number | null, p95: number | null, max: number) {
  const p50Width = Math.max(0, Math.min(100, (safe(p50) / max) * 100));
  const p95Width = Math.max(0, Math.min(100, (safe(p95) / max) * 100));
  return `
    <div class="bar-row">
      <div class="bar-label">${label}</div>
      <div class="bar-track">
        <div class="bar bar-p95" style="width: ${p95Width}%"></div>
        <div class="bar bar-p50" style="width: ${p50Width}%"></div>
      </div>
      <div class="bar-value">p50 ${fmt(p50)} / p95 ${fmt(p95)} ms</div>
    </div>
  `;
}

function histogram(values: number[], edges: number[]) {
  return edges.map((edge, index) => {
    const next = edges[index + 1] ?? Number.POSITIVE_INFINITY;
    const label = Number.isFinite(next) ? `${edge}-${next}` : `${edge}+`;
    const count = values.filter((value) => value >= edge && value < next).length;
    return { label, count };
  });
}

function groupBy<T>(items: T[], getKey: (item: T) => string) {
  const map = new Map<string, T[]>();
  for (const item of items) {
    const key = getKey(item);
    const list = map.get(key) || [];
    list.push(item);
    map.set(key, list);
  }
  return map;
}

function mean(values: Array<number | null>) {
  const filtered = values.filter((value): value is number => value !== null && Number.isFinite(value));
  if (filtered.length === 0) return null;
  return filtered.reduce((sum, value) => sum + value, 0) / filtered.length;
}

function percentile(values: number[], p: number) {
  const filtered = values.filter((value) => Number.isFinite(value)).sort((a, b) => a - b);
  if (filtered.length === 0) return null;
  const index = Math.min(filtered.length - 1, Math.max(0, Math.floor((filtered.length - 1) * p)));
  return filtered[index];
}

function safe(value: number | null) {
  return value ?? Number.POSITIVE_INFINITY;
}

function fmt(value: number | null) {
  if (value === null || !Number.isFinite(value)) return 'n/a';
  return value.toFixed(2);
}

function formatDate(value: string) {
  if (!value) return 'never';
  return new Date(value).toLocaleString();
}
