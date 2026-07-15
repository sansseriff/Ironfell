import type { Point } from '../shared/paths';
import type { TestOptions } from '../shared/protocol';
import { readSharedPointer, viewSharedPointer, type SharedPointerView } from '../shared/sab-pointer';

const FRAME_DELAY_MS = 1000 / 60;

let canvas: OffscreenCanvas | null = null;
let options: TestOptions | null = null;
let pointerView: SharedPointerView | null = null;
let device: any = null;
let context: any = null;
let pipeline: any = null;
let vertexBuffer: any = null;
let pointer: Point = { x: 0, y: 0 };
let rendered: Point = { x: 0, y: 0 };
let dragOffset: Point = { x: 0, y: 0 };
let dragging = false;
let previousButtons = 0;
let timeoutId: number | null = null;
let active = false;
let initialized = false;
let seq = 0;

self.onmessage = (event: MessageEvent) => {
  const data = event.data;
  switch (data.ty) {
    case 'init':
      pointerView = viewSharedPointer(data.pointerBuffer as SharedArrayBuffer);
      init(data.canvas as OffscreenCanvas, data.options as TestOptions).catch((error) => {
        self.postMessage({ ty: 'error', message: String(error?.stack || error) });
      });
      break;
    case 'start':
      pointer = data.pointer;
      rendered = data.rendered;
      dragging = false;
      previousButtons = 0;
      active = true;
      seq = 0;
      draw();
      break;
    case 'stop':
      active = false;
      break;
    case 'dispose':
      active = false;
      if (timeoutId !== null) clearTimeout(timeoutId);
      break;
  }
};

async function init(nextCanvas: OffscreenCanvas, nextOptions: TestOptions) {
  canvas = nextCanvas;
  options = nextOptions;
  canvas.width = Math.round(options.width * options.dpr);
  canvas.height = Math.round(options.height * options.dpr);

  const gpu = (self.navigator as any).gpu;
  if (!gpu) throw new Error('WebGPU is unavailable in this worker');
  const adapter = await gpu.requestAdapter();
  if (!adapter) throw new Error('WebGPU adapter is unavailable in this worker');
  device = await adapter.requestDevice();
  context = canvas.getContext('webgpu');
  if (!context) throw new Error('Timeout-triggered worker WebGPU canvas context is unavailable');
  const format = gpu.getPreferredCanvasFormat();
  context.configure({ device, format, alphaMode: 'opaque' });

  const shader = device.createShaderModule({
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

  pipeline = device.createRenderPipeline({
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
    fragment: { module: shader, entryPoint: 'fs_main', targets: [{ format }] },
    primitive: { topology: 'triangle-list' },
  });

  vertexBuffer = device.createBuffer({
    size: 8192,
    usage: (globalThis as any).GPUBufferUsage.VERTEX | (globalThis as any).GPUBufferUsage.COPY_DST,
  });
  initialized = true;
  draw();
  self.postMessage({ ty: 'ready' });
  scheduleNext();
}

function scheduleNext() {
  timeoutId = setTimeout(frame, FRAME_DELAY_MS) as unknown as number;
}

function frame() {
  const frameNow = performance.now();
  const renderStartNow = performance.now();
  seq += 1;
  const snapshot = pointerView ? readSharedPointer(pointerView) : null;
  if (snapshot) {
    pointer = snapshot.point;
    const leftPressed = (snapshot.buttons & 1) === 1;
    const wasPressed = (previousButtons & 1) === 1;
    if (leftPressed && !wasPressed) {
      dragging = isInside(pointer);
      dragOffset = { x: pointer.x - rendered.x, y: pointer.y - rendered.y };
    } else if (!leftPressed && wasPressed) {
      dragging = false;
    }
    previousButtons = snapshot.buttons;
  }
  if (dragging) {
    rendered = { x: pointer.x - dragOffset.x, y: pointer.y - dragOffset.y };
  }
  draw();
  const renderEndNow = performance.now();
  if (active) {
    self.postMessage({
      ty: 'rendered',
      seq,
      frameNow,
      workerAbsFrameNow: performance.timeOrigin + frameNow,
      workerRenderStartNow: renderStartNow,
      workerRenderEndNow: renderEndNow,
      workerAbsRenderStartNow: performance.timeOrigin + renderStartNow,
      workerAbsRenderEndNow: performance.timeOrigin + renderEndNow,
      pointer,
      rendered,
      dragging,
    });
  }
  scheduleNext();
}

function draw() {
  if (!initialized || !options || !device || !context) return;
  const t = performance.now() / 1000;
  const vertices = new Float32Array([
    ...rectVertices(0, 0, options.width, options.height, [0.965, 0.969, 0.973, 1]),
    ...rectVertices(options.width * 0.18 + Math.sin(t * 0.7) * 90 - 42, options.height * 0.50 + Math.cos(t * 0.58) * 46 - 42, 84, 84, [0.184, 0.502, 0.929, 0.16]),
    ...rectVertices(options.width * 0.52 + Math.sin(t * 0.95) * 82 - 42, options.height * 0.35 + Math.cos(t * 0.79) * 52 - 42, 84, 84, [0.000, 0.659, 0.420, 0.16]),
    ...rectVertices(options.width * 0.78 + Math.sin(t * 1.25) * 76 - 42, options.height * 0.62 + Math.cos(t * 1.04) * 42 - 42, 84, 84, [0.486, 0.227, 0.929, 0.16]),
    ...rectVertices(pointer.x - 1, pointer.y - 14, 2, 28, [0.031, 0.467, 1, 1]),
    ...rectVertices(pointer.x - 14, pointer.y - 1, 28, 2, [0.031, 0.467, 1, 1]),
    ...rectVertices(
      rendered.x - options.squareSize / 2,
      rendered.y - options.squareSize / 2,
      options.squareSize,
      options.squareSize,
      dragging ? [0.898, 0.224, 0.208, 1] : [0.188, 0.204, 0.231, 1],
    ),
  ]);
  device.queue.writeBuffer(vertexBuffer, 0, vertices);
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: context.getCurrentTexture().createView(),
      clearValue: { r: 0.965, g: 0.969, b: 0.973, a: 1 },
      loadOp: 'clear',
      storeOp: 'store',
    }],
  });
  pass.setPipeline(pipeline);
  pass.setVertexBuffer(0, vertexBuffer);
  pass.draw(vertices.length / 6);
  pass.end();
  device.queue.submit([encoder.finish()]);
}

function rectVertices(x: number, y: number, w: number, h: number, color: number[]) {
  if (!options) return [];
  const x0 = (x / options.width) * 2 - 1;
  const x1 = ((x + w) / options.width) * 2 - 1;
  const y0 = 1 - (y / options.height) * 2;
  const y1 = 1 - ((y + h) / options.height) * 2;
  return [
    x0, y0, ...color,
    x1, y0, ...color,
    x0, y1, ...color,
    x0, y1, ...color,
    x1, y0, ...color,
    x1, y1, ...color,
  ];
}

function isInside(point: Point) {
  if (!options) return false;
  const half = options.squareSize / 2;
  return Math.abs(point.x - rendered.x) <= half && Math.abs(point.y - rendered.y) <= half;
}
