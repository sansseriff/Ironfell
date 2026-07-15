import {
  DEFAULT_OPTIONS,
  type LatencyTestApp,
  type TestAppId,
  type TestOptions,
  type TrialConfig,
  type TrialResult,
} from '../shared/protocol';
import { estimateRefreshHz, TrialRecorder } from '../shared/recorder';

type ProbeMode = 'plain' | 'webgpu';

interface RafPoint {
  seq: number;
  now: number;
  absNow: number;
}

interface WorkerRafPoint extends RafPoint {
  renderStartNow: number;
  renderEndNow: number;
  absRenderStartNow: number;
  absRenderEndNow: number;
}

interface PhaseSummary {
  mainHz?: number;
  workerHz?: number;
  phaseDeltaMeanMs?: number;
  phaseDeltaMinMs?: number;
  phaseDeltaMaxMs?: number;
  phaseDeltaSlopeMsPerSec?: number;
  beatPeriodEstimateMs?: number | null;
}

export class WorkerRafPhaseProbeApp implements LatencyTestApp {
  readonly name: TestAppId;

  private canvas!: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D | null = null;
  private hud!: HTMLPreElement;
  private worker!: Worker;
  private options = DEFAULT_OPTIONS;
  private recorder: TrialRecorder | null = null;
  private cleanup: Array<() => void> = [];
  private active = false;
  private mainRafId: number | null = null;
  private mainSeq = 0;
  private mainFrames: RafPoint[] = [];
  private workerFrames: WorkerRafPoint[] = [];
  private latestMain: RafPoint | null = null;
  private latestWorker: WorkerRafPoint | null = null;
  private latestSummary: PhaseSummary = {};

  constructor(private readonly mode: ProbeMode) {
    this.name = mode === 'webgpu' ? 'worker-webgpu-phase-probe' : 'worker-raf-phase-probe';
  }

  async mount(root: HTMLElement, options: TestOptions): Promise<void> {
    this.options = options;
    this.canvas = document.createElement('canvas');
    this.canvas.className = 'test-canvas';
    this.canvas.style.width = `${options.width}px`;
    this.canvas.style.height = `${options.height}px`;
    this.canvas.width = Math.round(options.width * options.dpr);
    this.canvas.height = Math.round(options.height * options.dpr);
    root.append(this.canvas);

    this.hud = document.createElement('pre');
    this.hud.className = 'phase-probe-hud';
    root.append(this.hud);

    let offscreen: OffscreenCanvas | undefined;
    if (this.mode === 'webgpu') {
      offscreen = this.canvas.transferControlToOffscreen();
    } else {
      this.ctx = this.canvas.getContext('2d');
      this.ctx?.setTransform(options.dpr, 0, 0, options.dpr, 0, 0);
      this.drawPlainCanvas();
    }

    this.worker = new Worker(new URL('../workers/worker-raf-phase-probe.ts', import.meta.url), { type: 'module' });
    await new Promise<void>((resolve, reject) => {
      this.worker.onmessage = (event) => {
        if (event.data.ty === 'ready') resolve();
        if (event.data.ty === 'error') reject(new Error(event.data.message));
      };
      const payload = { ty: 'init', mode: this.mode, canvas: offscreen, options };
      if (offscreen) {
        this.worker.postMessage(payload, [offscreen]);
      } else {
        this.worker.postMessage(payload);
      }
    });
    this.worker.onmessage = (event) => this.onWorkerMessage(event.data);
    this.mainRafId = requestAnimationFrame((now) => this.mainFrame(now));
    this.writeHud();
  }

  async startTrial(trial: TrialConfig): Promise<void> {
    this.recorder = new TrialRecorder(trial, this.options);
    this.recorder.start(await estimateRefreshHz());
    this.resetMeasurement();
    this.active = true;
    this.worker.postMessage({ ty: 'start' });
  }

  beginMotion() {
    this.recorder?.restart();
    this.resetMeasurement();
  }

