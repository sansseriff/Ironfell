import wasmUrl from './wasm/ironfell_bg.wasm?url'

const OVERRIDE_SCALE = false;
const OVERRIDE_SCALE_FACTOR = 2;

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
  private worker: Worker;
  private canvas: HTMLCanvasElement;

  // Input handling properties
  private latestPick: any[] = [];
  private latestMouseX = 0;
  private latestMouseY = 0;
  private mouseMoveScheduled = false;

  // Key handling with throttling
  private pressedKeys = new Set<string>();
  private keyUpdateScheduled = false;

  /**
   * Creates a new WorkerController instance
   * @param canvas The canvas element where rendering occurs
   */
  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;

    // Initialize the worker
    console.log("Initializing Web Worker...");
    this.worker = new Worker(new URL('./worker.js', import.meta.url), {
      type: 'module'
    });

    // Set up message handling from worker
    this.worker.onmessage = this.handleWorkerMessage.bind(this);

    // Send the WASM URL to the worker
    this.worker.postMessage({ ty: "wasmUrl", wasmUrl: wasmUrl });

    // Listen for tab visibility changes
    this.setupVisibilityListener();
  }

  /**
   * Sets up listener to pause rendering when tab is not visible
   */
  private setupVisibilityListener() {
    document.addEventListener('visibilitychange', () => {
      if (document.hidden) {
        this.worker.postMessage({ ty: "stopRunning" });
        console.log("Tab hidden, paused rendering");
      } else {
        this.worker.postMessage({ ty: "startRunning" });
        console.log("Tab visible, resumed rendering");
      }
    });
  }

  /**
   * Checks if WebGPU is supported and initializes the worker
   * @returns A promise that resolves when initialization is complete
   */
  async initialize(): Promise<void> {
    try {
      // Request GPU adapter to confirm WebGPU support
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

      // Capture error details for debugging
      this.initializationError = {
        message: error.message || "Unknown error",
        name: error.name,
        stack: error.stack,
        gpuStatus: (navigator as any).gpu ? "Available" : "Not available"
      };

      throw error;
    }
  }

  /**
   * Handles keyboard input with debouncing
   */
  public handleKeyDown = (event: KeyboardEvent) => {
    const validKeys = ["w", "a", "s", "d", "f", "shift", "g"];
    const key = event.key.toLowerCase();

    if (validKeys.includes(key)) {
      // Add to pressed keys set
      this.pressedKeys.add(key);

      // Schedule key state update if not already scheduled
      if (!this.keyUpdateScheduled) {
        this.keyUpdateScheduled = true;
        requestAnimationFrame(() => {
          // Send all currently pressed keys
          this.pressedKeys.forEach(key => {
            this.worker.postMessage({ ty: "keydown", key });
          });
          this.keyUpdateScheduled = false;
        });
      }
    }
  }

  public handleKeyUp = (event: KeyboardEvent) => {
    const key = event.key.toLowerCase();

    // If it was in our pressed keys set
    if (this.pressedKeys.has(key)) {
      // Remove it from the set
      this.pressedKeys.delete(key);
      // Send the key up event immediately
      this.worker.postMessage({ ty: "keyup", key });
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
    const devicePixelRatio = OVERRIDE_SCALE ? OVERRIDE_SCALE_FACTOR : window.devicePixelRatio;

    this.worker.postMessage(
      { ty: "init", canvas: offscreenCanvas, devicePixelRatio },
      [offscreenCanvas]
    );
  }

  /**
   * Requests canvas resize with proper pixel ratio scaling
   */
  public requestCanvasResize(width: number, height: number) {
    const devicePixelRatio = OVERRIDE_SCALE ? OVERRIDE_SCALE_FACTOR : window.devicePixelRatio;

    // Update stored dimensions if provided
    if (width > 0) this.width = width;
    if (height > 0) this.height = height;

    // Only proceed if we have valid dimensions
    if (this.width <= 0 || this.height <= 0) return;

    // Set the display size through CSS
    this.canvas.style.width = this.width + "px";
    this.canvas.style.height = this.height + "px";

    // Calculate physical pixels
    const physicalWidth = Math.floor(this.width * devicePixelRatio);
    const physicalHeight = Math.floor(this.height * devicePixelRatio);

    // console.log(`Resizing canvas to: ${physicalWidth} × ${physicalHeight} (CSS: ${this.width} × ${this.height})`);

    // Send dimensions to worker
    this.worker.postMessage({
      ty: "resize",
      width: physicalWidth,
      height: physicalHeight
    });
  }

  /**
   * Legacy resize method - prefer using requestCanvasResize instead
   */
  public resizeCanvas() {
    const rect = this.canvas.getBoundingClientRect();
    console.log("Canvas bounding rect:", rect.width, "×", rect.height);

    if (rect.width > 0 && rect.height > 0) {
      this.requestCanvasResize(rect.width, rect.height);
    }
  }

  /**
   * Handles messages received from the worker
   */
  private handleWorkerMessage(event: MessageEvent) {
    const data = event.data;

    switch (data.ty) {
      case "workerIsReady":
        this.workerIsReady = true;
        // Start listening for mouse events once worker is ready
        this.addMouseEventObservers();
        break;

      case "pick":
        this.latestPick = data.list;
        // Notify the worker which entities should have hover effects enabled
        this.worker.postMessage({ ty: "hover", list: this.latestPick });
        break;

      case "inspector_result":
        console.log(`Inspector command ${data.command} result:`, data.success);
        if (data.entity_id) {
          console.log(`New entity ID: ${data.entity_id}`);
        }
        break;

      case "inspector_update":
        // console.log("Inspector update received:", data.update);
        // Handle streaming updates from the inspector if needed

        this.process_update(data.update);
        break;

      default:
        break;
    }
  }

  /**
   * Adds mouse event listeners to the canvas
   */
  private addMouseEventObservers() {
    // Throttled mouse move handling
    this.canvas.addEventListener("mousemove", (event) => {

      // console.log("DEVICE PIXEL RATIO", window.devicePixelRatio);
      // const devicePixelRatio = OVERRIDE_SCALE ? OVERRIDE_SCALE_FACTOR : window.devicePixelRatio;

      // Store the latest position
      this.latestMouseX = event.offsetX;
      this.latestMouseY = event.offsetY;

      const physicalWidth = Math.floor(event.offsetX * devicePixelRatio);
      const physicalHeight = Math.floor(event.offsetY * devicePixelRatio);

      // console.log(`Mouse move to to: ${physicalWidth} × ${physicalHeight} (CSS: ${event.offsetX} × ${event.offsetY})`);

      // Schedule update on next animation frame if not already scheduled
      if (!this.mouseMoveScheduled) {
        this.mouseMoveScheduled = true;
        requestAnimationFrame(() => {
          this.latestPick = []; // Clear last pick cache
          this.worker.postMessage({
            ty: "mousemove",
            x: this.latestMouseX,
            y: this.latestMouseY
          });
          this.mouseMoveScheduled = false;
        });
      }
    });

    // Mouse click and selection handling
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

  // Inspector command methods

  /**
   * Update a component on an entity
   */
  public inspectorUpdateComponent(entityId: string, componentId: number, valueJson: string) {
    this.worker.postMessage({
      ty: "inspector_update_component",
      entity_id: entityId,
      component_id: componentId,
      value_json: valueJson
    });
  }

  /**
   * Toggle a component on an entity (add if missing, remove if present)
   */
  public inspectorToggleComponent(entityId: string, componentId: number) {
    this.worker.postMessage({
      ty: "inspector_toggle_component",
      entity_id: entityId,
      component_id: componentId
    });
  }

  /**
   * Remove a component from an entity
   */
  public inspectorRemoveComponent(entityId: string, componentId: number) {
    this.worker.postMessage({
      ty: "inspector_remove_component",
      entity_id: entityId,
      component_id: componentId
    });
  }

  /**
   * Insert a component on an entity
   */
  public inspectorInsertComponent(entityId: string, componentId: number, valueJson: string) {
    this.worker.postMessage({
      ty: "inspector_insert_component",
      entity_id: entityId,
      component_id: componentId,
      value_json: valueJson
    });
  }

  /**
   * Despawn an entity
   */
  public inspectorDespawnEntity(entityId: string, kind: string = "Recursive") {
    this.worker.postMessage({
      ty: "inspector_despawn_entity",
      entity_id: entityId,
      kind: kind
    });
  }

  /**
   * Toggle visibility of an entity
   */
  public inspectorToggleVisibility(entityId: string) {
    this.worker.postMessage({
      ty: "inspector_toggle_visibility",
      entity_id: entityId
    });
  }

  /**
   * Reparent an entity
   */
  public inspectorReparentEntity(entityId: string, parentId?: string) {
    this.worker.postMessage({
      ty: "inspector_reparent_entity",
      entity_id: entityId,
      parent_id: parentId
    });
  }

  /**
   * Spawn a new entity
   */
  public inspectorSpawnEntity(parentId?: string) {
    this.worker.postMessage({
      ty: "inspector_spawn_entity",
      parent_id: parentId
    });
  }

  /**
   * Cleanup method for controller resources
   */
  public dispose() {
    // Optional cleanup if needed
    // Could terminate worker, remove event listeners, etc.
    console.log("WorkerController cleanup");
  }



  private process_update(update: any) {
    // if update is an array longer than length 7, then save to json file
    return

  }

}