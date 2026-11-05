# Holding the page's frame for the worker (adaptive frame hold)

**Status:** Phases 0–2 done 2026-11-05 (§6b, §6c): `?frameHold=on` on a dev
server with `IRON_COI=1`, adaptive by default. Phase 3 (isolation in
deployment, default on) next. Follows the worker tick-on-input
fix in [`latency-view.md`](latency-view.md) §7. Timelines of the problem, the fix
and this proposal: [`latency-timelines.html`](latency-timelines.html).

## 1. Goal

Keep Vello in the same displayed frame as the DOM when the worker's engine tick
is too long for the tick-on-input fix alone, without making the DOM or the
page slower and without spending main-thread time or power when it buys
nothing.

Today (Chrome, macOS, 60 Hz, measured): pointer motion reaches the worker about
12.5 ms into the 16.7 ms frame, and the compositor draws a light page about
1.2 ms after the next vsync. The worker's tick therefore has about 5 ms; at ≈4 ms
it fits. A tick over that misses the draw, and Vello shows a frame after the DOM.
The compositor waits for the **page's** frame, though, not the worker's. If the
page's main thread holds its frame until the worker's frame has been sent, the
draw includes both. Measured with a blind fixed hold (`?latencyBusy=N`): 6 ms
past vsync kept Vello and the DOM in one frame with the DOM's latency unchanged;
9 ms made the whole page a frame late. The budget would roughly double, from
≈5 ms to ≈10 ms, if the limit is near 6 ms.

## 2. Mechanism

Shared memory between the worker and the main thread (`SharedArrayBuffer`,
`Int32Array`/`Float64Array` views), written with `Atomics`:

| Field | Writer | Meaning |
|---|---|---|
| `state` | worker | 0 idle, 1 tick running, 2 frame sent |
| `tickStart` | worker | `performance.timeOrigin + now()` when the tick started |
| `tickEstimate` | worker | expected tick duration: a high percentile of recent drag ticks |
| `frameSeq` | worker | increments when a frame is sent |

Worker (`BrowserRuntime`, worker mode):

1. On a pointer message that starts a tick (the existing tick-on-input path):
   write `tickStart`, set `state = 1`, run the tick.
2. After the frame has been **sent to the compositor**, set `state = 2` and
   increment `frameSeq`. A worker canvas's frame is dispatched at the worker's
   next BeginFrame, just after its rAF callback (trace: rAF +0.04 ms,
   `CanvasResourceDispatcher::DispatchFrame` +0.13 ms). So "sent" has to be
   signalled after that point, for example from a task queued in the rAF
   callback. Phase 1 confirms the order in a trace; releasing the page before
   the dispatch would lose the frame.

Main thread, once per frame, at the latest point before the page commits:

1. If the hold is not armed (§3), or `state != 1`, return at once.
2. Compute the cap (§4). Predict the worker's finish:
   `tickStart + tickEstimate + dispatch allowance`.
3. If the prediction is past the cap, return: Vello will be a frame late either
   way, and spinning would waste time.
4. Otherwise spin on `Atomics.load(state)` until it is 2 or the cap is reached.
   The main thread may not use `Atomics.wait`, and `Atomics.waitAsync` cannot
   hold a frame (the frame commits when the callback returns).

**Where to hold.** A rAF callback runs among the page's other rAF callbacks
(Iron has `browser_runtime`, `input_manager`, `text_input`; views may add more),
before style and layout. A `ResizeObserver` callback runs after all rAF
callbacks and after layout, the last script before paint and commit, so it
leaves the least rendering work after the hold. That needs an observed element
whose size changes each held frame. Phase 1 compares the two in a trace.

## 3. When the hold is armed

Only during latency-sensitive input, and only while it helps:

- **Armed by:** pointer motion over a canvas view (hover or drag), wheel and
  pinch scrolling, keyboard navigation of a canvas view. Hover already uses the
  tick-on-input path.
