import { pointAt, type Point } from '../shared/paths';
import {
  DEFAULT_OPTIONS,
  type LatencyTestApp,
  type TestOptions,
  type TrialConfig,
  type TrialResult,
} from '../shared/protocol';
import { estimateRefreshHz, TrialRecorder } from '../shared/recorder';
import { draw2D, normalizePointer, setupCanvas } from '../shared/render';

export class Canvas2DRafApp implements LatencyTestApp {
  readonly name = 'canvas-2d-raf';

  private canvas!: HTMLCanvasElement;
  private ctx!: CanvasRenderingContext2D;
  private options = DEFAULT_OPTIONS;
  private recorder: TrialRecorder | null = null;
  private pointer: Point = { x: 0, y: 0 };
  private rendered: Point = { x: 0, y: 0 };
  private dragOffset: Point = { x: 0, y: 0 };
  private dragging = false;
  private justReleased = false;
  private sampleAugmenter: (() => { reference: Point; speedPxPerMs: number } | null) | null = null;
  private manualMode = false;
  private rafId: number | null = null;
  private active = false;
  private cleanup: Array<() => void> = [];

  async mount(root: HTMLElement, options: TestOptions): Promise<void> {
    this.options = options;
    this.canvas = document.createElement('canvas');
    this.canvas.className = 'test-canvas';
    this.ctx = setupCanvas(this.canvas, options);
    root.append(this.canvas);
    this.draw();
    this.addPointerListeners();
    this.rafId = requestAnimationFrame((now) => this.frame(now));
  }

  async startTrial(trial: TrialConfig) {
    this.pointer = pointAt(trial, 0);
    this.rendered = { ...this.pointer };
    this.dragging = false;
    this.manualMode = trial.referenceMode === 'event';
    this.recorder = new TrialRecorder(trial, this.options);
    this.recorder.start(await estimateRefreshHz());
    this.active = true;
    this.draw();
  }

  setSampleAugmenter(augmenter: (() => { reference: Point; speedPxPerMs: number } | null) | null) {
    this.sampleAugmenter = augmenter;
  }

  beginMotion() {
    this.recorder?.restart();
  }

  async stopTrial(): Promise<TrialResult> {
    this.active = false;
    if (!this.recorder) throw new Error('No active trial');
    return this.recorder.result();
  }

  dispose() {
    this.active = false;
    if (this.rafId !== null) cancelAnimationFrame(this.rafId);
    for (const cleanup of this.cleanup) cleanup();
    this.canvas?.remove();
  }

  private addPointerListeners() {
    const onDown = (event: PointerEvent) => {
      this.pointer = normalizePointer(event, this.canvas);
      this.dragging = this.isInside(this.pointer);
      this.justReleased = false;
      this.dragOffset = {
        x: this.pointer.x - this.rendered.x,
        y: this.pointer.y - this.rendered.y,
      };
      this.canvas.setPointerCapture(event.pointerId);
      this.recorder?.timing({ ty: 'pointer', t: performance.now(), eventTs: event.timeStamp, x: this.pointer.x, y: this.pointer.y });
      this.sampleImmediate();
    };
    const onMove = (event: PointerEvent) => {
      this.pointer = normalizePointer(event, this.canvas);
      this.justReleased = false;
      this.recorder?.timing({ ty: 'pointer', t: performance.now(), eventTs: event.timeStamp, x: this.pointer.x, y: this.pointer.y });
      this.sampleImmediate();
    };
    const onUp = (event: PointerEvent) => {
      this.pointer = normalizePointer(event, this.canvas);
      this.justReleased = this.dragging;
      this.dragging = false;
      this.recorder?.timing({ ty: 'pointer', t: performance.now(), eventTs: event.timeStamp, x: this.pointer.x, y: this.pointer.y });
      this.sampleImmediate();
    };

    this.canvas.addEventListener('pointerdown', onDown);
    this.canvas.addEventListener('pointermove', onMove);
    this.canvas.addEventListener('pointerup', onUp);
    this.cleanup.push(() => this.canvas.removeEventListener('pointerdown', onDown));
    this.cleanup.push(() => this.canvas.removeEventListener('pointermove', onMove));
    this.cleanup.push(() => this.canvas.removeEventListener('pointerup', onUp));
  }

  private frame(now: number) {
    this.recorder?.timing({ ty: 'frame-start', t: now });
    if (this.dragging) {
      this.rendered = {
        x: this.pointer.x - this.dragOffset.x,
        y: this.pointer.y - this.dragOffset.y,
      };
      this.recorder?.timing({ ty: 'state', t: performance.now(), x: this.rendered.x, y: this.rendered.y });
    }
    this.draw();
    const end = performance.now();
    this.recorder?.timing({ ty: 'frame-end', t: end });
    if (this.active) {
      const relative = this.sampleAugmenter?.();
      this.recorder?.sample(end, this.referenceCenter(), this.rendered, 'raf', this.phase(), relative?.reference, relative?.speedPxPerMs);
    }
    this.justReleased = false;
    this.rafId = requestAnimationFrame((nextNow) => this.frame(nextNow));
  }

  private draw() {
    draw2D(this.ctx, this.options, this.pointer, this.rendered, this.dragging);
  }

  private isInside(point: Point) {
    const half = this.options.squareSize / 2;
    return Math.abs(point.x - this.rendered.x) <= half && Math.abs(point.y - this.rendered.y) <= half;
  }

  private referenceCenter(): Point {
    return this.dragging
      ? { x: this.pointer.x - this.dragOffset.x, y: this.pointer.y - this.dragOffset.y }
      : this.rendered;
  }

  private phase() {
    if (this.dragging) return 'dragging';
    if (this.justReleased) return 'released';
    return 'idle';
  }

  private sampleImmediate() {
    if (!this.manualMode || !this.active || !this.recorder) return;
    const now = performance.now();
    const relative = this.sampleAugmenter?.();
    this.recorder.sample(now, this.referenceCenter(), this.rendered, 'event', this.phase(), relative?.reference, relative?.speedPxPerMs);
  }
}
