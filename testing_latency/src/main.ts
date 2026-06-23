import { createLatencyApp } from './apps';
import { renderAnalysisPage } from './analysis';
import { pointAt, type Point } from './shared/paths';
import {
  APP_IDS,
  DEFAULT_OPTIONS,
  DEFAULT_TRIAL,
  PATH_IDS,
  type TestAppId,
  type TrialConfig,
  type TrialResult,
  type TrialPathId,
} from './shared/protocol';
import { ReferenceOverlay } from './shared/reference-overlay';
import { normalizePointer } from './shared/render';
import './style.css';

declare global {
  interface Window {
    __latency?: {
      ready: boolean;
      appId: TestAppId;
      startTrial: (overrides?: Partial<TrialConfig>) => Promise<void>;
      stopTrial: () => Promise<TrialResult>;
      beginMotion: () => void;
      getCanvasBox: () => DOMRect | null;
    };
  }
}

const root = document.querySelector<HTMLDivElement>('#app');
if (!root) throw new Error('Missing #app root');

const appId = appIdFromLocation();
if (location.pathname === '/analysis') {
  renderAnalysisPage(root);
} else if (appId) {
  renderTestPage(root, appId).catch((error) => {
    root.innerHTML = `<main class="page"><h1>Latency Test Error</h1><pre>${escapeHtml(String(error?.stack || error))}</pre></main>`;
  });
} else {
  renderIndex(root);
}

function renderIndex(container: HTMLElement) {
  container.innerHTML = `
    <main class="page">
      <h1>Iron Latency Testing</h1>
      <p class="lede">Open one isolated test page at a time. The report page should use saved JSON results rather than running multiple canvases side by side.</p>
      <section class="panel">
        <h2>Test Pages</h2>
        <div class="link-grid">
          ${APP_IDS.map((id) => `<a href="/test/${id}">${id}</a>`).join('')}
        </div>
      </section>
      <section class="panel">
        <h2>Analysis</h2>
        <p>Generate compact chart data after an automated run, then open the analysis page.</p>
        <pre>npm run run
npm run analyze</pre>
        <p><a href="/analysis">Open analysis page</a></p>
      </section>
      <section class="panel">
        <h2>Runner</h2>
        <pre>cd testing_latency
npm install
npm run run</pre>
      </section>
    </main>
  `;
}

async function renderTestPage(container: HTMLElement, appId: TestAppId) {
  container.innerHTML = `
    <main class="test-page">
      <header class="topbar">
        <div>
          <a href="/" class="back-link">Latency tests</a>
          <h1>${appId}</h1>
        </div>
        <div class="controls">
          <label>
            Path
            <select id="path-select">
              ${PATH_IDS.map((id) => `<option value="${id}">${id}</option>`).join('')}
            </select>
          </label>
          <button id="start-button">Start Manual Trial</button>
          <button id="stop-button">Stop</button>
          <button id="download-button" disabled>Download Last JSON</button>
        </div>
      </header>
      <section class="stage-wrap">
        <div id="stage" class="stage"></div>
      </section>
      <section class="result-panel">
        <h2>Last Result</h2>
        <pre id="result-output">No trial recorded yet.</pre>
      </section>
    </main>
  `;

  const stage = container.querySelector<HTMLElement>('#stage');
  const output = container.querySelector<HTMLElement>('#result-output');
  const pathSelect = container.querySelector<HTMLSelectElement>('#path-select');
  const startButton = container.querySelector<HTMLButtonElement>('#start-button');
  const stopButton = container.querySelector<HTMLButtonElement>('#stop-button');
  const downloadButton = container.querySelector<HTMLButtonElement>('#download-button');
  if (!stage || !output || !pathSelect || !startButton || !stopButton || !downloadButton) throw new Error('Missing test page elements');

  const testApp = createLatencyApp(appId);
  let lastResult: TrialResult | null = null;
  let overlay: ReferenceOverlay | null = null;
  let overlayEnabled = false;
  const options = {
    ...DEFAULT_OPTIONS,
    dpr: window.devicePixelRatio || 1,
  };
  await testApp.mount(stage, options);
  overlay = new ReferenceOverlay(stage, options);
  testApp.setSampleAugmenter?.(() => {
    if (!overlayEnabled || !overlay) return null;
    const snapshot = overlay.snapshot();
    return { reference: snapshot.point, speedPxPerMs: snapshot.speedPxPerMs };
  });

  const makeTrial = (overrides: Partial<TrialConfig> = {}): TrialConfig => ({
    ...DEFAULT_TRIAL,
    app: appId,
    path: (pathSelect.value as TrialPathId) || DEFAULT_TRIAL.path,
    ...overrides,
  });

  window.__latency = {
    ready: true,
    appId,
    startTrial: async (overrides = {}) => {
      const trial = makeTrial(overrides);
      overlayEnabled = trial.referenceMode === 'event';
      overlay?.start(pointAt(trial, 0));
      await testApp.startTrial(trial);
    },
    beginMotion: () => testApp.beginMotion(),
    stopTrial: async () => {
      const result = await testApp.stopTrial();
      output.textContent = JSON.stringify(result.summary, null, 2);
      return result;
    },
    getCanvasBox: () => stage.querySelector('canvas')?.getBoundingClientRect() || null,
  };

  startButton.addEventListener('click', async () => {
    lastResult = null;
    downloadButton.disabled = true;
    output.textContent = 'Recording manual browser-event telemetry. Drag the square, then press Stop.';
    await window.__latency?.startTrial({ referenceMode: 'event' });
  });
  stopButton.addEventListener('click', async () => {
    const result = await window.__latency?.stopTrial();
    lastResult = result || null;
    downloadButton.disabled = !lastResult;
    output.textContent = JSON.stringify({
      metric: 'Manual mode uses latest browser pointer events as the reference. It is not true mouse-to-photon latency.',
      relativeMetric: 'relativeLatencyMs compares this app against the green immediate main-thread 2D reference overlay.',
      summary: result?.summary,
      draggingRelative: summarizeRelativeDragging(result),
    }, null, 2);
  });
  downloadButton.addEventListener('click', () => {
    if (!lastResult) return;
    const stamp = new Date().toISOString().replaceAll(':', '-').replaceAll('.', '-');
    downloadJson(lastResult, `${stamp}_${appId}_manual.json`);
  });

  attachOverlayPointerForwarding(stage, overlay, () => overlayEnabled, () => lastRenderedCenterFromResult(lastResult) || pointAt({ ...DEFAULT_TRIAL, app: appId }, 0));
}

