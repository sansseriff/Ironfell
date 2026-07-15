import { PanelManager } from "./panel_manager";
import type { RuntimeMode } from "./runtime/session_adapter";

/**
 * Thin reactive wrapper around PanelManager for Svelte components.
 */
class ControllerManagerClass {
  isInitialized = $state(false);
  showWebGPUWarning = $state(false);
  loadingInProgress = $state(false);
  webGPUSupported = $state(true);
  runtimeMode: RuntimeMode = $state('worker');

  private manager = new PanelManager();

  constructor() {
    this.manager.onInitialized = () => {
      this.syncFlags();
    };
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

  registerPanel(id: string, kind: string, el: HTMLElement): void {
    this.manager.registerPanel(id, kind, el);
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
    this.loadingInProgress = true;
    this.runtimeMode = mode;
    try {
      await this.manager.switchMode(mode);
    } finally {
      this.syncFlags();
    }
  }

  dismissWebGPUWarning(): void {
    this.showWebGPUWarning = false;
  }

  dispose(): void {
    this.manager.dispose();
    this.isInitialized = false;
    this.loadingInProgress = false;
  }

  private syncFlags() {
    this.isInitialized = this.manager.isInitialized;
    this.loadingInProgress = this.manager.loadingInProgress;
    this.webGPUSupported = this.manager.webGPUSupported;
    this.showWebGPUWarning = this.manager.showWebGPUWarning;
  }
}

// Export singleton instance
export const controllerManager = new ControllerManagerClass();
