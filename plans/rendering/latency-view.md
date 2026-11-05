# The Latency view: built-in input-latency checks

**Status:** built and measured 2026-11-05 (web: Chrome, Safari; native: Tauri).
Owning record for the Latency view, its probe and its measurements. Background
on native frame pacing: [`native-frame-pacing.md`](native-frame-pacing.md).

## 1. Why

Earlier latency work used separate programs (the standalone Bevy probe,
`src/iron_latency_probe`) or a full-page filming scene (`?latencyFilm=1`).
Neither shows whether the rest of the app — an Inspector updating during a
drag, the Model view, other compositions — changes how far a dragged shape
trails the pointer. The Latency view is an ordinary dockable view, so it can sit
beside any of them, and a scripted probe measures it the same way every time to
catch regressions.

## 2. What it draws

A hybrid view (`notation: latency`, `src-ui/views/views.ts`). Its canvas part is
the session composition `latency` (`standard::latency_ops`): never saved, never
in undo, built only once a Latency view asks (`document_bridge::LatencyRequested`).
Every tracker is centred on the pointer, so with the pointer still they are
concentric; while it moves each trails by its own latency.

| Tracker | Path | Colour | Size (CSS px) |
|---|---|---|---|
| Square | the select tool's ordinary drag: input → engine → gesture → document preview → Vello | yellow `#ffd600` | 40 square |
| Engine ring | the engine's pointer state drawn by Vello as a screen overlay (`src/bevy_app/latency_view.rs`); no document | magenta `#ff00aa` | r 34 |
| Canvas2D, event | a main-thread `<canvas>` redrawn inside the `pointermove` handler | green `#00e000` | r 44 |
| Canvas2D, rAF | a second `<canvas>` redrawn on the next `requestAnimationFrame` | orange `#ff7a00` | r 54 |
| SVG | an `<svg>` ring moved inside the handler | violet `#9a4dff` | r 64 |

The square is 40 px, under the select tool's 48 px threshold for corner-radius
dots, so a press anywhere on it is a move. Colours keep clear of the accent blue
of selection chrome, so recordings can separate them by hue. The card (top left)
holds the legend, **Recentre**, a **Continuous rendering** switch (the
composition's run mode) and live readouts (events/s, `pointermove` age p50, rAF
rate).

### Opening it

- **Developer mode** (Settings → Developer): the "+" menu gains a Developer group
  with **Latency**.
- **URL:** `?latency=solo|panels|full` lays the workspace out for a measurement and
  never saves that layout: `solo` the view alone; `panels` the Model view left,
  the Latency view, the Inspector right; `full` the app's usual layout with the
  Latency view in front of Main. `?runtime=worker|main` overrides the stored
  runtime for that page.
- **Native:** `IRON_NATIVE_LATENCY=solo|panels|full` with any native script (the
  window opens at 1600×1000 pt, centred).

### Engine changes it needed

- `engine_api::document_latency` (`{op: 'latency', panel, fields}`): builds the
  composition on first call and puts the square's centre at a panel-local CSS
  point (`{"place": [x, y]}`) as session ops.
- `Store::apply_session` now reports what it changes like any other edit: written
  leaves are marked changed (the reconciler redraws a session node nothing binds
  to) and its ops reach the spatial scene's geometry changes (`geometry.note_ops`).
  Before, a session composition created after the last transaction was never
  indexed, so presses missed it and became marquees; and a recentred square was
  drawn at its old place. Tests: `src/iron_document/tests/latency.rs`.

## 3. Checks

`.claude/skills/run-iron/latency-view.ts` (headless Chrome, both runtimes, `solo`
and `panels`, DPR 2 by default): the view opens from `?latency=`, the square sits
at the view's centre, every tracker is centred on a still pointer (by colour in a
screenshot, within 1.5 CSS px), a drag carries the square and all trackers stay
concentric while held, the drag adds nothing to history, and Recentre works.

## 4. Measuring latency (the probe)

`scripts/latency/latency-probe` (Swift, `scripts/latency/build.sh`), driven by
`.claude/skills/run-iron/latency-measure.ts` for Chrome, Safari or the native
binary. It needs Accessibility and Screen Recording for the app that runs it
(on macOS 27 Accessibility is listed as "Device Control and Data Access").

1. A still capture of the main display finds the yellow square.
2. The OS pointer moves to its centre and presses (CGEvent at the HID tap).
3. A triangle-wave sweep, ±180 pt at 600 pt/s for 6 s, posts drag events at
   250 Hz, each with its host time.
4. ScreenCaptureKit records a band around the sweep; each frame carries the host
   time the compositor displayed it.
5. In each frame each tracker's centre is found by hue; its lag is the frame's
   display time minus the time the pointer was where the tracker is drawn
   (interpolated between events). Frames within 250 ms of a reversal are skipped.

The lag runs from the OS event to the compositor's frame. Panel scanout (a few
ms) is not included, equally for every tracker, so differences between trackers
and between builds are exact; absolute values read a few ms low. The run also
checks that every tracker comes to rest on the final pointer position
(`rest_offset_pt`). `scripts/latency/summarize.py` tabulates runs.

## 5. Results, 2026-11-05

