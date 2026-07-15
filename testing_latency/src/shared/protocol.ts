export type TestAppId =
  | 'canvas-2d-event'
  | 'canvas-2d-raf'
  | 'worker-2d'
  | 'worker-2d-sab'
  | 'worker-raf-phase-probe'
  | 'worker-webgpu-phase-probe'
  | 'webgl-raf'
  | 'webgpu-raf'
  | 'worker-webgpu'
  | 'worker-webgpu-sab'
  | 'worker-webgpu-sab-main-raf-triggered'
  | 'worker-webgpu-sab-timeout-triggered'
  | 'main-raf-immediate-probe'
  | 'main-raf-buffered-schedule-probe'
  | 'worker-webgpu-buffered-input-probe'
  | 'worker-webgpu-buffered-block-probe'
  | 'worker-webgpu-buffered-vello-cost-probe'
  | 'rust-winit-main'
  | 'rust-bevy-main'
  | 'rust-wgpu-worker'
  | 'rust-bevy-worker'
  | 'rust-bevy-vello-worker';

export type TrialPathId =
  | 'constant-horizontal'
  | 'sine-horizontal'
  | 'step-horizontal'
  | 'micro-jitter';

export interface TestOptions {
  width: number;
  height: number;
  squareSize: number;
  dpr: number;
  background: string;
  squareIdle: string;
  squareDragging: string;
  reference: string;
}

export interface TrialConfig {
  app: TestAppId;
  path: TrialPathId;
  referenceMode: 'scripted' | 'event';
  durationMs: number;
  sampleHz: number;
  velocityPxPerMs: number;
  startX: number;
  startY: number;
  amplitudePx: number;
}

export interface LatencySample {
  t: number;
  dragging: boolean;
  phase: 'idle' | 'hover' | 'dragging' | 'released';
  pointerX: number;
  pointerY: number;
  renderedX: number;
  renderedY: number;
  errorPx: number;
  estimatedLatencyMs: number | null;
  relativeReferenceX?: number;
  relativeReferenceY?: number;
  relativeErrorPx?: number;
  relativeLatencyMs?: number | null;
  referenceSpeedPxPerMs?: number;
  frameKind: 'event' | 'raf' | 'worker-raf' | 'main-triggered-worker' | 'worker-timeout';
}

export interface EventTimingSample {
  ty:
    | 'pointer'
    | 'state'
    | 'frame-start'
    | 'frame-end'
    | 'main-raf'
    | 'worker-message'
    | 'worker-raf'
    | 'worker-render'
    | 'worker-render-start'
    | 'worker-render-end'
    | 'worker-trigger';
  t: number;
  eventTs?: number;
  x?: number;
  y?: number;
  label?: string;
  seq?: number;
  mainSeq?: number;
  workerSeq?: number;
  mainNow?: number;
  workerNow?: number;
  mainAbsNow?: number;
  workerAbsNow?: number;
  deltaMs?: number;
  durationMs?: number;
  mainHz?: number;
  workerHz?: number;
  phaseDeltaMeanMs?: number;
  phaseDeltaMinMs?: number;
  phaseDeltaMaxMs?: number;
  phaseDeltaSlopeMsPerSec?: number;
  beatPeriodEstimateMs?: number | null;
}

export interface TrialSummary {
  sampleCount: number;
  p50LatencyMs: number | null;
  p95LatencyMs: number | null;
  maxLatencyMs: number | null;
  meanErrorPx: number | null;
  maxErrorPx: number | null;
}

export interface TrialResult {
  app: TestAppId;
  path: TrialPathId;
  options: TestOptions;
  config: TrialConfig;
  userAgent: string;
  dpr: number;
  refreshHzEstimate: number | null;
  startedAtIso: string;
  samples: LatencySample[];
  timings: EventTimingSample[];
  summary: TrialSummary;
}

export interface LatencyTestApp {
  name: string;
  mount(root: HTMLElement, options: TestOptions): Promise<void>;
  setSampleAugmenter?(augmenter: (() => {
    reference: { x: number; y: number };
    speedPxPerMs: number;
  } | null) | null): void;
  startTrial(trial: TrialConfig): Promise<void>;
  beginMotion(): void;
  stopTrial(): Promise<TrialResult>;
  dispose(): void;
}

export const DEFAULT_OPTIONS: TestOptions = {
  width: 960,
  height: 540,
  squareSize: 72,
  dpr: typeof window === 'undefined' ? 1 : window.devicePixelRatio || 1,
  background: '#f6f7f8',
  squareIdle: '#30343b',
  squareDragging: '#e53935',
  reference: '#0877ff',
};

