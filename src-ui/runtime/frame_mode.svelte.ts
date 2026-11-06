/**
 * The worker canvas's frame mode (plans/rendering/worker-frame-hold.md §6g),
 * for the FPS readout. Written by FrameHold when the mode changes, not per
 * frame.
 *
 * - `early`: the page holds its frame for the worker's (lowest latency).
 * - `late`: the fallback; the worker's frame follows the page's, one frame
 *   behind the DOM, never flipping.
 * - `half`: the worker draws every second vsync (its tick no longer fits).
 * - `ahead`: playback rendered for the next vsync.
 * - `free`: no frame modes (no cross-origin isolation, or not Chromium).
 */
export type FrameModeName = 'free' | 'early' | 'late' | 'half' | 'ahead';

class FrameMode {
  mode: FrameModeName = $state('free');
  /** Why the mode is what it is, in a few words (for the tooltip). */
  reason = $state('');
  /** When it last changed (performance.now()). */
  since = $state(0);
  /** Early-mode numbers for the tooltip: learned limit, tick p90, page's later work p90 (ms). */
  numbers: { limit: number; tick: number; after: number } | null = $state(null);

  set(mode: FrameModeName, reason: string): void {
    if (mode === this.mode && reason === this.reason) return;
    this.mode = mode;
    this.reason = reason;
    this.since = performance.now();
  }
}

export const frameMode = new FrameMode();