  async stopTrial(): Promise<TrialResult> {
    this.active = false;
    this.worker.postMessage({ ty: 'stop' });
    if (!this.recorder) throw new Error('No active trial');
    this.latestSummary = this.computeSummary();
    this.recorder.timing({
      ty: 'state',
      t: performance.now(),
      label: 'phase-summary',
      ...this.latestSummary,
    });
    this.writeHud();
    return this.recorder.result();
  }

  dispose() {
    this.active = false;
    if (this.mainRafId !== null) cancelAnimationFrame(this.mainRafId);
    for (const cleanup of this.cleanup) cleanup();
    this.worker?.postMessage({ ty: 'dispose' });
    this.worker?.terminate();
    this.canvas?.remove();
    this.hud?.remove();
  }

  private mainFrame(now: number) {
    this.mainSeq += 1;
    const point = {
      seq: this.mainSeq,
      now,
      absNow: performance.timeOrigin + now,
    };
    this.latestMain = point;
    if (this.active) {
      this.mainFrames.push(point);
      this.recorder?.timing({
        ty: 'main-raf',
        t: performance.now(),
        seq: point.seq,
        mainSeq: point.seq,
        mainNow: point.now,
        mainAbsNow: point.absNow,
      });
    }
    if (this.mode === 'plain') this.drawPlainCanvas();
    this.writeHud();
    this.mainRafId = requestAnimationFrame((nextNow) => this.mainFrame(nextNow));
  }

  private onWorkerMessage(data: any) {
    if (data.ty === 'error') throw new Error(data.message);
    if (data.ty !== 'worker-raf') return;

    const workerPoint: WorkerRafPoint = {
      seq: data.seq,
      now: data.rafNow,
      absNow: data.workerAbsRafNow,
      renderStartNow: data.renderStartNow,
      renderEndNow: data.renderEndNow,
      absRenderStartNow: data.workerAbsRenderStartNow,
      absRenderEndNow: data.workerAbsRenderEndNow,
    };
    this.latestWorker = workerPoint;
    this.workerFrames.push(workerPoint);
    const latestMain = this.latestMain;
    const receivedNow = performance.now();
    this.recorder?.timing({
      ty: 'worker-raf',
      t: receivedNow,
      workerSeq: workerPoint.seq,
      workerNow: workerPoint.now,
      workerAbsNow: workerPoint.absNow,
      mainSeq: latestMain?.seq,
      mainNow: latestMain?.now,
      mainAbsNow: latestMain?.absNow,
      deltaMs: latestMain ? workerPoint.absNow - latestMain.absNow : undefined,
      durationMs: workerPoint.renderEndNow - workerPoint.renderStartNow,
    });
  }

  private resetMeasurement() {
    this.mainFrames = [];
    this.workerFrames = [];
    this.latestSummary = {};
  }

  private computeSummary(): PhaseSummary {
    const mainHz = fitHz(this.mainFrames);
    const workerHz = fitHz(this.workerFrames);
    const phaseDeltas = this.workerFrames
      .map((workerFrame) => {
        const mainFrame = nearestFrame(this.mainFrames, workerFrame.absNow);
        return mainFrame ? workerFrame.absNow - mainFrame.absNow : null;
      })
      .filter((value): value is number => value !== null && Number.isFinite(value));
    const phaseSlope = fitSlopeMsPerSec(this.workerFrames, phaseDeltas);
    const frequencyDelta = mainHz !== undefined && workerHz !== undefined ? Math.abs(mainHz - workerHz) : 0;
    return {
      mainHz,
      workerHz,
      phaseDeltaMeanMs: phaseDeltas.length ? mean(phaseDeltas) : undefined,
      phaseDeltaMinMs: phaseDeltas.length ? Math.min(...phaseDeltas) : undefined,
      phaseDeltaMaxMs: phaseDeltas.length ? Math.max(...phaseDeltas) : undefined,
      phaseDeltaSlopeMsPerSec: phaseSlope,
      beatPeriodEstimateMs: frequencyDelta > 0.0001 ? 1000 / frequencyDelta : null,
    };
  }