export const DEFAULT_TRIAL: Omit<TrialConfig, 'app'> = {
  path: 'constant-horizontal',
  referenceMode: 'scripted',
  durationMs: 2200,
  sampleHz: 120,
  velocityPxPerMs: 0.45,
  startX: 180,
  startY: 270,
  amplitudePx: 260,
};

export const APP_IDS: TestAppId[] = [
  'canvas-2d-event',
  'canvas-2d-raf',
  'worker-2d',
  'worker-2d-sab',
  'worker-raf-phase-probe',
  'worker-webgpu-phase-probe',
  'webgl-raf',
  'webgpu-raf',
  'worker-webgpu',
  'worker-webgpu-sab',
  'worker-webgpu-sab-main-raf-triggered',
  'worker-webgpu-sab-timeout-triggered',
  'main-raf-immediate-probe',
  'main-raf-buffered-schedule-probe',
  'worker-webgpu-buffered-input-probe',
  'worker-webgpu-buffered-block-probe',
  'worker-webgpu-buffered-vello-cost-probe',
  'rust-winit-main',
  'rust-bevy-main',
  'rust-wgpu-worker',
  'rust-bevy-worker',
  'rust-bevy-vello-worker',
];

export const APP_DESCRIPTIONS: Record<TestAppId, string> = {
  'canvas-2d-event': 'Main-thread 2D canvas draws immediately inside pointer events.',
  'canvas-2d-raf': 'Main-thread 2D canvas records pointer events, then applies drag state in RAF.',
  'worker-2d': 'Pointer events postMessage to a worker that draws 2D canvas in worker RAF.',
  'worker-2d-sab': 'Pointer events write SharedArrayBuffer state read by a worker 2D RAF.',
  'worker-raf-phase-probe': 'Diagnostic: logs main-thread RAF and plain worker RAF timestamps for frequency/phase fitting.',
  'worker-webgpu-phase-probe': 'Diagnostic: logs main-thread RAF and OffscreenCanvas WebGPU worker RAF timestamps for frequency/phase fitting.',
  'webgl-raf': 'Minimal main-thread WebGL renderer with drag state applied in RAF.',
  'webgpu-raf': 'Minimal main-thread WebGPU renderer with drag state applied in RAF.',
  'worker-webgpu': 'Main thread posts every pointer event to a worker WebGPU RAF renderer.',
  'worker-webgpu-sab': 'Pointer events write SharedArrayBuffer state read by a worker WebGPU RAF renderer.',
  'worker-webgpu-sab-main-raf-triggered': 'Pointer events write SharedArrayBuffer state, while main-thread RAF messages trigger worker WebGPU renders.',
  'worker-webgpu-sab-timeout-triggered': 'Pointer events write SharedArrayBuffer state read by a worker WebGPU renderer driven by a fixed 60 Hz setTimeout loop.',
  'main-raf-immediate-probe': 'TypeScript control: main-thread RAF renderer with immediate pointer state updates.',
  'main-raf-buffered-schedule-probe': 'TypeScript control: main-thread RAF renderer with next-frame input, schedule cost, and one render-stage delay.',
  'worker-webgpu-buffered-input-probe': 'TypeScript control: worker WebGPU renderer that buffers pointer moves until worker RAF.',
  'worker-webgpu-buffered-block-probe': 'TypeScript control: buffered worker WebGPU plus small per-frame worker blocking.',
  'worker-webgpu-buffered-vello-cost-probe': 'TypeScript control: buffered worker WebGPU plus heavier render cost and one render-stage delay.',
  'rust-winit-main': 'Compiled Rust/winit main-thread wasm app with a draggable shape.',
  'rust-bevy-main': 'Compiled Rust/Bevy main-thread wasm app with a draggable shape.',
  'rust-wgpu-worker': 'Compiled Rust/wgpu worker wasm app using an OffscreenCanvas WebGPU surface.',
  'rust-bevy-worker': 'Compiled Rust/Bevy worker wasm app with a stripped draggable shape.',
  'rust-bevy-vello-worker': 'Compiled Rust/Bevy worker wasm app using bevy_vello for the draggable shape.',
};

export const PATH_IDS: TrialPathId[] = [
  'constant-horizontal',
  'sine-horizontal',
  'step-horizontal',
  'micro-jitter',
];
