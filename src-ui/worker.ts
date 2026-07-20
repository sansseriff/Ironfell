// from bevy-in-web-worker https://github.com/jinleili/bevy-in-web-worker

import init, {
  init_bevy_app,
  is_preparation_completed,
  create_window_by_offscreen_canvas,
  enter_frame_with_mouse,
  left_bt_down,
  left_bt_up,
  set_auto_animation,
  resize,
  mouse_wheel,
  key_down,
  key_up,
  set_panel_viewport,
  despawn_panel,
  release_app,
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
import { CadenceProbe } from "./runtime/cadence_probe";

class IronWorker {
  private probe = new CadenceProbe();
  private appHandle: bigint = BigInt(0);
  private initFinished = 0;
  private isStoppedRunning = false;
  private offscreenCanvas: OffscreenCanvas | null = null;
  private frameIndex = 0;
  private frameCount = 0;
  private frameFlag = 0;
  private streamingEnabled = false;
  private rafId: number | null = null;
  private latestMouseX: number = 0;
  private latestMouseY: number = 0;
  private hasMouseUpdate: boolean = false;
  private postedEnginePrepared: boolean = false;

  constructor() {
    // Create a dedicated object for Rust FFI functions
    const rustBridge = {
      send_pick_from_worker: (pickList: any[]) => this.sendPickFromWorker(pickList),
      send_hover_from_worker: (list: any[]) => this.sendHoverFromWorker(list),
      send_selection_from_worker: (list: any[]) => this.sendSelectionFromWorker(list),
      send_inspector_update_from_worker: (updateJson: string) => this.sendInspectorUpdateFromWorker(updateJson)
    };

    // Make it globally accessible
    (self as any).rustBridge = rustBridge;

    // Expose the functions to the global scope so they're accessible from Wasm
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
          this.appHandle = init_bevy_app(data.variantFlags >>> 0);
          console.log("App handle initialized:", this.appHandle);

          // Notify the main thread that the worker is ready
          self.postMessage({ ty: "workerIsReady" });
          break;

        case "init":
          console.log("creating worker app window (single full-window canvas)");
          this.createWorkerAppWindow(data.canvas, data.devicePixelRatio);
          break;

        case "resize":
          this.canvasResize(data.width, data.height);
          break;

        case "setPanelViewport":
          if (this.appHandle !== BigInt(0)) {
            set_panel_viewport(this.appHandle, data.id, data.kind, data.x, data.y, data.w, data.h);
          }
          break;

        case "despawnPanel":
          if (this.appHandle !== BigInt(0)) {
            despawn_panel(this.appHandle, data.id);
          }
          break;

        case "startRunning":
          if (this.isStoppedRunning) {
            this.isStoppedRunning = false;
            if (this.rafId === null) {
              this.rafId = requestAnimationFrame((dt) => this.enterFrame(dt));
            }
          }
          break;

        case "stopRunning":
          this.isStoppedRunning = true;
          if (this.rafId !== null) {
            cancelAnimationFrame(this.rafId);
            this.rafId = null;
          }
          break;

        case "releaseApp":
          this.isStoppedRunning = true;
          if (this.rafId !== null) {
            cancelAnimationFrame(this.rafId);
            this.rafId = null;
          }
          if (this.appHandle !== BigInt(0)) {
            try { release_app(this.appHandle); } catch (e) { console.error("release_app failed", e); }
            this.appHandle = BigInt(0);
          }
          break;

        case "mousemove":
          // Buffer the latest mouse position; applied at the next frame tick
          this.latestMouseX = data.x;
          this.latestMouseY = data.y;
          this.hasMouseUpdate = true;
          break;

        case "leftBtDown":
          left_bt_down(this.appHandle);
          break;

        case "leftBtUp":
          left_bt_up(this.appHandle);
          break;

        case "mouseWheel":
          if (this.appHandle !== BigInt(0)) {
            mouse_wheel(this.appHandle, data.dx, data.dy, data.mode);
          }
          break;

        case "autoAnimation":
          set_auto_animation(this.appHandle, data.autoAnimation);
          break;

        case "keydown":
          if (this.appHandle !== BigInt(0)) {
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

        case "probeStats":
          self.postMessage({ ty: "probeStats", stats: this.probe.stats() });
          break;

        case "probeReset":
          this.probe.reset();
          break;

        default:
          break;
      }
    };
  }

  private canvasResize(width: number, height: number) {
    if (this.offscreenCanvas) {
      this.offscreenCanvas.width = width;
      this.offscreenCanvas.height = height;
      resize(this.appHandle, width, height);
    }
  }

  private createWorkerAppWindow(offscreenCanvas: OffscreenCanvas, devicePixelRatio: number) {
    this.offscreenCanvas = offscreenCanvas;
    create_window_by_offscreen_canvas(
      this.appHandle,
      offscreenCanvas,
      devicePixelRatio,
      true, // is_in_worker
    );

    // Check ready state
    this.getPreparationState();

    // Start frame loop only if not already running and not stopped
    if (this.rafId === null && !this.isStoppedRunning) {
      this.rafId = requestAnimationFrame((dt) => this.enterFrame(dt));
    }
  }

  private enterFrame(rafTs: number) {
    // Clear the RAF ID since this call was triggered
    this.rafId = null;

    if (this.appHandle === BigInt(0) || this.isStoppedRunning) return;

    // Execute the app's frame loop when ready. Returning from this callback gives
    // the worker event loop a chance to process messages before the next RAF.
    if (this.initFinished > 0) {
      if (
        this.frameIndex >= this.frameFlag ||
        (this.frameIndex < this.frameFlag && this.frameCount % 60 == 0)
      ) {
        const tickStart = performance.now();
        if (this.hasMouseUpdate) {
          enter_frame_with_mouse(this.appHandle, this.latestMouseX, this.latestMouseY, true);
          this.hasMouseUpdate = false;
        } else {
          enter_frame_with_mouse(this.appHandle, 0, 0, false);
        }
        this.probe.record(rafTs, performance.now() - tickStart);
        this.frameIndex++;
      }
      this.frameCount++;
    } else {
      this.getPreparationState();
    }

    if (!this.isStoppedRunning) {
      this.rafId = requestAnimationFrame((dt) => this.enterFrame(dt));
    }
  }

  private getPreparationState() {
    this.initFinished = is_preparation_completed(this.appHandle);
    if (!this.postedEnginePrepared && this.initFinished > 0) {
      this.postedEnginePrepared = true;
      self.postMessage({ ty: "enginePrepared" });
    }
  }

  private sendPickFromWorker(pickList: any[]) {
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
}

// Initialize the worker
new IronWorker();
