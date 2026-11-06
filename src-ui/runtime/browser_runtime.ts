/** Browser bootstrap and frame driver, originally based on
 * https://github.com/jinleili/bevy-in-web-worker.
 * Ordinary input/control semantics live in Rust's engine_protocol. */
import { loadGlue, type Glue } from './glue';
import { FrameHoldWriter } from './frame_hold';
import { CadenceProbe } from './cadence_probe';
import { dispatchEngineMessage } from './engine_messages';
import { FontLoader } from './font_loader';
import type { UiEvent, UiRequest } from './protocol';
import { installWasmEvents } from './wasm_events';

type BrowserMode = 'main' | 'worker';
type Canvas = HTMLCanvasElement | OffscreenCanvas;

export class BrowserRuntime {
  onmessage: ((event: { data: UiEvent }) => void) | null = null;

  private glue: Glue | null = null;
  private appHandle = 0n;
  private canvas: Canvas | null = null;
  private variantFlags = 0;
  private prepared = false;
  private running = true;
  private disposed = false;
  private rafId: number | null = null;
  private pointer: { x: number; y: number; sent?: number; arrived: number } | null = null;
  private probe = new CadenceProbe();
  private fonts = new FontLoader(
    bytes => this.engine.register_font(bytes),
    families => this.emit({ ty: 'fontsLoaded', families }),
  );

  constructor(private readonly mode: BrowserMode) {
    installWasmEvents(globalThis, this.emit, requests => this.fonts.request(requests));
  }

  async postMessage(data: UiRequest, _transfer?: Transferable[]): Promise<void> {
    if (this.disposed) return;
    switch (data.ty) {
      case 'wasmData':
        this.glue = await loadGlue(data.backend);
        if (this.disposed) return;
        await this.glue.default({ module_or_path: data.wasmData });
        if (this.disposed) return;
        this.variantFlags = data.variantFlags >>> 0;
        this.emit({ ty: 'workerIsReady' });
        return;
      case 'init':
        this.tickOnInput = data.tick !== 'raf';
        this.tickBusy = data.tickBusy ?? 0;
        this.halfRateAllowed = data.halfRate ?? false;
        if (data.hold) {
          const writer = new FrameHoldWriter(data.hold);
          this.hold = writer;
          this.release.port1.onmessage = () => writer.sent();
        }
        this.attachCanvas(data.canvas, data.devicePixelRatio);
        return;
      case 'resize':
        if (this.canvas && this.appHandle !== 0n) {
          this.engine.set_scale_factor(this.appHandle, data.dpr);
          this.canvas.width = data.width;
          this.canvas.height = data.height;
          this.engine.resize(this.appHandle, data.width, data.height);
        }
        return;
      case 'startRunning':
        this.running = true;
        this.scheduleFrame();
        return;
      case 'stopRunning':
        this.stop();
        return;
      case 'releaseApp':
        this.releaseApp();
        return;
      case 'mousemove':
        if (this.mode === 'worker') {
          this.pointer = { x: data.x, y: data.y, sent: (data as { t?: number }).t, arrived: performance.timeOrigin + performance.now() };
          // See `tickOnInput`: the frame for this motion starts now, once per frame.
          if (this.tickOnInput && !this.late() && !this.tickedSinceRaf && this.prepared && this.running && this.appHandle !== 0n) {
            this.tickedSinceRaf = true;
            if (this.ahead) { this.engine.set_frame_time(this.appHandle, NaN); this.ahead = false; }
            this.tick(performance.now(), true);
          }
        } else if (this.appHandle !== 0n) this.engine.mouse_move(this.appHandle, data.x, data.y);
        return;
      case 'probeStats':
        this.emit({ ty: 'probeStats', stats: this.probe.stats() });
        return;
      case 'probeReset':
        this.probe.reset();
        return;
      default: {
        const handle = this.appHandle;
        await dispatchEngineMessage(this.glue, handle, data, this.emit,
          () => !this.disposed && this.appHandle === handle && handle !== 0n);
      }
    }
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.releaseApp();
    this.onmessage = null;
  }

  private get engine(): Glue {
    if (!this.glue) throw new Error('WASM is not initialized');
    return this.glue;
  }

  private emit = (data: UiEvent): void => {
    if (!this.disposed) this.onmessage?.({ data });
  };

