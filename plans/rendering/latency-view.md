# The Latency view: built-in input-latency checks

**Status:** built and measured 2026-11-05 (web: Chrome, Safari; native: Tauri).
The worker runtime's extra Vello frame is found and fixed (§7).
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
| Konva | a Konva stage (its own canvas, `listening: false`), `batchDraw()` from the handler (`latency_trackers.ts`) | red `#ff2a2a` | r 74 |
| WebGPU | a raw main-thread WebGPU canvas, drawn on the next rAF | cyan `#00e5ff` | r 84 |
| WebGL2 | a raw main-thread WebGL2 canvas, drawn on the next rAF | lime `#a0ff00` | r 94 |

Konva, WebGPU and WebGL2 are controls: a GPU canvas on the page's own thread,
so a difference between them and the engine is the engine's path, not the GPU
API. (WebGPU is absent where the browser has none, such as headless Chrome.)

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
  window opens at 1600×1000 pt on the main display).

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
macOS 27.2, on AC power, Chrome (WebGPU) and Safari (WebGL2) on this worktree's
dev-opt wasm. p50 lag in ms; p90 is within 2 ms of p50 unless noted. Raw JSON
and per-frame CSV are in `.agents/latency/measure/` (round 1),
`measure2/` (experiments) and `measure3/` (round 2), all local.

### Round 2: after the worker fix (§7), all trackers

"DOM" is the five page-thread trackers (Canvas2D event and rAF, SVG, Konva,
WebGPU, WebGL2), which agreed within 0.4 ms in every run.

| Setup | DOM | Engine ring | Square | Vello − DOM |
|---|---|---|---|---|
| Chrome worker, `solo` `panels` `full` (6 runs) | 35.2–36.0 | 35.2–36.0 | 35.2–36.0 | 0 |
| Chrome worker, `?workerTick=raf` (the old tick), `solo` `panels` | 35.1–35.6 | 49.7 | 51.8–52.2 | +1 frame |
| Chrome worker, `?workerTick=raf`, `full` | 35.9–36.3 | 36.3 | 36.3 | 0 |
| Chrome main, `solo` (2 runs) | 35.7–36.1 | 35.7–36.1 | 35.7–36.1 | 0 |
| Chrome main, `panels` `full` (4 runs) | 51.8–53.0 | 51.8–53.0 | 51.8–53.0 | 0 (whole page +1 frame) |
| Safari worker, `solo` `panels` | 52.2–52.4 | 52.3–52.4 | 52.3–52.4 | 0 |
| Safari worker, `?workerTick=raf`, `solo` | 52.3–52.6 | 66.4 | 67.8 | +1 frame |
| Safari main, `solo` `panels` | 67.9–69.7 | 67.9–68.9 | 67.9–69.7 | 0 (whole page +1 frame) |
| Native baseline, `solo` `panels` | 51.9–52.3 | 51.9–52.1 | 51.9–52.1 | 0 |
| Native baseline, `full` | 52.8–53.2 | 67.2 | 69.1 | +1 frame |
| Native adaptive, `solo` `panels` `full` | 52.4–53.3 | 37.4–38.3 | 35.2–35.9 | −1 frame |

Konva behaves exactly like a plain Canvas2D canvas: it adds no latency, and it
cannot remove any either, since the page's own frame bounds every DOM tracker.
In native `full` runs the p90 of the DOM trackers is unreliable (it climbs by
~17 ms per ring, outermost worst, a detection artefact of the busier layout);
their p50s are sound. Several round-2 runs logged cursor interference (the
person was working on a second display); their p50s match the clean runs.

### On a 6K display (2026-11-05, after the fix)

Two LG UltraFine 6K displays (3008×1692 pt at 2×, 60 Hz), laptop lid closed,
Chrome worker runtime, measured on the main display. "Late" columns are the
share of frames, computed from the probe's per-frame CSV
(a gap over 5 pt between trackers; at 600 pt/s one frame is 10 pt).

