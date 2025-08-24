// Main thread adapter to replace worker functionality for testing
import init, {
  init_bevy_app,
  is_preparation_completed,
  create_window_by_offscreen_canvas,
  enter_frame,
  mouse_move,
  left_bt_down,
  left_bt_up,
  set_auto_animation,
  resize,
  key_down,
  key_up,
  // Inspector FFI functions
  inspector_update_component,
  inspector_toggle_component,
  inspector_remove_component,
  inspector_insert_component,
  inspector_despawn_entity,
  inspector_toggle_visibility,
  inspector_reparent_entity,
  inspector_spawn_entity,
  // Streaming FFI functions
  enable_inspector_streaming,
  disable_inspector_streaming,
  set_inspector_streaming_frequency,
  force_inspector_update,
  get_type_registry_schema,
  inspector_reset_streaming_state,
} from "./wasm/ironfell.js";

export class MainThreadAdapter {
  private appHandle: bigint = BigInt(0);
  private initFinished = 0;
  private isStoppedRunning = false;
  private renderBlockTime = 1;
  private canvas: HTMLCanvasElement | null = null;
  private frameIndex = 0;
  private frameCount = 0;
  private frameFlag = 0;
  private streamingEnabled = false;
  private streamingInterval: number | null = null;
  private messageHandler: ((event: any) => void) | null = null;
  private rafId: number | null = null;

  constructor() {
    // Create a dedicated object for Rust FFI functions
    const rustBridge = {
      block_from_worker: (blockTime?: number) => this.blockFromWorker(blockTime),
      send_pick_from_worker: (pickList: any[]) => this.sendPickFromWorker(pickList),
      send_inspector_update_from_worker: (updateJson: string) => this.sendInspectorUpdateFromWorker(updateJson),
      send_hover_from_worker: (list: any[]) => this.sendHoverFromWorker(list),
      send_selection_from_worker: (list: any[]) => this.sendSelectionFromWorker(list)
    };

    // Make it globally accessible
    (window as any).rustBridge = rustBridge;

    // Expose the functions to the global scope so they're accessible from Wasm
    (window as any).block_from_worker = (blockTime?: number) => this.blockFromWorker(blockTime);
    (window as any).send_pick_from_worker = (pickList: any[]) => this.sendPickFromWorker(pickList);
    (window as any).send_inspector_update_from_worker = (updateJson: string) => this.sendInspectorUpdateFromWorker(updateJson);
    (window as any).send_hover_from_worker = (list: any[]) => this.sendHoverFromWorker(list);
    (window as any).send_selection_from_worker = (list: any[]) => this.sendSelectionFromWorker(list);
  }

  // Simulate worker's onmessage interface
  set onmessage(handler: (event: any) => void) {
    this.messageHandler = handler;
  }

