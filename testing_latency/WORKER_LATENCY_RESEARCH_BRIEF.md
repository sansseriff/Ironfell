# Worker Latency Drift Research Brief

## Goal

Research a recurring latency pattern seen in the `testing_latency` worker-based apps: while dragging the test square, the worker-rendered square appears to drift in and out of a low-latency state every few seconds. Visually it feels beat-frequency-like: the worker output periodically lines up closely with the event-based 2D canvas reference, then falls behind again, then comes back.

The question is whether there is known browser/platform behavior that could explain this, especially around worker `requestAnimationFrame`, `OffscreenCanvas`, WebGPU presentation, worker/main-thread scheduling, or synchronization between the main-thread event loop and a worker render loop.

## Observed Symptom

- Applies primarily to worker variants.
- The effect appears periodic over multiple seconds, not just random jitter.
- It is visible while manually dragging the square.
- The event-based 2D canvas reference remains comparatively low latency and stable.
- The worker-rendered square sometimes gets close to the reference square, then gradually drifts away, then returns.
- This suggests a possible phase relationship between:
  - main-thread pointer event delivery,
  - main-thread reference overlay rendering/sampling,
  - worker `requestAnimationFrame`,
  - OffscreenCanvas/WebGPU presentation,
  - display vsync/compositor timing.

Treat this as a hypothesis, not a conclusion.

## Repo Context

This is in `testing_latency/` of the repo.

Important app IDs:

- `canvas-2d-event`: low-latency reference path.
- `worker-2d`: main thread posts pointer events to a worker; worker draws 2D canvas from worker RAF.
- `worker-2d-sab`: pointer state is shared via `SharedArrayBuffer`; worker draws from worker RAF.
- `worker-webgpu`: main thread posts every pointer event to a worker WebGPU renderer.
- `worker-webgpu-sab`: pointer state is shared via `SharedArrayBuffer`; worker WebGPU renderer reads latest state during worker RAF.
- `worker-webgpu-buffered-*`: TypeScript probe variants that intentionally buffer input or add render-stage delay/cost.
- `rust-wgpu-worker`: compiled Rust/WASM worker using `wgpu` with an `OffscreenCanvas` WebGPU surface.
- `rust-bevy-worker` and `rust-bevy-vello-worker`: planned, not implemented yet at time of writing.

Useful files:

- `testing_latency/src/apps/rust-wasm-app.ts`
- `testing_latency/src/workers/rust-wasm-worker.ts`
- `testing_latency/rust/src/lib.rs`
- `testing_latency/src/workers/worker-webgpu.ts`
- `testing_latency/src/workers/worker-webgpu-sab.ts`
- `testing_latency/src/workers/worker-webgpu-buffered.ts`
- `testing_latency/src/workers/worker2d.ts`
- `testing_latency/src/workers/worker2d-sab.ts`
- `testing_latency/src/shared/recorder.ts`
- `testing_latency/src/shared/reference-overlay.ts`
- `testing_latency/src/shared/protocol.ts`

## Current Worker Architecture

The test page owns an HTML canvas. Worker apps call `transferControlToOffscreen()` and send the resulting `OffscreenCanvas` to a module worker.

For non-SAB worker variants:

1. Main thread receives `pointerdown`, `pointermove`, and `pointerup`.
2. Main thread records pointer timing.
3. Main thread posts pointer messages to the worker.
4. Worker stores latest pointer/drag state.
5. Worker RAF draws the current rendered square.
6. Worker posts a `rendered` message back to the main thread.
7. Main thread records a latency sample when that rendered message arrives.

For SAB variants:

1. Main thread receives pointer events.
2. Main thread writes latest pointer state into a `SharedArrayBuffer`.
3. Worker RAF reads the latest shared pointer state.
4. Worker renders and posts `rendered`.

For `rust-wgpu-worker`:

1. Main thread creates a worker from `src/workers/rust-wasm-worker.ts`.
2. Worker dynamically imports `/wasm/latency_rust.js`.
3. Worker calls the Rust export `start_wgpu_worker(canvas, options, onFrame)`.
4. Rust creates a `wgpu::Surface` from `wgpu::SurfaceTarget::OffscreenCanvas`.
5. Rust installs a worker `requestAnimationFrame` loop through `wasm_bindgen`.
6. Rust updates drag state, draws WebGPU rectangles, presents the frame, then invokes the JS callback.
7. Worker posts the frame sample back to the main thread.

The Rust worker implementation is in `testing_latency/rust/src/lib.rs`, module `rust_wgpu_worker`.

## Reference Overlay

All apps are compared against an event-based 2D canvas reference overlay. The reference square is rendered directly from pointer events on the main thread. The underlying app square is slightly larger than the overlay square, so if the two paths perfectly align it looks like a square with a border.

This matters because the visible drift is judged against this event-based reference, not against a separate hardware measurement.

## Important Measurement Detail

The harness samples worker-rendered latency when the worker posts a `rendered` message back to the main thread. That means measured latency includes at least:

- pointer event arrival on main thread,
- main-to-worker communication or SAB read timing,
- worker RAF timing,
- render work,
- WebGPU/OffscreenCanvas present scheduling,
- worker-to-main `postMessage`,
- main-thread handling of the rendered callback.

