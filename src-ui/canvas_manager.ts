import { CanvasControl, type CanvasKind } from './canvas_control';
import { SessionAdapter, type RuntimeMode } from './runtime/session_adapter';
import type { RenderSession } from './runtime/render_session';
import { InputManager } from './runtime/input_manager';
import { InspectorClient } from './runtime/inspector_client';
import { SystemState } from './system_state.svelte';

export class CanvasManager {
  private controls = new Map<string, CanvasControl>();
  private session: SessionAdapter | null = null;
  private mode: RuntimeMode = 'worker';
  private input = new InputManager({ enableRaw: true });
  private inspector = new InspectorClient(new SystemState());
  public onInitialized: (() => void) | null = null;

  // Resize debounce configuration
  private resizeDebounceEnabled = true;
  private resizeDebounceMs = 16;
  private resizeTimers = new Map<string, number>();

  // UI flags
  isInitialized = false;
  loadingInProgress = false;
  webGPUSupported = true;
  showWebGPUWarning = false;

  constructor(mode: RuntimeMode = 'worker') {
    this.mode = mode;
  }

  setMode(mode: RuntimeMode) { this.mode = mode; }

  private ensureSession(): RenderSession {
    if (!this.session) {
      this.session = new SessionAdapter(this.mode);
      this.session.onMessage((data) => this.handleSessionMessage(data));
      // Inspector will use the session.post
      this.inspector.init({ post: (data: any, transfer?: any[]) => this.session?.post(data, transfer) } as any);
    }
    return this.session;
  }

  private handleSessionMessage(data: any) {
    switch (data?.ty) {
      case 'workerIsReady':
        // No-op: wait for enginePrepared to ensure Bevy finished preparation
        break;
      case 'enginePrepared':
        this.isInitialized = true;
        if (this.onInitialized) try { this.onInitialized(); } catch {}
        // Initialize input on primary canvas (assume 'viewer-canvas' if present)
        {
          const primary = this.controls.get('viewer-canvas') || Array.from(this.controls.values())[0];
          if (primary) {
            this.input.init(primary.getElement(), {
              post: (payload: any, transfer?: any[]) => this.session?.post(payload, transfer)
            } as any);
          }
        }
        // Force an initial resize on all canvases now that Bevy is prepared
        for (const [id, control] of this.controls.entries()) {
          const el = control.getElement();
          const rect = el.getBoundingClientRect();
          const w = Math.max(1, Math.floor(rect.width));
          const h = Math.max(1, Math.floor(rect.height));
          this.requestCanvasResize(id, w, h, true);
        }
        // Short, bounded delayed sweep to catch late layout and final DPR adjustments
        setTimeout(() => {
          for (const [id, control] of this.controls.entries()) {
            const el = control.getElement();
            const rect = el.getBoundingClientRect();
            const w = Math.max(1, Math.floor(rect.width));
            const h = Math.max(1, Math.floor(rect.height));
            this.requestCanvasResize(id, w, h, true);
          }
        }, 10);
        break;
      case 'inspector_update':
        this.inspector.handleUpdate(data.update);
        break;
      default:
        break;
    }
  }

  async registerCanvas(element: HTMLCanvasElement, id: string, kind: CanvasKind, isPrimary = false): Promise<void> {
    const control = new CanvasControl(id, kind, element, isPrimary);
    this.controls.set(id, control);

    // Lazy-create session and attach
    const session = this.ensureSession();

    // Check WebGPU support (simple check)
    // @ts-ignore
    if (!navigator.gpu) {
      this.webGPUSupported = false;
      this.showWebGPUWarning = true;
      return;
    }

    try {
      this.loadingInProgress = true;
      const dpr = window.devicePixelRatio;
      control.attach(session, dpr);
      const sess = this.ensureSession();
      await sess.initialize();
    } finally {
      this.loadingInProgress = false;
    }
  }

  removeCanvas(id: string): void {
    const c = this.controls.get(id);
    if (!c) return;
    c.detach();
    this.controls.delete(id);
  }

  requestCanvasResize(id: string, width: number, height: number, force = false) {
    
    if (!this.session) return;
    const doCall = () => {
      const dpr = window.devicePixelRatio || 1;
      const physicalW = Math.max(1, Math.floor(width * dpr));
      const physicalH = Math.max(1, Math.floor(height * dpr));
      this.controls.get(id)?.updateSize(this.session as SessionAdapter, physicalW, physicalH, force);
    };

    if (!this.resizeDebounceEnabled || force) {
        console.log("requestCanvasResize", id, width, height, force);
      doCall();
      return;
    }

    const existing = this.resizeTimers.get(id);
    if (existing) {
      // @ts-ignore
      clearTimeout(existing);
    }
    const handle = setTimeout(doCall, this.resizeDebounceMs) as unknown as number;
    this.resizeTimers.set(id, handle);
  }

  async switchMode(mode: RuntimeMode) {
    if (mode === this.mode) return;
    this.mode = mode;

    // Snapshot containers
    const snapshots = Array.from(this.controls.values()).map(c => ({
      id: c.id,
      control: c,
      container: c.getElement().parentElement as HTMLElement
    }));

    // Detach and dispose old session
    snapshots.forEach(s => s.control.detach());
    this.session?.stop();
    this.session?.dispose();
    this.session = null;
    this.isInitialized = false;

    // New session
    const session = this.ensureSession();
    const dpr = window.devicePixelRatio;

    // Recreate DOM canvases and reattach
    for (const s of snapshots) {
      s.control.recreateDomCanvas(s.container);
      // Primary if viewer-canvas or if none selected yet
      const isPrimary = s.id === 'viewer-canvas';
      s.control.attach(session, dpr);
    }

    const sess2 = this.ensureSession();
    await sess2.initialize();
  }

  start() { this.session?.start(); }
  stop() { this.session?.stop(); }
  dispose() { this.session?.dispose(); }

  // Public controls for debounce
  setResizeDebounce(ms: number, enabled: boolean = this.resizeDebounceEnabled) {
    this.resizeDebounceMs = Math.max(0, ms | 0);
    this.resizeDebounceEnabled = enabled;
  }

  setResizeDebounceEnabled(enabled: boolean) {
    this.resizeDebounceEnabled = enabled;
  }
}


