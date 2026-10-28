# Native frame pacing: where the latency is, and what we learned

**Status:** findings, 2026-10-27. macOS native (Tauri) build only. The web build cannot
do any of this: the browser schedules canvas presentation and exposes no vsync times,
drawables, GPU completion or compositor timing. This document is the synthesis; the
dated, step-by-step record (methods, every run, verification) is in
[vision 08](../vision/08_native-target-via-tauri.md) under "Native latency controls",
"Vsync-timed presentation", "GPU-aware budget" and "Adaptive pacing", with probe tables
in [`src/iron_latency_probe/README.md`](../../src/iron_latency_probe/README.md).

**Current recommendation:** `bun run native:adaptive`. Low latency (≈21–24 ms present
delay) while the user drags; stock Bevy pacing (≈45 ms, but spike-proof) otherwise.

All numbers: Apple M5, macOS 27.2, Bevy 0.20, wgpu 30.0.1, dev-opt, 60 Hz LG displays
(6016×3384 then 3840×2160 backing), 2560×1636 window, unless stated.

## 1. How a frame reaches the screen (macOS, Metal, windowed)

```
input event ─► AppKit queue ─► main thread reads input (Update)
   ─► extract/prepare/encode ─► nextDrawable (may block) ─► submit ─► GPU work
   ─► presentDrawable ─► FIFO: waits for the compositor's next pickup
   ─► WindowServer composites (pickup ≈ 3–4 ms before vsync)
   ─► scanout during the following refresh ─► panel
```

- With display sync on (FIFO), a frame presented *just before* a pickup reaches glass
  about one refresh after that vsync. **Floor: ≈ 20 ms present delay at 60 Hz**; the best
  measured here was 21–24 ms. Below that needs 120 Hz, fullscreen/direct, or tearing.
