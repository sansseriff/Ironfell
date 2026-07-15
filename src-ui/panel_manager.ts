import { SessionAdapter, type RuntimeMode, type PanelRectMsg } from './runtime/session_adapter';
import { InputManager } from './runtime/input_manager';
import { InspectorClient } from './runtime/inspector_client';
import { SystemState } from './system_state.svelte';

interface PanelEntry {
  id: string;
  kind: string;
  el: HTMLElement;
  ro: ResizeObserver;
}

/**
 * Owns the single full-window canvas, the render session (worker or main thread),
 * and the registry of Bevy panels (DOM placeholder divs whose rects are mirrored
 * to camera viewports / vello regions in Rust).
 *
 * Layout is strictly DOM -> Bevy: the browser lays panels out, we measure and post.
 */
export class PanelManager {
  private session: SessionAdapter | null = null;
  private mode: RuntimeMode = 'worker';
  private canvas: HTMLCanvasElement | null = null;
  private input = new InputManager({ enableRaw: true });
  private inspector = new InspectorClient(new SystemState());
  private panels = new Map<string, PanelEntry>();
  private windowResizeHandler: (() => void) | null = null;

  public onInitialized: (() => void) | null = null;

  // UI flags (mirrored into the svelte controller)
  isInitialized = false;
  loadingInProgress = false;
  webGPUSupported = true;
  showWebGPUWarning = false;

  getMode(): RuntimeMode { return this.mode; }

  /** Boot the app on the given full-window canvas. Called once on App mount. */
  async boot(canvas: HTMLCanvasElement, mode: RuntimeMode = 'worker'): Promise<void> {
    this.mode = mode;
    this.canvas = canvas;

    // @ts-ignore
    if (!navigator.gpu) {
      this.webGPUSupported = false;
      this.showWebGPUWarning = true;
      return;
    }

    this.loadingInProgress = true;
    this.sizeCanvasBackingStore();
    this.startSession();

    if (!this.windowResizeHandler) {
      this.windowResizeHandler = () => this.handleWindowResize();
      window.addEventListener('resize', this.windowResizeHandler, { passive: true });
    }
  }

  /** Register a Bevy panel placeholder. Safe to call before or after boot. */
  registerPanel(id: string, kind: string, el: HTMLElement): void {
    this.unregisterPanel(id);
    const ro = new ResizeObserver(() => this.postPanelRect(id));
    ro.observe(el);
    this.panels.set(id, { id, kind, el, ro });
    this.postPanelRect(id);
  }

  unregisterPanel(id: string): void {
    const entry = this.panels.get(id);
    if (!entry) return;
    entry.ro.disconnect();
    this.panels.delete(id);
    this.session?.despawnPanel(id);
  }

  /** Re-measure and re-post every panel rect (SplitPane drags, late layout, etc.). */
  syncAllPanels(): void {
    for (const id of this.panels.keys()) {
      this.postPanelRect(id);
    }
  }

  /**
   * Tear down the current session and start a fresh one in the other mode.
   * The canvas element is replaced: a transferred (or GPU-bound) canvas can't be reused.
   */
  async switchMode(mode: RuntimeMode): Promise<void> {
    if (mode === this.mode) return;
    if (!this.canvas) {
      this.mode = mode;
      return;
    }

    this.loadingInProgress = true;
    this.isInitialized = false;
    this.mode = mode;

    // 1. Detach input from the old canvas.
    this.input.dispose();

    // 2. Dispose old session (stops RAF; main mode releases the Bevy app; worker terminates).
    this.session?.dispose();
    this.session = null;

    // 3. Replace the canvas element.
    this.canvas = this.recreateCanvas(this.canvas);
    this.sizeCanvasBackingStore();

    // 4. Fresh session; enginePrepared will resync sizes, panels, and input.
    this.startSession();
  }

  dispose(): void {
    this.input.dispose();
    this.session?.dispose();
    this.session = null;
    if (this.windowResizeHandler) {
      window.removeEventListener('resize', this.windowResizeHandler);
      this.windowResizeHandler = null;
    }
    for (const entry of this.panels.values()) entry.ro.disconnect();
    this.panels.clear();
    this.isInitialized = false;
  }

