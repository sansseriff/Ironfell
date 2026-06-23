import { AdapterBridge } from './adapter_bridge';
import { WasmLoader } from './wasm_loader';
import type { RenderSession } from './render_session';

export type RuntimeMode = 'worker' | 'main';

export class SessionAdapter implements RenderSession {
  private mode: RuntimeMode;
  private bridge: AdapterBridge | null = null;
  private wasmLoader = new WasmLoader();
  private messageHandler: ((msg: any) => void) | null = null;
  private workerIsReady = false;
  private enginePrepared = false;
  private primaryCanvasId: string | null = null;
  private pendingWindows: Array<{ canvas: HTMLCanvasElement, id: string, kind: string, dpr: number, isPrimary: boolean }> = [];
  private pendingResizes: Array<{ id: string, width: number, height: number }> = [];

  constructor(mode: RuntimeMode) {
    this.mode = mode;
  }

  async initialize(): Promise<void> {
    // Lazy initialization occurs when primary window is created.
    if (this.bridge && this.workerIsReady) return;
    // If bridge exists but not ready, wait for ready.
    if (this.bridge && !this.workerIsReady) {
      await this.waitForReady();
    }
  }

  onMessage(handler: (msg: any) => void): void {
    this.messageHandler = handler;
  }

  post(data: any, transfer?: any[]): void {
    if (!this.bridge) return;
    this.bridge.post(data, transfer as any);
  }

  createWindow(canvas: HTMLCanvasElement, id: string, kind: string, dpr: number, isPrimary: boolean): void {
    // Establish bridge on first window (primary preferred, but not required)
    if (!this.bridge) {
      if (isPrimary) this.primaryCanvasId = id;
      this.bridge = new AdapterBridge(this.mode, canvas);
      this.bridge.setHandler((data: any) => this.handleBridgeMessage(data));
      // Kick wasm send; readiness will be signaled by worker
      this.wasmLoader.sendToAdapter(this.bridge).catch(e => console.error('WASM send failed', e));
    }

    // If not ready yet, queue the window creation and return
    if (!this.workerIsReady) {
      this.pendingWindows.push({ canvas, id, kind, dpr, isPrimary });
      return;
    }

    this.postCreateWindow(canvas, id, kind, dpr, isPrimary);
  }

  resizeWindow(id: string, width: number, height: number): void {
    // Defer resizes until enginePrepared so Bevy windows are fully ready
    if (!this.bridge || !this.enginePrepared) {
      this.pendingResizes.push({ id, width, height });
      return;
    }
    this.bridge.post({ ty: 'resize', canvasId: id, width, height });
  }

  start(): void {
    if (!this.bridge) return;
    this.bridge.post({ ty: 'startRunning' });
  }

  stop(): void {
    if (!this.bridge) return;
    this.bridge.post({ ty: 'stopRunning' });
  }

  dispose(): void {
    try { this.stop(); } catch {}
    try { this.bridge?.dispose(); } catch {}
    this.bridge = null;
    this.workerIsReady = false;
    this.primaryCanvasId = null;
    this.pendingWindows = [];
    this.pendingResizes = [];
  }

  private handleBridgeMessage(data: any) {
    if (data?.ty === 'workerIsReady') {
      this.workerIsReady = true;
      // Flush any queued window creations
      const queued = this.pendingWindows;
      this.pendingWindows = [];
      for (const w of queued) {
        this.postCreateWindow(w.canvas, w.id, w.kind, w.dpr, w.isPrimary);
      }
    }
    if (data?.ty === 'enginePrepared') {
      this.enginePrepared = true;
      // Flush any queued resizes now that Bevy is fully prepared
      const resizes = this.pendingResizes;
      this.pendingResizes = [];
      for (const r of resizes) {
        if (this.bridge) this.bridge.post({ ty: 'resize', canvasId: r.id, width: r.width, height: r.height });
      }
    }
    if (this.messageHandler) this.messageHandler(data);
  }

  private waitForReady(): Promise<void> {
    return new Promise((resolve) => {
      if (this.workerIsReady) return resolve();
      const interval = setInterval(() => {
        if (this.workerIsReady) {
          clearInterval(interval);
          resolve();
        }
      }, 50);
    });
  }

  private postCreateWindow(canvas: HTMLCanvasElement, id: string, kind: string, dpr: number, isPrimary: boolean) {
    if (!this.bridge) return;
    const messageType = isPrimary ? 'init' : 'createAdditionalWindow';
    if (this.mode === 'worker') {
      try {
        const offscreen: OffscreenCanvas = (canvas as any).transferControlToOffscreen();
        this.bridge.post({ ty: messageType, canvas: offscreen, devicePixelRatio: dpr, canvasId: id, kind }, [offscreen as any]);
      } catch (e: any) {
        console.error(`Offscreen transfer failed for ${id}:`, e);
        this.bridge.post({ ty: messageType, canvas: canvas, devicePixelRatio: dpr, canvasId: id, kind });
      }
    } else {
      this.bridge.post({ ty: messageType, canvas: canvas, devicePixelRatio: dpr, canvasId: id, kind });
    }
  }
}


