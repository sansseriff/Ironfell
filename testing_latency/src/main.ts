import { createLatencyApp } from './apps';
import { renderAnalysisPage } from './analysis';
import { pointAt, type Point } from './shared/paths';
import {
  APP_IDS,
  APP_DESCRIPTIONS,
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
import {
  createGuidedSession,
  nextIncompleteStep,
  requireGuidedSession,
  saveGuidedResult,
  skipGuidedStep,
  type GuidedSession,
  type GuidedStep,
} from './shared/guided-store';
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
} else if (location.pathname === '/guided/complete') {
  renderGuidedCompletePage(root).catch((error) => {
    root.innerHTML = `<main class="page"><h1>Guided Export Error</h1><pre>${escapeHtml(String(error?.stack || error))}</pre></main>`;
  });
} else if (location.pathname === '/guided') {
  renderGuidedPage(root).catch((error) => {
    root.innerHTML = `<main class="page"><h1>Guided Test Error</h1><pre>${escapeHtml(String(error?.stack || error))}</pre></main>`;
  });
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
          ${APP_IDS.map((id) => `<a href="/test/${id}" title="${escapeHtml(APP_DESCRIPTIONS[id])}">${id}</a>`).join('')}
        </div>
      </section>
      <section class="panel">
        <h2>Guided Manual Run</h2>
        <p>Run selected tests in one session, keep every JSON result client-side, then save one package at the end.</p>
        <p><a href="/guided">Open guided runner</a></p>
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

interface ManualPackage {
  schema: 'iron-latency-manual-package/v1';
  createdAtIso: string;
  platformLabel: string;
  browserLabel: string;
  userAgent: string;
  dpr: number;
  requestedApps: TestAppId[];
  requestedPaths: TrialPathId[];
  results: TrialResult[];
}

async function renderGuidedPage(container: HTMLElement) {
  container.innerHTML = `
    <main class="page guided-page">
      <header class="topbar">
        <div>
          <a href="/" class="back-link">Latency tests</a>
          <h1>Guided Manual Run</h1>
        </div>
        <div class="controls">
          <button id="start-session-button">Start Session</button>
        </div>
      </header>
      <section class="panel">
        <h2>Session</h2>
        <p class="note">Each selected app runs on its own URL. Results are saved to IndexedDB after every step, then exported as one package at the end.</p>
        <label class="field-label">Platform
          <input id="platform-input" placeholder="macOS M3 Pro" autocomplete="off">
        </label>
        <label class="field-label">Browser
          <input id="browser-input" placeholder="Chrome 126" autocomplete="off">
        </label>
        <label class="field-label">Path
          <select id="guided-path-select">
            ${PATH_IDS.map((id) => `<option value="${id}" ${id === DEFAULT_TRIAL.path ? 'selected' : ''}>${id}</option>`).join('')}
          </select>
        </label>
      </section>
      <section class="panel">
        <h2>Apps</h2>
        <div class="check-list">
          ${APP_IDS.map((id) => `
            <label>
              <input type="checkbox" value="${id}" checked>
              <span>${id}</span>
            </label>
          `).join('')}
        </div>
      </section>
      <section class="panel">
        <h2>Status</h2>
        <pre id="guided-output">No session started.</pre>
      </section>
    </main>
  `;

  const output = container.querySelector<HTMLElement>('#guided-output');
  const startButton = container.querySelector<HTMLButtonElement>('#start-session-button');
  const platformInput = container.querySelector<HTMLInputElement>('#platform-input');
  const browserInput = container.querySelector<HTMLInputElement>('#browser-input');
  const pathSelect = container.querySelector<HTMLSelectElement>('#guided-path-select');
  if (!output || !startButton || !platformInput || !browserInput || !pathSelect) {
    throw new Error('Missing guided runner elements');
  }
  const outputEl = output;
  const startButtonEl = startButton;
  const platformInputEl = platformInput;
  const browserInputEl = browserInput;
  const pathSelectEl = pathSelect;

  startButtonEl.addEventListener('click', async () => {
    const selectedApps = selectedGuidedApps(container);
    if (selectedApps.length === 0) {
      outputEl.textContent = 'Select at least one app.';
      return;
    }
    startButtonEl.disabled = true;
    outputEl.textContent = 'Creating guided session...';
    const path = pathSelectEl.value as TrialPathId;
    const steps = selectedApps.map((app): GuidedStep => ({ app, path }));
    const session = await createGuidedSession({
      platformLabel: platformInputEl.value.trim() || 'unlabeled-platform',
      browserLabel: browserInputEl.value.trim() || 'unlabeled-browser',
      steps,
    });
    navigateToGuidedStep(session, 0);
  });
}

async function renderTestPage(container: HTMLElement, appId: TestAppId) {
  const guidedContext = guidedContextFromLocation();
  container.innerHTML = `
    <main class="test-page">
      <header class="topbar">
        <div>
          <a href="/" class="back-link">Latency tests</a>
          <h1>${appId}</h1>
          ${guidedContext ? '<p class="guided-step-label" id="guided-step-label">Guided step</p>' : ''}
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
          ${guidedContext ? '<button id="skip-guided-button">Skip Step</button>' : ''}
        </div>
      </header>
      ${guidedContext ? '<section id="guided-step-panel" class="guided-instructions">Loading guided session...</section>' : ''}
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
  const skipGuidedButton = container.querySelector<HTMLButtonElement>('#skip-guided-button');
  const guidedStepPanel = container.querySelector<HTMLElement>('#guided-step-panel');
  const guidedStepLabel = container.querySelector<HTMLElement>('#guided-step-label');
  if (!stage || !output || !pathSelect || !startButton || !stopButton || !downloadButton) throw new Error('Missing test page elements');

  let guidedSession: GuidedSession | null = null;
  let guidedStepIndex = -1;
  if (guidedContext) {
    guidedSession = await requireGuidedSession(guidedContext.sessionId);
    guidedStepIndex = guidedContext.stepIndex;
    const step = guidedSession.steps[guidedStepIndex];
    if (!step) throw new Error(`Missing guided step ${guidedStepIndex + 1}`);
    if (step.app !== appId) throw new Error(`Guided step expected ${step.app}, but this page is ${appId}`);
    pathSelect.value = step.path;
    pathSelect.disabled = true;
    startButton.textContent = 'Start Guided Step';
    stopButton.textContent = 'Stop and Save Step';
    downloadButton.style.display = 'none';
    if (guidedStepPanel) {
      guidedStepPanel.textContent = `Guided session ${guidedStepIndex + 1} of ${guidedSession.steps.length}. Drag the square, then save this step. A full navigation will load the next app with a fresh JS/Wasm context.`;
    }
    if (guidedStepLabel) {
      guidedStepLabel.textContent = `Guided step ${guidedStepIndex + 1} of ${guidedSession.steps.length}`;
    }
  }

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
    output.textContent = guidedSession
      ? 'Recording guided telemetry. Drag the square, then press Stop and Save Step.'
      : 'Recording manual browser-event telemetry. Drag the square, then press Stop.';
    await window.__latency?.startTrial({ referenceMode: 'event' });
  });
  stopButton.addEventListener('click', async () => {
    const result = await window.__latency?.stopTrial();
    lastResult = result || null;
    downloadButton.disabled = !lastResult;
    if (guidedSession && result) {
      output.textContent = 'Saving guided result...';
      const updated = await saveGuidedResult(guidedSession.id, guidedStepIndex, result);
      navigateToNextGuidedStep(updated, guidedStepIndex);
      return;
    }
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
  skipGuidedButton?.addEventListener('click', async () => {
    if (!guidedSession) return;
    output.textContent = 'Skipping guided step...';
    const updated = await skipGuidedStep(guidedSession.id, guidedStepIndex);
    navigateToNextGuidedStep(updated, guidedStepIndex);
  });

  attachOverlayPointerForwarding(stage, overlay, () => overlayEnabled, () => lastRenderedCenterFromResult(lastResult) || pointAt({ ...DEFAULT_TRIAL, app: appId }, 0));
}

function appIdFromLocation(): TestAppId | null {
  const pathMatch = location.pathname.match(/^\/test\/([^/]+)$/);
  const candidate = (pathMatch?.[1] || new URLSearchParams(location.search).get('app')) as TestAppId | null;
  return candidate && APP_IDS.includes(candidate) ? candidate : null;
}

function guidedContextFromLocation(): { sessionId: string; stepIndex: number } | null {
  const params = new URLSearchParams(location.search);
  const sessionId = params.get('guidedSession');
  const step = params.get('step');
  if (!sessionId || step === null) return null;
  const stepIndex = Number(step);
  if (!Number.isInteger(stepIndex) || stepIndex < 0) return null;
  return { sessionId, stepIndex };
}

function navigateToGuidedStep(session: GuidedSession, stepIndex: number) {
  const step = session.steps[stepIndex];
  if (!step) {
    location.href = `/guided/complete?session=${encodeURIComponent(session.id)}`;
    return;
  }
  location.href = `/test/${step.app}?guidedSession=${encodeURIComponent(session.id)}&step=${stepIndex}`;
}

function navigateToNextGuidedStep(session: GuidedSession, currentStepIndex: number) {
  const next = nextIncompleteStep(session, currentStepIndex);
  if (next === -1) {
    location.href = `/guided/complete?session=${encodeURIComponent(session.id)}`;
    return;
  }
  navigateToGuidedStep(session, next);
}

async function renderGuidedCompletePage(container: HTMLElement) {
  const sessionId = new URLSearchParams(location.search).get('session');
  if (!sessionId) throw new Error('Missing guided session id');
  const session = await requireGuidedSession(sessionId);
  const results = session.results.filter((result): result is TrialResult => Boolean(result));
  const pack = makeManualPackage(session.platformLabel, session.browserLabel, session.steps, results);

  container.innerHTML = `
    <main class="page">
      <header class="analysis-header">
        <div>
          <a href="/guided" class="back-link">Guided runner</a>
          <h1>Guided Run Complete</h1>
          <p class="lede">${results.length} result(s) saved from ${session.steps.length} requested step(s).</p>
        </div>
        <div class="controls">
          <button id="save-package-button" ${results.length === 0 ? 'disabled' : ''}>Save Package</button>
        </div>
      </header>
      <section class="panel">
        <h2>Session</h2>
        <table>
          <tbody>
            <tr><th>Platform</th><td>${escapeHtml(session.platformLabel)}</td></tr>
            <tr><th>Browser</th><td>${escapeHtml(session.browserLabel)}</td></tr>
            <tr><th>Created</th><td>${escapeHtml(session.createdAtIso)}</td></tr>
            <tr><th>Updated</th><td>${escapeHtml(session.updatedAtIso)}</td></tr>
          </tbody>
        </table>
      </section>
      <section class="panel">
        <h2>Steps</h2>
        ${renderGuidedStepTable(session)}
      </section>
      <section class="panel">
        <h2>Package Preview</h2>
        <pre>${escapeHtml(JSON.stringify({
          schema: pack.schema,
          platformLabel: pack.platformLabel,
          browserLabel: pack.browserLabel,
          requestedApps: pack.requestedApps,
          requestedPaths: pack.requestedPaths,
          results: pack.results.length,
        }, null, 2))}</pre>
      </section>
    </main>
  `;

  const saveButton = container.querySelector<HTMLButtonElement>('#save-package-button');
  saveButton?.addEventListener('click', () => saveManualPackage(pack));
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

function selectedGuidedApps(container: HTMLElement): TestAppId[] {
  return [...container.querySelectorAll<HTMLInputElement>('.check-list input[type="checkbox"]:checked')]
    .map((input) => input.value as TestAppId)
    .filter((id) => APP_IDS.includes(id));
}

function renderProgress(progress: HTMLOListElement, steps: Array<{ app: TestAppId; path: TrialPathId }>, stepIndex: number, results: TrialResult[]) {
  const completed = new Set(results.map((result) => `${result.app}:${result.path}`));
  progress.innerHTML = steps.map((step, index) => {
    const key = `${step.app}:${step.path}`;
    const status = completed.has(key) ? 'saved' : index === stepIndex ? 'running' : index < stepIndex ? 'skipped' : 'pending';
    return `<li class="${status}"><span>${step.app}</span><small>${status}</small></li>`;
  }).join('');
}

function renderGuidedStepTable(session: GuidedSession) {
  return `
    <table>
      <thead><tr><th>#</th><th>App</th><th>Path</th><th>Status</th><th>Samples</th><th>Rel Drag p95</th></tr></thead>
      <tbody>
        ${session.steps.map((step, index) => {
          const result = session.results[index];
          const status = result ? 'saved' : session.skipped[index] ? 'skipped' : 'pending';
          const relative = result ? summarizeRelativeDragging(result) : null;
          return `
            <tr>
              <td>${index + 1}</td>
              <td>${escapeHtml(step.app)}</td>
              <td>${escapeHtml(step.path)}</td>
              <td>${status}</td>
              <td>${result?.samples.length ?? ''}</td>
              <td>${relative?.p95LatencyMs === null || relative?.p95LatencyMs === undefined ? '' : relative.p95LatencyMs.toFixed(2)}</td>
            </tr>
          `;
        }).join('')}
      </tbody>
    </table>
  `;
}

function makeManualPackage(
  platformLabel: string,
  browserLabel: string,
  steps: Array<{ app: TestAppId; path: TrialPathId }>,
  results: TrialResult[],
): ManualPackage {
  return {
    schema: 'iron-latency-manual-package/v1',
    createdAtIso: new Date().toISOString(),
    platformLabel,
    browserLabel,
    userAgent: navigator.userAgent,
    dpr: window.devicePixelRatio || 1,
    requestedApps: [...new Set(steps.map((step) => step.app))],
    requestedPaths: [...new Set(steps.map((step) => step.path))],
    results,
  };
}

async function saveManualPackage(pack: ManualPackage) {
  const stamp = pack.createdAtIso.replaceAll(':', '-').replaceAll('.', '-');
  const fileName = `${stamp}_${safeFilePart(pack.platformLabel)}_${safeFilePart(pack.browserLabel)}_latency_package.json`;
  const json = JSON.stringify(pack, null, 2);
  const picker = (window as any).showSaveFilePicker;
  if (typeof picker === 'function') {
    const handle = await picker({
      suggestedName: fileName,
      types: [{
        description: 'Latency JSON package',
        accept: { 'application/json': ['.json'] },
      }],
    });
    const writable = await handle.createWritable();
    await writable.write(new Blob([json], { type: 'application/json' }));
    await writable.close();
    return;
  }
  const blob = new Blob([json], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = fileName;
  link.click();
  URL.revokeObjectURL(url);
}

function safeFilePart(value: string) {
  return value.trim().toLowerCase().replace(/[^a-z0-9._-]+/g, '-').replace(/^-+|-+$/g, '') || 'unlabeled';
}

function attachOverlayPointerForwarding(
  stage: HTMLElement,
  overlay: ReferenceOverlay | null,
  isEnabled: () => boolean,
  fallbackRendered: () => Point,
) {
  if (!overlay) return null;
  const canvas = stage.querySelector('canvas.test-canvas');
  if (!(canvas instanceof HTMLCanvasElement)) return null;

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
  return () => {
    canvas.removeEventListener('pointerdown', onDown, { capture: true });
    canvas.removeEventListener('pointermove', onMove, { capture: true });
    canvas.removeEventListener('pointerup', onUp, { capture: true });
  };
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
