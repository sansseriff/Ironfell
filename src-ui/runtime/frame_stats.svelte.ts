/**
 * The FPS readout's data (a composition view's settings menu, "Show FPS"):
 * the engine's per-view render counts (`frame_stats`, src/view_rates.rs),
 * read twice a second while any readout is open, as frames per second per
 * view. Views render on demand, so a frozen view reads 0 while an animating
 * one reads the display's rate.
 */
import { controllerManager } from '../controller-manager.svelte';

type Reading = {
  t: number;
  views: Record<string, number>;
  tick_fps: number | null;
  canvas: [number, number] | null;
  build: { profile: string; opt: string; simd: boolean; fast: boolean; gfx: string };
};

const PERIOD_MS = 500;

class FrameStats {
  /** Frames per second each panel rendered over the last period. */
  fps: Record<string, number> = $state({});
  /** The app's frame (tick) rate, which runs whether or not views render. */
  tick: number | null = $state(null);
  canvas: [number, number] | null = $state(null);
  build: Reading['build'] | null = $state(null);
  private last: Reading | null = null;
  private users = 0;
  private timer: ReturnType<typeof setInterval> | null = null;

  /** Keep readings coming while the returned function is not called. */
  use(): () => void {
    if (this.users++ === 0) {
      this.read();
      this.timer = setInterval(() => this.read(), PERIOD_MS);
    }
    return () => {
      if (--this.users === 0 && this.timer) {
        clearInterval(this.timer);
        this.timer = null;
        this.last = null;
      }
    };
  }

  private async read() {
    let r: Reading;
    try {
      r = await controllerManager.docClient.command<Reading>({ op: 'frame_stats' });
    } catch {
      return;
    }
    if (!r || typeof r.t !== 'number') return;
    const prev = this.last;
    this.last = r;
    this.tick = r.tick_fps;
    this.canvas = r.canvas;
    this.build = r.build;
    if (!prev || r.t <= prev.t) return;
    const dt = r.t - prev.t;
    const next: Record<string, number> = {};
    for (const [panel, n] of Object.entries(r.views)) next[panel] = Math.max(0, n - (prev.views[panel] ?? 0)) / dt;
    this.fps = next;
  }
}

export const frameStats = new FrameStats();
