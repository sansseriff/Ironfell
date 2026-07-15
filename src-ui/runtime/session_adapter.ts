import { AdapterBridge } from './adapter_bridge';
import { WasmLoader } from './wasm_loader';

export type RuntimeMode = 'worker' | 'main';

export interface PanelRectMsg {
  id: string;
  kind: string;
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
export class SessionAdapter {
  readonly mode: RuntimeMode;
  private bridge: AdapterBridge | null = null;
  private wasmLoader = new WasmLoader();
  private messageHandler: ((msg: any) => void) | null = null;
  private workerIsReady = false;
  private enginePrepared = false;
  private disposed = false;

  private pendingInit: { canvas: HTMLCanvasElement; dpr: number } | null = null;
  private latestCanvasSize: { width: number; height: number } | null = null;
  private latestPanelRects = new Map<string, PanelRectMsg>();

  constructor(mode: RuntimeMode) {
    this.mode = mode;
  }

  onMessage(handler: (msg: any) => void): void {
    this.messageHandler = handler;
  }

  post(data: any, transfer?: any[]): void {
    if (!this.bridge) return;
    this.bridge.post(data, transfer as any);
  }

  isPrepared(): boolean {
    return this.enginePrepared;
  }

  /** Create the bridge, ship the wasm, and create the single Bevy window. */
  attachCanvas(canvas: HTMLCanvasElement): void {
    if (this.bridge) {
      console.warn('SessionAdapter: canvas already attached');
      return;
    }
    this.bridge = new AdapterBridge(this.mode, canvas);
    this.bridge.setHandler((data: any) => this.handleBridgeMessage(data));
    this.pendingInit = { canvas, dpr: window.devicePixelRatio || 1 };
    this.wasmLoader.sendToAdapter(this.bridge).catch(e => console.error('WASM send failed', e));
  }

  /** Full-window canvas backing size (physical px). */
  resizeCanvas(width: number, height: number): void {
    this.latestCanvasSize = { width, height };
    if (this.enginePrepared) {
      this.post({ ty: 'resize', width, height });
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
    this.disposed = true;
    try { this.stop(); } catch { }
    // Release the Bevy app (main mode frees GPU resources in-place; the worker is
    // torn down with terminate() inside bridge.dispose()).
    try { this.post({ ty: 'releaseApp' }); } catch { }
    try { this.bridge?.dispose(); } catch { }
    this.bridge = null;
    this.workerIsReady = false;
    this.enginePrepared = false;
    this.latestPanelRects.clear();
    this.latestCanvasSize = null;
    this.pendingInit = null;
  }

  private handleBridgeMessage(data: any) {
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
        this.bridge.post({ ty: 'init', canvas: offscreen, devicePixelRatio: dpr }, [offscreen as any]);
        return;
      } catch (e: any) {
        console.error('Offscreen transfer failed, falling back to direct canvas:', e);
      }
    }
    this.bridge.post({ ty: 'init', canvas, devicePixelRatio: dpr });
  }
}