| Window, render scale | Engine tick p50 / p95 (ms) | Vello behind the DOM | DOM late | Screen update interval, mean |
|---|---|---|---|---|
| built-in display, 1710×1080, 2× (round 2) | 3.9 / 4.4 | 1–7% | 0–1.5% | 16.7 ms |
| 6K, 1710×1080, 2× | 3.9 / 4.3 | 0.4–2% (one run 9%) | 0.7% | 16.7 ms |
| 6K, 3008×1660, 2× | 4.8–5.0 / 6.5–7.5 | 0–69% by run | 18–30% | 19–24.5 ms |
| 6K, 3008×1660, 1× | 4.3–4.4 / 6.5–6.8 | 26–85% by run | 10–14% | 16.7 ms |

What the traces show for the large window (`trace/6k-big`, `6k-big-s1`,
against `6k-small` on the same display):

- **The whole page drops frames at full resolution.** At 2× the GPU
  process's `SwapBuffers` takes 1.6 ms at p50 (0.27 ms with the small window)
  and up to 27 ms, and the screen updates at 41–52 Hz. At 1× the screen holds
  60 Hz. The GPU cost of a ~6000×3200 px Vello canvas is the cause; the 6K
  display itself is not (the small window on it matches the built-in).
- **Input delivery becomes irregular and late under that load.** Gaps between
  the drag events Chrome receives: p10 9 / p90 24 ms (15.9 / 17.3 ms with the
  small window), and the event arrives ~15 ms into the frame instead of ~12.5.
  Chrome's browser thread is idle, so the delay is before Chrome (WindowServer,
  sharing the GPU). The probe's cursor read-back lags too, so its
  interference warning fires on every large-window run: there, it is a sign of
  load, not of someone moving the mouse.
- **Vello misses the draw more often** because the tick-on-input budget
  shrinks from both ends: motion arrives later (~1.6 ms before vsync) and the
  tick is longer (p90 6.8 ms). At 2× the page's own frame is slow enough that
  the draw is late and Vello often makes it, but the page drops frames instead.
- **Extra ticks.** With irregular input, two pointer messages can land in one
  frame, and the rAF tick then runs as well: 61–70 ticks/s, about 20% of
  frames with two or three ticks, wasted GPU work at the worst time.

### Round 1: before the fix (Canvas2D, SVG, engine and square only)

| Setup | DOM trackers (canvas / rAF / SVG) | Engine ring | Square | Vello − DOM |
|---|---|---|---|---|
| Chrome, worker or main, `solo` (5 runs) | 35.3–36.0 | 49.7–50.7 | 52.1–52.8 | +1 frame |
| Chrome, worker or main, `panels` (4 runs) | 35.2–35.7 | 49.7–51.1 | 52.0–53.2 | +1 frame |
| Chrome, worker or main, `full` (7 runs) | 35.4–36.3 | 35.6–36.3 | 35.6–36.3 | 0 |
| Chrome, Continuous rendering on, main `solo` | 35.6 | 35.6 | 35.6 | 0 |
| Chrome, Continuous rendering on, worker `solo`/`panels` (5 runs) | 35.1–35.6 | 35.4 ×3, 49.4–49.7 ×2 | 35.4 ×3, 51.7–51.8 ×2 | 0 or +1 frame |
| Safari, worker or main, `solo` or `panels` (4 runs) | 52.0–52.5 | 66.5–67.1 | 67.9–68.7 | +1 frame |
| Native baseline, `solo` `panels` `full` (10 runs) | 51.6–52.8 | 51.0–52.5 ×7; 38.2, 39.4 ×2; 66.2 ×1 | 51.2–52.5 ×7; 35.7, 36.8 ×2; 68.7 ×1 | usually 0; −1 or +1 frame by run |
| Native adaptive, `solo` `panels` `full` (6 runs) | 51.8–52.7 | 37.5–38.5 | 35.0–36.0 | −1 frame |

