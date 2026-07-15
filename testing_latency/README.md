# Iron Latency Testing

Standalone browser latency lab for comparing mouse-to-movement behavior across isolated render paths.

Each app runs on its own page under `/test/:appId`; the runner opens one page, runs one trial, saves JSON, closes the page, then moves to the next app/path. This avoids measuring cross-canvas RAF, worker, GPU, or layout interference.

## Install

```sh
cd testing_latency
npm install
npm run install:browsers
```

## Manual Pages

```sh
npm run dev
```

Open:

- `http://127.0.0.1:5177/test/canvas-2d-event`
- `http://127.0.0.1:5177/test/canvas-2d-raf`
- `http://127.0.0.1:5177/test/worker-2d`
- `http://127.0.0.1:5177/test/worker-2d-sab`
- `http://127.0.0.1:5177/test/webgl-raf`
- `http://127.0.0.1:5177/test/webgpu-raf`
- `http://127.0.0.1:5177/test/worker-webgpu`
- `http://127.0.0.1:5177/test/worker-webgpu-sab`
- `http://127.0.0.1:5177/test/main-raf-immediate-probe`
- `http://127.0.0.1:5177/test/main-raf-buffered-schedule-probe`
- `http://127.0.0.1:5177/test/worker-webgpu-buffered-input-probe`
- `http://127.0.0.1:5177/test/worker-webgpu-buffered-block-probe`
- `http://127.0.0.1:5177/test/worker-webgpu-buffered-vello-cost-probe`
- `http://127.0.0.1:5177/test/rust-winit-main`
- `http://127.0.0.1:5177/test/rust-bevy-main`
- `http://127.0.0.1:5177/test/rust-wgpu-worker`
- `http://127.0.0.1:5177/test/rust-bevy-worker`
- `http://127.0.0.1:5177/test/rust-bevy-vello-worker`

For repeatable hand-driven runs, open:

- `http://127.0.0.1:5177/guided`

The guided runner lets you choose a subset of apps, records one manual trial per selected app, stores every JSON result client-side during the session, asks for platform/browser labels, and saves one package JSON at the end. Browsers with the File System Access API show a save-location picker; other browsers use a normal download.

Guided mode deliberately navigates to a dedicated `/test/:app` URL for every step instead of swapping apps inside one page. This gives each app a fresh JS/Wasm page context, which matters for real `winit` and Bevy web apps where teardown is best-effort rather than a strong isolation guarantee. Step results and skips are persisted in IndexedDB before navigating, so a refresh or browser crash does not discard already saved steps.

## Rust Wasm Apps

The real Rust test pages load wasm-bindgen output from `public/wasm`.

```sh
npm run build:rust
```

To build both the Rust wasm and the Vite app:

```sh
npm run build:all
```

Current status:

- `rust-winit-main` is a compiled Rust/winit wasm app. Winit owns the canvas event loop and Rust draws the draggable shape to the canvas.
- `rust-bevy-main` is a compiled Rust/Bevy wasm app using Bevy's standard web path: `DefaultPlugins`, `WindowPlugin.primary_window.canvas`, Bevy's winit integration, and a 2D sprite square.
- `rust-wgpu-worker`, `rust-bevy-worker`, and `rust-bevy-vello-worker` have reserved wasm entry points and UI wrappers, but intentionally report "not implemented" until their real worker renderers are filled in. They do not fall back to TypeScript probes.

## Automated Trials

```sh
npm run run
```

Useful subsets:

```sh
npm run run -- --apps=canvas-2d-event,canvas-2d-raf,worker-2d --paths=constant-horizontal
npm run run -- --apps=webgl-raf,webgpu-raf --paths=constant-horizontal,sine-horizontal --headed
```

Results are written to `testing_latency/results/*.json`.

## Manual Trials

Open any `/test/...` page, press **Start Manual Trial**, drag the square by hand, press **Stop**, then press **Download Last JSON**.

Manual trials use the latest browser pointer event as the reference point. That is useful for comparing app pipelines under your real hand movement, but it is still not a true physical mouse-to-photon measurement because JavaScript cannot observe the hardware cursor position or display photons.

Manual pages also draw a translucent green main-thread 2D reference square during drag. Exported samples include `relativeErrorPx` and `relativeLatencyMs`, which compare the tested renderer against that immediate reference overlay. For manual analysis, prefer dragging-only relative metrics.

Guided package JSON files can be placed directly in `testing_latency/results` for `npm run analyze`, or in `testing_latency/manual_results` for `npm run analyze:manual`. The analysis scripts expand package `results[]` automatically.

## Current Test Apps

- `canvas-2d-event`: pointer event updates state and draws immediately.
- `canvas-2d-raf`: pointer events update input state; RAF updates square position and draws.
- `worker-2d`: main thread pointer events are posted to a worker; worker RAF draws to `OffscreenCanvas`.
- `worker-2d-sab`: main thread writes latest pointer state to `SharedArrayBuffer`; worker RAF reads it directly.
- `webgl-raf`: minimal WebGL RAF renderer.
- `webgpu-raf`: minimal WebGPU RAF renderer.
- `worker-webgpu`: WebGPU renderer in a worker using pointer `postMessage`.
- `worker-webgpu-sab`: WebGPU renderer in a worker using `SharedArrayBuffer` pointer state.
- `main-raf-immediate-probe`: TypeScript control with main-thread RAF rendering and immediate pointer state updates.
- `main-raf-buffered-schedule-probe`: TypeScript control with next-frame input, schedule cost, and one render-stage delay.
- `worker-webgpu-buffered-input-probe`: TypeScript control with worker WebGPU rendering and pointer moves applied only inside worker RAF.
- `worker-webgpu-buffered-block-probe`: TypeScript control with buffered worker WebGPU plus small per-frame worker blocking.
- `worker-webgpu-buffered-vello-cost-probe`: TypeScript control with buffered worker WebGPU plus heavier render cost and one render-stage delay.
- `rust-winit-main`: compiled Rust/winit main-thread wasm app.
- `rust-bevy-main`: compiled Rust/Bevy main-thread wasm app using the standard Bevy web canvas selector path.
- `rust-wgpu-worker`: reserved compiled Rust/wgpu worker wasm app slot.
- `rust-bevy-worker`: reserved compiled Rust/Bevy worker wasm app slot.
- `rust-bevy-vello-worker`: reserved compiled Rust/Bevy worker wasm app slot using bevy_vello.

The SAB tests use localhost plus Vite COOP/COEP headers. No HTTPS certificate is needed for local development.

## Metric

The main visual latency estimate is:

```txt
estimatedLatencyMs = distance(pointerReference, renderedSquare) / instantaneousPointerSpeed
```

For automated trials, `pointerReference` is the deterministic path position at sample time, not merely the last delivered DOM pointer event. Raw pointer event timestamps and positions are preserved in `timings` so the software pipeline can be inspected separately from the visual trailing estimate.
