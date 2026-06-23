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

type GpuDevice = any;
type GpuCanvasContext = any;

export class WebGPURafApp implements LatencyTestApp {
  readonly name = 'webgpu-raf';

  private canvas!: HTMLCanvasElement;
  private context!: GpuCanvasContext;
  private device!: GpuDevice;
  private pipeline!: any;
  private vertexBuffer!: any;
  private options = DEFAULT_OPTIONS;
  private recorder: TrialRecorder | null = null;
  private pointer: Point = { x: 0, y: 0 };
  private rendered: Point = { x: 0, y: 0 };
  private dragOffset: Point = { x: 0, y: 0 };
  private dragging = false;
  private justReleased = false;
  private sampleAugmenter: (() => { reference: Point; speedPxPerMs: number } | null) | null = null;
  private manualMode = false;
  private active = false;
  private rafId: number | null = null;
  private cleanup: Array<() => void> = [];

  async mount(root: HTMLElement, options: TestOptions): Promise<void> {
    this.options = options;
    this.canvas = document.createElement('canvas');
    this.canvas.className = 'test-canvas';
    this.canvas.width = Math.round(options.width * options.dpr);
    this.canvas.height = Math.round(options.height * options.dpr);
    this.canvas.style.width = `${options.width}px`;
    this.canvas.style.height = `${options.height}px`;
    root.append(this.canvas);
    await this.initGpu();
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

  private async initGpu() {
    const gpu = (navigator as any).gpu;
    if (!gpu) throw new Error('WebGPU is unavailable in this browser');
    const adapter = await gpu.requestAdapter();
    if (!adapter) throw new Error('WebGPU adapter is unavailable');
    this.device = await adapter.requestDevice();
    this.context = this.canvas.getContext('webgpu');
    if (!this.context) throw new Error('WebGPU canvas context is unavailable');
    const format = gpu.getPreferredCanvasFormat();
    this.context.configure({
      device: this.device,
      format,
      alphaMode: 'opaque',
    });

    const shader = this.device.createShaderModule({
      code: `
        struct VertexOut {
          @builtin(position) position: vec4f,
          @location(0) color: vec4f,
        };

        @vertex
        fn vs_main(@location(0) position: vec2f, @location(1) color: vec4f) -> VertexOut {
          var out: VertexOut;
          out.position = vec4f(position, 0.0, 1.0);
          out.color = color;
          return out;
        }

        @fragment
        fn fs_main(@location(0) color: vec4f) -> @location(0) vec4f {
          return color;
        }
      `,
    });

    this.pipeline = this.device.createRenderPipeline({
      layout: 'auto',
      vertex: {
        module: shader,
        entryPoint: 'vs_main',
        buffers: [{
          arrayStride: 24,
          attributes: [
            { shaderLocation: 0, offset: 0, format: 'float32x2' },
            { shaderLocation: 1, offset: 8, format: 'float32x4' },
          ],
        }],
      },
      fragment: {
        module: shader,
        entryPoint: 'fs_main',
        targets: [{ format }],
      },
      primitive: { topology: 'triangle-list' },
    });

    this.vertexBuffer = this.device.createBuffer({
      size: 4096,
      usage: (globalThis as any).GPUBufferUsage.VERTEX | (globalThis as any).GPUBufferUsage.COPY_DST,
    });
  }

  private addPointerListeners() {
    const onDown = (event: PointerEvent) => {
      this.pointer = normalizePointer(event, this.canvas);
      this.dragging = this.isInside(this.pointer);
      this.justReleased = false;
      this.dragOffset = { x: this.pointer.x - this.rendered.x, y: this.pointer.y - this.rendered.y };
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
      this.rendered = { x: this.pointer.x - this.dragOffset.x, y: this.pointer.y - this.dragOffset.y };
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
    const vertices = new Float32Array([
      ...this.rectVertices(this.pointer.x - 1, this.pointer.y - 14, 2, 28, [0.031, 0.467, 1, 1]),
      ...this.rectVertices(this.pointer.x - 14, this.pointer.y - 1, 28, 2, [0.031, 0.467, 1, 1]),
      ...this.rectVertices(
        this.rendered.x - this.options.squareSize / 2,
        this.rendered.y - this.options.squareSize / 2,
        this.options.squareSize,
        this.options.squareSize,
        this.dragging ? [0.898, 0.224, 0.208, 1] : [0.188, 0.204, 0.231, 1],
      ),
    ]);
    this.device.queue.writeBuffer(this.vertexBuffer, 0, vertices);

    const encoder = this.device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [{
        view: this.context.getCurrentTexture().createView(),
        clearValue: { r: 0.965, g: 0.969, b: 0.973, a: 1 },
        loadOp: 'clear',
        storeOp: 'store',
      }],
    });
    pass.setPipeline(this.pipeline);
    pass.setVertexBuffer(0, this.vertexBuffer);
    pass.draw(vertices.length / 6);
    pass.end();
    this.device.queue.submit([encoder.finish()]);
  }

  private rectVertices(x: number, y: number, w: number, h: number, color: number[]) {
    const x0 = (x / this.options.width) * 2 - 1;
    const x1 = ((x + w) / this.options.width) * 2 - 1;
    const y0 = 1 - (y / this.options.height) * 2;
    const y1 = 1 - ((y + h) / this.options.height) * 2;
    return [
      x0, y0, ...color,
      x1, y0, ...color,
      x0, y1, ...color,
      x0, y1, ...color,
      x1, y0, ...color,
      x1, y1, ...color,
    ];
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
