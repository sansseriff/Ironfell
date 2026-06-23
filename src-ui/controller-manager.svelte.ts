import { CanvasManager } from "./canvas_manager";
import type { RuntimeMode } from "./runtime/session_adapter";

class ControllerManagerClass {
  // State properties - can be accessed directly in Svelte 5
  controller: { requestCanvasResize: (id: string, width: number, height: number, force?: boolean) => void } | undefined = $state();
  isInitialized = $state(false);
  showWebGPUWarning = $state(false);
  loadingInProgress = $state(false);
  webGPUSupported = $state(true);
  runtimeMode: RuntimeMode = $state('worker');

  private manager = new CanvasManager(this.runtimeMode);

  constructor() {
    this.manager.onInitialized = () => {
      this.isInitialized = true;
    };
  }

  /**
   * Register a canvas for initialization (collects all canvases before creating controller)
   */
  async registerCanvas(canvas: HTMLCanvasElement, isPrimary = false, mode: RuntimeMode = this.runtimeMode): Promise<void> {
    console.log(`Registering canvas: ${canvas.id} (primary: ${isPrimary})`);
    this.runtimeMode = mode;
    this.manager.setMode(mode);
    const kind = canvas.id === 'viewer-canvas' ? 'viewer' : (canvas.id.includes('timeline') ? 'timeline' : 'other');
    this.loadingInProgress = true;
    try {
      await this.manager.registerCanvas(canvas, canvas.id, kind as any, isPrimary);
      this.isInitialized = this.manager.isInitialized;
      this.webGPUSupported = this.manager.webGPUSupported;
      this.showWebGPUWarning = this.manager.showWebGPUWarning;
      this.controller = {
        requestCanvasResize: (id: string, width: number, height: number, force?: boolean) =>
          this.manager.requestCanvasResize(id, width, height, !!force)
      };
    } finally {
      this.loadingInProgress = false;
    }
  }

  /**
   * Initialize the controller with all registered canvases
   */
  // initialization is handled inside CanvasManager

  /**
   * Add a canvas after controller is initialized. not currently used. 
   */
  // async addCanvas(canvas: HTMLCanvasElement): Promise<void> {
  //   if (!this.isInitialized || !this.controller) {
  //     // If controller isn't ready, register for later initialization
  //     await this.registerCanvas(canvas, false);
  //     return;
  //   }

  //   const config: CanvasConfig = {
  //     canvas,
  //     id: canvas.id,
  //     isPrimary: false
  //   };

  //   await this.controller.addCanvas(config);
  //   console.log(`Added canvas after initialization: ${canvas.id}`);
  // }

  /**
   * Remove a canvas window when it's destroyed
   */
  removeCanvas(canvasId: string): void {
    this.manager.removeCanvas(canvasId);
    console.log(`Removed canvas: ${canvasId}`);
  }

  /**
   * Switch runtime mode - recreates all canvases
   */
  async switchMode(mode: RuntimeMode) {
    console.log("SWITCHMODE switching mode from ", this.runtimeMode, " to ", mode);
    if (this.loadingInProgress) return;
    if (this.runtimeMode === mode && this.isInitialized) return; // no-op
    this.loadingInProgress = true;
    try {
      await this.manager.switchMode(mode);
      this.runtimeMode = mode;
      this.isInitialized = this.manager.isInitialized;
    } finally {
      this.loadingInProgress = false;
    }
  }

  // Dismiss WebGPU warning
  dismissWebGPUWarning(): void {
    this.showWebGPUWarning = false;
  }

  /**
   * Reset state (useful for cleanup or reinitializing)
   */
  reset(): void {
    this.controller = undefined;
    this.isInitialized = false;
    this.showWebGPUWarning = false;
    this.loadingInProgress = false;
    this.webGPUSupported = true;
  }

  /**
   * Cleanup
   */
  dispose(): void {
    this.manager.dispose();
    this.reset();
  }
}

// Export singleton instance
export const controllerManager = new ControllerManagerClass();