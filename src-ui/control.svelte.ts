import { SvelteMap } from 'svelte/reactivity';
import wasmUrl from './wasm/ironfell_bg.wasm?url'
import { SystemState } from './system_state.svelte';
import { MainThreadAdapter } from './main-thread-adapter';



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
  private adapter: MainThreadAdapter;
  private canvas: HTMLCanvasElement;

  // Input handling properties
  private latestPick: any[] = [];
  private latestMouseX = 0;
  private latestMouseY = 0;
  private mouseMoveScheduled = false;

  // Key handling with throttling
  private pressedKeys = new Set<string>();
  private keyUpdateScheduled = false;


  public state = new SystemState();

  /**
   * Creates a new WorkerController instance
   * @param canvas The canvas element where rendering occurs
   */
  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;

    // Start fetching WASM immediately
    console.log("Starting WASM fetch immediately...");
    this.wasmDataPromise = fetch(wasmUrl).then(response => response.arrayBuffer());

    // Initialize the main thread adapter
    console.log("Initializing Main Thread Adapter...");
    this.adapter = new MainThreadAdapter();

    // Set up message handling from adapter
    this.adapter.onmessage = this.handleWorkerMessage.bind(this);

    // Send the WASM data to the adapter once it's loaded
    this.wasmDataPromise.then(wasmData => {
      console.log("WASM data loaded, sending to adapter...");
      this.adapter.postMessage({ ty: "wasmData", wasmData }, [wasmData]);
    }).catch(error => {
      console.error("Failed to load WASM:", error);
    });

    // Listen for tab visibility changes
    this.setupVisibilityListener();
  }

  /**
   * Sets up listener to pause rendering when tab is not visible
   */
  private setupVisibilityListener() {
    document.addEventListener('visibilitychange', () => {
      if (document.hidden) {
        this.adapter.postMessage({ ty: "stopRunning" });
        console.log("Tab hidden, paused rendering");
      } else {
        this.adapter.postMessage({ ty: "startRunning" });
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

      // Wait for the adapter to be ready
      await this.waitForWorkerReady();

      // Initialize canvas with the adapter (no transfer needed)
      this.initializeCanvasWithAdapter();
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
            this.adapter.postMessage({ ty: "keydown", key });
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
      this.adapter.postMessage({ ty: "keyup", key });
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
   * Initialize canvas with the main thread adapter
   */
  private initializeCanvasWithAdapter() {
    const devicePixelRatio = OVERRIDE_SCALE ? OVERRIDE_SCALE_FACTOR : window.devicePixelRatio;

    this.adapter.postMessage(
      { ty: "init", canvas: this.canvas, devicePixelRatio }
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
    this.adapter.postMessage({
      ty: "resize",
      width: physicalWidth,
      height: physicalHeight
    });
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
        this.adapter.postMessage({ ty: "hover", list: this.latestPick });
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

        this.state.process_update(data.update);
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
      // Store the latest position
      this.latestMouseX = event.offsetX;
      this.latestMouseY = event.offsetY;

      // Schedule update on next animation frame if not already scheduled
      if (!this.mouseMoveScheduled) {
        this.mouseMoveScheduled = true;
        requestAnimationFrame(() => {
          this.latestPick = []; // Clear last pick cache
          this.adapter.postMessage({
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
        this.adapter.postMessage({
          ty: "leftBtDown",
          pickItem: this.latestPick[0],
          x: event.offsetX,
          y: event.offsetY,
        });
      }
    });

    this.canvas.addEventListener("mouseup", (_event) => {
      this.adapter.postMessage({ ty: "leftBtUp" });
    });

    this.canvas.addEventListener("click", (_event) => {
      if (Array.isArray(this.latestPick) && this.latestPick.length > 0) {
        this.adapter.postMessage({
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
    this.adapter.postMessage({ ty: "startRunning" });
    this.setCanvasOpacity("100%");
  }

  /**
   * Stops the worker engine instance
   */
  stopWorkerApp() {
    this.adapter.postMessage({ ty: "stopRunning" });
    this.setCanvasOpacity("50%");
  }

  /**
   * Turns on/off engine animation
   */
  setWorkerAutoAnimation(needsAnimation: boolean) {
    this.adapter.postMessage({
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
    this.adapter.postMessage({
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
    this.adapter.postMessage({
      ty: "inspector_toggle_component",
      entity_id: entityId,
      component_id: componentId
    });
  }

  /**
   * Remove a component from an entity
   */
  public inspectorRemoveComponent(entityId: string, componentId: number) {
    this.adapter.postMessage({
      ty: "inspector_remove_component",
      entity_id: entityId,
      component_id: componentId
    });
  }

  /**
   * Insert a component on an entity
   */
  public inspectorInsertComponent(entityId: string, componentId: number, valueJson: string) {
    this.adapter.postMessage({
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
    this.adapter.postMessage({
      ty: "inspector_despawn_entity",
      entity_id: entityId,
      kind: kind
    });
  }

  /**
   * Toggle visibility of an entity
   */
  public inspectorToggleVisibility(entityId: string) {
    this.adapter.postMessage({
      ty: "inspector_toggle_visibility",
      entity_id: entityId
    });
  }

  /**
   * Reparent an entity
   */
  public inspectorReparentEntity(entityId: string, parentId?: string) {
    this.adapter.postMessage({
      ty: "inspector_reparent_entity",
      entity_id: entityId,
      parent_id: parentId
    });
  }

  /**
   * Spawn a new entity
   */
  public inspectorSpawnEntity(parentId?: string) {
    this.adapter.postMessage({
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




}