  // Simulate worker's postMessage interface
  async postMessage(data: any, transfer?: any[]) {
    switch (data.ty) {
      case "wasmData":
        // Initialize the wasm module with the provided data
        console.log("Received WASM data from main thread, initializing...");
        await init(data.wasmData);
        console.log("WASM module initialized");
        this.appHandle = init_bevy_app();
        console.log("App handle initialized:", this.appHandle);

        // Notify that the "worker" is ready
        console.log("Sending workerIsReady message");
        this.sendMessage({ ty: "workerIsReady" });
        break;

      case "init":
        this.canvas = data.canvas;
        console.log(`running init of main thread app window: ${data.canvasId || 'primary'}`);
        this.createAppWindow(data.canvas, data.devicePixelRatio);
        break;

      case "createAdditionalWindow":
        console.log(`creating additional main thread app window: ${data.canvasId || 'unknown'}`);
        this.createAdditionalAppWindow(data.canvas, data.devicePixelRatio);
        break;

      case "startRunning":
        if (this.isStoppedRunning) {
          this.isStoppedRunning = false;
          // Only start a new frame loop if one isn't already running
          if (this.rafId === null) {
            this.rafId = requestAnimationFrame((dt) => this.enterFrame(dt));
          }
        }
        break;

      case "stopRunning":
        this.isStoppedRunning = true;
        // Cancel any pending RAF to prevent multiple loops
        if (this.rafId !== null) {
          cancelAnimationFrame(this.rafId);
          this.rafId = null;
        }
        break;

      case "mousemove":
        mouse_move(this.appHandle, data.x, data.y);
        break;

      case "leftBtDown":
        // No entity id passed now; maintain old arity shim until wasm export updated
        left_bt_down(this.appHandle);
        break;

      case "leftBtUp":
        left_bt_up(this.appHandle);
        break;

      case "blockRender":
        this.renderBlockTime = data.blockTime;
        break;

      case "autoAnimation":
        set_auto_animation(this.appHandle, data.autoAnimation);
        break;

      case "resize":
        this.canvasResize(data.canvasId, data.width, data.height);
        break;

      case "keydown":
        if (this.appHandle !== BigInt(0)) {
          console.log("Key down event received:", data.key);
          key_down(this.appHandle, data.key);
        }
        break;

      case "keyup":
        if (this.appHandle !== BigInt(0)) {
          key_up(this.appHandle, data.key);
        }
        break;

      // Inspector commands
      case "inspector_update_component":
        if (this.appHandle !== BigInt(0)) {
          const success = inspector_update_component(
            this.appHandle,
            BigInt(data.entity_id),
            data.component_id,
            data.value_json
          );
          this.sendMessage({ ty: "inspector_result", command: "update_component", success });
        }
        break;

      case "inspector_toggle_component":
        if (this.appHandle !== BigInt(0)) {
          const success = inspector_toggle_component(
            this.appHandle,
            BigInt(data.entity_id),
            data.component_id
          );
          this.sendMessage({ ty: "inspector_result", command: "toggle_component", success });
        }
        break;

      case "inspector_remove_component":
        if (this.appHandle !== BigInt(0)) {
          const success = inspector_remove_component(
            this.appHandle,
            BigInt(data.entity_id),
            data.component_id
          );
          this.sendMessage({ ty: "inspector_result", command: "remove_component", success });
        }
        break;

      case "inspector_insert_component":
        if (this.appHandle !== BigInt(0)) {
          const success = inspector_insert_component(
            this.appHandle,
            BigInt(data.entity_id),
            data.component_id,
            data.value_json
          );
          this.sendMessage({ ty: "inspector_result", command: "insert_component", success });
        }
        break;

      case "inspector_despawn_entity":
        if (this.appHandle !== BigInt(0)) {
          const success = inspector_despawn_entity(
            this.appHandle,
            BigInt(data.entity_id),
            data.kind
          );
          this.sendMessage({ ty: "inspector_result", command: "despawn_entity", success });
        }
        break;

      case "inspector_toggle_visibility":
        if (this.appHandle !== BigInt(0)) {
          const success = inspector_toggle_visibility(
            this.appHandle,
            BigInt(data.entity_id)
          );
          this.sendMessage({ ty: "inspector_result", command: "toggle_visibility", success });
        }
        break;

      case "inspector_reparent_entity":
        if (this.appHandle !== BigInt(0)) {
          const success = inspector_reparent_entity(
            this.appHandle,
            BigInt(data.entity_id),
            data.parent_id ? BigInt(data.parent_id) : undefined
          );
          this.sendMessage({ ty: "inspector_result", command: "reparent_entity", success });
        }
        break;

      case "inspector_spawn_entity":
        if (this.appHandle !== BigInt(0)) {
          const entityId = inspector_spawn_entity(
            this.appHandle,
            data.parent_id ? BigInt(data.parent_id) : undefined
          );
          this.sendMessage({
            ty: "inspector_result",
            command: "spawn_entity",
            success: entityId !== BigInt(0),
            entity_id: entityId.toString()
          });
        }
        break;

      case "enable_streaming":
        this.enableContinuousStreaming();
        break;

      case "disable_streaming":
        this.disableContinuousStreaming();
        break;

      case "set_streaming_frequency":
        this.setStreamingFrequency(data.ticks || 3);
        break;

      case "force_inspector_update":
        this.forceInspectorUpdate();
        break;

      case "get_type_registry_schema":
        const schema = this.getTypeRegistrySchema();
        this.sendMessage({
          ty: "type_registry_schema",
          schema: schema,
          requestId: data.requestId
        });
        break;

      case "reset_streaming_state":
        this.resetStreamingState();
        break;

      default:
        break;
    }
  }

  private sendMessage(data: any) {
    if (this.messageHandler) {
      // Simulate the MessageEvent structure
      this.messageHandler({ data });
    }
  }

  private canvasResize(canvasId: string, width: number, height: number) {
    if (this.canvas) {
      // Update the canvas size
      this.canvas.width = width;
      this.canvas.height = height;

      // console.log("Resized canvas to:", this.canvas.width, "×", this.canvas.height);

      // Notify bevy it's changed
      resize(this.appHandle, width, height);
    }
  }

  private createAppWindow(canvas: HTMLCanvasElement, devicePixelRatio: number) {
    // Store the canvas reference
    this.canvas = canvas;

    // For main thread, we need to convert the regular canvas to an offscreen canvas
    // or use it directly - let's try using the offscreen function with regular canvas
    try {
      create_window_by_offscreen_canvas(
        this.appHandle,
        canvas as any, // Cast to any to bypass TypeScript checks
        devicePixelRatio
      );
    } catch (error) {
      console.error("Failed to create window:", error);
      // If that doesn't work, we might need a different approach
      throw error;
    }

    // Check ready state
    this.getPreparationState();

    // Start frame loop only if not already running and not stopped
    if (this.rafId === null && !this.isStoppedRunning) {
      this.rafId = requestAnimationFrame((dt) => this.enterFrame(dt));
    }
  }

