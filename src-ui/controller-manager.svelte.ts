import { isNativeHost } from './runtime/native_host';
import { PanelManager } from "./panel_manager";
import type { RuntimeMode } from "./runtime/session_adapter";
import { saveSettings, settings } from "./runtime/settings.svelte";

function urlRuntime(): RuntimeMode | null {
  const r = new URLSearchParams(location.search).get('runtime');
  return r === 'worker' || r === 'main' ? r : null;
}

/**
 * Thin reactive wrapper around PanelManager for Svelte components.
 */
class ControllerManagerClass {
  isInitialized = $state(false);
  showBackendWarning = $state(false);
  loadingInProgress = $state(false);
  backendSupported = $state(true);
  // `?runtime=worker|main` overrides the stored setting for this page only
  // (repeatable measurements, plans/rendering/latency-view.md).
  runtimeMode: RuntimeMode = $state(isNativeHost() ? 'native' : urlRuntime() ?? settings.runtime);

  private manager = new PanelManager();

  /** The document client: save, load, and the tree view the shell shows. */
  get docClient() {
    return this.manager.docClient;
  }

  constructor() {
    this.manager.onInitialized = () => {
      this.syncFlags();
    };
    this.manager.onRenderLost = (kind, error) => void this.recoverRenderer(kind, error);
  }

  /** When the renderer recovered on its own (`recoverRenderer`), recently. */
  private recoveries: number[] = [];

  /**
   * The GPU device was lost or wgpu reported an error, and the engine
   * stopped rendering (src/render_loss.rs): every view would stay blank.
   * Restart as the Settings dialog does, keeping the document. A renderer
   * that fails again and again (a real bug, not the browser dropping the
   * device) is left stopped after three tries a minute, with the error
   * shown, rather than restarting in a loop.
   */
  private async recoverRenderer(kind: string, error: string): Promise<void> {
    const now = performance.now();
    this.recoveries = this.recoveries.filter(t => now - t < 60_000);
    if (this.recoveries.length >= 3) {
      this.docClient.error = `Rendering stopped (${kind}): ${error}. Reload the page to try again.`;
      return;
    }
    this.recoveries.push(now);
    console.warn(`[gfx] restarting the renderer after ${kind}`);
    await this.restart();
  }

  /** Boot the app on the full-window canvas (called once from App.svelte). */
  async boot(canvas: HTMLCanvasElement): Promise<void> {
    this.loadingInProgress = true;
    try {
      await this.manager.boot(canvas, this.runtimeMode);
    } finally {
      this.syncFlags();
    }
  }

  registerPanel(id: string, kind: string, el: HTMLElement, composition = ""): void {
    this.manager.registerPanel(id, kind, el, composition);
  }

  unregisterPanel(id: string): void {
    this.manager.unregisterPanel(id);
  }

  /** Re-measure all panels (SplitPane drag callback). */
  syncAllPanels(): void {
    this.manager.syncAllPanels();
  }

  /** Switch runtime mode — clean teardown and re-init on a fresh canvas element. */
  async switchMode(mode: RuntimeMode) {
    if (this.loadingInProgress) return;
    if (this.runtimeMode === mode && this.isInitialized) return;
    this.runtimeMode = isNativeHost() ? 'native' : mode;
    saveSettings({ runtime: mode });
    await this.restart();
  }

  /** Re-size the canvas, panels and pointer mapping for the current pixel
   *  ratio (the render scale setting changed); no restart. */
  refreshPixelRatio(): void {
    this.manager.refreshPixelRatio();
  }

  /**
   * Restart the app so changed settings take effect, keeping the document:
   * it is saved first and loaded into the fresh app (its undo history does
   * not survive).
   */
  async restart(): Promise<void> {
    if (this.loadingInProgress) return;
    let carry: string | null = null;
    if (this.isInitialized && this.docClient.scopes?.active) {
      // The new engine reopens the scope from storage; write it first.
      await this.docClient.scopes.flush().catch(() => {});
    } else if (this.isInitialized) {
      try {
        carry = await this.docClient.save();
      } catch {
        carry = null;
      }
    }
    this.loadingInProgress = true;
    this.docClient.carryOver(carry);
    try {
      await this.manager.restart(this.runtimeMode);
    } finally {
      this.syncFlags();
    }
  }

  dismissBackendWarning(): void {
    this.showBackendWarning = false;
  }

  dispose(): void {
    this.manager.dispose();
    this.isInitialized = false;
    this.loadingInProgress = false;
  }

  private syncFlags() {
    this.isInitialized = this.manager.isInitialized;
    this.loadingInProgress = this.manager.loadingInProgress;
    this.backendSupported = this.manager.backendSupported;
    this.showBackendWarning = this.manager.showBackendWarning;
  }
}

// Export singleton instance
export const controllerManager = new ControllerManagerClass();