- **Disarmed:** after a few quiet frames with no such input (about 100 ms), so
  a pause mid-gesture does not toggle it.
- **Backs off:** if a held frame is followed by a missed frame (a rAF interval
  above ~1.5 frame intervals), disable it for the rest of that interaction. One
  late frame, then the plain tick-on-input behaviour, never repeated hitches.
- **Developer override:** `?frameHold=off|on|always` for measurement.

The user estimates latency-sensitive input at half or less of the time the app
is open. Within that, the main thread spins only on frames whose tick runs past
vsync.

## 4. The adaptive cap

Each held frame, with `t0` the rAF timestamp (the frame's vsync):

> cap = t0 + limit − remaining page work − margin

- `limit`: how long after vsync Chrome waits for the page before drawing
  without it. Start from a value below the measured 6 ms, then adapt (below).
- **Remaining page work:** the style, layout, paint and commit still to run
  after the hold. Estimated from recent frames: time from the hold's end to the
  start of the next task (a `MessageChannel` message posted at the hold's end).
  With a `ResizeObserver` hold point most of that work is already done. Heavy
  panels then get shorter holds; light pages, longer ones.
- **Feedback:** grow `limit` slowly while held frames land and shrink it
  quickly after a missed one, the same shape as the native app's
  feedback-adapted present margin (`native-frame-pacing.md`). Chrome's limit is
  not documented, may change between versions, and Safari's is unmeasured, so
  the hold learns it rather than relying on a constant.

## 5. Cross-origin isolation

`SharedArrayBuffer` needs `crossOriginIsolated`, which needs both response
headers:

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp   (or credentialless, Chrome only)
```

What that touches here (audit, 2026-11-05):

- **Dev server:** Vite's `server.headers`, behind a flag at first.
- **Deployment:** the web build is on GitHub Pages (`.github/workflows/deploy.yml`),
  which cannot set response headers. The usual workaround is a service worker
  that re-serves the app's responses with the headers (the `coi-serviceworker`
  approach); the first visit reloads once. Safari's support for isolation
  granted this way must be verified.
- **Page contents:** the shell references other origins only through plain
  links (`github.com`, `get.webgl.org`; `target="_blank" rel="noopener"`, which
  COOP does not affect) and a `fetch` to the local agent bridge on `127.0.0.1`,
  a CORS request that COEP allows if the bridge answers with CORS headers (it
  must already, being cross-origin). No iframes or cross-origin scripts, fonts
  or images were found. Re-check before Phase 3; anything new that loads
  cross-origin needs CORP or CORS.
- **Native:** unaffected. The native app renders through its own host, not a
  page worker.
- **Fallback:** when `crossOriginIsolated` is false (an old deployment, a
  browser without it), skip the hold: today's tick-on-input behaviour.

## 6. Phases

**Phase 0: measure before building (no isolation needed).**

- Chrome's limit: sweep `?latencyBusy` 6–9 ms in 0.5 ms steps, worker `solo`,
  3 runs each; the last value with the DOM unchanged is the limit.
- Heavy ticks: add a dev knob, `?workerTickBusy=N`, that spins N ms in the
  worker before each tick. Confirm that tick-on-input misses at N ≈ 2 (a ~6 ms
  tick) and that `latencyBusy` then brings Vello back.
- Idle worker tick cost (cadence probe, app idle for a minute): if idle ticks
  are expensive, that is a larger power cost than any hold, and worth fixing
  first.
- Wheel, pinch and pan: whether Vello still trails the DOM there (the
  tick-on-input path starts only on pointer motion). Hybrid views with DOM over
  Vello (the timeline's column): whether the layers drift by a frame while
  scrolling.
- Repeat the limit sweep in Safari.

**Phase 1: prototype behind `?frameHold=on` (dev server headers only).**

- Shared state, the worker's signal after dispatch, a fixed cap, rAF and
  `ResizeObserver` hold points.
- Trace: the release comes after the worker's `DispatchFrame`, and the draw
  includes the worker's frame.
- Probe with `workerTickBusy` 0, 2, 4, 6: Vello equals the DOM while the hold
  fits the cap, and the DOM's latency is unchanged.

**Phase 2: adaptive cap, arming, back-off.**

- The cap from §4 with feedback; the arming rules of §3; missed-frame back-off.
- Acceptance: over a 60 s mixed drag and hover sweep with heavy ticks, Vello
  equals the DOM in ≥95% of frames, missed frames are no more than without the
  hold, and the DOM's p50 and p90 are unchanged.
- Main-thread cost: report time spent holding per second of interaction.
  Power: a `powermetrics` comparison run by the user (it needs `sudo`).

**Phase 3: ship.**

- Isolation in deployment (the service-worker approach), with the fallback.
- Default on in the worker runtime once Phase 2's acceptance holds in Chrome and
  Safari, with `?frameHold=off` to compare.
- Regression: the Latency view procedure (`latency-view.md` §6) gains a
  heavy-tick run (`workerTickBusy`), expecting Vello equal to the DOM.

## 6a. Findings on a 6K display (2026-11-05)

See `latency-view.md` §5, "On a 6K display". For this plan:

- With the test's own canvas load removed (`?latencyTrackers=light`), the page
  presents every frame on a large window; the remaining stutter is Vello alone
  missing the draw in 1–17% of frames (`solo`, large window) and ~4% (small).
- The cause is the margin this plan widens: Chrome draws a light page ~0.7 ms
  after vsync, motion arrives ~12 ms in, and the tick takes ~4.1 ms (p90 to
  5 ms), leaving about 1 ms.
- Phase 0 adds the large window (3008×1660 pt) as a standing case, measured
  with `?latencyTrackers=light`, and Chrome's presentation feedback from a
  trace as a second, recording-free measure of dropped frames (the recording
  itself drops frames of a heavier page).
- An earlier reading, that the GPU was saturated at full resolution, was the
  Latency view's own five full-view canvases, not Iron.

## 6b. Results so far (2026-11-05)

Large window on a 6K display (3008×1660 pt, 60 Hz), Chrome, worker `solo`,
`?latencyTrackers=light`, 2 runs per row, p50 lags; "behind" is the share of
frames with the square a frame behind the DOM.

**Chrome's limit (Phase 0), blind hold `?latencyBusy=N`:** 4 ms: Vello behind
in 0% of frames (3–5% with no hold), DOM unchanged; 6 ms: 0.7%, slight drops
(screen interval 17.2 ms); 7 ms: drops (26 ms) and DOM p90 52 ms; 8 ms: the
whole page a frame late (≈52 ms). The limit is ~6 ms after vsync; the
prototype's cap is 4.5 ms.

**Prototype (Phase 1).** `src-ui/runtime/frame_hold.ts` (the page's rAF hold
and the worker's writer); `BrowserRuntime` marks each tick and posts `sent`
as a task from its next rAF callback; `?workerTickBusy=N` lengthens ticks.

| Tick | Hold off: square / behind | Hold on: square / behind | Hold p50 / p90 |
|---|---|---|---|
| ~4 ms | 35.5–35.9 ms / 0.7–4.9% | 35.2–35.4 ms / 1.1–1.4% | 0.2–0.5 / 1.0–2.2 ms |
| ~6 ms (`workerTickBusy=2`) | 52 ms / 97–98% | 35.6–35.9 ms / 0–1.1% | 1.7–1.9 / 2.5–2.7 ms |
| ~8 ms (`workerTickBusy=4`) | 52 ms / 98% | 35.7–35.9 ms / 9–13% | 3.1 / 3.7 ms |

The DOM's lag (≈35.5 ms) and the screen's update interval (16.7 ms) were
unchanged in every run. Trace (`hold-tb2`): the worker's frame was sent before
the page let go in 173 of 176 held frames (release ~0.04 ms after it), and
made the draw in 175. With the 8 ms tick, 107–121 frames were not held because
the prediction passed the cap; those are the remaining misses. Main-thread
time spent holding: ~8% of the run at a 6 ms tick, ~10% at 8 ms.
`latency-view.ts` and the fast smoke pass with and without `frameHold=on`.

## 6c. Phase 2: adaptive cap, learned limit, back-off (2026-11-05)

Built in `FrameHold` (`src-ui/runtime/frame_hold.ts`):

- **Cap** = vsync + limit − the page's work after the hold (p90 of recent
  frames, timed from the end of the hold's rAF callback to the next task) −
  0.3 ms. No room left: no hold (`noRoom`).
- **Limit** starts at 4.5 ms, grows 0.02 ms per held frame that lands and
  drops 1 ms after a missed one, within 2.5–5.5 ms; both bounds scale with the
  frame interval (a 120 Hz frame halves them).
- **Back-off:** two misses after holds in one interaction switch the hold off
  until input has been quiet for 100 ms.
- `&holdAdapt=0` keeps Phase 1's fixed cap for comparison.

Large window, light trackers, Chrome worker `solo`, 2 runs per row:

| Case | Square behind the DOM | Screen interval, mean | Notes |
|---|---|---|---|
| 6 ms tick, adaptive | 0–2.2% | 16.7 ms | limit grew to the 5.5 ms ceiling |
| 6 ms tick, fixed cap | 0.4–2.4% | 16.7–16.8 ms | same as adaptive |
| heavy page (3 ms of page work) + 8 ms tick, no hold | 0% / 47% (by session) | 16.7 ms | the slow page delays the draw, so Vello often makes it |
| same, fixed cap | 2.3–28% | **18.9–22.6 ms** | the hold plus the page's work passes Chrome's limit: dropped frames |
| same, adaptive | 0–2.1% | 16.7 ms | measured 3 ms after the hold, so it almost never held |
| **acceptance:** 6 ms tick, 20 s sweeps, adaptive | **0.5–0.6%** (hold off: 99.6%) | 16.68–16.70 ms (off: 16.67) | DOM p50/p90 unchanged; 1–2 single dropped frames per run with the hold, none without |

- **Acceptance (§6, Phase 2): met** for Vello = DOM (≥95%: 99.4–99.5%) and the
  DOM's lag; dropped frames were 1–2 per ~1350 with the hold against none.
- **Main-thread cost:** 1.9–2.0 s of holding in a ~22 s run with a 6 ms tick
  (~9%), hold p50 1.6 / p90 2.4 ms.
- **The miss detector is blind to presentation drops.** In the fixed-cap heavy
  runs the screen dropped frames while rAF intervals stayed regular (0 misses
  counted), so the learned limit does not discover Chrome's limit by itself;
  the ceiling and the measured after-hold work are what protect the page.
  Raising the ceiling needs a measured limit per browser, not learning.
- Not measured yet: hover (the probe drags), wheel and pinch, Safari, the
  `full` layout, 120 Hz.

## 7. Risks and open questions

- **A late frame for the whole page.** Overrunning Chrome's limit makes the
  DOM a frame late too, which is worse than Vello alone being late. The
  margin, the "only hold when it will succeed" rule and the back-off bound this.
- **Chrome's behaviour over a long run.** The blind-hold measurements were 6–10
  s sweeps. Whether Chrome's scheduler changes (for example into a
  higher-latency mode) after many late page frames is unknown.
- **Work queued behind the frame.** Timers and messages start later; they
  still have until the next vsync, but a long one can delay the next frame.
  Script cannot see the task queue, so this is handled by the back-off only.
- **Power.** A spin keeps a core awake at full clock. Unmeasured; roughly half a
  watt to a watt for a 6 ms hold every frame of interaction, by estimate.
- **Other displays.** At 120 Hz the frame is 8.3 ms and both budgets shrink;
  untested. Not relevant natively.
- **Safari.** Its limit, its isolation support via a service worker, and
  whether its compositor also waits for the page's frame, are all unverified.
