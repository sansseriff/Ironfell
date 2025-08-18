import { WorkerController, type RuntimeMode } from "./control.svelte";

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

  // Initialize the controller if WebGPU is supported
  async initialize(canvas: HTMLCanvasElement, mode: RuntimeMode = this.runtimeMode): Promise<void> {
    if (this.loadingInProgress) return;

    // Check WebGPU support first
    if (!this.checkWebGPUSupport()) {
      this.webGPUSupported = false;
      this.showWebGPUWarning = true;
      return;
    }

    try {
      this.loadingInProgress = true;

      // Create the controller (respect mode)
      this.runtimeMode = mode;
      this.controller = new WorkerController(canvas, mode);

      // Initialize the controller (checks WebGPU support, waits for worker, transfers canvas)
      await this.controller.initialize();
      this.isInitialized = true;

      // Set up key event listeners
      window.addEventListener("keydown", this.controller.handleKeyDown);
      window.addEventListener("keyup", this.controller.handleKeyUp);
    } catch (error) {
      console.error("Controller initialization failed:", error);
      this.webGPUSupported = false;
      this.showWebGPUWarning = true;
      this.controller = undefined;
    } finally {
      this.loadingInProgress = false;
    }
  }

  async switchMode(_canvas: HTMLCanvasElement, mode: RuntimeMode) {
    if (this.loadingInProgress) return;
    if (this.runtimeMode === mode && this.isInitialized) return; // no-op
    const container = document.getElementById('container') || _canvas.parentElement;
    if (!container) {
      console.error('No container found for canvas switching');
      return;
    }
    // Dispose existing controller
    this.dispose();
    // Create a fresh canvas (never reuse one that may have a context / offscreen transfer)
    const newCanvas = document.createElement('canvas');
    newCanvas.id = _canvas.id || 'worker-canvas';
    newCanvas.className = _canvas.className;
    newCanvas.style.cssText = _canvas.style.cssText;
    if (_canvas.hasAttribute('tabindex')) newCanvas.setAttribute('tabindex', _canvas.getAttribute('tabindex')!);
    // Replace old canvas node if still present
    if (_canvas.parentElement === container) {
      try { container.replaceChild(newCanvas, _canvas); } catch { container.appendChild(newCanvas); }
    } else {
      container.appendChild(newCanvas);
    }
    await this.initialize(newCanvas, mode);
    // Ensure listeners attached if ready already (main thread path may attach immediately after init)
    this.controller?.ensureInputListenersAttached?.();
  }

  // Dismiss WebGPU warning
  dismissWebGPUWarning(): void {
    this.showWebGPUWarning = false;
  }

  // Reset state (useful for cleanup or reinitializing)
  reset(): void {
    this.controller = undefined;
    this.isInitialized = false;
    this.showWebGPUWarning = false;
    this.loadingInProgress = false;
    this.webGPUSupported = true;
  }

  // Cleanup
  dispose(): void {
    if (this.controller) {
      // Remove key event listeners
      window.removeEventListener("keydown", this.controller.handleKeyDown);
      window.removeEventListener("keyup", this.controller.handleKeyUp);
      this.controller.dispose();
    }
    this.reset();
  }
}

// Export singleton instance
export const controllerManager = new ControllerManagerClass();