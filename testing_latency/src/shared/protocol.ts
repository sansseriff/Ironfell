export type TestAppId =
  | 'canvas-2d-event'
  | 'canvas-2d-raf'
  | 'worker-2d'
  | 'worker-2d-sab'
  | 'webgl-raf'
  | 'webgpu-raf'
  | 'worker-webgpu'
  | 'worker-webgpu-sab';

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
  frameKind: 'event' | 'raf' | 'worker-raf';
}

export interface EventTimingSample {
  ty: 'pointer' | 'state' | 'frame-start' | 'frame-end' | 'worker-message' | 'worker-render';
  t: number;
  eventTs?: number;
  x?: number;
  y?: number;
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
  'webgl-raf',
  'webgpu-raf',
  'worker-webgpu',
  'worker-webgpu-sab',
];

export const PATH_IDS: TrialPathId[] = [
  'constant-horizontal',
  'sine-horizontal',
  'step-horizontal',
  'micro-jitter',
];
