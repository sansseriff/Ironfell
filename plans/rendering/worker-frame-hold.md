# Holding the page's frame for the worker (adaptive frame hold)

**Status:** Phases 0–2 done 2026-11-05 (§6b, §6c): `?frameHold=on` on a dev
server with `IRON_COI=1`, adaptive by default, Chromium only (Safari: §6d).
Phase 3 (isolation in deployment, default on) is **deferred to the app's own
backend server** (decided 2026-11-05, §6e).
**Phase 4, frame modes, built 2026-11-06 (§6g, results at its end):** on by
default wherever it can work (Chromium with cross-origin isolation, e.g.
`bun run dev`): early (the hold) per interaction, late as the fallback
(drawn after the worker's rAF, so it goes out at the next vsync), late with
the clock set ahead between interactions, the mode in the FPS readout, and a
per-device memory. Early mode works first, then waits (the page's own work
and layout before the hold, the wait only for what the worker still needs).
Half rate is an experiment (`?halfRate=1`). Follows the worker tick-on-input
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

**Phase 3: ship** (deferred, §6e).

- Isolation in deployment: the two headers sent by the app's own server (§6e);
  the fallback when `crossOriginIsolated` is false stays.
- Default on in the worker runtime once Phase 2's acceptance holds in Chrome and
  Safari, with `?frameHold=off` to compare.
- Regression: the Latency view procedure (`latency-view.md` §6) gains a
  heavy-tick run (`workerTickBusy`), expecting Vello equal to the DOM.

## 6e. Deployment decision (2026-11-05)

GitHub Pages cannot send response headers. The workaround is a service worker
that re-serves the app's responses with them; a page cannot become isolated
after it has loaded (isolation is decided at navigation), so the first visit
is never isolated and the usual script reloads once. Because the hold is an
enhancement, a quiet variant (install the worker, no reload; isolated from the
next visit) would also do.

Neither is built: the app is to move to its own backend server (for saving
projects and scopes to a database), and that server will send the headers
with the first response, so no service worker and no reload. What that server
needs:

- `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp` on the app's HTML (all responses
  is simplest); `credentialless` is not supported by Safari.
- The app's own files (wasm, worker, fonts) from the same origin, as today. An
  API on the same origin needs nothing more; on another origin it must answer
  with CORS headers (CORS-mode requests are allowed under COEP).
- Anything else loaded from another origin needs CORS or
  `Cross-Origin-Resource-Policy`, or it stops loading. A popup the app opens
  (an OAuth sign-in, say) cannot talk back under `same-origin`; that case
  needs `same-origin-allow-popups`.
