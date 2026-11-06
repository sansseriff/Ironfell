/**
 * Frame modes for the worker's canvas (plans/rendering/worker-frame-hold.md §6g).
 *
 * The compositor draws when the page's own frame is ready and does not wait
 * for a worker's canvas, so where the worker's frame lands is a race unless
 * its timing is fixed:
 *
 * - **Early (the hold):** when the worker's tick for this frame is still
 *   running at the page's rAF, the page spins here until the worker says its
 *   frame has been sent, so one draw carries both. Lowest latency.
 * - **Late:** the worker draws in a task after its rAF callback, so its frame
 *   goes out at the canvas's next BeginFrame and always makes that vsync's
 *   draw: one frame behind the DOM, steady, with nearly a whole frame for the
 *   tick (BrowserRuntime). Needs no shared memory.
 *
 * Shared memory (the hold, and switching modes from the page) needs
 * cross-origin isolation (`crossOriginIsolated`).
 *
 * Layout of the shared buffer:
 *   Int32 [0] state (worker): 0 idle, 1 tick running, 2 frame sent
 *   Int32 [1] sequence (worker): counts frames sent
 *   Int32 [2] mode (page): 0 free, 1 early, 2 late
 *   Int32 [3] half rate (worker): 1 while late mode draws every second vsync
 *   Float64 [0] tick start (worker; ms, performance.timeOrigin + now(), a shared clock)
 *   Float64 [1] tick estimate (worker; ms): a high percentile of recent ticks
 */
export const HOLD_BYTES = 32;
export const IDLE = 0, TICKING = 1, SENT = 2;
export const FREE = 0, EARLY = 1, LATE = 2;
const STATE = 0, SEQ = 1, MODE = 2, HALF = 3;
const START = 0, ESTIMATE = 1;
const ints = (b: SharedArrayBuffer) => new Int32Array(b, 0, 4);
const floats = (b: SharedArrayBuffer) => new Float64Array(b, 16, 2);
const clock = () => performance.timeOrigin + performance.now();
import { flushSync } from 'svelte';
import { frameMode } from './frame_mode.svelte';

/** The worker's side: mark a tick running, then sent; read the mode. */
export class FrameHoldWriter {
  private i32: Int32Array;
  private f64: Float64Array;
  private recent: number[] = [];

  constructor(buffer: SharedArrayBuffer) {
    this.i32 = ints(buffer);
    this.f64 = floats(buffer);
  }

  /** A tick starts now. */
  begin(): void {
    this.f64[START] = clock();
    Atomics.store(this.i32, STATE, TICKING);
  }

  /**
   * The tick took `ms`; its frame goes out at the next frame push. The
   * estimate the page predicts with counts only ticks started by input
   * (`input`): those are the ones it holds for, and late mode's ticks at vsync
   * ran longer (measured 2026-11-06, §6g).
   */
  measured(ms: number, input = true): void {
    // Leaving late mode starts a new estimate, after the first few ticks
    // (those can run long while the canvas settles back).
    const late = this.late();
    if (late !== this.wasLate) { this.wasLate = late; if (!late) { this.recent = []; this.skip = 3; } }
    if (!input) return;
    if (this.skip > 0) { this.skip--; return; }
    this.recent.push(ms);
    if (this.recent.length > 40) this.recent.shift();
    const sorted = [...this.recent].sort((a, b) => a - b);
    // A high percentile, but one a couple of outliers in a short window cannot set.
    this.f64[ESTIMATE] = sorted[Math.floor(sorted.length * 0.75)] ?? ms;
  }
  private wasLate = false;
  private skip = 0;

  /** The frame has been handed to the compositor. */
  sent(): void {
    Atomics.add(this.i32, SEQ, 1);
    Atomics.store(this.i32, STATE, SENT);
  }

  idle(): void {
    Atomics.store(this.i32, STATE, IDLE);
  }

  /** Half rate (late mode), for the page's readout. */
  half(on: boolean): void {
    Atomics.store(this.i32, HALF, on ? 1 : 0);
  }