Round 1's main-runtime "+1 frame" in `solo` was a whole-page startup state, not
Vello alone: in later runs of the same session (`SESSION=4`) the page was on
time in 11 of 12.

### What it shows

- **The extra Vello frame was the worker's tick timing**, not the views beside
  it, the GPU API or the document drag (§7). Ticking the worker when the motion
  arrives removes it in Chrome and Safari, in every layout.
- **The Model and Inspector views cost nothing measurable** in the worker
  runtime. On the main runtime they can make the whole page, DOM included, a
  frame late, since Iron then renders inside the page's own frame.
- **The document drag adds ~2 ms over the bare engine pointer** (square versus
  ring), never a frame, except natively (below).
- **Safari is one frame slower than Chrome for everything**, DOM included; the
  native app's WKWebView DOM matches Safari (~52 ms).
- **Natively, adaptive pacing puts Vello a frame ahead of the DOM** (~36 ms).
  Baseline pacing lands in the DOM's frame, but in round 2's `full` layout the
  square and ring were a frame later (67–69 ms); its phase against input is
  not controlled. The engine ring trails the square by ~2.5 ms with adaptive
  pacing; not investigated (likely the two read the pointer at different
  points of the frame).

## 6. Regression procedure

```sh
scripts/latency/build.sh
# Vite on the worktree; restart it after every edit (a worktree under .agents/
# is outside Vite's watch), e.g. bunx vite --port 5185 --strictPort --force
cd .claude/skills/run-iron
bun run latency-view.ts                       # functional, headless
REPEAT=2 bun run latency-measure.ts           # Chrome, worker and main
BROWSER=safari RUNTIMES=worker,main bun run latency-measure.ts
BROWSER=native MODES=baseline,adaptive LAYOUTS=solo,panels bun run latency-measure.ts
../../../scripts/latency/summarize.py ../../../.agents/latency/measure/*.json
```

Hands off the mouse while it runs (~15 s a run); the probe reads the cursor back
and flags a run that something else moved. Run on AC power: on battery at or
below 20% Chrome's Energy Saver halves the page's frame rate, and the driver
warns when the page's rAF falls under 55 Hz. Compare the DOM trackers first:
they bound what the platform allows; a change in the Vello columns with the DOM
unchanged is Iron's. Expect Vello = DOM in the worker runtime everywhere.

Driver settings: `URL` (base page), `OUT` (output directory), `TAG` (label
suffix), `REPEAT`, `SESSION=k` (k runs in one browser session), `CONTINUOUS=1`.
Page parameters for experiments (all dev-only):

- `?workerTick=raf` — the worker ticks only at rAF (the old behaviour).
- `?latencyBusy=N` — the Latency view spins N ms in its rAF (a slow page frame).
- `&with=viewer,timeline,stats,document,library` — add views below the Latency
  view (`solo`, `panels`); `&without=<view ids>|all` — close tabs in `full`.

`latency-trace.ts` records a Chrome trace (compositor, GPU, canvas, input) of
one measurement, for questions the lags alone cannot answer.

## 7. Why Vello was a frame behind the DOM, and the fix

Timelines of the three cases (before, the fix, the cross-origin-isolated hold
proposed in §8): [`latency-timelines.html`](latency-timelines.html).

Traced with `latency-trace.ts` (Chrome 2026-11 on macOS; viz, cc, blink, GPU and
input categories), with times relative to the display compositor's (viz)
BeginFrame for each vsync, 16.7 ms apart:

1. **Input arrives late in the frame.** However fast the probe posts (250 Hz),
   the browser forwards one coalesced drag event per frame, ~12 ms after
   BeginFrame (`RenderWidgetHostImpl::ForwardMouseEvent` p50 12.0–12.4 ms in
   every trace). The page's `pointerrawupdate` runs at ~12.6 ms and its
   rAF-aligned `pointermove` at the next BeginFrame. DOM trackers drawn from
   either appear in the next vsync's draw.
