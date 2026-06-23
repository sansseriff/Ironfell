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

## Current Test Apps

- `canvas-2d-event`: pointer event updates state and draws immediately.
- `canvas-2d-raf`: pointer events update input state; RAF updates square position and draws.
- `worker-2d`: main thread pointer events are posted to a worker; worker RAF draws to `OffscreenCanvas`.
- `worker-2d-sab`: main thread writes latest pointer state to `SharedArrayBuffer`; worker RAF reads it directly.
- `webgl-raf`: minimal WebGL RAF renderer.
- `webgpu-raf`: minimal WebGPU RAF renderer.
- `worker-webgpu`: WebGPU renderer in a worker using pointer `postMessage`.
- `worker-webgpu-sab`: WebGPU renderer in a worker using `SharedArrayBuffer` pointer state.

The SAB tests use localhost plus Vite COOP/COEP headers. No HTTPS certificate is needed for local development.

## Metric

The main visual latency estimate is:

```txt
estimatedLatencyMs = distance(pointerReference, renderedSquare) / instantaneousPointerSpeed
```

For automated trials, `pointerReference` is the deterministic path position at sample time, not merely the last delivered DOM pointer event. Raw pointer event timestamps and positions are preserved in `timings` so the software pipeline can be inspected separately from the visual trailing estimate.