  private attachCanvas(canvas: Canvas, dpr: number): void {
    this.canvas = canvas;
    // HTMLCanvasElement and OffscreenCanvas provide the same surface operations
    // here; wasm-bindgen declares only the worker's OffscreenCanvas shape.
    this.appHandle = this.engine.init_bevy_app_with_canvas(
      canvas as OffscreenCanvas, dpr, this.mode === 'worker', this.variantFlags,
    );
    this.checkPrepared();
    this.scheduleFrame();
  }

  private checkPrepared(): void {
    if (this.prepared || this.appHandle === 0n) return;
    if (this.engine.is_preparation_completed(this.appHandle) > 0) {
      this.prepared = true;
      this.emit({ ty: 'enginePrepared' });
    }
  }

  private scheduleFrame(): void {
    if (!this.disposed && this.running && this.appHandle !== 0n && this.rafId === null) {
      this.rafId = requestAnimationFrame(this.frame);
    }
  }

  /**
   * Worker only (plans/rendering/latency-view.md §7): run the engine tick when
   * pointer motion arrives, at most once per frame, instead of at the next
   * rAF. The browser hands the page each frame's coalesced motion a few ms
   * before vsync (~12 ms into a 16.7 ms frame on macOS Chrome); the DOM shows
   * it at the next vsync's draw. A worker canvas is a separate surface the
   * compositor does not wait for, and a light page is drawn ~1.2 ms after
   * vsync, so a tick that starts at the rAF (ready ~4.5 ms in) shows a frame
   * after the DOM. Started on arrival, it is ready before that draw.
   * `?workerTick=raf` turns this off (SessionAdapter).
   */
  private tickOnInput = true;
  /**
   * Holding the page's frame (frame_hold.ts): the page spins while a tick
   * runs, until the frame it renders has been handed to the compositor. A
   * worker canvas's frame goes out at the worker's next frame push, right
   * after its rAF callback, so `sent` is posted as a task from that callback.
   */
  private hold: FrameHoldWriter | null = null;
  private releasePending = false;
  private release = new MessageChannel();
  /** `?workerTickBusy=N` (dev): N ms of extra work per tick, a heavier scene. */
  private tickBusy = 0;
  /** Late mode's tick, run as a task after the rAF callback. */
  private lateTick = (() => {
    const channel = new MessageChannel();
    channel.port1.onmessage = (e: MessageEvent<number>) => {
      if (this.disposed || !this.running || this.appHandle === 0n || !this.prepared) return;
      // Ahead (§6g): this frame is shown at the next vsync, so it is drawn
      // for that vsync's time; playback lands on time and steps evenly.
      const frames = this.halfRate ? 2 : 1;
      this.engine.set_frame_time(this.appHandle, e.data + frames * this.interval() - performance.now());
      this.ahead = true;
      const start = performance.now();
      this.tick(e.data);
      this.judgeRate(performance.now() - start);
      // No page waits on this tick; leave no stale "ticking" for early mode.
      this.releasePending = false;
      this.hold?.idle();
    };
    return channel;
  })();
  /**
   * Half rate (§6g, experiment): in late mode a tick has about a frame; when ticks keep
   * running past that, frames would go out at 60 and 30 fps by turns, so
   * the worker draws every second vsync instead, steadily, until its ticks
   * are comfortably short again.
   */
  private halfRate = false;
  /**
   * `?halfRate=1` only. Measured 2026-11-06 (§6g): with ~16–20 ms ticks it ran
   * at 30 fps but not evenly (steps of 0–3 frames), and late mode's ticks run
   * longer than input ticks, so it also engaged where free running kept 60.
   */
  private halfRateAllowed = false;
  private lateTicks: number[] = [];
  private judgeRate(ms: number): void {
    if (!this.halfRateAllowed) return;
    this.lateTicks.push(ms);
    if (this.lateTicks.length > 30) this.lateTicks.shift();
    const interval = this.interval();
    // Only ticks past a whole frame drop frames (a ~16 ms tick still kept
    // 60 fps at 16.7 ms, measured 2026-11-06), so that is the line.
    const slow = this.lateTicks.filter((t) => t > 1.05 * interval).length;
    const next = this.halfRate
      ? !(this.lateTicks.length >= 30 && this.lateTicks.every((t) => t < 0.75 * interval))
      : slow >= 5;
    if (next !== this.halfRate) {
      this.halfRate = next;
      this.lateTicks = [];
      this.hold?.half(next);
    }
  }
  /** The engine's clock is set ahead (late mode); reset when ticking otherwise. */
  private ahead = false;
  private rafIntervals: number[] = [];
  private lastRaf = NaN;
  /** The display's frame interval: median of recent rAF intervals (16.7 ms until known). */
  private interval(): number {
    if (this.rafIntervals.length < 5) return 1000 / 60;
    return [...this.rafIntervals].sort((a, b) => a - b)[this.rafIntervals.length >> 1];
  }
  /** Late mode (frame_hold.ts): set by the page through shared memory. */
  private late(): boolean {
    return this.hold?.late() ?? false;
  }
  /** A pointer message already ran the tick for the coming frame. */
  private tickedSinceRaf = false;