- Until then the hold runs only on a dev server with `IRON_COI=1`
  (`vite.config.ts`) or under `bun run dev:agent`, isolated by default since
  2026-11-06 (`IRON_COI=0` turns it off), and real users get tick-on-input alone (Vello in the
  DOM's frame for ticks up to ~5 ms in Chrome).

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

## 6d. Safari (2026-11-05)

Safari on macOS 27, worker `solo` (WebGL2: `backend_policy.ts` gives WebGPU to Chromium only; Firefox below too), `?latencyTrackers=light`, a
1710×1073 pt window on the 6K display, 2 runs per row after a discarded
warm-up. Cross-origin isolation works there (`crossOriginIsolated` true with
the dev headers). The DOM is ≈52 ms throughout, a frame later than Chrome's.

| Case | Square p50 | Behind the DOM |
|---|---|---|
| 4 ms tick, hold off / on | 52.2–52.7 / 52.0–53.1 ms | 1–2.5% |
| 6 ms tick (`workerTickBusy=2`), hold off / on | 52.4–52.9 / 52.0–52.4 ms | 0.7–1.8% |
| 8 ms tick (`=4`), hold off / on | 52.1–52.4 / 52.4–52.7 ms | 6% / 8.5–10% |
| 12 ms tick (`=8`), hold off / on | 69.0 / 68.9–69.0 ms | a frame behind either way |
| 16 ms tick (`=12`), hold off | 85.5 ms | two frames behind |
| blind hold (`latencyBusy`) 4–8 ms | 52–53 ms; DOM unchanged | — |
| blind hold 10–14 ms | 52–53 ms; **DOM 68–69 ms** | Vello a frame *ahead* of the DOM |

- **Safari shows the worker's frames independently of the page's.** Holding the
  page past ~8–10 ms makes the DOM a frame late while Vello stays on time, and
  `frameHold=on` changes nothing for a long tick. Holding the page cannot carry
  the worker's frame here: the mechanism is Chromium's alone.
- **Safari's worker has more slack:** ticks to ~8 ms land in the DOM's frame
  with no hold (Chrome: ~5 ms); 12 ms is a frame late.
- So `frameHold=on` is Chromium only (`navigator.userAgentData` brands);
  elsewhere it warns and does nothing (`frameHold=force` overrides, for
  measurement). Per-engine table so far:

| Engine | Compositor waits for the worker canvas? | Holding the page carries it? | Page limit after vsync (60 Hz) | Worker tick that still lands (light page) |
|---|---|---|---|---|
| Chrome (Blink), macOS | no | yes | ~6 ms | ~5 ms without the hold, ~8 ms with it |
| Safari (WebKit), macOS | no | **no** | 8–10 ms | ~8 ms |
| Firefox 156 (Gecko), macOS | not needed so far | no difference (`force`) | over 14 ms | ~12 ms |

Firefox (2026-11-05, same window and settings as Safari, a separate instance
with a temporary profile per run): the DOM ≈52 ms throughout, like Safari. The
square matched it (≈52 ms) for ticks of 4, 6, 8 and 12 ms, with or without
`frameHold=force`; a 16 ms tick was a frame late (68 ms). A blind hold of 6, 10
or 14 ms left the DOM unchanged. Firefox gives both the page and the worker
far more room than Chrome, at a frame more latency for everything. The hold
stays Chromium only.

- The first measured run after Safari launches is slow for the dragged square
  (≈88 ms, the engine ring ≈52 ms) in two of two batches; later runs are not.
  Not investigated; the runs above follow a discarded warm-up.
- The Safari driver now launches a fresh Safari per run and closes all windows
  and quits after it (`latency-key --only com.apple.Safari`), so relaunches
  restore no test tabs.

## 6f. Stutter during timeline playback (2026-11-05)

Reported on the 6K display: a shape animating by itself (timeline playback,
no pointer) stutters. Test: `.claude/skills/run-iron/latency-anim.ts` (the
demo square recoloured yellow, its clip looped at 4× speed, the default
layout; `latency-probe --watch=1` records it with no input;
`scripts/latency/judder.py` counts repeated and doubled screen steps).

- **Not the hold.** With `frameHold=on` it is disarmed (no input); with
  `always` it held 0 of ~490 frames, every one `noRoom`: the page's own work
  after its rAF is 4.8–5.5 ms p50 (5.8–6.3 p90) during playback, so the cap
  (vsync + limit − that − 0.3) is already past when rAF runs. Holding anyway
  would make the whole page miss vsync. Diagram: `playback-race.html`. Repeated plus doubled steps were 0–2.9% with
  the hold off, on or always, by run, in both window sizes (small window:
  0–4%).
- **The cause is a race.** Traces of stuttering runs (with tracing load, 8% and
  20% bad steps): the page's own frame is drawn 5.7–6.1 ms after vsync, and
  the worker's frame, ticked at vsync (tick p50 ~6.5 ms in the large window),
  is dispatched 6.5–6.7 ms after it. Some frames make that draw and some miss
  it, flipping 120–167 times in ~7 s; each flip is a repeated or doubled
  step on screen. A smooth run flipped once: consistently early or
  consistently late both look smooth.
- **Why the page is that late:** ~4 ms of Blink layout every frame of
  playback (240–255 ms/s), half of it text shaping, a full (non-partial)
  layout of all 766 layout objects with 31 dirty each frame. Something on
  the page changes text every frame while the clock runs: the Document
  view's markup (isolated below, and fixed).

What would fix it, best first:

1. **Render playback a frame ahead.** Without input, the next frame's
   animation time is known: the worker can tick right after its previous
   frame is pushed, for the next vsync's time, so its frame is ready at the
   next BeginFrame and always makes the draw (as the `workerLead` timer did
   for input, which failed there only because input arrives late). Smooth and
   on time, whatever the page costs. Needs the engine's clock to advance to
   the target display time rather than the time of the tick.
2. **Stop the per-frame layout.** Find the view that rewrites text every
   frame and update it without a full layout (only when its text changes, at
   a lower rate, or contained with CSS `contain`). Then the page draws ~1 ms
   after vsync, the worker consistently misses it, and playback is smooth but
   a frame later (which does not matter without input). Worth doing anyway:
   it is ~25% of the main thread during playback.
3. **Arm the hold during playback.** It would make the worker consistently
   catch the draw, but the page is already ~6 ms in, at Chrome's limit, so a
   hold would risk dropped frames. Not recommended.

### Fix 2 done (2026-11-05): the Document view follows commits

`.claude/skills/run-iron/latency-churn.ts` records DOM mutations by dock
panel during playback and reads Chrome's layout time with each panel hidden
in turn. The whole cost was the **Document view**: its `<pre>` holds the
whole document's markup (~68 KB, 758 lines, at `full` fidelity) with
resolved values, so every frame of playback (and of a drag, before release)
replaced it, and Blink re-shaped all of it. Hidden: layout 241 → 1 ms/s.
The **Inspector** was not a cost: with the square selected and playing it
adds ~10 ms/s of layout (~0.2 ms a frame) and ~20 ms/s of script, since it
reads one node and Svelte updates only the fields that changed.

Change (`src-ui/runtime/query_subscriptions.ts`): whole-document reads
refresh at once on a commit (new authored version: an edit, a drag's
release, undo, redo, load, structure) and, for live values alone (playback,
a drag in progress, a simulation), once they have been quiet for 250 ms
(`SETTLE_MS`). A drag keeps the document version until release (checked:
one version step per drag), so the view updates once, at release.

Result on the 6K monitor (3008×1660 window, no input, tracing on):

| | page draw after vsync p50 | made/missed flips (7 s) | bad steps |
|---|---|---|---|
| before | 5.7–6.1 ms | 120–167 | 0–20% (by run) |
| after (3 runs) | 0.36–0.40 ms | 0 | 0%, 0.2%, 0% |

The worker's frame now misses the draw every frame, consistently: smooth,
one frame behind (fix 1 would make it a frame earlier). Layout during
playback with the square selected: 238 → 11 ms/s. The hold (`always`) still
does not act during plain playback, now for the other reason: the page's
later work is 0.15 ms, but the worker's tick, started at vsync, takes ~7 ms
on this window, so it is predicted past the cap (`skipped`). In a drag the
tick starts at the input, earlier, as in §6b–c.

## 6g. Frame modes: early by default, late as the fallback (planned and built 2026-11-06)

The hold (§2–§4) makes Vello appear in the same draw as the DOM when the
worker can finish in time. When it cannot, today nothing decides where the
worker's frame lands: it races the page's frame, and if the two finish near
the same moment the order flips frame to frame and Vello judders (§6f, and
the diagram in [`playback-race.html`](playback-race.html)). Iron also has to
run on weaker hardware, where both threads are slower. So the order is now
chosen on purpose, per interaction, and kept.

**Decision:** lowest latency is the default whenever the hardware can deliver
it. Late is a fallback that is smooth everywhere, not the default.

### Modes

| Mode | What happens | Vello vs the DOM | Used when |
|---|---|---|---|
| **Early** (default) | the page's rAF waits for the worker's frame (the hold), filling the wait with its own work (below) | same draw | the worker's frame plus the page's later work fit Chrome's limit (~5–6 ms after vsync at 60 Hz, learned, §4) |
| **Ahead** | during playback without input, the worker renders the frame for the *next* vsync's time right after pushing the previous one | same draw, no hold | playback; also the best mode on a taxed machine, since a playback tick then has a whole frame |
| **Late** (fallback) | as planned, a gate: the worker finishes its frame, then hands it over only after the page's frame is out. **As built:** the worker draws in a task right after its rAF callback, so the frame goes out at the canvas's next BeginFrame (results below) | always one frame behind the DOM, never flipping | early failed (misses), the page is heavy or uneven (a chat layout), or the device is known to be slow |
| **Half rate** (experiment, `?halfRate=1`) | the worker draws on every second vsync, steadily | late, at 30 fps | the worker's tick no longer fits one frame; a steady 30 beats alternating 60/30. Not steady yet (results below) |

Lowering the render scale (Settings) is the other lever on weak hardware and
stays with the person, not this controller.

### Time budgets per mode (60 Hz)

- **Early:** the page must have its frame out by the learned limit, ~5–6 ms
  after vsync, including the time it waits. The worker's tick starts at the
  input (~12 ms into the previous frame, tick-on-input) and must end before
  that limit: ~10 ms. Both are the lowest-latency budgets any page has in
  Chrome; the hold only lets the worker use the page's share.
- **Late:** the worker's frame has to reach the compositor before the *next*
  draw, so a tick can take about a whole frame (~16 ms, less Chrome's own
  work), as long as ticks average no more than one per frame. The page gets
  the ordinary web budget: under ~16 ms to stay smooth; past ~5–6 ms its own
  DOM is a frame later, as for any page, and the gate follows it, so the two
  stay one frame apart and consistent. Confidence: the worker's budget holds
  by construction. Two things to measure (Phase 4a): (1) a vsync where the
  page has nothing new: Chrome may draw without waiting for a page frame, and
  a gated worker frame could then land in that draw on quiet frames and the
  next on busy ones, which is the flip again. If so, the page makes a frame
  every vsync while the gate is on (any small style change in its rAF) so the
  order is always defined. (2) That a frame handed over right after the page's
  frame is reliably excluded from that draw (Chrome may still catch it); the
  gate adds a margin if not.
- **Ahead:** the worker has a whole frame per playback tick and does not
  depend on the page at all.
- **Half rate:** two frames per tick.

### When the page is heavy but the worker is fast

If the worker's frame is ready before the page's (a fast tick, or ahead
mode), the draw that carries the page's frame also carries the worker's: the
two are composited together, with no jitter. If the page then misses Chrome's
limit, Chrome may draw without waiting for it, at its deadline, with the
worker's new frame and the page's previous one: Vello on time, the DOM a
frame late. That is smooth as long as the page is late consistently (if not,
it is the DOM's own judder, as in any web app). Unmeasured; Phase 4a's traces
will show it. Not a goal: a page that heavy usually means a slow machine,
where the worker is slower too and late mode applies.

### Choosing the mode

Measured each frame, as the hold already does (§4): the worker's tick start
and a p90 of its length (shared memory), the page's work after its rAF (p90),
the learned limit, misses (rAF intervals), and the page's own rAF cost.

1. **Down at once, up slowly.** Early → late after the second miss in an
   interaction (the existing back-off), and late → half rate when ticks exceed
   a frame in 5 of the last 30. Up only on evidence: 30 frames where the
   predicted worker finish plus a 1.5 ms margin fits the cap (early), or ticks
   comfortably under a frame (from half rate).
2. **Hysteresis.** Enter early with a 1.5 ms margin; leave only on actual
   misses. Jitter alone cannot flap the mode.
3. **Switch where it cannot be seen:** at an interaction's start (press, or
   hover after 100 ms quiet) or end, playback start or stop, a window resize,
   a render scale change, a view opening or closing. The exception is the
   downgrade after misses mid-gesture, since staying is worse. Each switch is
   one repeated (early → late) or skipped (late → early) frame.
4. **Back off retries.** After a downgrade, wait before trying early again:
   2 s, then 4, 8, up to 60 s; reset after a minute of success.
5. **Remember per device.** Keep the outcome (did early hold up, the learned
   limit, tick p90) in local storage, keyed by backend, window size bucket
   and render scale. A weak laptop starts late instead of failing its first
   drag; a fast machine starts early. Retest when the key changes.

### Early mode: work, then wait (built 2026-11-06)

Planned first as a "working wait" (UI work and forced layout first, then
deferrable work in slices, then a spin). As built, the slices are dropped:
there is no deferrable per-frame work worth putting there, and
`requestIdleCallback` covers truly optional work without risking a frame.
What remains is the order:

1. The hold runs **last** among the page's rAF callbacks: it asks for its next
   frame from the task after the frame, after every other callback (the
   timeline's playhead, the Latency view) asked for theirs.
2. On a frame it will hold (the worker still ticking at rAF, early mode,
   input recent), it applies Svelte's pending updates (`flushSync`) and
   forces layout (`document.body.offsetWidth`), so the page's work is done.
3. Then it waits only for what the worker still needs, until the worker's
   frame is sent (the shared flag) or the cap.

Why it is better than wait-then-work (the Phase 1–4 order, kept as
`?holdOrder=first`): the page's frame goes out at max(page work, worker) +
paint instead of worker + page work + paint, never later; and what the cap
must allow for after the hold is only paint and commit (~0.5 ms, steady)
instead of the page's whole later work as a 90th-percentile estimate (0.1–
3 ms here, and the old cap gave up its spread on every frame). In early mode
the page can use up to ~5 ms for its own work in parallel with the worker's
tick; it waits only when it finishes first. The starting limit is 5.5 ms in
this order (4.5 before), since the after-hold part is small and measured.

What could still cost a second layout: a DOM change after the forced layout
in the same frame (a rAF callback that runs after the hold, a microtask from
one, a ResizeObserver callback when a size changes). Timers, worker messages
and network tasks cannot interleave with a frame's rendering steps. In
development a `MutationObserver` counts DOM changes between the hold's start
and the frame going out (`lateWrites` in `__frameHold.stats()`, a console
warning naming where, once); none were seen in any run. If third-party code
shows up there, the next step is routing `requestAnimationFrame` through
Iron's own frame scheduler.

### Ahead mode for playback

Without input the next frame's animation time is known. After the worker
pushes frame N it starts frame N+1 at once, for vsync N+2's display time:
the engine's clock advances to the target display time, not the time of the
tick. The frame is waiting before any draw, so it always lands early, however
heavy the page. Input ends ahead mode (an interaction switches to early or
late), and playback stopping ends it.

### The indicator

When the composition view's FPS readout (`FpsCounter.svelte`, "Show FPS") is
on screen, it shows the frame mode beside the frame rate whenever it is not
early: **late**, **half** (and **ahead** during playback, muted), in the
warning colour for late and half. Its tooltip gives the mode, why (misses,
heavy page, slow tick, remembered for this device), since when, and the
early-mode numbers (limit, tick p90, page after p90). Hidden with the
readout; the controller reports its state through `frame_stats`.

### Shared memory, and without it

The hold and the gate both need `SharedArrayBuffer`, so cross-origin
isolation (§5, §6e). Without it (GitHub Pages today) there is no early mode.
A message-based gate is possible there: the page posts "frame out" after its
rAF and the worker pushes its frame only on that message (a worker canvas's
frame goes out when the worker's task ends). It costs a message hop per frame
and is late only; worth building only if the deployment without isolation
stays long.

### Rejected: bitmaps through the main thread

Rendering in the worker, `transferToImageBitmap()`, posting the bitmap and
showing it with a `bitmaprenderer` canvas in the page's rAF keeps Vello and
the DOM in one page frame by construction. It is the path the API's designers
describe for frames that must match the DOM, but not used here:

- Every Vello frame would pass through a main-thread task, so a long
  main-thread task (the heavy UI the worker exists to make room for) would
  stall Vello. A transferred canvas's frames go to the compositor without the
  main thread, which is why the Bevy-in-a-worker guides use it.
- A page blocked in rAF cannot receive a message, so it cannot wait for a
  bitmap within the frame: a frame finished after rAF shows a frame later.
  Synchronized, but late only; it cannot give early mode's latency.
- Firefox and Safari read the pixels back in `transferToImageBitmap` (Flutter
  issue #145420, still open; Mozilla bug 1788206), and Iron uses WebGL2 there.

Sources: [web.dev OffscreenCanvas](https://web.dev/articles/offscreen-canvas),
[WICG thread](https://discourse.wicg.io/t/offscreencanvas-animations-in-workers/1989/),
[Remotion #12074](https://github.com/remotion-dev/remotion/issues/12074),
[Flutter #145420](https://github.com/flutter/flutter/issues/145420),
[Mozilla 1788206](https://bugzilla.mozilla.org/show_bug.cgi?id=1788206).

### Phase 4: the build plan (as planned; results follow)

- **4a. Late gate.** Shared flag: the page marks "frame out" (the existing
  post-rAF task); the worker, after its tick, `Atomics.wait`s on it (with a
  timeout of one frame) before returning from its rAF. Trace: the worker's
  `DispatchFrame` follows the page's draw, every frame; measure the two
  unknowns above (quiet page frames, the margin).
- **4b. Mode controller.** One state machine on the page holding the rules
  above, driving the hold (early), the gate (late) and the worker's rate
  (half); stats through `__frameHold.stats()` and `frame_stats`.
- **4c. Working wait.** The rAF ordering and the slice queue; first user:
  whatever per-frame UI work exists in that view.
- **4d. Ahead playback.** Worker scheduling and the engine clock's target
  time; acceptance with `latency-anim.ts`: page draw unchanged, the worker's
  frame before the draw in every frame, 0 flips.
- **4e. Half rate.**
- **4f. Indicator** in the FPS readout.
- **4g. Per-device memory.**
- **Tests for weaker hardware on this machine.** `?workerTickBusy=N` for a
  slow worker; a new `?latencyBusy=a-b` (a random page load between a and b
  ms each frame) for an uneven page. Expected: early while it fits; one
  switch to late when it stops fitting; late smooth (judder ~0) where free
  running judders; half rate steady when the tick passes a frame. Measured
  with `latency-anim.ts` (playback) and `latency-measure.ts` with the hold
  stats (drags), on the 6K monitor and the built-in display.

### Phase 4 results (2026-11-06)

Measured on the built-in display (1710×1080 window, 2×, 60 Hz, M5, Chrome,
WebGPU), the worker's tick ~3.8 ms for a drag, ~5.8 ms for playback.
`?workerTickBusy=N` adds N ms to every tick (a slower machine's worker);
`?pageBusy=a-b` spins a random a–b ms in the page's rAF (a busy UI). Drags:
`latency-measure.ts` (worker, solo), classified per displayed frame by
`scripts/latency/flips.py` (Vello's document square against the DOM's SVG
ring: same frame, one behind, further); playback: `latency-anim.ts` with
`scripts/latency/judder.py` and the trace analysis `scripts/latency/animtrace.py`.

**What changed from the plan, and why.**

1. **Late is not a gate.** The planned gate (the worker blocks in its rAF
   until the page's frame is out, then its frame is pushed) made the whole
   page late: with the worker's rAF callback held ~5 ms, Chrome drew
   everything at its deadline, the DOM at 52–69 ms instead of 36. And frames
   the worker draws on input go out at its next BeginFrame, before its rAF,
   so a gate there never held them. What works: in late mode the worker does
   not tick on input; its rAF callback returns at once and the tick runs in a
   task right after, so the frame goes out at the canvas's next BeginFrame
   (~0.1 ms after the next vsync) and makes that vsync's draw. Late needs no
   page signal at all (the planned page-out flag and 1 px change are gone).
2. **The 1 px change is harmful.** Changing a 1 px element every frame (to
   give the page a frame each vsync) moved Chrome to a pipeline one to two
   frames later for the whole page. Not used; `pageBusy` no longer does it.
3. **Chrome sometimes waits for the worker's canvas on its own** (the draw
   comes right after the worker's dispatch, at 1–4 ms), sometimes not. So a
   frame the hold did not wait for can still carry Vello. The controller
   judges each early-mode frame by what it can see: the worker's frame was
   already out at the page's rAF, the hold released, or the worker's frame
   counter moved before the page's frame went out.
4. **One tick per frame.** Under fast input the worker ticked on input and
   again in its rAF when more motion had arrived, doubling its work and
   alternating its timing (the estimate read 8.6 ms for a 6 ms tick). Motion
   that arrives during the input tick now waits for the next frame's.
5. **The estimate.** Early mode predicts with ticks started by input only, a
   75th percentile, restarted (after 3 ticks) each time late mode ends.
6. **Settling.** The first 8 early frames after a switch are not judged (the
   worker may still finish a late-mode tick); judging needs 20 frames, and
   early ends when more than 20% of the last 40 lacked the worker's frame.
7. **Half rate** engaged at 0.85 of a frame and ran at 30 fps where free
   running kept 60 (a ~16 ms tick fits at 16.7 ms), and its 30 was uneven
   (steps of 0–3 frames). Behind `?halfRate=1` until it is steady.
8. **The working wait (4c)** became work-then-wait (above): the page's work
   and layout first, then the wait. Its deferrable-work slices are dropped.

**Work, then wait, against the old order** (drags, default controller;
`?pageLayout=a-b` re-lays out off-screen text for a random a–b ms each frame,
measured and steered; two runs each where shown):

| worker | layout | old order (`holdOrder=first`) | work, then wait |
|---|---|---|---|
| +0 | 3 ms | early, 99% same | early, 99% same |
| +2 | 2, 3, 1–4 ms | early, 99% same: the hold mostly declined ("no room"), but the busy page's frame went out after the worker's anyway | early, 99% same, one run of 2 ms fell back |
| +3 | none | late in both runs (5–7% same) | **early in both, 99% same** (373 held, 360 released) |
| +3 | 2 ms | early in one run, late in the other | late in both at a 4.5 ms starting limit; early in one of two at 5.5 |
| +4 | none | late | late at 4.5; early in one of two at 5.5 |
| +4 | 2 ms | late | late |

The page's work after the hold fell from ~2.5–3.7 ms (p50 under layout load)
to ~0.5–0.6 ms. DOM latency 35.5–36.1 ms in every run, no frame missed after
a hold, no DOM change after a hold. The gain is real but narrower than
hoped: a busy page whose frame goes out after the worker's already carries
Vello without any hold, so work-then-wait matters where the worker finishes
just after the page's own work, within Chrome's limit. The cases that still
fall back sit at that limit, where the first half-second of a drag decides.

**Drags, the default against free running (`?frameHold=off`):**

| worker tick | free | default (mode chosen) | DOM |
|---|---|---|---|
| ~3.8 ms (+0) | 99–100% same | early, 99.4% same | 35–36 ms |
| ~5.8 ms (+2) | 48–66% same, 17–21 flips (the race) | early, 98.6% same | unchanged |
| ~6.8–7.8 ms (+3, +4) | varies: 100% same in one run, 43/57 racing with the hold forced on | late after ~0.45 s, 95% one behind, 2 flips | unchanged |
| ~11.8 ms (+8) | 99% one behind | late, 99.7% one behind | unchanged |
| ~15.8 ms (+12) | 94% one behind, 5 repeats and 13 doubles in 260 steps | late, two behind, 0 repeats, 0 doubles | unchanged |
| ~19.8 ms (+16) | uneven | uneven (both runs probe-flagged) | unchanged |

With uneven page work (`pageBusy` 0–4 and 2–6 ms) the DOM stayed at 35–36
ms in every run; at +0 the default stayed early (99–100% same); at +2 with
2–6 ms it stayed early (99.4% same: the worker's frame went out before the
busy page's), and with 0–4 ms it fell back to late (93% one behind, 4 flips).

**Playback** (no input; the default is late with the clock one frame ahead):
smooth in every run (judder 0% for +0, page 3–6 ms, worker +4; 0.3% at +12
where free running had 1.1%). The worker's frame goes out ~0.13 ms after
vsync and makes that vsync's draw; with a page drawing at 4–6 ms after vsync
(the §6f race) 310 of 310 frames made the draw, 0 flips. A few frames went
out at the end of their tick instead of at the next BeginFrame (the canvas's
BeginFrame was queued behind the tick); they landed on the same vsync. At a
~20 ms tick (+16) neither mode keeps 60 fps evenly (default 16% bad, free 54%
uneven steps): past what frame modes can fix; the render scale is the lever.

**The indicator.** With the FPS readout on: `late` (warning colour) during a
fallback or a fixed `?frameHold=late`, `ahead` (muted) while the view
animates between interactions, nothing in early mode; the tooltip gives the
reason, and in early mode the learned limit and estimates. In developer
mode (Settings) the readout always shows the mode, `early` and `off`
(struck through) included; `off` says why in the tooltip (not cross-origin
isolated, not Chromium, the main-thread runtime, `?frameHold=off`). The
tooltip also names the engine build and the frontend's commit (`+changes`
when the working tree differed), so it is plain what is running (added
2026-11-06).

**Defaults.** `src-ui/runtime/session_adapter.ts`: `frameHold` is `auto`
when the page is Chromium and cross-origin isolated (`bun run dev`, or
the future backend's headers), otherwise off; `?frameHold=off|on|late|always`
override. Per device: `localStorage['iron.frameModes.v1']`, keyed by canvas
size (256 px buckets) and backend; two fallbacks in a row without a minute
of early mode since start the next session late, retrying after a cooldown.

**Open.** Windows and other GPUs; 120 Hz; Safari and Firefox (frame modes
stay off there, §6d); half rate; the ~20 ms tick case; the working wait.

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
