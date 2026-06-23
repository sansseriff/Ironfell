import { pointAt, type Point } from '../shared/paths';
import {
  DEFAULT_OPTIONS,
  type LatencyTestApp,
  type TestOptions,
  type TrialConfig,
  type TrialResult,
} from '../shared/protocol';
import { estimateRefreshHz, TrialRecorder } from '../shared/recorder';
import { normalizePointer } from '../shared/render';

export class WorkerWebGPUApp implements LatencyTestApp {
  readonly name = 'worker-webgpu';

  private canvas!: HTMLCanvasElement;
  private worker!: Worker;
  private options = DEFAULT_OPTIONS;
  private recorder: TrialRecorder | null = null;
  private latestPointer: Point = { x: 0, y: 0 };
  private latestRendered: Point = { x: 0, y: 0 };
  private latestReference: Point = { x: 0, y: 0 };
  private mainDragging = false;
  private justReleased = false;
  private mainDragOffset: Point = { x: 0, y: 0 };
  private sampleAugmenter: (() => { reference: Point; speedPxPerMs: number } | null) | null = null;
  private manualMode = false;
  private cleanup: Array<() => void> = [];

  async mount(root: HTMLElement, options: TestOptions): Promise<void> {
    this.options = options;
    this.canvas = document.createElement('canvas');
    this.canvas.className = 'test-canvas';
    this.canvas.style.width = `${options.width}px`;
    this.canvas.style.height = `${options.height}px`;
    this.canvas.width = Math.round(options.width * options.dpr);
    this.canvas.height = Math.round(options.height * options.dpr);
    root.append(this.canvas);

    const offscreen = this.canvas.transferControlToOffscreen();
    this.worker = new Worker(new URL('../workers/worker-webgpu.ts', import.meta.url), { type: 'module' });
    await new Promise<void>((resolve, reject) => {
      this.worker.onmessage = (event) => {
        if (event.data.ty === 'ready') resolve();
        if (event.data.ty === 'error') reject(new Error(event.data.message));
      };
      this.worker.postMessage({ ty: 'init', canvas: offscreen, options }, [offscreen]);
    });
    this.worker.onmessage = (event) => {
      if (event.data.ty === 'error') throw new Error(event.data.message);
      if (event.data.ty !== 'rendered') return;
      const now = performance.now();
      this.latestRendered = event.data.rendered;
      if (!this.mainDragging) this.latestReference = this.latestRendered;
      this.recorder?.timing({ ty: 'worker-render', t: now });
      const relative = this.sampleAugmenter?.();
      this.recorder?.sample(now, this.latestReference, event.data.rendered, 'worker-raf', this.phase(), relative?.reference, relative?.speedPxPerMs);
      this.justReleased = false;
    };
    this.addPointerListeners();
  }

  async startTrial(trial: TrialConfig) {
    const initial = pointAt(trial, 0);
    this.latestPointer = initial;
    this.latestRendered = initial;
    this.latestReference = initial;
    this.mainDragging = false;
    this.justReleased = false;
    this.manualMode = trial.referenceMode === 'event';
    this.recorder = new TrialRecorder(trial, this.options);
    this.recorder.start(await estimateRefreshHz());
    this.worker.postMessage({ ty: 'start', pointer: initial, rendered: initial });
  }

  beginMotion() {
    this.recorder?.restart();
  }

  setSampleAugmenter(augmenter: (() => { reference: Point; speedPxPerMs: number } | null) | null) {
    this.sampleAugmenter = augmenter;
  }

  async stopTrial(): Promise<TrialResult> {
    this.worker.postMessage({ ty: 'stop' });
    if (!this.recorder) throw new Error('No active trial');
    return this.recorder.result();
  }

  dispose() {
    for (const cleanup of this.cleanup) cleanup();
    this.worker?.postMessage({ ty: 'dispose' });
    this.worker?.terminate();
    this.canvas?.remove();
  }

  private addPointerListeners() {
    const postPointer = (ty: 'pointerdown' | 'pointermove' | 'pointerup', event: PointerEvent) => {
      const point = normalizePointer(event, this.canvas);
      this.latestPointer = point;
      this.updateManualReference(ty, point);
      const now = performance.now();
      this.recorder?.timing({ ty: 'pointer', t: now, eventTs: event.timeStamp, x: point.x, y: point.y });
      this.recorder?.timing({ ty: 'worker-message', t: now, x: point.x, y: point.y });
      this.sampleImmediate(now);
      this.worker.postMessage({ ty, point });
    };
    const onDown = (event: PointerEvent) => {
      this.canvas.setPointerCapture(event.pointerId);
      postPointer('pointerdown', event);
    };
    const onMove = (event: PointerEvent) => postPointer('pointermove', event);
    const onUp = (event: PointerEvent) => postPointer('pointerup', event);

    this.canvas.addEventListener('pointerdown', onDown);
    this.canvas.addEventListener('pointermove', onMove);
    this.canvas.addEventListener('pointerup', onUp);
    this.cleanup.push(() => this.canvas.removeEventListener('pointerdown', onDown));
    this.cleanup.push(() => this.canvas.removeEventListener('pointermove', onMove));
    this.cleanup.push(() => this.canvas.removeEventListener('pointerup', onUp));
  }

  private updateManualReference(ty: 'pointerdown' | 'pointermove' | 'pointerup', point: Point) {
    if (ty === 'pointerdown') {
      this.mainDragging = this.isInsideRendered(point);
      this.justReleased = false;
      this.mainDragOffset = {
        x: point.x - this.latestRendered.x,
        y: point.y - this.latestRendered.y,
      };
    } else if (ty === 'pointerup') {
      this.justReleased = this.mainDragging;
      this.mainDragging = false;
    } else if (ty === 'pointermove') {
      this.justReleased = false;
    }

    this.latestReference = this.mainDragging
      ? { x: point.x - this.mainDragOffset.x, y: point.y - this.mainDragOffset.y }
      : this.latestRendered;
  }

  private phase() {
    if (this.mainDragging) return 'dragging';
    if (this.justReleased) return 'released';
    return 'idle';
  }

  private isInsideRendered(point: Point) {
    const half = this.options.squareSize / 2;
    return Math.abs(point.x - this.latestRendered.x) <= half && Math.abs(point.y - this.latestRendered.y) <= half;
  }

  private sampleImmediate(now = performance.now()) {
    if (!this.manualMode || !this.recorder) return;
    const relative = this.sampleAugmenter?.();
    this.recorder.sample(now, this.latestReference, this.latestRendered, 'event', this.phase(), relative?.reference, relative?.speedPxPerMs);
  }
}
