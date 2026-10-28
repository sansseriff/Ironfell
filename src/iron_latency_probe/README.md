# Standalone Bevy latency control

A separate, ordinary Bevy 0.20 application using `DefaultPlugins`, the standard
winit runner, its default pipelined renderer and default FIFO window presentation.
No Tauri, WKWebView, Vello, Iron document model, custom frame driver or late pacer.
The Cargo workspace is isolated so its `bevy_winit` feature cannot unify into the
Tauri host. It shares the build cache and matches Iron's `dev-opt` profile.

From the latency worktree root:

```sh
bun run native:standalone
```

The green diamond follows the pointer at −192 logical pixels, matching the hosted
control. Grab the yellow square at its centre and drag along the grey guide.
**R** recentres the square. The initial logical content size is 1280×818; at 2×
backing scale it renders 2560×1636, matching the hosted control. Compare on the
same monitor and run one test app at a time. Bevy's normal default frame loop is
continuous while focused; match the hosted control's Continuous rendering setting.

`IRON_BEVY_RENDER_MODE=sync bun run native:standalone` is an optional second
comparison. The default is the unmodified pipelined rendering configuration.

Latency knobs (all default to stock Bevy):

| Variable | Effect |
|---|---|
| `IRON_BEVY_PRESET=low` | sync rendering + frame latency 1 |
| `IRON_BEVY_PACING=late` | `low` + immediate, one frame per refresh timed to vsync (`src/pacer.rs`); tears |
| `IRON_BEVY_PACING=late-fifo` | `low` + FIFO, present `IRON_BEVY_PACER_MARGIN_MS` (default 5) before vsync |
| `IRON_BEVY_PACING=metal-link` | `low` + FIFO, drawables and deadlines from a `CAMetalDisplayLink` (latency 1) |
| `IRON_BEVY_MOTION=1` | full-height sweeping bars + 48 moving shapes, for judging tearing/judder |
| `IRON_BEVY_FRAME_LATENCY=N` | `desired_maximum_frame_latency`; Metal drawables = N+1 |
| `IRON_BEVY_PRESENT=immediate` | `displaySyncEnabled = false` |
| `IRON_BEVY_FULLSCREEN=1` | borderless fullscreen on the current monitor |
| `IRON_BEVY_SCALE_FACTOR=x` | scale override (ignored by borderless fullscreen) |

`.agents/latency/standalone-hud/capture.sh <wiggle> <case> [VAR=…]` runs the binary
directly with the Metal HUD and input trace, posts synthetic mouse motion over the
window, saves a five-second HUD report through System Events (needs Accessibility
for the calling app) and `summarize.py <case>…` tabulates them. Results, 2026-10-27,
M5, 6016×3384 60 Hz display, 2560×1636 window:

| Case | Present Delay avg (min) | Frame interval | Event age at PostUpdate p50 |
|---|---:|---:|---:|
| stock (pipelined, FIFO, 3 drawables) | 47.95 (47.08) | 16.67 | 31.9 |
| sync | 64.45 (60.20) | 16.67 | 31.6 |
| frame latency 1 | 47.82 (43.35) | 16.67 | 32.0 |
| low (sync + latency 1) | 47.79 (43.08) | 16.67 | 31.6 |
| stock + immediate | 46.90 (38.29) | 16.67 | 31.3 |
| low + immediate | 6.63 (2.72) | 4.47 (uncapped) | 16.2 |
| stock fullscreen (6016×3384) | 31.32 (29.51) | 16.67 | 16.2 |
| low fullscreen (6016×3384) | 30.81 (29.33) | 33.34 (30 fps) | 65.4 |

| late pacing (≈58 presents/s) | 7.86 (3.03) | 17.07 | 16.4 |

