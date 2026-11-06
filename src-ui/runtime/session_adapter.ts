import type { UiRequest, UiEvent } from './protocol';
import { pixelRatio } from './settings.svelte';
import { AdapterBridge } from './adapter_bridge';
import { FrameHold, HOLD_BYTES } from './frame_hold';
import { startPageBusy, startPageLayout } from './page_busy';
import type { Backend } from './backend_policy';

export type RuntimeMode = 'worker' | 'main' | 'native';

export interface PanelRectMsg {
  id: string;
  kind: string;
  /** The composition a "composition" panel shows: a node id ("#4") or a
   *  composition name; "" lets Rust choose. */
  composition: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * One render session = one wasm app instance (in a worker or on the main thread),
 * one full-window canvas, and a set of panel viewport rects.
 *
 * Messages that arrive before the engine is prepared are coalesced (latest canvas
 * size, latest rect per panel id) and flushed on `enginePrepared`.
 */
let nextSessionId = 1;

export class SessionAdapter {
  readonly id = nextSessionId++;
  readonly mode: RuntimeMode;
  private bridge: AdapterBridge | null = null;
  private messageHandler: ((msg: UiEvent) => void) | null = null;
  private workerIsReady = false;
  private enginePrepared = false;
  private disposed = false;
  private frameHold: FrameHold | null = null;
  private stopPageBusy: (() => void) | null = null;
  private stopPageLayout: (() => void) | null = null;
  /** The wasm backend, for the frame modes' per-device memory. */
  private backendName: Backend | null = null;

  private pendingInit: { canvas: HTMLCanvasElement; dpr: number } | null = null;
  private latestCanvasSize: { width: number; height: number; dpr: number } | null = null;
  private latestPanelRects = new Map<string, PanelRectMsg>();

  constructor(mode: RuntimeMode) {
    this.mode = mode;
  }

  onMessage(handler: (msg: UiEvent) => void): void {
    this.messageHandler = handler;
  }

  post(data: UiRequest, transfer?: any[]): void {
    if (!this.bridge || this.disposed) throw new Error('session disposed');
    this.bridge.post({ ...data, sessionId: this.id }, transfer as any);
  }

  isPrepared(): boolean {
    return this.enginePrepared;
  }

  /**
   * Create the bridge, ship the wasm, and create the single Bevy window.
   *
   * `backend` selects which of the two artifacts is loaded. It is decided once,
   * by the caller, and travels with the bytes so the worker pairs them with the
   * matching wasm-bindgen glue.
   */
  attachCanvas(canvas: HTMLCanvasElement, backend: Backend): void {
    if (this.bridge) {
      console.warn('SessionAdapter: canvas already attached');
      return;
    }
    this.bridge = new AdapterBridge(this.mode, canvas);
    this.backendName = backend;
    this.bridge.setHandler((data: any) => this.handleBridgeMessage(data));
    this.pendingInit = { canvas, dpr: pixelRatio() };
    if (import.meta.env.MODE !== 'native' && this.mode !== 'native') {
      const bridge = this.bridge;
      void import('./wasm_loader').then(({ WasmLoader }) => { if (!this.disposed) return new WasmLoader().sendToAdapter(bridge, backend); }).catch(e => { if (!this.disposed) { this.messageHandler?.({ty:'runtimeError', error:String(e), sessionId:this.id}); } });
    }
  }

  /** Full-window canvas backing size (physical px), and the pixel ratio it
   *  was sized at: the app maps pointer positions by it. */
  resizeCanvas(width: number, height: number, dpr: number): void {
    this.latestCanvasSize = { width, height, dpr };
    if (this.enginePrepared) {
      this.post({ ty: 'resize', width, height, dpr });
    }
  }

  /** Upsert a panel viewport rect (physical px, window coordinates). */
  setPanelViewport(rect: PanelRectMsg): void {
    this.latestPanelRects.set(rect.id, rect);
    if (this.enginePrepared) {
      this.post({ ty: 'setPanelViewport', ...rect });
    }
  }

  despawnPanel(id: string): void {
    this.latestPanelRects.delete(id);
    if (this.enginePrepared) {
      this.post({ ty: 'despawnPanel', id });
    }
  }

  /** Re-send canvas size and every known panel rect. Idempotent. */
  syncAll(): void {
    if (!this.enginePrepared) return;
    if (this.latestCanvasSize) {
      this.post({ ty: 'resize', ...this.latestCanvasSize });
    }
    for (const rect of this.latestPanelRects.values()) {
      this.post({ ty: 'setPanelViewport', ...rect });
    }
  }

  start(): void {
    this.post({ ty: 'startRunning' });
  }

  stop(): void {
    this.post({ ty: 'stopRunning' });
  }