  private frame = (timestamp: number): void => {
    this.rafId = null;
    if (this.disposed || !this.running || this.appHandle === 0n) return;
    try {
      if (this.prepared) {
        if (!Number.isNaN(this.lastRaf)) {
          const dt = timestamp - this.lastRaf;
          if (dt > 2 && dt < 40) { this.rafIntervals.push(dt); if (this.rafIntervals.length > 30) this.rafIntervals.shift(); }
        }
        this.lastRaf = timestamp;
        if (this.late()) {
          // Late mode (frame_hold.ts): draw in a task after this callback, not
          // in it. A frame drawn outside rAF is pushed at the canvas's next
          // BeginFrame, ~0.1 ms after the next vsync, so it always makes that
          // vsync's draw: one frame behind the DOM, every frame, with nearly a
          // whole frame for the tick. Returning at once also keeps the
          // compositor from waiting on this callback (a callback held ~5 ms
          // made Chrome draw the whole page at its deadline, §6g).
          this.tickedSinceRaf = false;
          // Half rate: every second vsync, by the vsync's number so a late
          // callback does not shift the beat.
          if (!this.halfRate || (Math.round(timestamp / this.interval()) & 1) === 0) this.lateTick.port2.postMessage(timestamp);
        } else {
          if (this.ahead) { this.engine.set_frame_time(this.appHandle, NaN); this.ahead = false; }
          // Skip this tick when motion already ran one for this frame. Motion
          // that arrived during that tick waits for the next frame's (on the
          // next move, or at the next rAF when the pointer stops): ticking
          // again here doubled the worker's work under fast input and made its
          // frames alternate between two timings (§6g).
          const covered = this.tickedSinceRaf;
          this.tickedSinceRaf = false;
          if (!covered) this.tick(timestamp);
          if (this.releasePending) {
            this.releasePending = false;
            this.release.port2.postMessage(0);
          }
        }
      } else {
        this.checkPrepared();
      }
      this.scheduleFrame();
    } catch (error) {
      this.stop();
      this.emit({ ty: 'runtimeError', error: String(error) });
    }
  };

  /** One engine frame; `timestamp` is the vsync it is for. */
  /** `input`: started by pointer motion (early mode's estimate counts only these). */
  private tick(timestamp: number, input = false): void {
    try {
      const start = performance.now();
      this.hold?.begin();
      if (this.tickBusy > 0) while (performance.now() < start + this.tickBusy) { /* a heavier tick */ }
      if (this.mode === 'worker') {
        // Only the latest motion is sampled at a worker tick. Main-thread
        // motion has already entered the engine synchronously.
        const pointer = this.pointer;
        this.pointer = null;
        if (pointer?.sent) {
          const at = performance.timeOrigin + start;
          this.probe.recordPointer(at - pointer.sent, at - pointer.arrived);
        }
        this.engine.enter_frame_with_mouse(this.appHandle, pointer?.x ?? 0, pointer?.y ?? 0, pointer !== null);
      } else {
        this.engine.enter_frame(this.appHandle);
      }
      this.probe.record(timestamp, performance.now() - start, start);
      if (this.hold) {
        this.hold.measured(performance.now() - start, input);
        this.releasePending = true;
      }
    } catch (error) {
      this.stop();
      this.emit({ ty: 'runtimeError', error: String(error) });
    }
  }

  private stop(): void {
    this.running = false;
    this.hold?.idle();
    if (this.rafId !== null) cancelAnimationFrame(this.rafId);
    this.rafId = null;
  }

  private releaseApp(): void {
    this.stop();
    const handle = this.appHandle;
    this.appHandle = 0n;
    this.canvas = null;
    this.pointer = null;
    this.prepared = false;
    if (handle !== 0n) {
      try { this.engine.release_app(handle); }
      catch (error) { console.error('release_app failed', error); }
    }
  }
}
