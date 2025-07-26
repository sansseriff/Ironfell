// from bevy-in-web-worker https://github.com/jinleili/bevy-in-web-worker

import init, {
  init_bevy_app,
  is_preparation_completed,
  create_window_by_offscreen_canvas,
  enter_frame,
  mouse_move,
  left_bt_down,
  left_bt_up,
  set_hover,
  set_selection,
  set_auto_animation,
  resize,
  key_down, // Add new FFI function
  key_up,   // Add new FFI function
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
  private offscreenCanvas: OffscreenCanvas | null = null;
  private frameIndex = 0;
  private frameCount = 0;
  private frameFlag = 0;
  private streamingEnabled = false;
  private streamingInterval: number | null = null;

  constructor() {
    // Create a dedicated object for Rust FFI functions
    const rustBridge = {
      block_from_worker: (blockTime?: number) => this.blockFromWorker(blockTime),
      send_pick_from_worker: (pickList: any[]) => this.sendPickFromWorker(pickList),
      send_inspector_update_from_worker: (updateJson: string) => this.sendInspectorUpdateFromWorker(updateJson)
    };

    // Make it globally accessible
    (self as any).rustBridge = rustBridge;

    // Expose the functions to the global scope so they're accessible from Wasm
    (self as any).block_from_worker = (blockTime?: number) => this.blockFromWorker(blockTime);
    (self as any).send_pick_from_worker = (pickList: any[]) => this.sendPickFromWorker(pickList);
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
          this.offscreenCanvas = data.canvas;
          console.log("running init of worker app window")
          this.createWorkerAppWindow(data.canvas, data.devicePixelRatio);
          break;

        case "startRunning":
          if (this.isStoppedRunning) {
            this.isStoppedRunning = false;
            // Start the frame loop
            requestAnimationFrame((dt) => this.enterFrame(dt));
          }
          break;

        case "stopRunning":
          this.isStoppedRunning = true;
          break;

        case "mousemove":
          mouse_move(this.appHandle, data.x, data.y);
          break;


        case "hover":
          // only called following a sendPickFromWorker call
          // Set hover (highlight) effect
          set_hover(this.appHandle, data.list);
          break;

        case "select":
          // Set selection effect
          set_selection(this.appHandle, data.list);
          break;

        case "leftBtDown":
          left_bt_down(this.appHandle, data.pickItem, data.x, data.y);
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
          this.canvasResize(data.width, data.height);
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

  private canvasResize(width: number, height: number) {
    if (this.offscreenCanvas) {
      // I think I update the the canvas size here
      this.offscreenCanvas.width = width;
      this.offscreenCanvas.height = height;

      // console.log("Resized canvas to:", this.offscreenCanvas.width, "×", this.offscreenCanvas.height);

      // And then notify bevy it's changed
      resize(this.appHandle, width, height);
    }
  }

  private createWorkerAppWindow(offscreenCanvas: OffscreenCanvas, devicePixelRatio: number) {
    // Store the canvas reference
    this.offscreenCanvas = offscreenCanvas;

    // Create rendering window
    create_window_by_offscreen_canvas(
      this.appHandle,
      offscreenCanvas,
      devicePixelRatio
    );

    // Check ready state
    this.getPreparationState();

    // Start frame loop
    requestAnimationFrame((dt) => this.enterFrame(dt));
  }

  private enterFrame(_dt: number) {
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
    requestAnimationFrame((dt) => this.enterFrame(dt));
  }

  private getPreparationState() {
    this.initFinished = is_preparation_completed(this.appHandle);
  }

  private sendPickFromWorker(pickList: any[]) {
    self.postMessage({ ty: "pick", list: pickList });
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