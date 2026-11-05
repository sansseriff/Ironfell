/**
 * Holding the page's frame for the worker's (plans/rendering/worker-frame-hold.md).
 *
 * The compositor draws when the page's own frame is ready and does not wait
 * for a worker's canvas. When the worker's tick for this frame is still
 * running at the page's rAF, the page spins here until the worker says its
 * frame has been sent, so one draw carries both. Shared memory needs
 * cross-origin isolation (`crossOriginIsolated`); without it there is no hold.
 *
 * Layout of the shared buffer (written by the worker, read here):
 *   Int32 [0] state: 0 idle, 1 tick running, 2 frame sent
 *   Int32 [1] sequence: counts frames sent
 *   Float64 [1] tick start (ms, performance.timeOrigin + now(), shared clock)
 *   Float64 [2] tick estimate (ms): a high percentile of recent ticks
 */
export const HOLD_BYTES = 32;
export const IDLE = 0, TICKING = 1, SENT = 2;

/** The worker's side: mark a tick running, then sent. */
export class FrameHoldWriter {
  private i32: Int32Array;
  private f64: Float64Array;
  private recent: number[] = [];

  constructor(buffer: SharedArrayBuffer) {
    this.i32 = new Int32Array(buffer, 0, 2);
    this.f64 = new Float64Array(buffer, 8, 3);
  }

  /** A tick starts now. */
  begin(): void {
    this.f64[1] = performance.timeOrigin + performance.now();
    Atomics.store(this.i32, 0, TICKING);
  }

  /** The tick took `ms`; its frame goes out at the next frame push. */
  measured(ms: number): void {
    this.recent.push(ms);
    if (this.recent.length > 40) this.recent.shift();
    const sorted = [...this.recent].sort((a, b) => a - b);
    this.f64[2] = sorted[Math.floor(sorted.length * 0.9)] ?? ms;
  }

  /** The frame has been handed to the compositor. */
  sent(): void {
    Atomics.add(this.i32, 1, 1);
    Atomics.store(this.i32, 0, SENT);
  }

  idle(): void {
    Atomics.store(this.i32, 0, IDLE);
  }
}

export interface FrameHoldStats {
  frames: number;
  /** Frames with a tick still running at the page's rAF. */
  pending: number;
  held: number;
  /** Held until the worker's frame was sent. */
  released: number;
  /** Held to the cap without the frame. */
  timedOut: number;
  /** Not held: the worker was predicted to finish past the cap. */
  skipped: number;
  /** Not held: no recent input. */
  disarmed: number;
  holdMs: { p50: number; p90: number; max: number; total: number };
  capMs: number;
}

/** The main thread's side: a rAF loop that holds while armed. */
export class FrameHold {
  private i32: Int32Array;
  private f64: Float64Array;
  private raf = 0;
  private lastInput = -Infinity;
  private holds: number[] = [];
  private counts = { frames: 0, pending: 0, held: 0, released: 0, timedOut: 0, skipped: 0, disarmed: 0 };
  private readonly onInput = () => { this.lastInput = performance.now(); };

  /**
   * `capMs`: how long after the frame's vsync the page may still commit. A
   * fixed hold of 6 ms past vsync was Chrome's limit on a 6K and the built-in
   * display; past it the whole page is late (§6a). `always`: hold without
   * recent input (measurement only).
   */
  constructor(buffer: SharedArrayBuffer, private readonly capMs = 4.5, private readonly always = false) {
    this.i32 = new Int32Array(buffer, 0, 2);
    this.f64 = new Float64Array(buffer, 8, 3);
  }

  start(): void {
    for (const type of ['pointermove', 'pointerdown', 'wheel', 'keydown'] as const) {
      window.addEventListener(type, this.onInput, { capture: true, passive: true });
    }
    const frame = (vsync: number) => {
      this.hold(vsync);
      this.raf = requestAnimationFrame(frame);
    };
    this.raf = requestAnimationFrame(frame);
    (globalThis as any).__frameHold = { stats: () => this.stats(), reset: () => this.reset() };
  }

  stop(): void {
    cancelAnimationFrame(this.raf);
    for (const type of ['pointermove', 'pointerdown', 'wheel', 'keydown'] as const) {
      window.removeEventListener(type, this.onInput, true);
    }
  }

  private hold(vsync: number): void {
    this.counts.frames++;
    if (Atomics.load(this.i32, 0) !== TICKING) return;
    this.counts.pending++;
    const now = performance.now();
    // Armed while input is recent: hover, drag, wheel, keys (§3).
    if (!this.always && now - this.lastInput > 100) { this.counts.disarmed++; return; }
    const cap = vsync + this.capMs;
    // Hold only when the worker can finish within the cap (§2, step 3).
    const start = this.f64[1] - performance.timeOrigin;
    const predicted = start + (this.f64[2] || 0) + 0.3;
    if (predicted > cap) { this.counts.skipped++; return; }
    this.counts.held++;
    let t = now;
    while (Atomics.load(this.i32, 0) === TICKING && t < cap) t = performance.now();
    this.holds.push(t - now);
    if (Atomics.load(this.i32, 0) === TICKING) this.counts.timedOut++;
    else this.counts.released++;
  }

  stats(): FrameHoldStats {
    const s = [...this.holds].sort((a, b) => a - b);
    const q = (p: number) => (s.length ? +s[Math.min(s.length - 1, Math.floor(s.length * p))].toFixed(2) : 0);
    return { ...this.counts, holdMs: { p50: q(0.5), p90: q(0.9), max: q(1), total: +s.reduce((a, b) => a + b, 0).toFixed(1) }, capMs: this.capMs };
  }

  reset(): void {
    this.holds = [];
    for (const k of Object.keys(this.counts) as (keyof typeof this.counts)[]) this.counts[k] = 0;
  }
}