The visual experience may also include browser compositor timing that is not fully captured by the `rendered` callback timestamp.

## Research Questions

Please search for evidence or explanations for these issues:

1. Are worker `requestAnimationFrame` callbacks guaranteed to be phase-aligned with main-thread `requestAnimationFrame` for the same document?
2. Is `DedicatedWorkerGlobalScope.requestAnimationFrame()` synchronized to the owning document's refresh driver, compositor, or a separate scheduler?
3. Do browsers throttle, batch, or jitter worker RAF callbacks differently from main-thread RAF?
4. Does `OffscreenCanvas` presentation from a worker introduce an extra compositor frame, buffering stage, or variable queue depth?
5. For WebGPU on `OffscreenCanvas`, can `queue.submit()` plus `surface.present()` complete on a cadence that is not exactly locked to main-thread RAF?
6. Is there known Chrome/Safari/Firefox behavior where worker animation drifts relative to input events or main-thread animation?
7. Is there browser-specific behavior for coalesced pointer events, event timestamps, or dispatch cadence that could produce a slow beat with a worker render loop?
8. Does `postMessage` between main and worker introduce periodic batching or scheduling artifacts under animation load?
9. Does using `SharedArrayBuffer` remove message latency but still leave worker RAF phase drift?
10. Are there known differences between 2D OffscreenCanvas and WebGPU OffscreenCanvas presentation latency?

## Search Terms To Try

- `OffscreenCanvas worker requestAnimationFrame latency drift`
- `DedicatedWorkerGlobalScope requestAnimationFrame phase aligned main thread`
- `worker requestAnimationFrame vs main thread requestAnimationFrame synchronization`
- `OffscreenCanvas presentation latency worker`
- `WebGPU OffscreenCanvas worker latency`
- `WebGPU canvas present worker vsync`
- `requestAnimationFrame worker compositor thread timing`
- `Chrome OffscreenCanvas worker RAF latency`
- `Safari OffscreenCanvas worker requestAnimationFrame latency`
- `pointer events worker OffscreenCanvas latency`
- `SharedArrayBuffer pointer input worker requestAnimationFrame latency`
- `OffscreenCanvas double buffering latency`
- `WebGPU surface present latency browser`

## Sources To Prioritize

Prefer primary or near-primary sources:

- MDN pages for `DedicatedWorkerGlobalScope.requestAnimationFrame`, `OffscreenCanvas`, pointer events, and `requestAnimationFrame`.
- Chrome/Chromium docs and design discussions.
- Chromium bugs or issues mentioning worker RAF, OffscreenCanvas, WebGPU, compositor, or input latency.
- W3C specs or issues:
  - HTML event loop and animation frame timing.
  - OffscreenCanvas spec/issues.
  - WebGPU spec/issues around canvas configuration and presentation.
  - Pointer Events spec/issues around event timestamp/coalescing.
- WebKit bugs for Safari behavior.
- Mozilla bugs or standards-position notes for worker RAF / OffscreenCanvas.
- wgpu issues involving web worker, OffscreenCanvas, WebGPU surface presentation, or latency.

Secondary sources like blog posts are useful only if they point to a browser/spec issue or contain reproducible experiments.

## Things To Look For In Findings

For each relevant source, capture:

- Browser and version/platform, if stated.
- Whether it is about 2D canvas, WebGL, WebGPU, or generic OffscreenCanvas.
- Whether it discusses worker RAF timing, compositor presentation, input event timing, or message passing.
- Whether the behavior is specified, browser-specific, or implementation-defined.
- Any mitigation:
  - move input handling into worker,
  - use SAB polling,
  - render on main thread,
  - align sampling to main RAF,
  - avoid worker RAF,
  - use `getCoalescedEvents()`,
  - use predicted events,
  - limit buffering / configure presentation,
  - force latest-input-at-render-time behavior.

## Local Experiments Suggested By The Research

If sources suggest worker/main RAF phase drift is plausible, add probes that record:

- main-thread RAF timestamp every frame,
- worker RAF timestamp every frame,
- delta between main and worker RAF timestamps over time,
- pointer event timestamp vs worker RAF timestamp,
- worker render completion message time,
- optional WebGPU submit/present timing markers if available.

Potential app variants:

- Worker render driven by main-thread RAF messages instead of worker RAF.
- Worker render driven by `setTimeout(0)` or fixed timer as a control.
- SAB input plus worker RAF, already partly present.
- SAB input plus main-thread-driven worker render.
- Worker draws every RAF but only samples on main-thread RAF.
- Main-thread WebGPU with identical render work to isolate OffscreenCanvas worker effects.

## Current Working Hypotheses

These are not confirmed:

- Worker RAF and main-thread RAF may be vsync-aligned but not phase-locked in a way that preserves consistent pointer-to-present latency.
- Worker OffscreenCanvas presentation may have a variable queue depth or compositor handoff that periodically changes apparent latency.
- Pointer event cadence and worker RAF cadence may be close but not identical, producing a slow beat in the relative timing between latest pointer input and worker frame render.
- `postMessage` round trips may add periodic scheduling noise, but if SAB variants show the same drift, the main cause is likely not pointer message delivery alone.
- WebGPU `OffscreenCanvas` may add presentation behavior different from 2D OffscreenCanvas, but the 2D worker variants should be checked as a control.

