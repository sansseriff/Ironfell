import { pointAt, type Point } from '../shared/paths';
import {
  DEFAULT_OPTIONS,
  type LatencyTestApp,
  type TestAppId,
  type TestOptions,
  type TrialConfig,
  type TrialResult,
} from '../shared/protocol';
import { estimateRefreshHz, TrialRecorder } from '../shared/recorder';
import { normalizePointer } from '../shared/render';

type RustFrame = {
  t?: number;
  pointer?: Point;
  rendered: Point;
  dragging?: boolean;
  phase?: 'idle' | 'hover' | 'dragging' | 'released';
  workerRafNow?: number;
  workerAbsRafNow?: number;
  workerRenderStartNow?: number;
  workerRenderEndNow?: number;
  workerAbsRenderStartNow?: number;
  workerAbsRenderEndNow?: number;
};

type RustMainHandle = {
  start?: (pointer: Point, rendered: Point) => void;
};

type RustModule = {
  default: (input?: unknown) => Promise<unknown>;
  start_winit_main?: (canvas: HTMLCanvasElement, options: TestOptions, onFrame: (frame: RustFrame) => void) => RustMainHandle;
  start_bevy_main?: (canvasId: string, options: TestOptions, onFrame: (frame: RustFrame) => void) => RustMainHandle;
};

const MAIN_RUST_APPS = new Set<TestAppId>(['rust-winit-main', 'rust-bevy-main']);

export class RustWasmApp implements LatencyTestApp {
  readonly name: string;

  private canvas!: HTMLCanvasElement;
  private worker: Worker | null = null;
  private mainHandle: RustMainHandle | null = null;
  private options = DEFAULT_OPTIONS;
  private recorder: TrialRecorder | null = null;
  private trial: TrialConfig | null = null;
  private latestPointer: Point = { x: 0, y: 0 };
  private latestRendered: Point = { x: 0, y: 0 };
  private latestReference: Point = { x: 0, y: 0 };
  private dragging = false;
  private justReleased = false;
  private dragOffset: Point = { x: 0, y: 0 };
  private sampleAugmenter: (() => { reference: Point; speedPxPerMs: number } | null) | null = null;
  private cleanup: Array<() => void> = [];

  constructor(private readonly appId: TestAppId) {
    this.name = appId;
  }

  async mount(root: HTMLElement, options: TestOptions): Promise<void> {
    this.options = options;
    this.canvas = document.createElement('canvas');
    this.canvas.className = 'test-canvas';
    this.canvas.id = `rust-latency-${this.appId}-${Math.random().toString(36).slice(2)}`;
    this.canvas.width = Math.round(options.width * options.dpr);
    this.canvas.height = Math.round(options.height * options.dpr);
    this.canvas.style.width = `${options.width}px`;
    this.canvas.style.height = `${options.height}px`;
    root.append(this.canvas);
    this.addRecorderPointerListeners();

    if (MAIN_RUST_APPS.has(this.appId)) {
      await this.mountMainThreadRust();
    } else {
      await this.mountWorkerRust();
    }
  }

  async startTrial(trial: TrialConfig): Promise<void> {
    this.trial = trial;
    const initial = pointAt(trial, 0);
    this.latestPointer = initial;
    this.latestRendered = initial;
    this.latestReference = initial;
    this.dragging = false;
    this.justReleased = false;
    this.recorder = new TrialRecorder(trial, this.options);
    this.recorder.start(await estimateRefreshHz());
    this.mainHandle?.start?.(initial, initial);
    this.worker?.postMessage({ ty: 'start', pointer: initial, rendered: initial });
  }

  beginMotion() {
    this.recorder?.restart();
  }

  setSampleAugmenter(augmenter: (() => { reference: Point; speedPxPerMs: number } | null) | null) {
    this.sampleAugmenter = augmenter;
  }

  async stopTrial(): Promise<TrialResult> {
    this.worker?.postMessage({ ty: 'stop' });
    if (!this.recorder) throw new Error('No active trial');
    return this.recorder.result();
  }

  dispose() {
    for (const cleanup of this.cleanup) cleanup();
    this.cleanup = [];
    this.worker?.postMessage({ ty: 'dispose' });
    this.worker?.terminate();
    this.worker = null;
    this.canvas?.remove();
  }

  private async mountMainThreadRust() {
    const module = await loadRustModule();
    await module.default();
    if (this.appId === 'rust-winit-main') {
      if (!module.start_winit_main) throw new Error('latency_rust wasm does not export start_winit_main');
      this.mainHandle = module.start_winit_main(this.canvas, this.options, (frame) => this.onRendered(frame, 'raf'));
      return;
    }
    if (!module.start_bevy_main) throw new Error('latency_rust wasm does not export start_bevy_main');
    this.mainHandle = module.start_bevy_main(this.canvas.id, this.options, (frame) => this.onRendered(frame, 'raf'));
  }

