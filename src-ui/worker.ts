// from bevy-in-web-worker https://github.com/jinleili/bevy-in-web-worker


// Workers have their own scope, cannot directly access global scope functions/objects, and cannot use ES6 modules.
// importScripts("./bevy_in_web_worker.js");


// Create a dedicated object for Rust FFI functions
const rustBridge = {
  block_from_worker(blockTime?: number) {
    const start = performance.now();
    while (performance.now() - start < (blockTime || renderBlockTime)) { }
  },

  send_pick_from_worker(pickList: any[]) {
    self.postMessage({ ty: "pick", list: pickList });
  }
};

// Make it globally accessible
(self as any).rustBridge = rustBridge;

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
} from "./wasm/ironfell.js";

let appHandle = 0;
let initFinished = 0;
let isStoppedRunning = false;
let renderBlockTime = 1;

async function init_wasm_in_worker() {
  // Load wasm file
  await init("./wasm/ironfell_bg.wasm");

  // Create app
  appHandle = init_bevy_app();

  // Listen for messages from the main thread
  self.onmessage = async (event) => {
    let data = event.data;
    switch (data.ty) {
      case "init":
        let canvas = data.canvas;
        createWorkerAppWindow(canvas, data.devicePixelRatio);
        break;

      case "startRunning":
        if (isStoppedRunning) {
          isStoppedRunning = false;
          // Start the frame loop
          requestAnimationFrame(enterFrame);
        }
        break;

      case "stopRunning":
        isStoppedRunning = true;
        break;

      case "mousemove":
        mouse_move(appHandle, data.x, data.y);
        break;

      case "hover":
        // Set hover (highlight) effect
        set_hover(appHandle, data.list);
        break;

      case "select":
        // Set selection effect
        set_selection(appHandle, data.list);
        break;

      case "leftBtDown":
        left_bt_down(appHandle, data.pickItem, data.x, data.y);
        break;

      case "leftBtUp":
        left_bt_up(appHandle);
        break;

      case "blockRender":
        renderBlockTime = data.blockTime;
        break;

      case "autoAnimation":
        set_auto_animation(appHandle, data.autoAnimation);
        break;

      default:
        break;
    }
  };

  // Notify the main thread that the worker is ready
  self.postMessage({ ty: "workerIsReady" });
}
init_wasm_in_worker();

function createWorkerAppWindow(offscreenCanvas, devicePixelRatio) {
  // Create rendering window
  create_window_by_offscreen_canvas(
    appHandle,
    offscreenCanvas,
    devicePixelRatio
  );

  // Check ready state
  getPreparationState();

  // Start frame loop
  requestAnimationFrame(enterFrame);
}

/**
 * Begin rendering frame
 *
 * https://developer.mozilla.org/en-US/docs/Web/API/DedicatedWorkerGlobalScope/requestAnimationFrame
 * requestAnimationFrame is synchronized with window drawing. Manually limiting the frame rate here 
 * would cause visual jerkiness due to mismatch with window refresh rate
 *
 * TODO: Wait 1 second between the first 3 frames
 */
let frameIndex = 0;
let frameCount = 0;
let frameFlag = 0;

function enterFrame(_dt) {
  if (appHandle === 0 || isStoppedRunning) return;

  // Execute the app's frame loop when ready
  if (initFinished > 0) {
    if (
      frameIndex >= frameFlag ||
      (frameIndex < frameFlag && frameCount % 60 == 0)
    ) {
      enter_frame(appHandle);
      frameIndex++;
    }
    frameCount++;
  } else {
    getPreparationState();
  }
  requestAnimationFrame(enterFrame);
}

/** Get bevy app ready state */
function getPreparationState() {
  initFinished = is_preparation_completed(appHandle);
}

/** Send ray pick results */
function send_pick_from_worker(pickList) {
  self.postMessage({ ty: "pick", list: pickList });
}

/** Execute blocking */
function block_from_worker() {
  const start = performance.now();
  while (performance.now() - start < renderBlockTime) { }
}


// Expose the function to the global scope so it's accessible from Wasm
self.block_from_worker = block_from_worker;

// Similarly, expose send_pick_from_worker
self.send_pick_from_worker = send_pick_from_worker;