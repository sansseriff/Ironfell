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

export class Canvas2DEventApp implements LatencyTestApp {
  readonly name = 'canvas-2d-event';

  private canvas!: HTMLCanvasElement;
  private ctx!: CanvasRenderingContext2D;
  private options = DEFAULT_OPTIONS;
  private recorder: TrialRecorder | null = null;
  private trial: TrialConfig | null = null;
  private pointer: Point = { x: 0, y: 0 };
  private rendered: Point = { x: 0, y: 0 };
  private dragging = false;
  private justReleased = false;
  private dragOffset: Point = { x: 0, y: 0 };
  private sampleAugmenter: (() => { reference: Point; speedPxPerMs: number } | null) | null = null;
  private cleanup: Array<() => void> = [];

  async mount(root: HTMLElement, options: TestOptions): Promise<void> {
    this.options = options;
    this.canvas = document.createElement('canvas');
    this.canvas.className = 'test-canvas';
    this.ctx = setupCanvas(this.canvas, options);
    root.append(this.canvas);
    this.draw();
    this.addPointerListeners();
  }

  async startTrial(trial: TrialConfig) {
    this.trial = trial;
    this.pointer = pointAt(trial, 0);
    this.rendered = { ...this.pointer };
    this.dragging = false;
    this.recorder = new TrialRecorder(trial, this.options);
    this.recorder.start(await estimateRefreshHz());
    this.draw();
  }

  setSampleAugmenter(augmenter: (() => { reference: Point; speedPxPerMs: number } | null) | null) {
    this.sampleAugmenter = augmenter;
  }

  beginMotion() {
    this.recorder?.restart();
  }

  async stopTrial(): Promise<TrialResult> {
    if (!this.recorder) throw new Error('No active trial');
    return this.recorder.result();
  }

  dispose() {
    for (const cleanup of this.cleanup) cleanup();
    this.cleanup = [];
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
      this.drawAndSample('event');
    };
    const onMove = (event: PointerEvent) => {
      this.pointer = normalizePointer(event, this.canvas);
      this.justReleased = false;
      this.recorder?.timing({ ty: 'pointer', t: performance.now(), eventTs: event.timeStamp, x: this.pointer.x, y: this.pointer.y });
      if (this.dragging) {
        this.rendered = {
          x: this.pointer.x - this.dragOffset.x,
          y: this.pointer.y - this.dragOffset.y,
        };
        this.recorder?.timing({ ty: 'state', t: performance.now(), x: this.rendered.x, y: this.rendered.y });
      }
      this.drawAndSample('event');
    };
    const onUp = (event: PointerEvent) => {
      this.pointer = normalizePointer(event, this.canvas);
      this.justReleased = this.dragging;
      this.dragging = false;
      this.recorder?.timing({ ty: 'pointer', t: performance.now(), eventTs: event.timeStamp, x: this.pointer.x, y: this.pointer.y });
      this.drawAndSample('event');
    };

    this.canvas.addEventListener('pointerdown', onDown);
    this.canvas.addEventListener('pointermove', onMove);
    this.canvas.addEventListener('pointerup', onUp);
    this.cleanup.push(() => this.canvas.removeEventListener('pointerdown', onDown));
    this.cleanup.push(() => this.canvas.removeEventListener('pointermove', onMove));
    this.cleanup.push(() => this.canvas.removeEventListener('pointerup', onUp));
  }

  private drawAndSample(frameKind: 'event') {
    const now = performance.now();
    this.recorder?.timing({ ty: 'frame-start', t: now });
    this.draw();
    const end = performance.now();
    this.recorder?.timing({ ty: 'frame-end', t: end });
    const relative = this.sampleAugmenter?.();
    this.recorder?.sample(end, this.referenceCenter(), this.rendered, frameKind, this.phase(), relative?.reference, relative?.speedPxPerMs);
    this.justReleased = false;
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
}
