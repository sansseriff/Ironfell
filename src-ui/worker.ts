// from bevy-in-web-worker https://github.com/jinleili/bevy-in-web-worker

// The wasm-bindgen glue is loaded dynamically because it is per-target: the
// WebGL2 build carries a shim for every WebGL2 call its GL backend makes and
// is not interchangeable with the WebGPU one. Assigned on "wasmData", before
// any FFI call can occur.
import { loadGlue, type Glue } from "./runtime/glue";
import type { Backend } from "./runtime/backend_policy";
import { CadenceProbe } from "./runtime/cadence_probe";
import { modelAsk, modelEdit } from "./model";

// Assigned before any FFI call; see the "wasmData" case below.
let glue: Glue;

class IronWorker {
  private probe = new CadenceProbe();
  private appHandle: bigint = BigInt(0);
  // Perf-grid variant selector, received with the wasm bytes but not usable
  // until the canvas arrives and the app is actually built.
  private variantFlags: number = 0;
  private backend: Backend = 'webgl2';
  private initFinished = 0;
  private isStoppedRunning = false;
  private offscreenCanvas: OffscreenCanvas | null = null;
  private frameIndex = 0;
  private frameCount = 0;
  private frameFlag = 0;
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
      send_tool_from_worker: (tool: string) => self.postMessage({ ty: "tool", tool }),
      send_convert_from_worker: (question: string) => self.postMessage({ ty: "convertQuestion", question: JSON.parse(question) }),
      send_cursor_from_worker: (cursor: string) => self.postMessage({ ty: "cursor", cursor }),
      send_document_changed_from_worker: (version: number) => self.postMessage({ ty: "documentChanged", version }),
    };

    // Make it globally accessible
    (self as any).rustBridge = rustBridge;

    // Expose the functions to the global scope so they're accessible from Wasm
    (self as any).send_pick_from_worker = (pickList: any[]) => this.sendPickFromWorker(pickList);
    (self as any).send_hover_from_worker = (list: any[]) => this.sendHoverFromWorker(list);
    (self as any).send_selection_from_worker = (list: any[]) => this.sendSelectionFromWorker(list);

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
          // The backend travels with the bytes: the glue and the wasm must be
          // the same build, so the decision is made once on the main thread.
          this.backend = data.backend as Backend;
          glue = await loadGlue(this.backend);
          await glue.default(data.wasmData);
          console.log(`WASM module initialized (${this.backend})`);
          // The app is built later, in "init": constructing it needs the canvas,
          // because the renderer picks its GPU adapter from the canvas's context.
          this.variantFlags = data.variantFlags >>> 0;

          // Notify the main thread that the worker is ready
          self.postMessage({ ty: "workerIsReady" });
          break;

        case "init":
          console.log("creating worker app window (single full-window canvas)");
          this.createWorkerAppWindow(data.canvas, data.devicePixelRatio);
          break;

        case "resize":
          if (typeof data.dpr === "number" && this.appHandle !== BigInt(0)) glue.set_scale_factor(this.appHandle, data.dpr);
        this.canvasResize(data.width, data.height);
          break;

        case "setPanelViewport":
          if (this.appHandle !== BigInt(0)) {
            glue.set_panel_viewport(this.appHandle, data.id, data.kind, data.composition ?? "", data.x, data.y, data.w, data.h);
          }
          break;

        case "despawnPanel":
          if (this.appHandle !== BigInt(0)) {
            glue.despawn_panel(this.appHandle, data.id);
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
            try { glue.release_app(this.appHandle); } catch (e) { console.error("release_app failed", e); }
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
          // The press lands where it happened, not at the last buffered move.
          if (typeof data.x === "number") glue.left_bt_down_at(this.appHandle, data.x, data.y);
          else glue.left_bt_down(this.appHandle);
          break;

        case "modifiers":
          if (this.appHandle !== BigInt(0)) glue.set_modifiers(this.appHandle, data.m);
          break;

        case "toolKey":
          if (this.appHandle !== BigInt(0)) glue.tool_key(this.appHandle, data.key, data.m);
          break;

        case "theme":
          if (this.appHandle !== BigInt(0)) glue.set_dark_mode(this.appHandle, !!data.dark);
          break;

        case "middleBt":
          if (this.appHandle !== BigInt(0)) glue.middle_bt(this.appHandle, !!data.down);
          break;

        case "leftBtUp":
          glue.left_bt_up(this.appHandle);
          break;

        case "mouseWheel":
          if (this.appHandle !== BigInt(0)) {
            glue.mouse_wheel(this.appHandle, data.dx, data.dy, data.mode);
          }
          break;

        case "autoAnimation":
          glue.set_auto_animation(this.appHandle, data.autoAnimation);
          break;

        case "keydown":
          if (this.appHandle !== BigInt(0)) {
            glue.key_down(this.appHandle, data.key);
          }
          break;

        case "keyup":
          if (this.appHandle !== BigInt(0)) {
            glue.key_up(this.appHandle, data.key);
          }
          break;

        // Editor history: the shell decided the shortcut; only the command arrives.
        case "undo":
          if (this.appHandle !== BigInt(0)) glue.undo(this.appHandle);
          break;

        case "redo":
          if (this.appHandle !== BigInt(0)) glue.redo(this.appHandle);
          break;

        // Document: save, load, view. Each answers with the caller's requestId.
        case "documentSave":
          self.postMessage({ ty: "documentSaved", requestId: data.requestId, text: glue.document_save(this.appHandle) });
          break;

        case "solveBench":
          self.postMessage({ ty: "solveBench", requestId: data.requestId, text: glue.solve_bench(data.iterations, data.repeats) });
          break;

        case "documentLoad":
          try {
            glue.document_load(this.appHandle, data.text);
            self.postMessage({ ty: "documentLoaded", requestId: data.requestId, ok: true });
          } catch (e) {
            self.postMessage({ ty: "documentLoaded", requestId: data.requestId, ok: false, error: String(e) });
          }
          break;

        // Document calls that answer with JSON (link, counterparts, focus):
        // `document_<fn>(handle, ...args)`.
        case "documentCall":
          try {
            const text = (glue as any)[`document_${data.fn}`](this.appHandle, ...data.args);
            self.postMessage({ ty: "documentCall", requestId: data.requestId, ok: true, text });
          } catch (e) {
            self.postMessage({ ty: "documentCall", requestId: data.requestId, ok: false, error: String(e) });
          }
          break;

        case "documentView":
          try {
            const text = glue.document_view(this.appHandle, JSON.stringify(data.query));
            self.postMessage({ ty: "documentView", requestId: data.requestId, ok: true, text });
          } catch (e) {
            self.postMessage({ ty: "documentView", requestId: data.requestId, ok: false, error: String(e) });
          }
          break;

        // Model surface: apply an <edit> as the model, or ask a model for one.
        case "modelEdit":
          try {
            const outcome = modelEdit(glue, this.appHandle, data.xml, data.label);
            self.postMessage({ ty: "modelEditResult", requestId: data.requestId, ok: outcome.ok, outcome });
          } catch (e) {
            self.postMessage({ ty: "modelEditResult", requestId: data.requestId, ok: false, error: String(e) });
          }
          break;

        case "modelAsk":
          modelAsk(glue, this.appHandle, data.instruction, { apiKey: data.apiKey, model: data.model })
            .then((result) => self.postMessage({ ty: "modelAskResult", requestId: data.requestId, ok: result.ok, result }))
            .catch((e) => self.postMessage({ ty: "modelAskResult", requestId: data.requestId, ok: false, error: String(e) }));
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
      glue.resize(this.appHandle, width, height);
    }
  }

  private createWorkerAppWindow(offscreenCanvas: OffscreenCanvas, devicePixelRatio: number) {
    this.offscreenCanvas = offscreenCanvas;
    this.appHandle = glue.init_bevy_app_with_canvas(
      offscreenCanvas,
      devicePixelRatio,
      true, // is_in_worker
      this.variantFlags,
    );
    console.log("App handle initialized:", this.appHandle);

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
          glue.enter_frame_with_mouse(this.appHandle, this.latestMouseX, this.latestMouseY, true);
          this.hasMouseUpdate = false;
        } else {
          glue.enter_frame_with_mouse(this.appHandle, 0, 0, false);
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
    this.initFinished = glue.is_preparation_completed(this.appHandle);
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

}

// Initialize the worker
new IronWorker();
