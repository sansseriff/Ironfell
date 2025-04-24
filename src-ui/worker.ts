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

  constructor() {
    // Create a dedicated object for Rust FFI functions
    const rustBridge = {
      block_from_worker: (blockTime?: number) => this.blockFromWorker(blockTime),
      send_pick_from_worker: (pickList: any[]) => this.sendPickFromWorker(pickList)
    };

    // Make it globally accessible
    (self as any).rustBridge = rustBridge;

    // Expose the functions to the global scope so they're accessible from Wasm
    self.block_from_worker = (blockTime?: number) => this.blockFromWorker(blockTime);
    self.send_pick_from_worker = (pickList: any[]) => this.sendPickFromWorker(pickList);

    // Initialize the worker
    this.initWasmInWorker();
  }

  private async initWasmInWorker() {
    // Load wasm file
    // await init("./wasm/ironfell_bg.wasm");
    // await init(wasmUrl);

    // Create app


    // Listen for messages from the main thread
    self.onmessage = async (event) => {
      let data = event.data;
      switch (data.ty) {


        // case "dummy":
        //   // Handle dummy message
        //   console.log("Received dummy message from main thread");
        //   break

        case "wasmUrl":
          // Initialize the wasm module with the provided URL

          // console.log("The received wasmUrl is:", data.wasmUrl);
          await init(data.wasmUrl);
          // console.log("init computed for wasm module");
          this.appHandle = init_bevy_app();
          // console.log("App handle initialized:", this.appHandle);

          // Notify the main thread that the worker is ready
          // console.log("Sending workerIsReady message");
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
    if (this.appHandle === 0 || this.isStoppedRunning) return;

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

  private blockFromWorker(blockTime?: number) {
    const start = performance.now();
    while (performance.now() - start < (blockTime || this.renderBlockTime)) { }
  }
}

// Initialize the worker
new IronWorker();