  private createAdditionalAppWindow(canvas: HTMLCanvasElement, devicePixelRatio: number) {
    // Create additional rendering window on the same Bevy app instance
    try {
      create_window_by_offscreen_canvas(
        this.appHandle,
        canvas as any, // Cast to any to bypass TypeScript checks
        devicePixelRatio
      );
      console.log("Additional main thread app window created");
    } catch (error) {
      console.error("Failed to create additional window:", error);
      throw error;
    }

    // No need to start another frame loop - the existing one handles all windows
  }

  private enterFrame(_dt: number) {
    // Clear the RAF ID since this frame is now executing
    this.rafId = null;

    if (this.appHandle === BigInt(0) || this.isStoppedRunning) return;

    // Execute the app's frame loop when ready
    if (this.initFinished > 0) {
      if (
        this.frameIndex >= this.frameFlag ||
        (this.frameIndex < this.frameFlag && this.frameCount % 60 == 0)
      ) {
        enter_frame(this.appHandle);
        this.frameIndex++;
      }
      this.frameCount++;
    } else {
      this.getPreparationState();
    }

    // Schedule next frame only if not stopped
    if (!this.isStoppedRunning) {
      this.rafId = requestAnimationFrame((dt) => this.enterFrame(dt));
    }
  }

  private getPreparationState() {
    this.initFinished = is_preparation_completed(this.appHandle);
  }

  private sendPickFromWorker(pickList: any[]) {
    this.sendMessage({ ty: "pick", list: pickList });
  }

  private sendHoverFromWorker(list: any[]) {
    this.sendMessage({ ty: "hover", list });
  }

  private sendSelectionFromWorker(list: any[]) {
    this.sendMessage({ ty: "selection", list });
  }

  private sendInspectorUpdateFromWorker(updateJson: string) {
    try {
      const update = JSON.parse(updateJson);
      this.sendMessage({ ty: "inspector_update", update });
    } catch (error) {
      console.error("Failed to parse inspector update JSON:", error);
    }
  }

  private blockFromWorker(blockTime?: number) {
    const start = performance.now();
    while (performance.now() - start < (blockTime || this.renderBlockTime)) { }
  }

  private enableContinuousStreaming() {
    if (this.appHandle === BigInt(0)) return;

    try {
      enable_inspector_streaming(this.appHandle);
      this.streamingEnabled = true;
      console.log("Continuous inspector streaming enabled (for animations)");
    } catch (error) {
      console.error("Failed to enable continuous streaming:", error);
    }
  }

  private disableContinuousStreaming() {
    if (this.appHandle === BigInt(0)) return;

    try {
      disable_inspector_streaming(this.appHandle);
      this.streamingEnabled = false;
      console.log("Continuous inspector streaming disabled");
    } catch (error) {
      console.error("Failed to disable continuous streaming:", error);
    }
  }

  private setStreamingFrequency(ticks: number) {
    if (this.appHandle === BigInt(0)) return;

    try {
      set_inspector_streaming_frequency(this.appHandle, ticks);
      console.log(`Continuous streaming frequency set to ${ticks} ticks`);
    } catch (error) {
      console.error("Failed to set streaming frequency:", error);
    }
  }

  private forceInspectorUpdate() {
    if (this.appHandle === BigInt(0)) return;

    try {
      force_inspector_update(this.appHandle);
      console.log("Forced inspector update");
    } catch (error) {
      console.error("Failed to force inspector update:", error);
    }
  }

  private getTypeRegistrySchema(): string {
    if (this.appHandle === BigInt(0)) return "{}";

    try {
      return get_type_registry_schema(this.appHandle);
    } catch (error) {
      console.error("Failed to get type registry schema:", error);
      return "{}";
    }
  }

  private resetStreamingState() {
    if (this.appHandle === BigInt(0)) return;

    try {
      inspector_reset_streaming_state(this.appHandle, 0); // Use client_id 0
      console.log("Inspector streaming state reset");
    } catch (error) {
      console.error("Failed to reset streaming state:", error);
    }
  }

  // Cleanup method to properly dispose of the adapter
  dispose() {
    // Immediately stop running and cancel any pending RAF
    this.isStoppedRunning = true;
    if (this.rafId !== null) {
      cancelAnimationFrame(this.rafId);
      this.rafId = null;
    }

    // Clear any streaming intervals
    if (this.streamingInterval !== null) {
      clearInterval(this.streamingInterval);
      this.streamingInterval = null;
    }

    // Reset state
    this.canvas = null;
    this.messageHandler = null;
    this.streamingEnabled = false;
  }
}