  /** Late mode: the worker draws after its rAF callback, not on input. */
  late(): boolean {
    return Atomics.load(this.i32, MODE) === LATE;
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
  /** The current mode: free, early or late. */
  mode: 'free' | 'early' | 'late';
  /** `auto`: recent mode changes. */
  events: string[];
  /** Recent skipped frames: when the tick started, its estimate, the cap (ms after vsync). */
  skips: string[];
  /** Work first: the forced layout before a hold (ms). */
  layoutMs: { p50: number; p90: number };
  /** Work first, development builds: DOM changes made after the hold began, in the same frame (each costs a second layout). */
  lateWrites: number;
  workFirst: boolean;
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
  /** The starting mode (§6g): early holds the page; late, the worker draws a frame later. */
  mode?: 'early' | 'late';
  /**
   * Choose the mode per interaction (§6g): early when the worker's frames
   * keep fitting, late after misses, retried after a doubling cooldown; late
   * between interactions.
   */
  auto?: boolean;
  /**
   * Work, then wait (§6g, default true): the hold runs after the page's
   * other rAF callbacks, applies Svelte's pending updates and forces layout
   * first, then waits only for what the worker still needs. false: the
   * Phase 1–4 order (wait, then the page's work), for comparison.
   */
  workFirst?: boolean;
  /**
   * `auto`: what to remember this device's outcome under (backend and canvas
   * size), so a machine where early mode keeps failing starts late (§6g, 4g).
   */
  profileKey?: string;
}

/** Remembered per device and canvas size (§6g, 4g), in local storage. */
type Profile = { streak: number; limit: number; at: number };
const PROFILES = 'iron.frameModes.v1';
function loadProfile(key: string): Profile | null {
  try { return (JSON.parse(localStorage.getItem(PROFILES) ?? '{}') as Record<string, Profile>)[key] ?? null; }
  catch { return null; }
}
function saveProfile(key: string, profile: Profile): void {
  try {
    const all = JSON.parse(localStorage.getItem(PROFILES) ?? '{}') as Record<string, Profile>;
    all[key] = profile;
    // Keep the 20 most recent sizes.
    const keep = Object.entries(all).sort((a, b) => b[1].at - a[1].at).slice(0, 20);
    localStorage.setItem(PROFILES, JSON.stringify(Object.fromEntries(keep)));
  } catch { /* storage unavailable: nothing is remembered */ }
}

/** No input for this long ends an interaction (§3). */
const QUIET_MS = 100;
/** Early mode's recent frames that are judged (§6g): the worker's frame was out in time, or held for. */
const WINDOW = 40;
/** Judged only once this many have been seen in the interaction (the hold's estimates settle first). */
const WARMUP = 20;
/** Frames not judged right after switching to early: the worker may still be finishing a late-mode tick. */
const SETTLE = 8;
/** Early mode ends when more than this share of them went out without the worker's frame. */
const MAX_UNHELD = 0.2;

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
  private readonly onInput = () => {
    const now = performance.now();
    const starting = now - this.lastInput > QUIET_MS;
    this.lastInput = now;
    if (this.auto && starting) this.interactionStart(now);
  };
  // The mode controller (`auto`, §6g).
  private readonly auto: boolean;
  private interacting = false;
  /** Late until then, after early failed. */
  private fallbackUntil = 0;
  private cooldownMs = 2000;
  private fallbackReason = '';
  /** Time in early mode since the last fallback (ms). */
  private earlyMs = 0;
  /** Early mode's recent frames: with the worker's frame (true) or without. */
  private outcomes: boolean[] = [];
  /** A frame not held while the worker ticked: the worker's sequence then, judged after the frame. */
  private unheldSeq: number | null = null;
  /** Early-mode frames still to skip before judging (SETTLE). */
  private settling = 0;
  /** Fallbacks in a row without a minute of early mode since (remembered). */
  private streak = 0;
  /** Recent mode changes (`auto`), for stats: time (ms), mode, why. */
  private events: string[] = [];
  /** Recent skipped frames' prediction (ms after vsync), for stats. */
  private skips: string[] = [];
  private readonly profileKey: string | null;
  private readonly workFirst: boolean;
  private frameCallback: ((vsync: number) => void) | null = null;
  private running = false;
  /** Work first: between the forced layout and the frame going out (the guard's window). */
  private holdWindow = false;
  private layouts: number[] = [];
  private lateWrites = 0;
  private guard: MutationObserver | null = null;