MacBook Pro built-in Liquid Retina display only (1710×1107 pt at 2×, 60 Hz),
macOS 27.2, Chrome (WebGPU) and Safari (WebGL2) on this worktree's dev-opt wasm.
p50 lag in ms (p90 within 2 ms of p50 for the DOM trackers, within ~4 ms for
Vello unless noted). 43 runs; raw JSON and per-frame CSV in `.agents/latency/measure/` (local).

### Web

| Setup | DOM trackers (canvas / rAF / SVG) | Engine ring | Square | Vello − DOM |
|---|---|---|---|---|
| Chrome, worker or main, `solo` (5 runs) | 35.3–36.0 | 49.7–50.7 | 52.1–52.8 | +1 frame |
| Chrome, worker or main, `panels` (4 runs) | 35.2–35.7 | 49.7–51.1 | 52.0–53.2 | +1 frame |
| Chrome, worker or main, `full` (7 runs) | 35.4–36.3 | 35.6–36.3 | 35.6–36.3 | 0 |
| Chrome, Continuous rendering on, main `solo` | 35.6 | 35.6 | 35.6 | 0 |
| Chrome, Continuous rendering on, worker `solo`/`panels` (5 runs) | 35.1–35.6 | 35.4 ×3, 49.4–49.7 ×2 | 35.4 ×3, 51.7–51.8 ×2 | 0 or +1 frame |
| Safari, worker or main, `solo` or `panels` (4 runs) | 52.0–52.5 | 66.5–67.1 | 67.9–68.7 | +1 frame |

### Native (Tauri, WKWebView over the Bevy/Metal layer)

The dev-opt `iron-native` binary launched directly; `baseline` is the default
(pipelined render, display pacing), `adaptive` the recommended `native:adaptive`
(sync render, just-in-time presentation while dragging).

| Setup | DOM trackers (WKWebView) | Engine ring | Square | Vello − DOM |
|---|---|---|---|---|
| baseline, `solo` `panels` `full` (10 runs) | 51.6–52.8 | 51.0–52.5 ×7; 38.2, 39.4 ×2; 66.2 ×1 | 51.2–52.5 ×7; 35.7, 36.8 ×2; 68.7 ×1 | usually 0; −1 or +1 frame by run |
| adaptive, `solo` `panels` `full` (6 runs) | 51.8–52.7 | 37.5–38.5 | 35.0–36.0 | −1 frame |

With adaptive pacing the dragged square is the fastest thing on screen in any
setup measured: ~36 ms, a frame ahead of the web view's own DOM. Its p90 stays
within 2 ms of p50 except in `solo`, where it reaches the next frame (51.8 ms).
The engine ring trails the square by ~2.5 ms with adaptive pacing, which is
less than a frame; not investigated. A likely cause is that the two read the
pointer at different points of the frame (the select tool's drag versus the
ring's `PostUpdate` system), so the ring more often shows the previous sample.

### What it shows

- **The Model and Inspector views cost nothing measurable.** `panels` (the
  Inspector redrawing the square's position every frame of the drag, the Model
  view idle) matches `solo` in every browser and runtime, run for run.
- **Vello is exactly 0 or 1 frame behind the DOM**, never in between, and the
  document drag (square) adds ~2 ms over the bare engine pointer (engine ring),
  not a frame.
- **The extra Vello frame depends on the engine's run state, not on the
  views beside it.** In the app's usual layout (`full`) Vello matches the DOM in
  all 7 runs; with the Latency view alone or with the Model and Inspector it is a
  frame late in all 9 runs. Turning the view's composition to Continuous
  rendering removes the frame on the main thread and in 3 of 5 worker runs. Why
  `full` is reliably same-frame while Continuous alone is not (in the worker) is
  open; candidates are what else free-runs there (the Stats view, the timeline)
  and the phase of the worker's rAF against input delivery.
- **Safari is one frame slower than Chrome for everything**, DOM included, and
  the native app's WKWebView DOM matches Safari (~52 ms).
- **Natively, adaptive pacing puts Vello a frame ahead of the DOM** (~36 ms),
  as fast as Chrome's DOM and faster than anything in Safari. The default
  baseline pacing lands Vello in the DOM's frame in most runs, but the frame
  it lands in varies from run to run (36, 52 or 68 ms): the phase of its
  pipelined frame against input is not controlled.
- **The worker and the main thread are equal** in Chrome and in Safari.

## 6. Regression procedure

```sh
scripts/latency/build.sh
.claude/skills/run-iron/serve.sh start   # or any Vite on the worktree
cd .claude/skills/run-iron
URL='http://127.0.0.1:5173/Ironfell/?demo' REPEAT=2 bun run latency-measure.ts
BROWSER=safari URL=… RUNTIMES=worker,main bun run latency-measure.ts
BROWSER=native MODES=baseline,adaptive LAYOUTS=solo,panels bun run latency-measure.ts
../../../scripts/latency/summarize.py ../../../.agents/latency/measure/*.json
```

Hands off the mouse while it runs (~15 s a run). Compare the DOM trackers first:
they bound what the platform allows; a change in the Vello columns with the DOM
unchanged is Iron's.

## 7. Open

- Why Continuous rendering in the worker sometimes still leaves Vello a frame
  late, and what in `full` prevents it; whether an auto-mode view could keep that
  same-frame behaviour during drags without rendering when idle.
- 120 Hz (ProMotion at variable refresh) was not available on this display
  setting; the display ran at 60 Hz.
- Pointer prediction; heavier documents under the view.
