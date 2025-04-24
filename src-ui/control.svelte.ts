import wasmUrl from './wasm/ironfell_bg.wasm?url'

/**
 * WorkerController manages interactions with a Web Worker that runs the engine instance.
 * It handles communication, mouse events, and state management between the UI and worker.
 */
export class WorkerController {
  // Whether the worker is ready
  workerIsReady = $state(false);

  width = $state(0);
  height = $state(0);

  private initializationError: any = null;

  // Latest pick result
  private latestPick: any[] = [];

  // Worker instance
  private worker: Worker;

  // Canvas reference for event handling
  private canvas: HTMLCanvasElement;



  /**
   * Creates a new WorkerController instance
   * @param canvas The canvas element where rendering occurs
   */
  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;

    // Resize the canvas for proper rendering
    this.resizeCanvas();

    // Initialize the worker
    console.log("Initializing Web Worker...");
    this.worker = new Worker(new URL('./worker.js', import.meta.url), {
      type: 'module'
    });

    // Set up message handling from worker
    this.worker.onmessage = this.handleWorkerMessage.bind(this);

    // this.worker.postMessage({ ty: "dummy" });

    this.worker.postMessage({ ty: "wasmUrl", wasmUrl: wasmUrl })

    this.setupVisibilityListener();
  }

  private setupVisibilityListener() {
    document.addEventListener('visibilitychange', () => {
      if (document.hidden) {
        // When tab becomes hidden, stop the rendering loop
        this.worker.postMessage({ ty: "stopRunning" });
        console.log("Tab hidden, paused rendering");
      } else {
        // When tab becomes visible again, resume rendering
        this.worker.postMessage({ ty: "startRunning" });
        console.log("Tab visible, resumed rendering");
      }
    });
  }

  /**
   * Checks if WebGPU is supported and initializes the worker
   * @returns A promise that resolves when initialization is complete or rejects if WebGPU is not supported
   */
  async initialize(): Promise<void> {
    // Check if WebGPU is supported
    // if (!("navigator" in window && "gpu" in navigator)) {
    //   throw new Error("WebGPU is not supported in this browser");
    // }

    try {
      // Request a GPU adapter to confirm WebGPU support


      const adapter = await (navigator as any).gpu.requestAdapter();
      if (!adapter) {
        throw new Error("No appropriate GPU adapter found");
      }

      // Wait for the worker to be ready
      await this.waitForWorkerReady();

      // Create offscreen canvas and transfer control to the worker
      this.transferCanvasToWorker();

      return;
    } catch (error: Error | any) {
      console.error("WebGPU initialization failed:", error);

      // Capture more details about the error
      const errorDetails = {
        message: error.message || "Unknown error",
        name: error.name,
        stack: error.stack,
        // Additional WebGPU specific info if available
        gpuStatus: (navigator as any).gpu ? "Available" : "Not available"
      };

      console.error("Error details:", errorDetails);

      // You could also store this information in a property for display in the UI
      this.initializationError = errorDetails;
      throw error;
    }
  }

  /**
   * Waits for the worker to signal it's ready
   */
  private waitForWorkerReady(): Promise<void> {
    return new Promise((resolve) => {
      if (this.workerIsReady) {
        resolve();
        return;
      }

      const checkInterval = setInterval(() => {
        if (this.workerIsReady) {
          clearInterval(checkInterval);
          resolve();
        }
      }, 50);
    });
  }

  /**
   * Transfers canvas control to the worker
   */
  private transferCanvasToWorker() {
    const offscreenCanvas = this.canvas.transferControlToOffscreen();
    const devicePixelRatio = window.devicePixelRatio;

    this.worker.postMessage(
      { ty: "init", canvas: offscreenCanvas, devicePixelRatio },
      [offscreenCanvas]
    );
  }

  public requestCanvasResize(width: number, height: number) {
    const devicePixelRatio = window.devicePixelRatio;
    // console.log("width:", width, "height:", height);


    if (width != 0) {
      this.width = width;
    }
    if (height != 0) {
      this.height = height;
    }

    if (this.width != 0 && this.height != 0) {
      // Set the display size through CSS
      this.canvas.style.width = this.width + "px";
      this.canvas.style.height = this.height + "px";

      this.worker.postMessage({
        ty: "resize",
        width: this.width * devicePixelRatio,
        height: this.height * devicePixelRatio
      })
    }

  }

  public resizeCanvas() {
    const rect = this.canvas.getBoundingClientRect();
    const devicePixelRatio = window.devicePixelRatio;

    console.log("Canvas dimensions:", rect.width, "×", rect.height);
    console.log("Device pixel ratio:", devicePixelRatio);

    // Set the actual canvas dimensions, accounting for device pixel ratio
    this.canvas.width = rect.width * devicePixelRatio;
    this.canvas.height = rect.height * devicePixelRatio;

    // Set the display size through CSS
    this.canvas.style.width = rect.width + "px";
    this.canvas.style.height = rect.height + "px";

    console.log("Resized canvas to:", this.canvas.width, "×", this.canvas.height);
  }

  /**
   * Handles messages received from the worker
   */
  private async handleWorkerMessage(event: MessageEvent) {
    const data = event.data;
    // window.blockMS(window.onmessageBlockTime);

    switch (data.ty) {
      case "workerIsReady":
        this.workerIsReady = true;
        // Start listening for mouse events once worker is ready
        this.addMouseEventObservers();
        break;

      case "pick":
        // Display pick results on the 
        // const ele = document.getElementById("pick-list");
        // ele.innerText = data.list;

        this.latestPick = data.list;
        // Notify the worker which entities should have hover effects enabled
        this.worker.postMessage({ ty: "hover", list: this.latestPick });
        break;

      default:
        break;
    }
  }

  /**
   * Adds mouse event listeners to the canvas
   */
  private addMouseEventObservers() {
    this.canvas.addEventListener("mousemove", (event) => {
      // window.blockMS(window.mousemoveBlockTime);
      // Clear last pick cache before sending mouse move event to worker
      this.latestPick = [];
      this.worker.postMessage({
        ty: "mousemove",
        x: event.offsetX,
        y: event.offsetY
      });
    });

    this.canvas.addEventListener("mousedown", (event) => {
      if (typeof this.latestPick[0] !== "undefined") {
        this.worker.postMessage({
          ty: "leftBtDown",
          pickItem: this.latestPick[0],
          x: event.offsetX,
          y: event.offsetY,
        });
      }
    });

    this.canvas.addEventListener("mouseup", (_event) => {
      this.worker.postMessage({ ty: "leftBtUp" });
    });

    this.canvas.addEventListener("click", (_event) => {
      if (Array.isArray(this.latestPick) && this.latestPick.length > 0) {
        this.worker.postMessage({
          ty: "select",
          list: this.latestPick,
        });
      }
    });
  }

  /**
   * Blocks worker rendering for specified time
   */
  // blockWorkerRender(dt: number) {
  //   this.worker.postMessage({ ty: "blockRender", blockTime: dt });
  // }

  /**
   * Starts the worker engine instance
   */
  startWorkerApp() {
    this.worker.postMessage({ ty: "startRunning" });
    this.setCanvasOpacity("100%");
  }

  /**
   * Stops the worker engine instance
   */
  stopWorkerApp() {
    this.worker.postMessage({ ty: "stopRunning" });
    this.setCanvasOpacity("50%");
  }

  /**
   * Turns on/off engine animation
   */
  setWorkerAutoAnimation(needsAnimation: boolean) {
    this.worker.postMessage({
      ty: "autoAnimation",
      autoAnimation: needsAnimation
    });
  }

  /**
   * Sets the canvas opacity
   */
  private setCanvasOpacity(opacity: string) {
    this.canvas.style.opacity = opacity;
  }
}