// from bevy-in-web-worker https://github.com/jinleili/bevy-in-web-worker

import init, {
  init_bevy_app,
  is_preparation_completed,
  create_window_by_offscreen_canvas_with_id,
  enter_frame,
  enter_frame_with_mouse, // Add new function
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
  // Streaming FFI functions (now available after WASM rebuild)
  enable_inspector_streaming,
  disable_inspector_streaming,
  set_inspector_streaming_frequency,
  force_inspector_update,
  get_type_registry_schema,
  inspector_reset_streaming_state,
} from "./wasm/ironfell.js";

// import wasmUrl from './wasm/ironfell_bg.wasm?url'

class IronWorker {
  private appHandle: bigint = BigInt(0);
  private initFinished = 0;
  private isStoppedRunning = false;
  private renderBlockTime = 1;
  private offscreenCanvases: Map<string, OffscreenCanvas> = new Map();
  private frameIndex = 0;
  private frameCount = 0;
  private frameFlag = 0;
  private streamingEnabled = false;
  private streamingInterval: number | null = null;
  private rafId: number | null = null;
  private latestMouseX: number = 0;
  private latestMouseY: number = 0;
  private hasMouseUpdate: boolean = false;

  constructor() {
    // Create a dedicated object for Rust FFI functions
    const rustBridge = {
      block_from_worker: (blockTime?: number) => this.blockFromWorker(blockTime),
      send_pick_from_worker: (pickList: any[]) => this.sendPickFromWorker(pickList),
      send_hover_from_worker: (list: any[]) => this.sendHoverFromWorker(list),
      send_selection_from_worker: (list: any[]) => this.sendSelectionFromWorker(list),
      send_inspector_update_from_worker: (updateJson: string) => this.sendInspectorUpdateFromWorker(updateJson)
    };

    // Make it globally accessible
    (self as any).rustBridge = rustBridge;

    // Expose the functions to the global scope so they're accessible from Wasm
    (self as any).block_from_worker = (blockTime?: number) => this.blockFromWorker(blockTime);
    (self as any).send_pick_from_worker = (pickList: any[]) => this.sendPickFromWorker(pickList);
    (self as any).send_hover_from_worker = (list: any[]) => this.sendHoverFromWorker(list);
    (self as any).send_selection_from_worker = (list: any[]) => this.sendSelectionFromWorker(list);
    (self as any).send_inspector_update_from_worker = (updateJson: string) => this.sendInspectorUpdateFromWorker(updateJson);

    // Initialize the worker
    this.initWasmInWorker();
  }

