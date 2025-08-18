import { SystemState } from './system_state.svelte';
import { AdapterBridge } from './runtime/adapter_bridge';
import { WasmLoader } from './runtime/wasm_loader';
import { InputManager } from './runtime/input_manager';
import { ResizeManager } from './runtime/resize_manager';
import { InspectorClient } from './runtime/inspector_client';



const OVERRIDE_SCALE = false;
const OVERRIDE_SCALE_FACTOR = 2;

/**
 * WorkerController manages interactions with a Web Worker that runs the engine instance.
 * It handles communication, mouse events, and state management between the UI and worker.
 */
export type RuntimeMode = 'worker' | 'main';

export class WorkerController {
  // Whether the worker is ready
  workerIsReady = $state(false);

  width = $state(0);
  height = $state(0);

  private initializationError: any = null;
  private runtimeMode: RuntimeMode;
  private canvas: HTMLCanvasElement;
  private bridge!: AdapterBridge;
  private wasmLoader = new WasmLoader();
  private input = new InputManager({ enableRaw: true });
  private resizeManager = new ResizeManager();
  private inspector = new InspectorClient(new SystemState());

  public state = this.inspector.state;

  /**
   * Creates a new WorkerController instance
   * @param canvas The canvas element where rendering occurs
   */
  constructor(canvas: HTMLCanvasElement, mode: RuntimeMode = 'worker') {
    this.canvas = canvas;
    this.runtimeMode = mode;

    this.bridge = new AdapterBridge(this.runtimeMode, this.canvas);
    this.bridge.setHandler(data => this.handleBridgeMessage(data));
    this.wasmLoader.startFetch();
    this.inspector.init(this.bridge);
    // Listen for tab visibility changes
    this.setupVisibilityListener();
  }

  /**
   * Sets up listener to pause rendering when tab is not visible
   */
  private setupVisibilityListener() {
    document.addEventListener('visibilitychange', () => {
      if (document.hidden) {
        this.bridge.post({ ty: 'stopRunning' });
        console.log("Tab hidden, paused rendering");
      } else {
        this.bridge.post({ ty: 'startRunning' });
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
   * Waits for the worker to signal it's ready
   */
  private waitForWorkerReady(): Promise<void> {
    // We dispatch wasm early; worker readiness message will flip flag
    return new Promise((resolve) => {
      if (this.workerIsReady) {
        resolve();
        return;
      }

      const id = setInterval(() => {
        if (this.workerIsReady) {
          clearInterval(id);
          resolve();
        }
      }, 50);

      // Kick wasm send if not already
      this.wasmLoader.sendToAdapter(this.bridge).catch(e => console.error('WASM send failed', e));
    });
  }

  /**
   * Initialize canvas with the main thread adapter
   */
  private initializeCanvasWithAdapter() {
    const devicePixelRatio = OVERRIDE_SCALE ? OVERRIDE_SCALE_FACTOR : window.devicePixelRatio;

    if (this.runtimeMode === 'worker') {
      try {
        // Attempt to transfer existing canvas. This will fail if a rendering context was already created.
        // @ts-ignore
        const offscreen: OffscreenCanvas = (this.canvas as any).transferControlToOffscreen();
        this.bridge.post({ ty: 'init', canvas: offscreen, devicePixelRatio }, [offscreen as any]);
      } catch (e: any) {
        console.error('Offscreen transfer failed:', e);
        this.bridge.post({ ty: 'init', canvas: this.canvas, devicePixelRatio });
      }
    } else {
      this.bridge.post({ ty: 'init', canvas: this.canvas, devicePixelRatio });
    }

    this.resizeManager.init(this.canvas, this.bridge);
  }

  /**
   * Requests canvas resize with proper pixel ratio scaling
   */
  public requestCanvasResize(width: number, height: number, force = false) {
    this.resizeManager.request(width, height, force);
  }

  /**
   * Handles messages received from the worker
   */
  private handleBridgeMessage(data: any) {
    switch (data.ty) {
      case "workerIsReady":
        this.workerIsReady = true;
        this.input.init(this.canvas, this.bridge);
        break;

      case "pick":
        // Deprecated: legacy pick message; store for debug / transition.
        this.input.setPick(data.list);
        break;
      case "hover":
        // Hover list coming directly from Rust picking
        this.input.setPick(data.list);
        break;
      case "selection":
        // Selection list from Rust; could drive UI panels
        this.input.setPick(data.list);
        break;

      case "inspector_result":
        // Optionally surface result logging
        break;

      case "inspector_update":
        this.inspector.handleUpdate(data.update);
        break;

      default:
        break;
    }
  }

  /**
   * Starts the worker engine instance
   */
  startWorkerApp() {
    this.bridge.post({ ty: 'startRunning' });
    this.setCanvasOpacity("100%");
  }

  /**
   * Stops the worker engine instance
   */
  stopWorkerApp() {
    this.bridge.post({ ty: 'stopRunning' });
    this.setCanvasOpacity("50%");
  }

  /**
   * Turns on/off engine animation
   */
  setWorkerAutoAnimation(needsAnimation: boolean) {
    this.bridge.post({ ty: 'autoAnimation', autoAnimation: needsAnimation });
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
    this.inspector.updateComponent(entityId, componentId, valueJson);
  }

  /**
   * Toggle a component on an entity (add if missing, remove if present)
   */
  public inspectorToggleComponent(entityId: string, componentId: number) {
    this.inspector.toggleComponent(entityId, componentId);
  }

  /**
   * Remove a component from an entity
   */
  public inspectorRemoveComponent(entityId: string, componentId: number) {
    this.inspector.removeComponent(entityId, componentId);
  }

  /**
   * Insert a component on an entity
   */
  public inspectorInsertComponent(entityId: string, componentId: number, valueJson: string) {
    this.inspector.insertComponent(entityId, componentId, valueJson);
  }

  /**
   * Despawn an entity
   */
  public inspectorDespawnEntity(entityId: string, kind: string = "Recursive") {
    this.inspector.despawnEntity(entityId, kind);
  }

  /**
   * Toggle visibility of an entity
   */
  public inspectorToggleVisibility(entityId: string) {
    this.inspector.toggleVisibility(entityId);
  }

  /**
   * Reparent an entity
   */
  public inspectorReparentEntity(entityId: string, parentId?: string) {
    this.inspector.reparentEntity(entityId, parentId);
  }

  /**
   * Spawn a new entity
   */
  public inspectorSpawnEntity(parentId?: string) {
    this.inspector.spawnEntity(parentId);
  }

  /**
   * Cleanup method for controller resources
   */
  public dispose() {
    try { this.bridge.post({ ty: 'stopRunning' }); } catch { }
    this.bridge.dispose();
  }

  public getMode(): RuntimeMode { return this.runtimeMode; }




}


