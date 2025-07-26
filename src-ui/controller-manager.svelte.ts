import { WorkerController } from "./control.svelte";

class ControllerManagerClass {
  // State properties - can be accessed directly in Svelte 5
  controller: WorkerController | undefined = $state();
  isInitialized = $state(false);
  showWebGPUWarning = $state(false);
  loadingInProgress = $state(false);
  webGPUSupported = $state(true);

  // Check for WebGPU support
  private checkWebGPUSupport(): boolean {
    // @ts-ignore
    return !!navigator.gpu;
  }

  // Initialize the controller if WebGPU is supported
  async initialize(canvas: HTMLCanvasElement): Promise<void> {
    if (this.loadingInProgress) return;

    // Check WebGPU support first
    if (!this.checkWebGPUSupport()) {
      this.webGPUSupported = false;
      this.showWebGPUWarning = true;
      return;
    }

    try {
      this.loadingInProgress = true;

      // Create the controller
      this.controller = new WorkerController(canvas);

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