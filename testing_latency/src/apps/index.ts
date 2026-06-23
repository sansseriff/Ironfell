import type { LatencyTestApp, TestAppId } from '../shared/protocol';
import { Canvas2DEventApp } from './canvas2d-event';
import { Canvas2DRafApp } from './canvas2d-raf';
import { WebGLRafApp } from './webgl-raf';
import { WebGPURafApp } from './webgpu-raf';
import { Worker2DApp } from './worker2d';
import { Worker2DSabApp } from './worker2d-sab';
import { WorkerWebGPUApp } from './worker-webgpu';
import { WorkerWebGPUSabApp } from './worker-webgpu-sab';

export function createLatencyApp(id: TestAppId): LatencyTestApp {
  switch (id) {
    case 'canvas-2d-event':
      return new Canvas2DEventApp();
    case 'canvas-2d-raf':
      return new Canvas2DRafApp();
    case 'worker-2d':
      return new Worker2DApp();
    case 'worker-2d-sab':
      return new Worker2DSabApp();
    case 'webgl-raf':
      return new WebGLRafApp();
    case 'webgpu-raf':
      return new WebGPURafApp();
    case 'worker-webgpu':
      return new WorkerWebGPUApp();
    case 'worker-webgpu-sab':
      return new WorkerWebGPUSabApp();
  }
}
