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
  /** Not held: the page's own remaining work left no room before the limit. */
  noRoom: number;
  /** Not held: no recent input. */
  disarmed: number;
  /** Not held: backed off for the rest of this interaction after misses. */
  backedOff: number;
  /** rAF intervals over 1.5 frames, and those right after a held frame. */
  missed: number;
  missedAfterHold: number;
  /** Interactions the hold switched itself off in. */
  backoffs: number;
  holdMs: { p50: number; p90: number; max: number; total: number };
  /** The learned limit (ms after vsync the page may commit by), now. */
  limitMs: number;
  /** The page's own work after the hold: style, layout, paint, commit. */
  afterMs: { p50: number; p90: number };
  intervalMs: number;
  adaptive: boolean;
}

export interface FrameHoldOptions {
  /** Starting limit, ms after vsync (§6b: Chrome's was ~6 ms at 60 Hz). */
  limitMs?: number;
  /** Learning stays within [floor, ceiling]; both also scale to the frame interval. */
  floorMs?: number;
  ceilingMs?: number;
  /** Hold without recent input (measurement only). */
  always?: boolean;
  /** false: the Phase 1 fixed cap (vsync + limit), for comparison. */
  adaptive?: boolean;
}

const quantile = (v: number[], p: number) => {
  if (!v.length) return 0;
  const s = [...v].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(s.length * p))];
};

/**
 * The main thread's side: a rAF loop that holds while armed
 * (plans/rendering/worker-frame-hold.md §3, §4).
 *
 * Each held frame may last until `vsync + limit − after − margin`, where
 * `after` is the page's own work still to come in that frame (measured each
 * frame as the time from the end of this rAF callback to the next task) and
 * `limit` is learned: it creeps up while held frames land and drops a
 * millisecond after a missed one. Two misses in one interaction switch the
 * hold off until input goes quiet. Script cannot see when frames are
 * presented, so a miss is a rAF interval over 1.5 frames; a page Chrome draws
 * a steady frame late is not visible here, which is why the ceiling stays
 * below the measured limit.
 */
export class FrameHold {
  private i32: Int32Array;
  private f64: Float64Array;
  private raf = 0;
  private lastInput = -Infinity;
  private lastVsync = NaN;
  private lastHeld = false;
  private intervals: number[] = [];
  private after: number[] = [];
  private holds: number[] = [];
  private limit: number;
  private interactionMisses = 0;
  private backedOff = false;
  private readonly floor: number;
  private readonly ceiling: number;
  private readonly always: boolean;
  private readonly adaptive: boolean;
  private counts = { frames: 0, pending: 0, held: 0, released: 0, timedOut: 0, skipped: 0, noRoom: 0, disarmed: 0, backedOff: 0, missed: 0, missedAfterHold: 0, backoffs: 0 };
  private readonly channel = new MessageChannel();
  private readonly onInput = () => { this.lastInput = performance.now(); };

  constructor(buffer: SharedArrayBuffer, options: FrameHoldOptions = {}) {
    this.i32 = new Int32Array(buffer, 0, 2);
    this.f64 = new Float64Array(buffer, 8, 3);
    this.limit = options.limitMs ?? 4.5;
    this.floor = options.floorMs ?? 2.5;
    this.ceiling = options.ceilingMs ?? 5.5;
    this.always = options.always ?? false;
    this.adaptive = options.adaptive ?? true;
    // The page's work after this callback: from posting at its end to the
    // next task, which runs once the frame's rendering steps are done.
    this.channel.port1.onmessage = (e: MessageEvent<number>) => {
      this.after.push(performance.now() - e.data);
      if (this.after.length > 30) this.after.shift();
    };
  }

  start(): void {
    for (const type of ['pointermove', 'pointerdown', 'wheel', 'keydown'] as const) {
      window.addEventListener(type, this.onInput, { capture: true, passive: true });
    }
    const frame = (vsync: number) => {
      this.frame(vsync);
      this.channel.port2.postMessage(performance.now());
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
    this.channel.port1.close();
  }

  /** The frame interval (median of recent rAF intervals; 16.7 ms until known). */
  private interval(): number {
    return this.intervals.length >= 5 ? quantile(this.intervals, 0.5) : 1000 / 60;
  }

  private frame(vsync: number): void {
    this.counts.frames++;
    const now = performance.now();
    const interval = this.interval();
    // The limits scale with the frame interval (a 120 Hz frame is half as long).
    const ceiling = Math.min(this.ceiling, this.ceiling * interval / (1000 / 60));
    const floor = Math.min(this.floor, this.floor * interval / (1000 / 60));
    if (!Number.isNaN(this.lastVsync)) {
      const dt = vsync - this.lastVsync;
      const missed = dt > 1.5 * interval;
      if (!missed) { this.intervals.push(dt); if (this.intervals.length > 30) this.intervals.shift(); }
      if (missed) this.counts.missed++;
      if (this.lastHeld && this.adaptive) {
        if (missed) {
          this.counts.missedAfterHold++;
          this.limit = Math.max(floor, this.limit - 1);
          if (++this.interactionMisses >= 2 && !this.backedOff) { this.backedOff = true; this.counts.backoffs++; }
        } else {
          this.limit = Math.min(ceiling, this.limit + 0.02);
        }
      } else if (this.lastHeld && missed) {
        this.counts.missedAfterHold++;
      }
    }
    this.lastVsync = vsync;
    this.lastHeld = false;
    const quiet = now - this.lastInput > 100;
    // A new interaction starts with a clean slate.
    if (quiet) { this.interactionMisses = 0; this.backedOff = false; }

    if (Atomics.load(this.i32, 0) !== TICKING) return;
    this.counts.pending++;
    // Armed while input is recent: hover, drag, wheel, keys (§3).
    if (!this.always && quiet) { this.counts.disarmed++; return; }
    if (this.backedOff) { this.counts.backedOff++; return; }
    const cap = this.adaptive
      ? vsync + Math.min(this.limit, ceiling) - quantile(this.after, 0.9) - 0.3
      : vsync + this.limit;
    if (cap <= now) { this.counts.noRoom++; return; }
    // Hold only when the worker can finish within the cap (§2, step 3).
    const start = this.f64[1] - performance.timeOrigin;
    const predicted = start + (this.f64[2] || 0) + 0.3;
    if (predicted > cap) { this.counts.skipped++; return; }
    this.counts.held++;
    this.lastHeld = true;
    let t = now;
    while (Atomics.load(this.i32, 0) === TICKING && t < cap) t = performance.now();
    this.holds.push(t - now);
    if (Atomics.load(this.i32, 0) === TICKING) this.counts.timedOut++;
    else this.counts.released++;
  }

  stats(): FrameHoldStats {
    const r = (x: number) => +x.toFixed(2);
    return {
      ...this.counts,
      holdMs: { p50: r(quantile(this.holds, 0.5)), p90: r(quantile(this.holds, 0.9)), max: r(quantile(this.holds, 1)), total: r(this.holds.reduce((a, b) => a + b, 0)) },
      limitMs: r(this.limit),
      afterMs: { p50: r(quantile(this.after, 0.5)), p90: r(quantile(this.after, 0.9)) },
      intervalMs: r(this.interval()),
      adaptive: this.adaptive,
    };
  }

  reset(): void {
    this.holds = [];
    for (const k of Object.keys(this.counts) as (keyof typeof this.counts)[]) this.counts[k] = 0;
  }
}
