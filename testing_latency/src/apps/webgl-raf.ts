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

export class WebGLRafApp implements LatencyTestApp {
  readonly name = 'webgl-raf';

  private canvas!: HTMLCanvasElement;
  private gl!: WebGLRenderingContext;
  private program!: WebGLProgram;
  private buffer!: WebGLBuffer;
  private colorLocation!: WebGLUniformLocation;
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

    const gl = this.canvas.getContext('webgl', { antialias: false, preserveDrawingBuffer: false });
    if (!gl) throw new Error('WebGL context is unavailable');
    this.gl = gl;
    this.initGl();
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

  private initGl() {
    const vs = `
      attribute vec2 a_position;
      void main() {
        gl_Position = vec4(a_position, 0.0, 1.0);
      }
    `;
    const fs = `
      precision mediump float;
      uniform vec4 u_color;
      void main() {
        gl_FragColor = u_color;
      }
    `;
    this.program = createProgram(this.gl, vs, fs);
    this.buffer = this.gl.createBuffer()!;
    this.colorLocation = this.gl.getUniformLocation(this.program, 'u_color')!;
    const positionLocation = this.gl.getAttribLocation(this.program, 'a_position');
    this.gl.useProgram(this.program);
    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.buffer);
    this.gl.enableVertexAttribArray(positionLocation);
    this.gl.vertexAttribPointer(positionLocation, 2, this.gl.FLOAT, false, 0, 0);
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
    const gl = this.gl;
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.clearColor(0.965, 0.969, 0.973, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.useProgram(this.program);
    this.drawRect(this.pointer.x - 1, this.pointer.y - 14, 2, 28, [0.031, 0.467, 1, 1]);
    this.drawRect(this.pointer.x - 14, this.pointer.y - 1, 28, 2, [0.031, 0.467, 1, 1]);
    this.drawRect(
      this.rendered.x - this.options.squareSize / 2,
      this.rendered.y - this.options.squareSize / 2,
      this.options.squareSize,
      this.options.squareSize,
      this.dragging ? [0.898, 0.224, 0.208, 1] : [0.188, 0.204, 0.231, 1],
    );
  }

  private drawRect(x: number, y: number, w: number, h: number, color: number[]) {
    const x0 = (x / this.options.width) * 2 - 1;
    const x1 = ((x + w) / this.options.width) * 2 - 1;
    const y0 = 1 - (y / this.options.height) * 2;
    const y1 = 1 - ((y + h) / this.options.height) * 2;
    const vertices = new Float32Array([x0, y0, x1, y0, x0, y1, x0, y1, x1, y0, x1, y1]);
    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.buffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, vertices, this.gl.DYNAMIC_DRAW);
    this.gl.uniform4fv(this.colorLocation, color);
    this.gl.drawArrays(this.gl.TRIANGLES, 0, 6);
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

function createProgram(gl: WebGLRenderingContext, vertexSource: string, fragmentSource: string): WebGLProgram {
  const vertex = compileShader(gl, gl.VERTEX_SHADER, vertexSource);
  const fragment = compileShader(gl, gl.FRAGMENT_SHADER, fragmentSource);
  const program = gl.createProgram();
  if (!program) throw new Error('Unable to create WebGL program');
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error(gl.getProgramInfoLog(program) || 'Unable to link WebGL program');
  }
  return program;
}

function compileShader(gl: WebGLRenderingContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type);
  if (!shader) throw new Error('Unable to create WebGL shader');
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    throw new Error(gl.getShaderInfoLog(shader) || 'Unable to compile WebGL shader');
  }
  return shader;
}
