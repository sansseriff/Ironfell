/**
 * Frame-cadence probe for the perf grid.
 *
 * Records, for every rAF-driven engine tick:
 *   - the rAF callback timestamp (vsync-aligned, from the rAF argument)
 *   - how long the wasm tick (app.update) took on this thread
 *
 * Ring buffer holds the most recent ~2 minutes at 60fps; recording is two
 * typed-array writes per frame so the probe itself cannot perturb cadence.
 *
 * Usage: runs unconditionally in both adapters; dump with `window.__probe()`
 * from the page console (see panel_manager), or post {ty:'probeStats'} directly.
 *
 * Reading the output:
 *   - rafDeltaMs p50 ~16.7 with tight p95/p99 = healthy 60fps cadence.
 *   - Spikes in p95/p99/max with LOW tickMs = frames are being *withheld*
 *     (compositor/BeginFrame side), not caused by CPU in the tick.
 *   - rafDeltaBuckets shows the shape: d33 = halved frames (30fps), d50 = 20fps, etc.
 */

const CAP = 7200;

function summary(values: number[]) {
  const s = [...values].sort((a, b) => a - b);
  const at = (p: number) => +(s[Math.min(s.length - 1, Math.floor(p * s.length))] ?? 0).toFixed(2);
  return { p50: at(0.5), p95: at(0.95), p99: at(0.99), max: +(s[s.length - 1] ?? 0).toFixed(2) };
}

export interface CadenceStats {
  frames: number;
  elapsedMs: number;
  effectiveFps: number;
  rafDeltaMs: { p50: number; p95: number; p99: number; max: number };
  /** counts of rAF deltas by bucket: <18ms (healthy 60), 18-24, 24-36 (~30fps), 36-60 (~20fps), >60 */
  rafDeltaBuckets: { under18: number; d18to24: number; d24to36: number; d36to60: number; over60: number };
  tickMs: { p50: number; p95: number; p99: number; max: number };
  /** When the tick started and ended, ms after its rAF timestamp (the
   *  frame's vsync-aligned time): how close to the frame's deadline the
   *  engine finished (plans/rendering/latency-view.md §7). */
  startMs: { p50: number; p95: number; p99: number; max: number };
  endMs: { p50: number; p95: number; p99: number; max: number };
  /** Worker runtime: for the pointer a tick consumed, ms since the main
   *  thread posted it and since it arrived in the worker. */
  pointerAgeMs?: { p50: number; p95: number; p99: number; max: number };
  pointerWaitMs?: { p50: number; p95: number; p99: number; max: number };
}

export class CadenceProbe {
  private rafTs = new Float64Array(CAP);
  private tick = new Float64Array(CAP);
  private start = new Float64Array(CAP);
  private ages: number[] = [];
  private waits: number[] = [];
  private n = 0;

  record(rafTimestamp: number, tickDurationMs: number, startedAt = rafTimestamp): void {
    const i = this.n % CAP;
    this.rafTs[i] = rafTimestamp;
    this.tick[i] = tickDurationMs;
    this.start[i] = startedAt - rafTimestamp;
    this.n++;
  }

  recordPointer(ageMs: number, waitMs: number): void {
    if (this.ages.length >= CAP) return;
    this.ages.push(ageMs);
    this.waits.push(waitMs);
  }

  reset(): void {
    this.n = 0;
    this.ages = [];
    this.waits = [];
  }

  stats(): CadenceStats | { frames: number } {
    const count = Math.min(this.n, CAP);
    if (count < 2) return { frames: count };

    // Reconstruct chronological order from the ring buffer.
    const first = this.n >= CAP ? this.n % CAP : 0;
    const ts: number[] = new Array(count);
    const tick: number[] = new Array(count);
    const start: number[] = new Array(count);
    for (let k = 0; k < count; k++) {
      const i = (first + k) % CAP;
      ts[k] = this.rafTs[i];
      tick[k] = this.tick[i];
      start[k] = this.start[i];
    }

    const deltas: number[] = new Array(count - 1);
    for (let k = 1; k < count; k++) deltas[k - 1] = ts[k] - ts[k - 1];

    const sortedD = [...deltas].sort((a, b) => a - b);
    const sortedT = [...tick].sort((a, b) => a - b);
    const pct = (arr: number[], p: number) =>
      +(arr[Math.min(arr.length - 1, Math.floor(p * arr.length))]).toFixed(2);

    const buckets = { under18: 0, d18to24: 0, d24to36: 0, d36to60: 0, over60: 0 };
    for (const d of deltas) {
      if (d < 18) buckets.under18++;
      else if (d < 24) buckets.d18to24++;
      else if (d < 36) buckets.d24to36++;
      else if (d < 60) buckets.d36to60++;
      else buckets.over60++;
    }

    const elapsed = ts[count - 1] - ts[0];
    return {
      frames: count,
      elapsedMs: +elapsed.toFixed(1),
      effectiveFps: +((deltas.length * 1000) / elapsed).toFixed(2),
      rafDeltaMs: {
        p50: pct(sortedD, 0.5),
        p95: pct(sortedD, 0.95),
        p99: pct(sortedD, 0.99),
        max: +sortedD[sortedD.length - 1].toFixed(2),
      },
      rafDeltaBuckets: buckets,
      tickMs: {
        p50: pct(sortedT, 0.5),
        p95: pct(sortedT, 0.95),
        p99: pct(sortedT, 0.99),
        max: +sortedT[sortedT.length - 1].toFixed(2),
      },
      startMs: summary(start),
      endMs: summary(start.map((s, k) => s + tick[k])),
      ...(this.ages.length ? { pointerAgeMs: summary(this.ages), pointerWaitMs: summary(this.waits) } : {}),
    };
  }
}