  private async mountWorkerRust() {
    const offscreen = this.canvas.transferControlToOffscreen();
    this.worker = new Worker(new URL('../workers/rust-wasm-worker.ts', import.meta.url), { type: 'module' });
    await new Promise<void>((resolve, reject) => {
      this.worker!.onmessage = (event) => {
        if (event.data.ty === 'ready') resolve();
        if (event.data.ty === 'error') reject(new Error(event.data.message));
      };
      this.worker!.postMessage({ ty: 'init', appId: this.appId, canvas: offscreen, options: this.options }, [offscreen]);
    });
    this.worker.onmessage = (event) => {
      if (event.data.ty === 'error') throw new Error(event.data.message);
      if (event.data.ty === 'rendered') this.onRendered(event.data.frame, 'worker-raf');
    };
  }

  private addRecorderPointerListeners() {
    const postPointer = (ty: 'pointerdown' | 'pointermove' | 'pointerup', event: PointerEvent) => {
      const point = normalizePointer(event, this.canvas);
      this.latestPointer = point;
      this.updateReference(ty, point);
      const now = performance.now();
      this.recorder?.timing({ ty: 'pointer', t: now, eventTs: event.timeStamp, x: point.x, y: point.y });
      if (this.worker) {
        this.recorder?.timing({ ty: 'worker-message', t: now, x: point.x, y: point.y });
        this.worker.postMessage({ ty, point });
      }
      this.sampleImmediate(now);
    };
    const onDown = (event: PointerEvent) => {
      this.canvas.setPointerCapture(event.pointerId);
      postPointer('pointerdown', event);
    };
    const onMove = (event: PointerEvent) => postPointer('pointermove', event);
    const onUp = (event: PointerEvent) => postPointer('pointerup', event);

    this.canvas.addEventListener('pointerdown', onDown, { capture: true });
    this.canvas.addEventListener('pointermove', onMove, { capture: true });
    this.canvas.addEventListener('pointerup', onUp, { capture: true });
    this.cleanup.push(() => this.canvas.removeEventListener('pointerdown', onDown, { capture: true }));
    this.cleanup.push(() => this.canvas.removeEventListener('pointermove', onMove, { capture: true }));
    this.cleanup.push(() => this.canvas.removeEventListener('pointerup', onUp, { capture: true }));
  }

  private onRendered(frame: RustFrame, frameKind: 'raf' | 'worker-raf') {
    const now = performance.now();
    this.latestRendered = frame.rendered;
    if (!this.dragging) this.latestReference = this.latestRendered;
    if (frameKind === 'worker-raf') {
      if (frame.workerRafNow !== undefined) {
        this.recorder?.timing({
          ty: 'worker-raf',
          t: now,
          workerNow: frame.workerRafNow,
          workerAbsNow: frame.workerAbsRafNow,
        });
      }
      this.recorder?.timing({
        ty: 'worker-render',
        t: now,
        workerNow: frame.workerRenderEndNow,
        workerAbsNow: frame.workerAbsRenderEndNow,
        durationMs: frame.workerRenderStartNow !== undefined && frame.workerRenderEndNow !== undefined
          ? frame.workerRenderEndNow - frame.workerRenderStartNow
          : undefined,
        deltaMs: frame.workerAbsRenderEndNow !== undefined
          ? performance.timeOrigin + now - frame.workerAbsRenderEndNow
          : undefined,
      });
    } else {
      this.recorder?.timing({ ty: 'frame-end', t: now });
    }
    const relative = this.sampleAugmenter?.();
    this.recorder?.sample(
      now,
      this.latestReference,
      this.latestRendered,
      frameKind,
      frame.phase || this.phase(),
      relative?.reference,
      relative?.speedPxPerMs,
    );
    this.justReleased = false;
  }

  private updateReference(ty: 'pointerdown' | 'pointermove' | 'pointerup', point: Point) {
    if (ty === 'pointerdown') {
      this.dragging = this.isInsideRendered(point);
      this.justReleased = false;
      this.dragOffset = {
        x: point.x - this.latestRendered.x,
        y: point.y - this.latestRendered.y,
      };
    } else if (ty === 'pointerup') {
      this.justReleased = this.dragging;
      this.dragging = false;
    } else {
      this.justReleased = false;
    }

    this.latestReference = this.dragging
      ? { x: point.x - this.dragOffset.x, y: point.y - this.dragOffset.y }
      : this.latestRendered;
  }

  private isInsideRendered(point: Point) {
    const half = this.options.squareSize / 2;
    return Math.abs(point.x - this.latestRendered.x) <= half && Math.abs(point.y - this.latestRendered.y) <= half;
  }

  private phase() {
    if (this.dragging) return 'dragging';
    if (this.justReleased) return 'released';
    return 'idle';
  }

  private sampleImmediate(now = performance.now()) {
    if (this.trial?.referenceMode !== 'event' || !this.recorder) return;
    const relative = this.sampleAugmenter?.();
    this.recorder.sample(now, this.latestReference, this.latestRendered, 'event', this.phase(), relative?.reference, relative?.speedPxPerMs);
  }
}

async function loadRustModule(): Promise<RustModule> {
  try {
    const url = '/wasm/latency_rust.js';
    const nativeImport = new Function('url', 'return import(url)') as (url: string) => Promise<unknown>;
    return await nativeImport(url) as RustModule;
  } catch (error) {
    throw new Error(`Missing compiled Rust latency wasm. Run "npm run build:rust" in testing_latency, then reload. ${String(error)}`);
  }
}
