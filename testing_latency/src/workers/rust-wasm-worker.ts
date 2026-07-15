import type { Point } from '../shared/paths';
import type { TestAppId, TestOptions } from '../shared/protocol';

type RustWorkerFrame = {
  rendered: Point;
  pointer?: Point;
  dragging?: boolean;
  phase?: 'idle' | 'hover' | 'dragging' | 'released';
  workerRafNow?: number;
  workerAbsRafNow?: number;
  workerRenderStartNow?: number;
  workerRenderEndNow?: number;
  workerAbsRenderStartNow?: number;
  workerAbsRenderEndNow?: number;
};

type RustWorkerHandle = {
  start?: (pointer: Point, rendered: Point) => void;
  pointer_down?: (point: Point) => void;
  pointer_move?: (point: Point) => void;
  pointer_up?: (point: Point) => void;
  stop?: () => void;
  dispose?: () => void;
};

type RustModule = {
  default: (input?: unknown) => Promise<unknown>;
  start_wgpu_worker?: (canvas: OffscreenCanvas, options: TestOptions, onFrame: (frame: RustWorkerFrame) => void) => Promise<RustWorkerHandle> | RustWorkerHandle;
  start_bevy_worker?: (canvas: OffscreenCanvas, options: TestOptions, onFrame: (frame: RustWorkerFrame) => void) => Promise<RustWorkerHandle> | RustWorkerHandle;
  start_bevy_vello_worker?: (canvas: OffscreenCanvas, options: TestOptions, onFrame: (frame: RustWorkerFrame) => void) => Promise<RustWorkerHandle> | RustWorkerHandle;
};

let handle: RustWorkerHandle | null = null;

self.onmessage = (event: MessageEvent) => {
  const data = event.data;
  void dispatch(data).catch((error) => {
    self.postMessage({ ty: 'error', message: String(error?.stack || error) });
  });
};

async function dispatch(data: any) {
  switch (data.ty) {
    case 'init':
      await init(data.appId, data.canvas, data.options);
      break;
    case 'start':
      handle?.start?.(data.pointer, data.rendered);
      break;
    case 'pointerdown':
      handle?.pointer_down?.(data.point);
      break;
    case 'pointermove':
      handle?.pointer_move?.(data.point);
      break;
    case 'pointerup':
      handle?.pointer_up?.(data.point);
      break;
    case 'stop':
      handle?.stop?.();
      break;
    case 'dispose':
      handle?.dispose?.();
      break;
  }
}

async function init(appId: TestAppId, canvas: OffscreenCanvas, options: TestOptions) {
  const module = await loadRustModule();
  await module.default();
  const onFrame = (frame: RustWorkerFrame) => {
    if (frame.workerRafNow !== undefined && frame.workerAbsRafNow === undefined) {
      frame.workerAbsRafNow = performance.timeOrigin + frame.workerRafNow;
    }
    if (frame.workerRenderStartNow !== undefined && frame.workerAbsRenderStartNow === undefined) {
      frame.workerAbsRenderStartNow = performance.timeOrigin + frame.workerRenderStartNow;
    }
    if (frame.workerRenderEndNow !== undefined && frame.workerAbsRenderEndNow === undefined) {
      frame.workerAbsRenderEndNow = performance.timeOrigin + frame.workerRenderEndNow;
    }
    self.postMessage({ ty: 'rendered', frame });
  };

  if (appId === 'rust-wgpu-worker') {
    if (!module.start_wgpu_worker) throw new Error('latency_rust wasm does not export start_wgpu_worker');
    handle = await module.start_wgpu_worker(canvas, options, onFrame);
  } else if (appId === 'rust-bevy-worker') {
    if (!module.start_bevy_worker) throw new Error('latency_rust wasm does not export start_bevy_worker');
    handle = await module.start_bevy_worker(canvas, options, onFrame);
  } else if (appId === 'rust-bevy-vello-worker') {
    if (!module.start_bevy_vello_worker) throw new Error('latency_rust wasm does not export start_bevy_vello_worker');
    handle = await module.start_bevy_vello_worker(canvas, options, onFrame);
  } else {
    throw new Error(`Unsupported Rust worker app: ${appId}`);
  }
  self.postMessage({ ty: 'ready' });
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
