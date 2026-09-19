// Main-thread adapter: same message protocol as worker.ts, but every message is a
// direct synchronous wasm call (no postMessage hop) — minimal input latency.
// The wasm-bindgen glue is loaded dynamically because it is per-target: the
// WebGL2 build carries a shim for every WebGL2 call its GL backend makes and
// is not interchangeable with the WebGPU one. Assigned on "wasmData", before
// any FFI call can occur.
import { loadGlue, type Glue } from "./runtime/glue";
import type { Backend } from "./runtime/backend_policy";
import { CadenceProbe } from "./runtime/cadence_probe";

// Assigned before any FFI call; see the "wasmData" case below.
let glue: Glue;

export class MainThreadAdapter {
  private probe = new CadenceProbe();
  private appHandle: bigint = BigInt(0);
  // Perf-grid variant selector, received with the wasm bytes but not usable
  // until the canvas arrives and the app is actually built.
  private variantFlags: number = 0;
  private backend: Backend = 'webgl2';
  private initFinished = 0;
  private isStoppedRunning = false;
  private canvas: HTMLCanvasElement | null = null;
  private frameIndex = 0;
  private frameCount = 0;
  private frameFlag = 0;
  private messageHandler: ((event: any) => void) | null = null;
  private rafId: number | null = null;
  private postedEnginePrepared: boolean = false;
  private disposed = false;

  constructor() {
    // Create a dedicated object for Rust FFI functions
    const rustBridge = {
      send_pick_from_worker: (pickList: any[]) => this.sendPickFromWorker(pickList),
      send_hover_from_worker: (list: any[]) => this.sendHoverFromWorker(list),
      send_selection_from_worker: (list: any[]) => this.sendSelectionFromWorker(list),
      send_document_changed_from_worker: (version: number) => this.sendMessage({ ty: "documentChanged", version }),
    };

    // Make it globally accessible
    (window as any).rustBridge = rustBridge;

    // Expose the functions to the global scope so they're accessible from Wasm.
    (window as any).send_pick_from_worker = (pickList: any[]) => this.sendPickFromWorker(pickList);
    (window as any).send_hover_from_worker = (list: any[]) => this.sendHoverFromWorker(list);
    (window as any).send_selection_from_worker = (list: any[]) => this.sendSelectionFromWorker(list);
  }

  // Simulate worker's onmessage interface
  set onmessage(handler: (event: any) => void) {
    this.messageHandler = handler;
  }

  // Simulate worker's postMessage interface
  async postMessage(data: any, _transfer?: any[]) {
    if (this.disposed) return;
    switch (data.ty) {
      case "wasmData":
        console.log("Received WASM data (main thread), initializing...");
        // The backend travels with the bytes: the glue and the wasm must be the
        // same build, so the decision is made once by the caller.
        this.backend = data.backend as Backend;
        glue = await loadGlue(this.backend);
        await glue.default(data.wasmData);
        console.log(`WASM module initialized (${this.backend})`);
        // The app is built later, in "init": constructing it needs the canvas,
        // because the renderer picks its GPU adapter from the canvas's context.
        this.variantFlags = data.variantFlags >>> 0;
        this.sendMessage({ ty: "workerIsReady" });
        break;

      case "init":
        console.log("creating main thread app window (single full-window canvas)");
        this.createAppWindow(data.canvas, data.devicePixelRatio);
        break;

      case "resize":
        this.canvasResize(data.width, data.height);
        break;

      case "setPanelViewport":
        if (this.appHandle !== BigInt(0)) {
          glue.set_panel_viewport(this.appHandle, data.id, data.kind, data.x, data.y, data.w, data.h);
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
        this.releaseApp();
        break;

      case "mousemove":
        // Direct synchronous call — the whole point of main-thread mode.
        if (this.appHandle !== BigInt(0)) {
          glue.mouse_move(this.appHandle, data.x, data.y);
        }
        break;

      case "leftBtDown":
        if (this.appHandle !== BigInt(0)) {
          glue.left_bt_down(this.appHandle);
        }
        break;

      case "leftBtUp":
        if (this.appHandle !== BigInt(0)) {
          glue.left_bt_up(this.appHandle);
        }
        break;

      case "mouseWheel":
        if (this.appHandle !== BigInt(0)) {
          glue.mouse_wheel(this.appHandle, data.dx, data.dy, data.mode);
        }
        break;

      case "autoAnimation":
        if (this.appHandle !== BigInt(0)) {
          glue.set_auto_animation(this.appHandle, data.autoAnimation);
        }
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
        if (this.appHandle !== BigInt(0)) {
          this.sendMessage({ ty: "documentSaved", requestId: data.requestId, text: glue.document_save(this.appHandle) });
        }
        break;

      case "documentLoad":
        if (this.appHandle !== BigInt(0)) {
          try {
            glue.document_load(this.appHandle, data.text);
            this.sendMessage({ ty: "documentLoaded", requestId: data.requestId, ok: true });
          } catch (e) {
            this.sendMessage({ ty: "documentLoaded", requestId: data.requestId, ok: false, error: String(e) });
          }
        }
        break;

      case "documentView":
        if (this.appHandle !== BigInt(0)) {
          try {
            const text = glue.document_view(this.appHandle, JSON.stringify(data.query));
            this.sendMessage({ ty: "documentView", requestId: data.requestId, ok: true, text });
          } catch (e) {
            this.sendMessage({ ty: "documentView", requestId: data.requestId, ok: false, error: String(e) });
          }
        }
        break;

      case "probeStats":
        this.sendMessage({ ty: "probeStats", stats: this.probe.stats() });
        break;

      case "probeReset":
        this.probe.reset();
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

  private canvasResize(width: number, height: number) {
    if (this.canvas && this.appHandle !== BigInt(0)) {
      this.canvas.width = width;
      this.canvas.height = height;
      glue.resize(this.appHandle, width, height);
    }
  }

  private createAppWindow(canvas: HTMLCanvasElement, devicePixelRatio: number) {
    this.canvas = canvas;

    // The wasm entry point takes an OffscreenCanvas; the HTML canvas is structurally
    // compatible for surface creation, as before.
    this.appHandle = glue.init_bevy_app_with_canvas(
      canvas as any,
      devicePixelRatio,
      false, // is_in_worker
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
    this.rafId = null;

    if (this.appHandle === BigInt(0) || this.isStoppedRunning) return;

    // Execute the app's frame loop when ready.
    // Mouse events were already applied synchronously as they arrived.
    if (this.initFinished > 0) {
      if (
        this.frameIndex >= this.frameFlag ||
        (this.frameIndex < this.frameFlag && this.frameCount % 60 == 0)
      ) {
        const tickStart = performance.now();
        glue.enter_frame(this.appHandle);
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
      this.sendMessage({ ty: "enginePrepared" });
    }
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

  private releaseApp() {
    this.isStoppedRunning = true;
    if (this.rafId !== null) {
      cancelAnimationFrame(this.rafId);
      this.rafId = null;
    }
    if (this.appHandle !== BigInt(0)) {
      try { glue.release_app(this.appHandle); } catch (e) { console.error("release_app failed", e); }
      this.appHandle = BigInt(0);
    }
  }

  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    this.releaseApp();
    this.canvas = null;
    this.messageHandler = null;
  }
}
