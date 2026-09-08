# A native target via Tauri

**Status: FOR LATER — deferred, not scheduled.** Recorded so the decision and its
preconditions are not re-derived from scratch. Nothing in this document should be
started before the damage-bounded raster cache described in
[`07_current-2d-backends-and-hybrid-architecture.md`](07_current-2d-backends-and-hybrid-architecture.md).

**Date:** 2026-09-06

---

## 1. The shape of it

A third compile target alongside the two web ones: Bevy runs **natively** — real
wgpu backend, full multithreading, no wasm size budget — while the Svelte UI is
hosted in a Tauri **webview**. The two are composited into one apparent window.

The communication architecture is what actually changes. Today the frontend talks
to a worker that hosts Bevy, over `postMessage`. Natively that channel becomes a
Tauri [Channel](https://v2.tauri.app/develop/calling-frontend/#channels), and Bevy
is a native process rather than a wasm module in a worker.

## 2. Why this is deferred, not merely unscheduled

1. **The cache pays off on three targets; this pays off on one.** `07` puts the
   next real win in damage-bounded retained rasterization. That work is renderer-
   and platform-agnostic. Native is one target's constant factor.
2. **Native removes the constraints that are currently shaping the design** —
   wasm size, single-threading, WebGL2's lack of compute and storage buffers.
   Designing against the loosest target first risks choices that do not survive
   the other two. The caching architecture survives everywhere.
3. **The blocking problem is upstream and unsolved.** See below. Time spent here
   is time spent debugging window compositing, not building the app.

## 3. What is proven, and what is not

Proven: the concept works.
[`langyo/native-bevy-with-tauri-hud-demo`](https://github.com/langyo/native-bevy-with-tauri-hud-demo)
stacks a transparent Tauri window over Bevy's own rendering window in real time.

Not solved: **window composition**, which is the whole difficulty.

- [tauri#12450](https://github.com/tauri-apps/tauri/issues/12450) — `transparent`
  on a child window does not take effect, which defeats the overlay approach.
- [tauri#3662](https://github.com/tauri-apps/tauri/issues/3662) — rendering a
  winit subwindow inside a Tauri app is still an open feature request.
- [tauri discussion #11944](https://github.com/orgs/tauri-apps/discussions/11944) —
  rendering wgpu frames as a webview overlay.

The current workaround is two stacked OS windows: overlay topmost, transparent,
with click-through passthrough so input reaches the right surface. That is
platform-specific and fragile.

The low-effort alternative — running the existing wasm build inside Tauri's
webview — is not worth doing. It keeps every web constraint and discards every
native benefit.

## 4. What this codebase would actually have to change

The seams already exist. This is the main reason the task is *deferrable* rather
than *urgent*: waiting does not make it harder.

| Concern | Today | Native |
|---|---|---|
| Transport | `postMessage` to a worker | Tauri `Channel` |
| Transport abstraction | `RuntimeMode = 'worker' \| 'main-thread'` in `src-ui/runtime/session_adapter.ts` | a third adapter |
| Surface | `ViewObj::Canvas \| Offscreen` in `src/canvas_view/mod.rs` | a third variant: a winit window |
| FFI | `src/web_ffi.rs`, thin `wasm_bindgen` over `init_app` | a parallel Tauri command layer over the same core |
| Threading | single-threaded wasm | full Bevy multithreading |

The one discipline worth keeping *now*, at no cost: **keep the FFI boundary
narrow and transport-agnostic.** `web_ffi.rs` should stay a thin shell over
`init_app` / `WorkerApp` and must not accumulate logic that belongs in the core.
Every line that leaks across that boundary is a line the native target has to
reimplement.

## 5. Preconditions for revisiting

Attempt a timeboxed spike only once all of these hold:

- The raster cache from `07` has landed and been measured.
- Both web targets build from a single branch via Cargo features (see the
  dual-target work), so a third target is an addition rather than a fork.
- tauri#12450 and tauri#3662 have moved, **or** the two-stacked-windows
  workaround has been validated on the platforms actually being shipped to.

Expected shape of the spike: prove window composition and input routing on one
platform first. If that fails, nothing else about the port matters — so it should
be the first thing tried, not the last.
