import type { LatencyTestApp, TestAppId } from '../shared/protocol';
import { Canvas2DEventApp } from './canvas2d-event';
import { Canvas2DRafApp } from './canvas2d-raf';
import { WebGLRafApp } from './webgl-raf';
import { WebGPURafApp } from './webgpu-raf';
import { Worker2DApp } from './worker2d';
import { Worker2DSabApp } from './worker2d-sab';
import { WorkerWebGPUApp } from './worker-webgpu';
import { WorkerWebGPUSabApp } from './worker-webgpu-sab';
import { WorkerWebGPUSabMainRafTriggeredApp } from './worker-webgpu-sab-main-raf-triggered';
import { WorkerWebGPUSabTimeoutTriggeredApp } from './worker-webgpu-sab-timeout-triggered';
import { BufferedWorkerWebGPUApp } from './worker-webgpu-buffered';
import { MainThreadProbeApp } from './main-thread-probe';
import { WorkerRafPhaseProbeApp } from './worker-raf-phase-probe';
import { RustWasmApp } from './rust-wasm-app';

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
    case 'worker-raf-phase-probe':
      return new WorkerRafPhaseProbeApp('plain');
    case 'worker-webgpu-phase-probe':
      return new WorkerRafPhaseProbeApp('webgpu');
    case 'webgl-raf':
      return new WebGLRafApp();
    case 'webgpu-raf':
      return new WebGPURafApp();
    case 'worker-webgpu':
      return new WorkerWebGPUApp();
    case 'worker-webgpu-sab':
      return new WorkerWebGPUSabApp();
    case 'worker-webgpu-sab-main-raf-triggered':
      return new WorkerWebGPUSabMainRafTriggeredApp();
    case 'worker-webgpu-sab-timeout-triggered':
      return new WorkerWebGPUSabTimeoutTriggeredApp();
    case 'main-raf-immediate-probe':
      return new MainThreadProbeApp({
        name: 'main-raf-immediate-probe',
        applyInputNextFrame: false,
      });
    case 'main-raf-buffered-schedule-probe':
      return new MainThreadProbeApp({
        name: 'main-raf-buffered-schedule-probe',
        applyInputNextFrame: true,
        scheduleBlockMs: 2,
        renderLagFrames: 1,
      });
    case 'worker-webgpu-buffered-input-probe':
      return new BufferedWorkerWebGPUApp({
        name: 'worker-webgpu-buffered-input-probe',
      });
    case 'worker-webgpu-buffered-block-probe':
      return new BufferedWorkerWebGPUApp({
        name: 'worker-webgpu-buffered-block-probe',
        blockMs: 1,
      });
    case 'worker-webgpu-buffered-vello-cost-probe':
      return new BufferedWorkerWebGPUApp({
        name: 'worker-webgpu-buffered-vello-cost-probe',
        blockMs: 3,
        renderLagFrames: 1,
      });
    case 'rust-winit-main':
    case 'rust-bevy-main':
    case 'rust-wgpu-worker':
    case 'rust-bevy-worker':
    case 'rust-bevy-vello-worker':
      return new RustWasmApp(id);
  }
}