2. **The compositor does not wait for a worker canvas.** viz draws as soon as
   the page's own frame is ready; an OffscreenCanvas in a worker is a separate
   surface (`CanvasResourceDispatcher::DispatchFrame`), and nothing holds the
   draw for it. A light page is drawn ~1.2 ms after BeginFrame.
3. **The worker ticked at rAF.** It started at BeginFrame +0.05 ms with the
   motion that had arrived at ~12.7 ms and dispatched its frame at ~4.8 ms:
   after the 1.2 ms draw in 192 of 192 frames, so the next draw showed it. That
   is the extra frame (~16 ms: engine ring 49.7 against DOM 35.6).
4. **Why it was absent in some setups.** When the page's own frame is slow, the
   draw waits for it, and the worker's frame makes it. In the `full` layout the
   Document view's forced style and layout in pointer handlers (~150 ms/s)
   pushed the draw to ~5.5 ms, after the worker's ~4.1 ms dispatch (180–185 of 191
   frames). The same happens artificially: `?latencyBusy=6` made Vello
   same-frame with the DOM unchanged; 3 ms was too little; 9 ms made the whole
   page a frame late. Continuous rendering helped only by chance of phase.
5. **Starting the tick earlier at a fixed time does not help.** A timer ~7 ms
   before vsync (a `workerLead` prototype, removed) ran at ~10.4 ms, before
   that frame's motion arrived at ~12.7 ms: its frame was ready before the draw
   in 188 of 190 frames but carried the previous frame's motion, so the lag
   was unchanged (49.6 ms).
6. **The fix: tick when the motion arrives** (`BrowserRuntime.tickOnInput`,
   the default in the worker runtime). A pointer message runs the engine frame
   at once, at most once per frame; the next rAF skips its tick unless more
   motion has come since. Trace: the worker's frame dispatched at BeginFrame
   +0.2 ms and was ready before the 1.1 ms draw in 190 of 190 frames; the lags
   match the DOM in every layout in Chrome and Safari (§5). Without motion the
   worker ticks at rAF as before.

Limits:

- **Slack is ~1 ms on this machine.** Motion at ~12.5 ms plus a ~4 ms tick
  finishes just before the next vsync. A tick longer than ~5 ms (a heavier
  document, a slower GPU) misses again, back to the old +1 frame, not worse.
  Keeping the drag tick short is now what keeps Vello in the DOM's frame.
- **The main runtime cannot be a frame behind the DOM** (Iron renders inside
  the page's rAF) **but makes the whole page late** once the frame is heavy:
  Chrome with `panels` or `full` (4 of 4 runs) and Safari in every layout. A
  late Chrome frame is drawn at ~0.3 ms without that frame's commit (traces
  `main1`–`main3`). Prefer the worker runtime.
- Measured at 60 Hz only, with one coalesced event per frame. At 120 Hz the
  frame is 8.3 ms and the slack tighter; a platform that delivers motion
  earlier in the frame has more.

## 8. Open

- 120 Hz (ProMotion at variable refresh) was not available on this display
  setting; the display ran at 60 Hz.
- Heavier documents under the view: how long a drag tick can be before the
  worker misses the draw again, and whether pointer prediction should cover it.
- Native baseline pacing in `full` (Vello +1 frame in round 2).
- A way to hold the page's frame for the worker (planned: [`worker-frame-hold.md`](worker-frame-hold.md)) (SharedArrayBuffer and
  `Atomics`, which need cross-origin isolation) was not tried; tick-on-input
  made it unnecessary at this scene size. It would roughly double the tick
  budget (≈5 → ≈10 ms) if Chrome waits ~6 ms past vsync for the page (a fixed
  6 ms hold kept everything on time, 9 ms made the whole page late). First
  step, no isolation needed: sweep `?latencyBusy` 6–9 ms for the exact limit,
  with a document heavy enough that the tick exceeds 5 ms.