- Metal HUD "Present Delay" is `presentDrawable` → on glass. It excludes input delivery
  and the work before present; CPU-side "update → present return" timings (the first
  experiments' 18 → 4 ms) say nothing about displayed latency, which is why that change
  felt no different in OBS recordings and drags.
- External reference (a Qt app, keypress → pixel, 60 Hz): 16.4 ms average when its
  binary is run directly, 29.7 ms when launched through LaunchServices (`open`), one
  frame more, unexplained ([Apple forum 841189](https://developer.apple.com/forums/thread/841189),
  [write-up](https://peeknext.com/research/investigating-macos-latency/)). The packaged
  Tauri app launches through LaunchServices; not yet measured here.

## 2. Why stock Bevy is ≈ 48 ms (major sources, largest first)

1. **Scheduling, not vsync.** Stock renders the moment a drawable frees, which is just
   after a vsync; the finished frame then waits almost a whole refresh for the next
   pickup, and with three drawables the CPU can queue one or two frames ahead. HUD
   present delay ≈ 48 ms (standalone probe), 45–61 ms in the host depending on scene.
2. **Input arrives late.** winit/Bevy's main thread blocks in `nextDrawable`, so AppKit
   events queue behind it: event age when Update reads it was p50 14.7 ms (real mouse),
   ≈ 32 ms with synthetic events. Fullscreen or paced modes roughly halved it.
3. **Pipelined rendering adds a frame** of simulation-to-render lag (sync render
   removes it; Bevy cannot toggle it at runtime).
4. The compositor itself costs about one refresh — unavoidable in a window with vsync.

Not causes: WKWebView overlay (detached vs overlaid: no measurable present-delay
difference under `late-fifo`, 23.8 vs 23.1 ms), Vello (sprite and Vello reference show
the same pointer sample), drawable count alone.

## 3. What helped

| Change | Effect | Why |
|---|---|---|
| Start each frame just in time for a vsync (`late-fifo`: CVDisplayLink grid, present ≈ 4–5 ms before vsync) | probe 48 → 22–24 ms; host 61 → 23 ms | removes the post-render wait and the queue |
| Vsync times from **CVDisplayLink** (own thread) | 55 → 60 presents/s, no doubled frames | main-thread `CADisplayLink` timestamps jump by part of a period when the main thread is busy; `targetTimestamp` drifts against the grid |
| **Backlog drain** from `MTLDrawable.presentedTime` | stopped a 30 → 47 ms ratchet | at one present per refresh, one late frame leaves every later one queued forever; skip one refresh to drain |
| **GPU-aware budget**: Metal `GPUEndTime` per command buffer (queue's `commandBuffer*` wrapped), excluding wgpu's present-only buffer | light 3D: 31 → 1 repeated frames per ~1700, 23.3 → 21.1 ms | misses were on-time CPU presents whose GPU work finished after the pickup |
| Start now when the display callback is only slightly late (≤ half the reserve, ≤ 4 ms) | planned skips → 0 | otherwise a whole refresh is given up |
| Degrade to callback starts when work + margin > 90% of a refresh | heavy (CPU-bound) scene 26.9 ms vs stock 43.5 ms, 2 repeats / 2160 | a start later than the next callback gets cancelled |
| **Adaptive**: just-in-time only while dragging (left button or any document gesture), stock otherwise | drags 20–24 ms; panning/flying keep stock smoothness | the queue absorbs spikes; draining it at mousedown is invisible |

## 4. What did not help, or was rejected

| Tried | Result | Lesson |
|---|---|---|
| Frame latency 1 (two drawables) alone | 47.8 ms, unchanged | queue depth was not the cause; timing is |
| Sync render alone | 64.5 ms (worse) | without timing, the CPU just queues earlier |
| Display sync off (`Immediate`) | 5–7 ms, **heavy tearing** | it flips straight to the display mid-scan: glass ≈ 4.6 ms after present at any phase. Also not a fixed vsync grid to align to |
| Immediate, paced to one frame per refresh | 6.6–7.9 ms, still tears | same flip; pacing only saves power |
| `CAMetalDisplayLink`, frame latency 1 (its drawable fed to wgpu) | ≈ 35 ms present→glass; latency grew in use | Apple sets the deadline two refreshes before presentation. Mixing the layer's own `nextDrawable` with an attached link raises an Objective-C exception |
| Fullscreen at 6K | stock 31 ms (−17), but low/sync drops to 30 fps | bypasses compositing; the app then renders 6016×3384 |
| Wider fixed margins (7–8 ms) | +2–3 ms, misses only roughly halved | GPU timing, not margin, was the gap |
| Bevy `Time` from the planned vsync instead of wall clock | no measurable difference (> 98% of steps already matched) | kept for `late-fifo`; harmless |
| Old `late` mode (CADisplayLink target, CPU budget, 2 ms margin) | 36.5 ms | margin too tight; wrong clock; no GPU budget |

## 5. The trade-off that remains

Just-in-time presentation removes the queue that absorbs frame-time spikes, so every
rare event (GPU spike, late compositor, late callback) shows as a repeated frame. Stock
shows none in the same tests because its queue hides them — and that queue *is* its
extra 20+ ms. Pure `late-fifo` in a continuous 2D pan still repeats roughly one frame
every 3–6 s (uneven between runs; not webview-related). That is why `adaptive` uses it
only during drags, where one repeated frame is hard to see and latency matters most.

## 6. Measurement methods and traps

- **Metal HUD**: `MTL_HUD_ENABLED=1 MTL_HUD_REPORT_URL=…`; a five-second report through
  the HUD menu, scripted with System Events (needs Accessibility for the calling app).
  Scripts: `.agents/latency/native-latency-2026-10-27/standalone-hud/{capture.sh,capture-host.sh,hud-host.sh,run-host.sh,summarize*.py}`
  (local, gitignored; written for the worktree layout, so fix their repo-root path
  before reuse).
- **Per-frame truth**: `IRON_NATIVE_PRESENT_TRACE=1` (host) — `presentedTime` per drawable,
  on-glass gaps in refreshes, GPU finish, misses, drains, engaged-vs-idle stats.
  `IRON_NATIVE_PACING_TRACE=path` — per-tick CPU timing. Repeated frames = on-glass gap
  of 2+ refreshes between consecutive drawables.
- **Window off screen**: launches without a diagnostic flag reuse the remembered
  position; after unplugging a monitor it was at x = −2144 and runs measured a window
  nobody could see. Set a pacing-trace path (centres the window) or move it with System
  Events. Several early 3D runs were discarded for this.
- **Locked screen / display asleep**: no active display → `CVDisplayLink` creation fails
  and windows stop ticking. The clock now retries; GUI measurements need an unlocked,
  awake display.
- **Synthetic input** (CGEvent) shows larger event ages than a real mouse; compare event
  ages only within one method.
- **Animation clocks**: host time is ≈ 10⁵ s; in `f32` it resolves only ≈ 16 ms, which
  made a test animation stutter while every frame was on time. Use `f64`.
- **Logging**: per-frame INFO lines can block on a slow stdout pipe and cost on the web
  console; the engine's event log is now DEBUG on `iron_debug`
  (`?bevy=debuglog` / `RUST_LOG=info,iron_debug=debug`).
- Two monitors, LaunchServices vs direct launch, and the HUD itself (it crashed with the
  link's fallback) all change results; record them with every capture.

## 7. Open questions

- Occasional stalls of 4+ refreshes, seen a few times, unexplained (not CPU or timer
  in the one trace that caught them).
- A GPU-bound scene (heavy fragment work); the 10,000-sphere load was CPU-bound.
- Whether on-time frames that still miss are WindowServer timing or residual GPU timing
  (log GPU finish against the pickup per miss).
- LaunchServices launch cost for the packaged app; 120 Hz displays; pointer prediction
  for drag previews; stdout blocking as a stall source.
- No physical (camera/photodiode) input-to-photon measurement has been made.

## 8. Where things live

- Host: `src-native/src/{pacing,frame_budget,vsync_clock,present_feedback,gpu_timing,display_link,motion_3d}.rs`.
  Modes: `native:baseline`, `native:low-latency` (old), `native:late-fifo`, `native:adaptive`.
  Knobs: `IRON_NATIVE_PACING_MARGIN_MS`, `IRON_NATIVE_GPU_BUDGET=0`, `IRON_NATIVE_FRAME_CLOCK=wall`,
  `IRON_NATIVE_2D_MOTION`, `IRON_NATIVE_3D_MOTION`, `IRON_NATIVE_3D_LOAD`.
- Standalone Bevy control (no Tauri/Vello/webview): `src/iron_latency_probe`,
  `native:standalone[:late-fifo|:motion|:stock]`; `IRON_BEVY_PACING=late|late-fifo|metal-link`.
- Durable measurements: [`benchmarks/native-latency-2026-10-27.json`](benchmarks/native-latency-2026-10-27.json),
  [`benchmarks/native-presentation-2026-10-27.json`](benchmarks/native-presentation-2026-10-27.json);
  raw HUD reports, logs, traces and capture scripts in
  `.agents/latency/native-latency-2026-10-27/` (local, not committed; copied from the
  `native-latency` worktree, which vision 08 calls "the worktree's `.agents/latency/`").