  constructor(buffer: SharedArrayBuffer, options: FrameHoldOptions = {}) {
    this.i32 = ints(buffer);
    this.f64 = floats(buffer);
    Atomics.store(this.i32, MODE, options.mode === 'late' ? LATE : EARLY);
    this.workFirst = options.workFirst ?? true;
    // Work first leaves only paint and commit after the hold (~0.5 ms
    // measured), so the starting limit can sit nearer Chrome's (~6 ms):
    // 5.5 ms kept the DOM unchanged with no missed frames (2026-11-06, §6g).
    this.limit = options.limitMs ?? (this.workFirst ? 5.5 : 4.5);
    this.floor = options.floorMs ?? 2.5;
    this.ceiling = options.ceilingMs ?? 5.5;
    this.always = options.always ?? false;
    this.adaptive = options.adaptive ?? true;
    this.auto = options.auto ?? false;
    this.profileKey = this.auto ? options.profileKey ?? null : null;
    if (this.auto) {
      Atomics.store(this.i32, MODE, LATE);
      // A device where early mode failed the last times starts late, and
      // retries after the cooldown those failures earned.
      const profile = this.profileKey ? loadProfile(this.profileKey) : null;
      if (profile) {
        this.streak = profile.streak;
        if (profile.limit > 0) this.limit = profile.limit;
        if (this.streak >= 2) {
          this.cooldownMs = Math.min(60000, 2000 * 2 ** this.streak);
          this.fallbackUntil = performance.now() + this.cooldownMs;
          this.fallbackReason = 'early mode failed on this device before';
        }
      }
    }
    // The page's work after this callback: from posting at its end to the
    // next task, which runs once the frame's rendering steps are done.
    this.channel.port1.onmessage = (e: MessageEvent<number>) => {
      this.after.push(performance.now() - e.data);
      if (this.after.length > 30) this.after.shift();
      this.holdWindow = false;
      // Work first: ask for the next frame now, after every other rAF callback
      // of this frame asked for theirs, so the hold runs last in it.
      if (this.workFirst && this.running && this.frameCallback) this.raf = requestAnimationFrame(this.frameCallback);
      // A frame the hold did not wait for still carries the worker's frame if
      // that was sent before the page's frame went out (this task runs after).
      if (this.unheldSeq !== null) {
        const sent = Atomics.load(this.i32, SEQ) !== this.unheldSeq;
        this.unheldSeq = null;
        if (this.auto && this.interacting && this.mode === 'early') this.judge(performance.now(), sent);
      }
    };
  }

  start(): void {
    for (const type of ['pointermove', 'pointerdown', 'wheel', 'keydown'] as const) {
      window.addEventListener(type, this.onInput, { capture: true, passive: true });
    }
    const frame = (vsync: number) => {
      this.frame(vsync);
      this.channel.port2.postMessage(performance.now());
      if (!this.workFirst) this.raf = requestAnimationFrame(frame);
    };
    this.running = true;
    this.frameCallback = frame;
    this.raf = requestAnimationFrame(frame);
    // Development: any DOM change after the hold began, before the frame goes
    // out, means a second layout; count it and say where once.
    if (this.workFirst && import.meta.env.DEV) {
      let warned = false;
      this.guard = new MutationObserver((records) => {
        if (!this.holdWindow) return;
        this.lateWrites += records.length;
        if (!warned) {
          warned = true;
          const t = records[0].target;
          const where = (t instanceof Element ? t : t.parentElement)?.closest('[data-panel-id], .dv-groupview, [class]');
          console.warn('frame hold: the DOM changed after the hold began (a second layout this frame):', records[0].type, where ?? t);
        }
      });
      this.guard.observe(document.documentElement, { subtree: true, childList: true, attributes: true, characterData: true });
    }
    // The readout shows the mode from the start (`auto` updates it as it changes).
    this.publish(this.auto ? '' : 'fixed by ?frameHold');
    (globalThis as any).__frameHold = { stats: () => this.stats(), reset: () => this.reset(), setMode: (m: 'free' | 'early' | 'late') => { this.mode = m; } };
  }