function appIdFromLocation(): TestAppId | null {
  const pathMatch = location.pathname.match(/^\/test\/([^/]+)$/);
  const candidate = (pathMatch?.[1] || new URLSearchParams(location.search).get('app')) as TestAppId | null;
  return candidate && APP_IDS.includes(candidate) ? candidate : null;
}

function escapeHtml(value: string) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
}

function downloadJson(value: unknown, fileName: string) {
  const blob = new Blob([JSON.stringify(value, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = fileName;
  link.click();
  URL.revokeObjectURL(url);
}

function attachOverlayPointerForwarding(
  stage: HTMLElement,
  overlay: ReferenceOverlay | null,
  isEnabled: () => boolean,
  fallbackRendered: () => Point,
) {
  if (!overlay) return;
  const canvas = stage.querySelector('canvas.test-canvas');
  if (!(canvas instanceof HTMLCanvasElement)) return;

  const onDown = (event: PointerEvent) => {
    if (!isEnabled()) return;
    const point = normalizePointer(event, canvas);
    overlay.pointerDown(point, fallbackRendered());
  };
  const onMove = (event: PointerEvent) => {
    if (!isEnabled()) return;
    overlay.pointerMove(normalizePointer(event, canvas));
  };
  const onUp = (event: PointerEvent) => {
    if (!isEnabled()) return;
    overlay.pointerUp(normalizePointer(event, canvas));
  };

  canvas.addEventListener('pointerdown', onDown, { capture: true });
  canvas.addEventListener('pointermove', onMove, { capture: true });
  canvas.addEventListener('pointerup', onUp, { capture: true });
}

function lastRenderedCenterFromResult(result: TrialResult | null): Point | null {
  const last = result?.samples.at(-1);
  return last ? { x: last.renderedX, y: last.renderedY } : null;
}

function summarizeRelativeDragging(result: TrialResult | undefined) {
  if (!result) return null;
  const latencies = result.samples
    .filter((sample) => sample.phase === 'dragging')
    .map((sample) => sample.relativeLatencyMs)
    .filter((value): value is number => value !== null && value !== undefined && Number.isFinite(value))
    .sort((a, b) => a - b);
  const errors = result.samples
    .filter((sample) => sample.phase === 'dragging')
    .map((sample) => sample.relativeErrorPx)
    .filter((value): value is number => value !== undefined && Number.isFinite(value))
    .sort((a, b) => a - b);

  return {
    sampleCount: latencies.length,
    p50LatencyMs: percentile(latencies, 0.5),
    p95LatencyMs: percentile(latencies, 0.95),
    p50ErrorPx: percentile(errors, 0.5),
    p95ErrorPx: percentile(errors, 0.95),
  };
}

function percentile(values: number[], p: number) {
  if (values.length === 0) return null;
  const index = Math.min(values.length - 1, Math.max(0, Math.floor((values.length - 1) * p)));
  return values[index];
}
