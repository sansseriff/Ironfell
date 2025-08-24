import { WorkerController, type RuntimeMode, type CanvasConfig } from "./control.svelte";

class ControllerManagerClass {
  // State properties - can be accessed directly in Svelte 5
  controller: WorkerController | undefined = $state();
  isInitialized = $state(false);
  showWebGPUWarning = $state(false);
  loadingInProgress = $state(false);
  webGPUSupported = $state(true);
  runtimeMode: RuntimeMode = $state('worker');

  // Check for WebGPU support
  private checkWebGPUSupport(): boolean {
    // @ts-ignore
    return !!navigator.gpu;
  }

  // Track canvas configurations before controller creation
  private pendingCanvases = new Map<string, CanvasConfig>();
  private initializationStarted = $state(false);

  /**
   * Register a canvas for initialization (collects all canvases before creating controller)
   */
  async registerCanvas(canvas: HTMLCanvasElement, isPrimary = false, mode: RuntimeMode = this.runtimeMode): Promise<void> {
    console.log(`Registering canvas: ${canvas.id} (primary: ${isPrimary})`);
    
    this.pendingCanvases.set(canvas.id, {
      canvas,
      id: canvas.id,
      isPrimary
    });

    // Start initialization when we have at least one canvas
    if (!this.initializationStarted) {
      this.initializationStarted = true;
      // Give a short delay to allow other canvases to register
      await new Promise(resolve => setTimeout(resolve, 10));
      await this.initializeController(mode);
    }
  }

  /**
   * Initialize the controller with all registered canvases
   */
  private async initializeController(mode: RuntimeMode): Promise<void> {
    if (this.loadingInProgress || this.controller) return;

    // Check WebGPU support first
    if (!this.checkWebGPUSupport()) {
      this.webGPUSupported = false;
      this.showWebGPUWarning = true;
      return;
    }

    try {
      this.loadingInProgress = true;
      this.runtimeMode = mode;

      const canvasConfigs = Array.from(this.pendingCanvases.values());
      if (canvasConfigs.length === 0) {
        throw new Error('No canvases registered for initialization');
      }

      console.log(`Creating controller with ${canvasConfigs.length} canvases`);
      this.controller = new WorkerController(canvasConfigs, mode);

      // Initialize the controller (checks WebGPU support, waits for worker, transfers canvases)
      await this.controller.initialize();

      this.isInitialized = true;
      console.log('Controller initialization completed');
    } catch (error) {
      console.error("Controller initialization failed:", error);
      this.webGPUSupported = false;
      this.showWebGPUWarning = true;
      this.controller = undefined;
      this.initializationStarted = false;
    } finally {
      this.loadingInProgress = false;
    }
  }

  /**
   * Add a canvas after controller is initialized
   */
  async addCanvas(canvas: HTMLCanvasElement): Promise<void> {
    if (!this.isInitialized || !this.controller) {
      // If controller isn't ready, register for later initialization
      await this.registerCanvas(canvas, false);
      return;
    }

    const config: CanvasConfig = {
      canvas,
      id: canvas.id,
      isPrimary: false
    };

    await this.controller.addCanvas(config);
    console.log(`Added canvas after initialization: ${canvas.id}`);
  }

  /**
   * Remove a canvas window when it's destroyed
   */
  removeCanvas(canvasId: string): void {
    // Remove from pending canvases if not yet initialized
    if (this.pendingCanvases.has(canvasId)) {
      this.pendingCanvases.delete(canvasId);
      console.log(`Removed pending canvas: ${canvasId}`);
      return;
    }

    // Remove from controller if initialized
    if (this.controller) {
      this.controller.removeCanvas(canvasId);
    }
    
    console.log(`Removed canvas: ${canvasId}`);
  }

  /**
   * Switch runtime mode - recreates all canvases
   */
  async switchMode(_referenceCanvas: HTMLCanvasElement, mode: RuntimeMode) {
    if (this.loadingInProgress) return;
    if (this.runtimeMode === mode && this.isInitialized) return; // no-op
    
    // Store current canvas configurations
    const currentConfigs = Array.from(this.pendingCanvases.values());
    if (this.controller) {
      // Get canvas IDs from controller if initialized
      for (const canvasId of this.controller.getCanvasIds()) {
        const canvas = this.controller.getCanvas(canvasId);
        if (canvas) {
          currentConfigs.push({ canvas, id: canvasId, isPrimary: canvasId === 'viewer-canvas' });
        }
      }
    }
    
    // Dispose existing controller
    this.dispose();
    
    // Recreate canvases (never reuse ones that may have a context / offscreen transfer)
    for (const config of currentConfigs) {
      const container = document.getElementById('container') || config.canvas.parentElement;
      if (!container) continue;
      
      const newCanvas = document.createElement('canvas');
      newCanvas.id = config.id;
      newCanvas.className = config.canvas.className;
      newCanvas.style.cssText = config.canvas.style.cssText;
      if (config.canvas.hasAttribute('tabindex')) {
        newCanvas.setAttribute('tabindex', config.canvas.getAttribute('tabindex')!);
      }
      
      // Replace old canvas
      if (config.canvas.parentElement === container) {
        try { 
          container.replaceChild(newCanvas, config.canvas); 
        } catch { 
          container.appendChild(newCanvas); 
        }
      } else {
        container.appendChild(newCanvas);
      }
      
      // Register new canvas
      await this.registerCanvas(newCanvas, config.isPrimary, mode);
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
    this.initializationStarted = false;
    this.pendingCanvases.clear();
  }

  /**
   * Cleanup
   */
  dispose(): void {
    if (this.controller) {
      // Key event listeners are cleaned up by InputManager in controller.dispose()
      this.controller.dispose();
    }
    this.reset();
  }
}

// Export singleton instance
export const controllerManager = new ControllerManagerClass();