  private async initWasmInWorker() {
    // Listen for messages from the main thread
    self.onmessage = async (event) => {
      let data = event.data;
      switch (data.ty) {

        case "wasmData":
          // Initialize the wasm module with the provided data
          console.log("Received WASM data from main thread, initializing...");
          await init(data.wasmData);
          console.log("WASM module initialized");
          this.appHandle = init_bevy_app();
          console.log("App handle initialized:", this.appHandle);

          // Notify the main thread that the worker is ready
          console.log("Sending workerIsReady message");
          self.postMessage({ ty: "workerIsReady" });
          break;

        case "wasmUrl":
          // Keep this for backward compatibility, but it shouldn't be used now
          console.warn("Received wasmUrl - this should not happen with the new implementation");
          await init(data.wasmUrl);
          this.appHandle = init_bevy_app();
          self.postMessage({ ty: "workerIsReady" });
          break;

        case "init":
          // Remember which canvas this init corresponds to
          (self as any).lastInitCanvasId = data.canvasId;
          console.log(`running init of worker app window: ${data.canvasId || 'primary'}`);
          this.createWorkerAppWindow(data.canvas, data.devicePixelRatio, data.canvasId || 'viewer-canvas');
          break;

        case "createAdditionalWindow":
          (self as any).lastInitCanvasId = data.canvasId;
          console.log(`creating additional worker app window: ${data.canvasId || 'unknown'}`);
          this.createAdditionalWorkerAppWindow(data.canvas, data.devicePixelRatio, data.canvasId || 'secondary');
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
          // Buffer the latest mouse position instead of immediately processing
          this.latestMouseX = data.x;
          this.latestMouseY = data.y;
          this.hasMouseUpdate = true;
          // Don't call mouse_move here anymore
          break;


        case "leftBtDown":
          // Ignore legacy payload fields (pickItem, x, y)
          left_bt_down(this.appHandle as any); // maintain old arity until wasm rebuild exposes new signature
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

        case "keydown": // Handle keydown event
          if (this.appHandle !== BigInt(0)) {
            console.log("Key down event received:", data.key);
            key_down(this.appHandle, data.key);
          }
          break;

        case "keyup": // Handle keyup event
          if (this.appHandle !== BigInt(0)) {
            key_up(this.appHandle, data.key);
          }
          break;

        // make the bevy inspector commands here!
        // begin commands

        case "inspector_update_component":
          if (this.appHandle !== BigInt(0)) {
            const success = inspector_update_component(
              this.appHandle,
              BigInt(data.entity_id),
              data.component_id,
              data.value_json
            );
            self.postMessage({ ty: "inspector_result", command: "update_component", success });
          }
          break;

        case "inspector_toggle_component":
          if (this.appHandle !== BigInt(0)) {
            const success = inspector_toggle_component(
              this.appHandle,
              BigInt(data.entity_id),
              data.component_id
            );
            self.postMessage({ ty: "inspector_result", command: "toggle_component", success });
          }
          break;

        case "inspector_remove_component":
          if (this.appHandle !== BigInt(0)) {
            const success = inspector_remove_component(
              this.appHandle,
              BigInt(data.entity_id),
              data.component_id
            );
            self.postMessage({ ty: "inspector_result", command: "remove_component", success });
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
            self.postMessage({ ty: "inspector_result", command: "insert_component", success });
          }
          break;

        case "inspector_despawn_entity":
          if (this.appHandle !== BigInt(0)) {
            const success = inspector_despawn_entity(
              this.appHandle,
              BigInt(data.entity_id),
              data.kind
            );
            self.postMessage({ ty: "inspector_result", command: "despawn_entity", success });
          }
          break;

        case "inspector_toggle_visibility":
          if (this.appHandle !== BigInt(0)) {
            const success = inspector_toggle_visibility(
              this.appHandle,
              BigInt(data.entity_id)
            );
            self.postMessage({ ty: "inspector_result", command: "toggle_visibility", success });
          }
          break;

        case "inspector_reparent_entity":
          if (this.appHandle !== BigInt(0)) {
            const success = inspector_reparent_entity(
              this.appHandle,
              BigInt(data.entity_id),
              data.parent_id ? BigInt(data.parent_id) : undefined
            );
            self.postMessage({ ty: "inspector_result", command: "reparent_entity", success });
          }
          break;

        case "inspector_spawn_entity":
          if (this.appHandle !== BigInt(0)) {
            const entityId = inspector_spawn_entity(
              this.appHandle,
              data.parent_id ? BigInt(data.parent_id) : undefined
            );
            self.postMessage({
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
          self.postMessage({
            ty: "type_registry_schema",
            schema: schema,
            requestId: data.requestId
          });
          break;

        case "reset_streaming_state":
          this.resetStreamingState();
          break;

        // end commands



        default:
          break;
      }
    };
  }

  private canvasResize(canvasId: string, width: number, height: number) {
    const osc = this.offscreenCanvases.get(canvasId);
    if (osc) {
      // Update the matched canvas
      osc.width = width;
      osc.height = height;

      // console.log("Resized canvas to:", this.offscreenCanvas.width, "×", this.offscreenCanvas.height);

      // And then notify bevy it's changed
      resize(this.appHandle, width, height);
    }
  }

  private createWorkerAppWindow(offscreenCanvas: OffscreenCanvas, devicePixelRatio: number, canvasId: string) {
    // Store the canvas reference keyed by id
    this.offscreenCanvases.set(canvasId, offscreenCanvas);

    // Decide window kind by id
    const kind = canvasId === 'viewer-canvas' ? 'viewer' : (canvasId.includes('timeline') ? 'timeline' : 'other');
    // Pass extra args via `any` cast for forward compatibility
  create_window_by_offscreen_canvas_with_id(
      this.appHandle,
      offscreenCanvas,
      devicePixelRatio,
      canvasId,
      kind
    );

    // Check ready state
    this.getPreparationState();

    // Start frame loop only if not already running and not stopped
    if (this.rafId === null && !this.isStoppedRunning) {
      this.rafId = requestAnimationFrame((dt) => this.enterFrame(dt));
    }
  }

  private createAdditionalWorkerAppWindow(offscreenCanvas: OffscreenCanvas, devicePixelRatio: number, canvasId: string) {
    // Create additional rendering window on the same Bevy app instance
    const kind = canvasId.includes('timeline') ? 'timeline' : 'other';
    this.offscreenCanvases.set(canvasId, offscreenCanvas);
  create_window_by_offscreen_canvas_with_id(
      this.appHandle,
      offscreenCanvas,
      devicePixelRatio,
      canvasId,
      kind
    );

    // No need to start another frame loop - the existing one handles all windows
    console.log("Additional worker app window created");
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
        // Use new combined function that processes mouse and frame together
        if (this.hasMouseUpdate) {
          enter_frame_with_mouse(this.appHandle, this.latestMouseX, this.latestMouseY, true);
          this.hasMouseUpdate = false;
        } else {
          enter_frame_with_mouse(this.appHandle, 0, 0, false);
        }
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
    // Deprecated path; retain for backward compatibility if needed.
    self.postMessage({ ty: "pick", list: pickList });
  }

  private sendHoverFromWorker(list: any[]) {
    self.postMessage({ ty: "hover", list });
  }

  private sendSelectionFromWorker(list: any[]) {
    self.postMessage({ ty: "selection", list });
  }

  private sendInspectorUpdateFromWorker(updateJson: string) {
    try {
      const update = JSON.parse(updateJson);
      self.postMessage({ ty: "inspector_update", update });

    } catch (error) {
      console.error("Failed to parse inspector update JSON:", error);
    }
  }

  private blockFromWorker(blockTime?: number) {
    const start = performance.now();
    while (performance.now() - start < (blockTime || this.renderBlockTime)) { }
  }

  private enableStreaming() {
    this.enableContinuousStreaming();
  }

  private disableStreaming() {
    this.disableContinuousStreaming();
  }

  private enableContinuousStreaming() {
    if (this.appHandle === BigInt(0)) return;

    try {
      // Enable continuous streaming for animations/automatic updates
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

  private enableInspectorStreaming() {
    // Alias for backward compatibility
    this.enableContinuousStreaming();
  }

  private disableInspectorStreaming() {
    // Alias for backward compatibility  
    this.disableContinuousStreaming();
  }

  private setStreamingFrequency(ticks: number) {
    this.setInspectorStreamingFrequency(ticks);
  }

  private setInspectorStreamingFrequency(ticks: number) {
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
}

// Initialize the worker
new IronWorker();