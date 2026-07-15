import type { TestOptions } from '../shared/protocol';

type ProbeMode = 'plain' | 'webgpu';

let mode: ProbeMode = 'plain';
let canvas: OffscreenCanvas | null = null;
let options: TestOptions | null = null;
let rafId: number | null = null;
let active = false;
let seq = 0;

let device: any = null;
let context: any = null;
let pipeline: any = null;
let vertexBuffer: any = null;
let format: any = null;

self.onmessage = (event: MessageEvent) => {
  const data = event.data;
  switch (data.ty) {
    case 'init':
      init(data.mode as ProbeMode, data.canvas as OffscreenCanvas | undefined, data.options as TestOptions).catch((error) => {
        self.postMessage({ ty: 'error', message: String(error?.stack || error) });
      });
      break;
    case 'start':
      active = true;
      seq = 0;
      break;
    case 'stop':
      active = false;
      break;
    case 'dispose':
      active = false;
      if (rafId !== null) cancelAnimationFrame(rafId);
      break;
  }
};

async function init(nextMode: ProbeMode, nextCanvas: OffscreenCanvas | undefined, nextOptions: TestOptions) {
  mode = nextMode;
  options = nextOptions;
  if (mode === 'webgpu') {
    if (!nextCanvas) throw new Error('worker-webgpu-phase-probe requires an OffscreenCanvas');
    await initWebGPU(nextCanvas, nextOptions);
  }
  self.postMessage({
    ty: 'ready',
    mode,
    workerTimeOrigin: performance.timeOrigin,
    workerNow: performance.now(),
  });
  rafId = requestAnimationFrame(frame);
}

async function initWebGPU(nextCanvas: OffscreenCanvas, nextOptions: TestOptions) {
  canvas = nextCanvas;
  canvas.width = Math.round(nextOptions.width * nextOptions.dpr);
  canvas.height = Math.round(nextOptions.height * nextOptions.dpr);

  const gpu = (self.navigator as any).gpu;
  if (!gpu) throw new Error('WebGPU is unavailable in this worker');
  const adapter = await gpu.requestAdapter();
  if (!adapter) throw new Error('WebGPU adapter is unavailable in this worker');
  device = await adapter.requestDevice();
  context = canvas.getContext('webgpu');
  if (!context) throw new Error('Worker WebGPU phase probe context is unavailable');
  format = gpu.getPreferredCanvasFormat();
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
    size: 4096,
    usage: (globalThis as any).GPUBufferUsage.VERTEX | (globalThis as any).GPUBufferUsage.COPY_DST,
  });
}

function frame(rafNow: number) {
  const renderStartNow = performance.now();
  seq += 1;
  if (mode === 'webgpu') {
    drawWebGPU(rafNow / 1000);
  }
  const renderEndNow = performance.now();
  if (active) {
    self.postMessage({
      ty: 'worker-raf',
      seq,
      rafNow,
      renderStartNow,
      renderEndNow,
      workerAbsRafNow: performance.timeOrigin + rafNow,
      workerAbsRenderStartNow: performance.timeOrigin + renderStartNow,
      workerAbsRenderEndNow: performance.timeOrigin + renderEndNow,
    });
  }
  rafId = requestAnimationFrame(frame);
}

function drawWebGPU(t: number) {
  if (!options || !device || !context || !pipeline || !vertexBuffer) return;
  const cx = options.width / 2 + Math.sin(t * 1.3) * 180;
  const cy = options.height / 2 + Math.cos(t * 0.9) * 110;
  const vertices = new Float32Array([
    ...rectVertices(0, 0, options.width, options.height, [0.965, 0.969, 0.973, 1]),
    ...rectVertices(cx - 64, cy - 64, 128, 128, [0.184, 0.502, 0.929, 0.32]),
    ...rectVertices(options.width - cx - 42, options.height - cy - 42, 84, 84, [0.898, 0.224, 0.208, 0.42]),
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