  private drawPlainCanvas() {
    if (!this.ctx) return;
    const ctx = this.ctx;
    const t = (this.latestMain?.now || performance.now()) / 1000;
    ctx.clearRect(0, 0, this.options.width, this.options.height);
    ctx.fillStyle = this.options.background;
    ctx.fillRect(0, 0, this.options.width, this.options.height);
    ctx.fillStyle = 'rgba(47, 128, 237, 0.18)';
    ctx.fillRect(120 + Math.sin(t * 1.4) * 80, 120, 160, 160);
    ctx.fillStyle = 'rgba(229, 57, 53, 0.22)';
    ctx.fillRect(560 + Math.cos(t * 0.9) * 90, 270, 120, 120);
    ctx.fillStyle = '#30343b';
    ctx.font = '16px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace';
    ctx.fillText(this.name, 24, 36);
    ctx.fillText('Records main RAF vs worker RAF clock phase.', 24, 62);
  }

  private writeHud() {
    if (!this.hud) return;
    const summary = this.latestSummary;
    const delta = this.latestMain && this.latestWorker ? this.latestWorker.absNow - this.latestMain.absNow : undefined;
    this.hud.textContent = [
      `mode: ${this.name}`,
      `main frames: ${this.mainFrames.length}`,
      `worker frames: ${this.workerFrames.length}`,
      `latest worker-main delta: ${fmt(delta)} ms`,
      `main Hz: ${fmt(summary.mainHz)}`,
      `worker Hz: ${fmt(summary.workerHz)}`,
      `phase range: ${fmt(summary.phaseDeltaMinMs)}..${fmt(summary.phaseDeltaMaxMs)} ms`,
      `phase slope: ${fmt(summary.phaseDeltaSlopeMsPerSec)} ms/s`,
      `beat estimate: ${summary.beatPeriodEstimateMs === null ? 'locked/unknown' : `${fmt(summary.beatPeriodEstimateMs)} ms`}`,
    ].join('\n');
  }
}

function fitHz(frames: RafPoint[]): number | undefined {
  if (frames.length < 3) return undefined;
  const first = frames[0];
  const last = frames[frames.length - 1];
  const dt = last.absNow - first.absNow;
  if (dt <= 0) return undefined;
  return ((frames.length - 1) * 1000) / dt;
}

function nearestFrame(frames: RafPoint[], absNow: number): RafPoint | null {
  if (frames.length === 0) return null;
  let best = frames[0];
  let bestDistance = Math.abs(best.absNow - absNow);
  for (let i = 1; i < frames.length; i += 1) {
    const distance = Math.abs(frames[i].absNow - absNow);
    if (distance < bestDistance) {
      best = frames[i];
      bestDistance = distance;
    }
  }
  return best;
}

function fitSlopeMsPerSec(frames: WorkerRafPoint[], deltas: number[]): number | undefined {
  if (frames.length < 3 || deltas.length < 3) return undefined;
  const count = Math.min(frames.length, deltas.length);
  const start = frames[0].absNow;
  const xs = frames.slice(0, count).map((frame) => (frame.absNow - start) / 1000);
  const ys = deltas.slice(0, count);
  const xMean = mean(xs);
  const yMean = mean(ys);
  let numerator = 0;
  let denominator = 0;
  for (let i = 0; i < count; i += 1) {
    numerator += (xs[i] - xMean) * (ys[i] - yMean);
    denominator += (xs[i] - xMean) ** 2;
  }
  return denominator > 0 ? numerator / denominator : undefined;
}

function mean(values: number[]) {
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

function fmt(value: number | undefined) {
  return value === undefined || !Number.isFinite(value) ? 'n/a' : value.toFixed(3);
}