  dispose(): void {
    if (this.disposed) return;
    this.frameHold?.stop();
    this.frameHold = null;
    this.stopPageBusy?.();
    this.stopPageBusy = null;
    this.stopPageLayout?.();
    this.stopPageLayout = null;
    try { this.stop(); } catch { }
    // Release the Bevy app (main mode frees GPU resources in-place; the worker is
    // torn down with terminate() inside bridge.dispose()).
    try { this.post({ ty: 'releaseApp' }); } catch { }
    try { this.bridge?.dispose(); } catch { }
    this.disposed = true;
    this.messageHandler = null;
    this.bridge = null;
    this.workerIsReady = false;
    this.enginePrepared = false;
    this.latestPanelRects.clear();
    this.latestCanvasSize = null;
    this.pendingInit = null;
  }

  private handleBridgeMessage(data: UiEvent) {
    if (this.disposed) return;
    data = { ...data, sessionId: this.id };
    if (data.ty === 'runtimeError') { const handler = this.messageHandler; this.dispose(); handler?.(data); return; }
    if (data?.ty === 'workerIsReady') {
      this.workerIsReady = true;
      this.postInit();
    }
    if (data?.ty === 'enginePrepared') {
      this.enginePrepared = true;
      this.syncAll();
    }
    if (this.messageHandler) this.messageHandler(data);
  }

  private postInit() {
    if (!this.bridge || !this.pendingInit) return;
    const { canvas, dpr } = this.pendingInit;
    this.pendingInit = null;
    if (this.mode === 'worker') {
      try {
        const offscreen: OffscreenCanvas = (canvas as any).transferControlToOffscreen();
        const params = new URLSearchParams(location.search);
        // `?workerTick=raf`: tick only at rAF, not on pointer motion (BrowserRuntime.tickOnInput).
        const tick = params.get('workerTick') ?? undefined;
        // `?workerTickBusy=N` (dev): N ms of extra work per worker tick.
        const tickBusy = Number(params.get('workerTickBusy') ?? 0) || 0;
        // `?frameHold=on|always|force|late` (prototype, plans/rendering/worker-frame-hold.md):
        // the page holds its frame for the worker's. Needs cross-origin isolation,
        // and helps only where the compositor draws the worker's canvas with the
        // page's frame: Chromium (§6d). Safari shows the worker's frames on their
        // own, so holding there only delays the DOM; `force` holds in any engine.
        const chromium = !!(navigator as any).userAgentData?.brands?.some((b: { brand: string }) => b.brand === 'Chromium');
        // On by default (§6g): the mode controller wherever it can work, Chromium
        // with cross-origin isolation (`bun run dev:hold`, or a server sending
        // the headers). `?frameHold=off` turns it off.
        const holdMode = params.get('frameHold') ?? (chromium && globalThis.crossOriginIsolated ? 'auto' : null);
        let hold: SharedArrayBuffer | undefined;
        if (holdMode && holdMode !== 'off' && !chromium && holdMode !== 'force') {
          console.warn('frameHold: not Chromium, where holding the page does not carry the worker\'s frame; not holding (frameHold=force to override)');
        } else if ((holdMode === 'on' || holdMode === 'always' || holdMode === 'force' || holdMode === 'late' || holdMode === 'auto') && globalThis.crossOriginIsolated) {
          hold = new SharedArrayBuffer(HOLD_BYTES);
          // `&holdCap=ms`: the starting limit; `&holdAdapt=0`: Phase 1's fixed cap.
          // `auto` (§6g): the mode controller, early by default, late as the fallback.
          // `&holdOrder=first`: the hold before the page's work (Phase 1–4), for comparison.
          // `late` (§6g): the worker draws a frame later instead, one frame
          // behind the DOM, steady.
          this.frameHold = new FrameHold(hold, {
            limitMs: Number(params.get('holdCap')) || undefined, always: holdMode === 'always', adaptive: params.get('holdAdapt') !== '0',
            workFirst: params.get('holdOrder') !== 'first', auto: holdMode === 'auto', profileKey: `${(canvas as HTMLCanvasElement).width >> 8}x${(canvas as HTMLCanvasElement).height >> 8}@${this.backendName ?? 'gpu'}`, mode: holdMode === 'late' ? 'late' : 'early',
          });
          this.frameHold.start();
        } else if (holdMode) {
          console.warn('frameHold needs cross-origin isolation (serve with IRON_COI=1); not holding');
        }
        // `?pageBusy=a-b` (dev): a random a–b ms of main-thread work per frame (page_busy.ts).
        this.stopPageBusy = startPageBusy(params.get('pageBusy'));
        // `?pageLayout=a-b` (dev): a random a–b ms of layout per frame (page_busy.ts).
        this.stopPageLayout = startPageLayout(params.get('pageLayout'));
        // `?halfRate=1` (experiment, §6g): late mode draws every second vsync
        // when ticks pass a frame. Off by default: not yet steady.
        const halfRate = params.get('halfRate') === '1';
        this.bridge.post({ ty: 'init', canvas: offscreen, devicePixelRatio: dpr, tick, tickBusy, hold, halfRate }, [offscreen as any]);
        return;
      } catch (e: any) {
        console.error('Offscreen transfer failed, falling back to direct canvas:', e);
      }
    }
    this.bridge.post({ ty: 'init', canvas, devicePixelRatio: dpr });
  }
}