  stop(): void {
    this.running = false;
    this.guard?.disconnect();
    this.guard = null;
    cancelAnimationFrame(this.raf);
    frameMode.set('free', '');
    for (const type of ['pointermove', 'pointerdown', 'wheel', 'keydown'] as const) {
      window.removeEventListener(type, this.onInput, true);
    }
    this.channel.port1.close();
    Atomics.store(this.i32, MODE, FREE);
  }

  /** The current mode. */
  get mode(): 'free' | 'early' | 'late' {
    const m = Atomics.load(this.i32, MODE);
    return m === LATE ? 'late' : m === EARLY ? 'early' : 'free';
  }

  set mode(mode: 'free' | 'early' | 'late') {
    Atomics.store(this.i32, MODE, mode === 'late' ? LATE : mode === 'early' ? EARLY : FREE);
  }

  /** An interaction starts (`auto`): early unless early failed recently. */
  private interactionStart(now: number): void {
    this.interacting = true;
    this.outcomes = [];
    if (now >= this.fallbackUntil) {
      this.settling = SETTLE;
      this.mode = 'early';
      this.events.push(`${now.toFixed(0)} early`);
      this.publish('');
    } else {
      this.publish(`${this.fallbackReason}; early again in ${Math.ceil((this.fallbackUntil - now) / 1000)} s`);
    }
  }

  /** Early failed this interaction (`auto`): late now, early again after the cooldown. */
  private fallBack(now: number, reason: string): void {
    this.events.push(`${now.toFixed(0)} late: ${reason}; outcomes ${this.outcomes.map((x) => (x ? '+' : '-')).join('')}`);
    this.mode = 'late';
    this.fallbackReason = reason;
    this.fallbackUntil = now + this.cooldownMs;
    this.cooldownMs = Math.min(60000, this.cooldownMs * 2);
    this.earlyMs = 0;
    this.streak++;
    this.remember();
    this.publish(reason);
  }

  private remember(): void {
    if (this.profileKey) saveProfile(this.profileKey, { streak: this.streak, limit: this.limit, at: Date.now() });
  }

  /** What the readout shows (§6g): ahead between interactions, half when the worker draws every second vsync. */
  private status(): 'early' | 'late' | 'half' | 'ahead' {
    const half = Atomics.load(this.i32, HALF) === 1;
    if (this.mode === 'early') return 'early';
    if (half) return 'half';
    return this.auto && !this.interacting ? 'ahead' : 'late';
  }

