import type {
  EventTimingSample,
  LatencySample,
  TestOptions,
  TrialConfig,
  TrialResult,
  TrialSummary,
} from './protocol';
import { instantaneousSpeedPxPerMs, pointAt, type Point } from './paths';

export class TrialRecorder {
  private samples: LatencySample[] = [];
  private timings: EventTimingSample[] = [];
  private pointerHistory: Array<{ t: number; x: number; y: number }> = [];
  private startedAt = performance.now();
  private startedAtIso = new Date().toISOString();
  private refreshEstimate: number | null = null;

  constructor(
    private readonly config: TrialConfig,
    private readonly options: TestOptions,
  ) {}

  start(refreshEstimate: number | null) {
    this.refreshEstimate = refreshEstimate;
    this.restart();
  }

  restart() {
    this.samples = [];
    this.timings = [];
    this.pointerHistory = [];
    this.startedAt = performance.now();
    this.startedAtIso = new Date().toISOString();
  }

  elapsed(now = performance.now()) {
    return now - this.startedAt;
  }

  timing(sample: EventTimingSample) {
    this.timings.push(sample);
    if (sample.ty === 'pointer' && sample.x !== undefined && sample.y !== undefined) {
      this.pointerHistory.push({ t: sample.t, x: sample.x, y: sample.y });
      if (this.pointerHistory.length > 240) {
        this.pointerHistory.splice(0, this.pointerHistory.length - 240);
      }
    }
  }

  sample(
    now: number,
    pointer: Point,
    rendered: Point,
    frameKind: LatencySample['frameKind'],
    phase: LatencySample['phase'] = 'idle',
    relativeReference?: Point,
    referenceSpeedPxPerMs?: number,
  ) {
    const elapsed = this.elapsed(now);
    const reference = this.config.referenceMode === 'event' ? pointer : pointAt(this.config, elapsed);
    const errorPx = Math.hypot(reference.x - rendered.x, reference.y - rendered.y);
    const speed = this.config.referenceMode === 'event'
      ? this.manualPointerSpeed(now)
      : instantaneousSpeedPxPerMs(this.config, elapsed);
    const relativeErrorPx = relativeReference
      ? Math.hypot(relativeReference.x - rendered.x, relativeReference.y - rendered.y)
      : undefined;
    const relativeLatencyMs = relativeErrorPx !== undefined && referenceSpeedPxPerMs !== undefined && referenceSpeedPxPerMs > 0.01
      ? relativeErrorPx / referenceSpeedPxPerMs
      : relativeErrorPx !== undefined
        ? null
        : undefined;

    this.samples.push({
      t: now,
      dragging: phase === 'dragging',
      phase,
      pointerX: reference.x,
      pointerY: reference.y,
      renderedX: rendered.x,
      renderedY: rendered.y,
      errorPx,
      estimatedLatencyMs: speed > 0.01 ? errorPx / speed : null,
      relativeReferenceX: relativeReference?.x,
      relativeReferenceY: relativeReference?.y,
      relativeErrorPx,
      relativeLatencyMs,
      referenceSpeedPxPerMs,
      frameKind,
    });
  }

  result(): TrialResult {
    return {
      app: this.config.app,
      path: this.config.path,
      options: this.options,
      config: this.config,
      userAgent: navigator.userAgent,
      dpr: window.devicePixelRatio || 1,
      refreshHzEstimate: this.refreshEstimate,
      startedAtIso: this.startedAtIso,
      samples: this.samples.slice(),
      timings: this.timings.slice(),
      summary: summarize(this.samples),
    };
  }

  private manualPointerSpeed(now: number): number {
    const recent = this.pointerHistory.filter((sample) => now - sample.t <= 120);
    if (recent.length < 2) return 0;
    const first = recent[0];
    const last = recent[recent.length - 1];
    const dt = last.t - first.t;
    if (dt <= 0) return 0;
    return Math.hypot(last.x - first.x, last.y - first.y) / dt;
  }
}

export function summarize(samples: LatencySample[]): TrialSummary {
  const latencies = samples
    .map((sample) => sample.estimatedLatencyMs)
    .filter((value): value is number => value !== null && Number.isFinite(value))
    .sort((a, b) => a - b);
  const errors = samples
    .map((sample) => sample.errorPx)
    .filter((value) => Number.isFinite(value));

  return {
    sampleCount: samples.length,
    p50LatencyMs: percentile(latencies, 0.5),
    p95LatencyMs: percentile(latencies, 0.95),
    maxLatencyMs: latencies.length > 0 ? latencies[latencies.length - 1] : null,
    meanErrorPx: errors.length > 0 ? errors.reduce((sum, value) => sum + value, 0) / errors.length : null,
    maxErrorPx: errors.length > 0 ? Math.max(...errors) : null,
  };
}

function percentile(values: number[], p: number): number | null {
  if (values.length === 0) return null;
  const index = Math.min(values.length - 1, Math.max(0, Math.floor((values.length - 1) * p)));
  return values[index];
}

export async function estimateRefreshHz(): Promise<number | null> {
  const times: number[] = [];
  return new Promise((resolve) => {
    const tick = (now: number) => {
      times.push(now);
      if (times.length < 32) {
        requestAnimationFrame(tick);
        return;
      }
      const deltas = times.slice(1).map((time, i) => time - times[i]);
      const median = deltas.sort((a, b) => a - b)[Math.floor(deltas.length / 2)];
      resolve(median > 0 ? Math.round(1000 / median) : null);
    };
    requestAnimationFrame(tick);
  });
}