Without display sync, each frame reaches the glass ≈4.6 ms after `presentDrawable`
regardless of refresh phase (glass-to-glass gaps are not multiples of the period),
so the immediate win is direct, unsynchronised flipping; tearing is possible. The
pacer takes the grid from a `CADisplayLink` (`timestamp` + `duration`;
`targetTimestamp` drifted) and presents median-present→glass before a vsync. On this
two-monitor setup the link's timestamps still jump by part of a period every few
frames, so frames land −10…+3 ms around the planned vsync rather than on it.
CPU (`ps`): stock ≈58 %, uncapped immediate up to ≈84 %, late ≈40–48 %.

One-display rerun (3840×2160 LG at 60 Hz, same window): stock 47.71 ms present delay,
uncapped immediate 5.17, late 6.50 (7.50 with the motion scene). The clock is now a
`CVDisplayLink` on its own thread, which brought presents to 60.3/s; the main-thread
`CADisplayLink` jumped while the pacer blocked that thread. Even so, the
`CVDisplayLink` vsync host times wander ±8 ms against a fixed grid and frames land
−4…+7 ms around the planned vsync. Glass time ≈ present + 4.6 ms at any phase
suggests the display refreshes when a frame arrives (variable refresh) rather than
on a fixed grid; unverified. With `IRON_BEVY_MOTION`, paced motion is a function of
the planned vsync, i.e. of when the frame is expected on glass.

Tear-free pacing, one display, `IRON_BEVY_MOTION=1` (the immediate modes tear
heavily in that scene):

| Case | Present Delay avg (min/max) | Frame interval |
|---|---:|---:|
| stock FIFO | 47.87 (46.96/48.22) | 16.67 |
| late-fifo, 3 ms margin | 26.33 (19.74/37.48), misses | 16.72 |
| late-fifo, 4 ms | 21.99 (21.19/22.53) | 16.67 |
| late-fifo, 5 ms | 23.13 (21.77/23.82), misses only at startup | 16.67 |
| late-fifo, 6 ms | 23.94 (22.55/24.38) | 16.67 |
| metal-link (pacer trace, no HUD) | present→glass ≈35 | 16.67 |

With FIFO a frame presented just before the compositor's pickup (≈3–4 ms before
vsync) reaches glass one refresh after that vsync. `CAMetalDisplayLink` at frame
latency 1 still sets its deadline two refreshes (33.3 ms) before its target
presentation, which frames meet exactly. Early builds aborted with an Objective-C
exception in a render system when the wrapper fell back to the layer's own
`nextDrawable` while the link was attached; it now always waits for the link's
drawable (6/6 launches stable afterwards).

Event age uses synthetic CGEvents; an earlier real-mouse run of stock Bevy measured
14.7 ms p50, so compare event ages only with each other. Present Delay is Apple's
`presentDrawable` → on-glass; with uncapped immediate presentation it averages only
the frames that reach the display.

For macOS application metadata / GUI automation, wrap the exact compiled binary:

```sh
src/iron_latency_probe/bundle-macos.sh
```

This creates `.agents/latency/standalone/Iron Bevy Control.app`. The wrapper adds
only Info.plist metadata and a copy of the same executable; it does not add a
window host or change Bevy's runner. The bundle must be refreshed after rebuilding.

Optional input timing:

```sh
IRON_BEVY_INPUT_TRACE=1 bun run native:standalone > .agents/latency/standalone-input.log 2>&1
```

The observational AppKit monitor returns every event unchanged. Each newly sampled
event emits `IRON_BEVY_INPUT`: original NSEvent timestamp, monitor receipt time,
PostUpdate time, cursor position, mouse button state and target position. Event
age includes time before the monitor receives it. These are CPU input-delivery
measurements, not GPU completion, display presentation or physical mouse-to-photon.
Trace logging is disabled by default; the GUI validation run used it to verify
mouse delivery and a held-button drag. `IRON_BEVY_PROBE` reports actual render-thread
state, window size/scale, default presentation mode and requested frame latency.

Build validation: the native executable compiles and starts; standard pipelined
rendering/FIFO and 1280×818 logical / 2560×1636 backing size are confirmed. The
follower renders and live input tracing confirms yellow-square movement while
held, followed by release. No comparative latency result is claimed yet.