  private publish(reason: string): void {
    const mode = this.status();
    frameMode.set(mode, mode === 'half' ? "the worker's frames take longer than a frame" : mode === 'ahead' ? 'between interactions: frames drawn for the vsync they are shown at' : reason);
    frameMode.numbers = { limit: this.limit, tick: this.f64[ESTIMATE] || 0, after: quantile(this.after, 0.9) };
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
    if (this.auto) {
      if (quiet && this.interacting) {
        // Between interactions late mode keeps playback and other motion steady.
        this.interacting = false;
        this.mode = 'late';
      }
      // Half rate is the worker's call; follow it (and the first frame's state).
      const shown = this.status();
      if (shown !== frameMode.mode) this.publish(this.mode === 'early' ? '' : this.fallbackReason);
      if (this.interacting && this.mode === 'early') {
        this.earlyMs += interval;
        if (this.earlyMs > 60000 && (this.cooldownMs !== 2000 || this.streak !== 0)) { this.cooldownMs = 2000; this.streak = 0; this.remember(); }
        if (this.backedOff) { this.fallBack(now, 'two late frames in this interaction'); }
      }
    }

    if (Atomics.load(this.i32, MODE) === LATE) return;
    if (Atomics.load(this.i32, STATE) !== TICKING) {
      // The worker's frame is already out (or there is none): this frame is fine.
      if (this.auto && this.interacting) this.judge(now, true);
      return;
    }
    this.counts.pending++;
    // Armed while input is recent: hover, drag, wheel, keys (§3).
    if (!this.always && quiet) { this.counts.disarmed++; return; }
    if (this.backedOff) { this.counts.backedOff++; return; }
    const seq = Atomics.load(this.i32, SEQ);
    const held = this.hold(now, vsync, ceiling);
    if (this.auto && this.interacting) {
      if (held) this.judge(now, true);
      else this.unheldSeq = seq;
    }
  }

  /** Early mode (`auto`): this frame carried the worker's frame, or not; fall back when too many did not. */
  private judge(now: number, ok: boolean): void {
    if (this.settling > 0) { this.settling--; return; }
    this.outcomes.push(ok);
    if (this.outcomes.length > WINDOW) this.outcomes.shift();
    const missed = this.outcomes.filter((x) => !x).length;
    if (this.outcomes.length >= WARMUP && missed > MAX_UNHELD * this.outcomes.length) {
      this.fallBack(now, `the worker's frame was not ready for ${missed} of the last ${this.outcomes.length} frames`);
    }
  }

  /** Hold this frame if the worker can finish within the cap; true if it did and the frame was sent. */
  private hold(now: number, vsync: number, ceiling: number): boolean {
    if (this.workFirst) {
      // The page's work first: Svelte's pending updates, then layout now
      // rather than after the hold, so the wait overlaps it and what is left
      // after the hold is only paint and commit (measured by \`after\`).
      const t0 = performance.now();
      flushSync();
      void document.body.offsetWidth;
      now = performance.now();
      this.layouts.push(now - t0);
      if (this.layouts.length > 60) this.layouts.shift();
      this.holdWindow = true;
    }
    const cap = this.adaptive
      ? vsync + Math.min(this.limit, ceiling) - quantile(this.after, 0.9) - 0.3
      : vsync + this.limit;
    if (cap <= now) { this.counts.noRoom++; return false; }
    // Hold only when the worker can finish within the cap (§2, step 3).
    const start = this.f64[START] - performance.timeOrigin;
    const predicted = start + (this.f64[ESTIMATE] || 0) + 0.3;
    if (predicted > cap) {
      this.counts.skipped++;
      this.skips.push(`start ${(start - vsync).toFixed(1)} est ${(this.f64[ESTIMATE] || 0).toFixed(1)} cap ${(cap - vsync).toFixed(1)}`);
      if (this.skips.length > 12) this.skips.shift();
      return false;
    }
    this.counts.held++;
    this.lastHeld = true;
    let t = now;
    while (Atomics.load(this.i32, STATE) === TICKING && t < cap) t = performance.now();
    this.holds.push(t - now);
    if (Atomics.load(this.i32, STATE) === TICKING) { this.counts.timedOut++; return false; }
    this.counts.released++;
    return true;
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
      mode: this.mode,
      events: this.events.slice(-20),
      layoutMs: { p50: r(quantile(this.layouts, 0.5)), p90: r(quantile(this.layouts, 0.9)) },
      lateWrites: this.lateWrites,
      workFirst: this.workFirst,
      skips: this.skips.slice(),
    };
  }

  reset(): void {
    this.holds = [];
    this.layouts = [];
    this.lateWrites = 0;
    for (const k of Object.keys(this.counts) as (keyof typeof this.counts)[]) this.counts[k] = 0;
  }
}