  start() { this.session?.start(); }
  stop() { this.session?.stop(); }

  private startSession(): void {
    const session = new SessionAdapter(this.mode);
    this.session = session;
    session.onMessage((data) => this.handleSessionMessage(data));
    this.inspector.init({ post: (data: any, transfer?: any[]) => session.post(data, transfer) } as any);
    session.attachCanvas(this.canvas!);
    session.resizeCanvas(...this.canvasPhysicalSize());
    this.syncAllPanels();
  }

  private handleSessionMessage(data: any): void {
    switch (data?.ty) {
      case 'enginePrepared':
        this.isInitialized = true;
        this.loadingInProgress = false;
        // Deterministic post-init sync: canvas size + all panel rects (the session
        // flushed its own coalesced state already; re-measure to catch late layout).
        this.sizeCanvasBackingStore();
        this.session?.resizeCanvas(...this.canvasPhysicalSize());
        this.syncAllPanels();
        if (this.canvas) {
          this.input.init(this.canvas, {
            post: (payload: any) => this.session?.post(payload),
          });
        }
        if (this.onInitialized) try { this.onInitialized(); } catch { }
        break;
      case 'inspector_update':
        this.inspector.handleUpdate(data.update);
        break;
      default:
        break;
    }
  }

  private handleWindowResize(): void {
    this.sizeCanvasBackingStore();
    this.session?.resizeCanvas(...this.canvasPhysicalSize());
    // Panel placeholders resize with the window; ResizeObserver fires, but re-post
    // anyway so position-only changes are never missed.
    this.syncAllPanels();
  }

  private canvasPhysicalSize(): [number, number] {
    const dpr = window.devicePixelRatio || 1;
    const rect = this.canvas?.getBoundingClientRect();
    const w = Math.max(1, Math.round((rect?.width ?? window.innerWidth) * dpr));
    const h = Math.max(1, Math.round((rect?.height ?? window.innerHeight) * dpr));
    return [w, h];
  }

  /**
   * Set the canvas backing-store size directly while we still own the canvas
   * (before transfer / before wgpu binds it). After init, worker mode resizes the
   * OffscreenCanvas inside the worker and main mode resizes it in the adapter.
   */
  private sizeCanvasBackingStore(): void {
    if (!this.canvas) return;
    if (this.session?.isPrepared()) return;
    try {
      const [w, h] = this.canvasPhysicalSize();
      this.canvas.width = w;
      this.canvas.height = h;
    } catch {
      // Canvas already transferred; the session owns sizing now.
    }
  }

  /**
   * A canvas that has been transferred to a worker (or bound to a wgpu surface)
   * cannot be reused for a new context — swap in a fresh element with the same
   * identity and styling.
   */
  private recreateCanvas(old: HTMLCanvasElement): HTMLCanvasElement {
    const fresh = document.createElement('canvas');
    fresh.id = old.id;
    fresh.className = old.className;
    fresh.style.cssText = old.style.cssText;
    if (old.hasAttribute('tabindex')) {
      fresh.setAttribute('tabindex', old.getAttribute('tabindex')!);
    }
    fresh.addEventListener('contextmenu', (e) => e.preventDefault());
    if (old.parentElement) {
      old.parentElement.replaceChild(fresh, old);
    }
    return fresh;
  }

  private postPanelRect(id: string): void {
    const entry = this.panels.get(id);
    if (!entry || !this.session || !this.canvas) return;
    const dpr = window.devicePixelRatio || 1;
    const canvasRect = this.canvas.getBoundingClientRect();
    const rect = entry.el.getBoundingClientRect();
    const msg: PanelRectMsg = {
      id: entry.id,
      kind: entry.kind,
      x: Math.max(0, Math.round((rect.left - canvasRect.left) * dpr)),
      y: Math.max(0, Math.round((rect.top - canvasRect.top) * dpr)),
      w: Math.max(1, Math.round(rect.width * dpr)),
      h: Math.max(1, Math.round(rect.height * dpr)),
    };
    this.session.setPanelViewport(msg);
  }
}
