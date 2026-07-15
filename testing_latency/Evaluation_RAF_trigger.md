# Reducing input-to-photon lag for Bevy on the web: worker vs main thread

A handoff brief for the agent building latency test programs. This captures a long
debugging discussion about why a worker-rendered square appears to drift in and out
of low latency relative to an event-based reference overlay, what the actual root
cause is, and which architectures can plausibly fix it.

The target stack is Bevy compiled to WASM, rendering via `wgpu` to a WebGPU
`OffscreenCanvas`, with pointer input arriving on the main thread. But almost
everything here applies to any worker-rendered canvas app (2D, WebGL, or WebGPU).

> Status note: most conclusions below are reasoned from browser architecture and
> primary-source documentation (Chromium docs, WHATWG/W3C specs, gpuweb issues,
> Mozilla/WebKit bugs), not from direct measurement on the target hardware. Several
> claims are explicitly flagged as "verify empirically." Treat the measurement
> sections as the source of truth once you have data; where data conflicts with the
> reasoning here, trust the data.

---

## 1. The symptom

While dragging a square, the worker-rendered square periodically lines up closely
with an event-based 2D-canvas reference overlay, then falls behind, then comes back.
The effect is **periodic over multiple seconds** — it reads as a beat frequency, not
as random jitter. The event-based reference path stays comparatively low-latency and
stable.

The key qualitative facts:

- It is periodic, not random.
- It is judged against a main-thread, event-driven reference overlay.
- It primarily affects the worker variants.

The periodicity matters enormously for diagnosis. See section 4.

---

## 2. Background: how worker rendering actually reaches the screen

A few architectural facts drive everything else. These are the load-bearing pieces.

### 2.1 Worker RAF is not phase-locked to main-thread RAF (by spec)

`DedicatedWorkerGlobalScope.requestAnimationFrame` was deliberately specified so that
worker animation callbacks are **independent of the browsing context's event loop**
and "not necessarily synchronized with graphics updates from the browsing context."
This is intentional. The whole point of OffscreenCanvas-in-a-worker is to decouple
worker rendering from a potentially janky main thread. The independence that gives
you jank-immunity is the same independence that permits phase drift. You cannot keep
one and drop the other — they are the same property.

### 2.2 In Chrome, the worker submits compositor frames directly to Viz

Per Chromium's own IPC documentation, when you `transferControlToOffscreen()` and
render in a worker, the worker's `OffscreenCanvasFrameDispatcher` is given its **own**
`CompositorFrameSink` with a direct two-way channel to the display compositor (the Viz
process). The worker calls `SetNeedsBeginFrame(true)` and receives BeginFrame signals
**directly from the compositor**, then submits compositor frames straight to Viz.

Consequences:

- The worker render path **bypasses the main thread entirely**. There is no
  main-thread hop for the pixels.
- For WebGPU specifically, the worker's frame is **already a GPU texture going
  straight to the compositor**. This is the shortest possible path, data-wise.
- The main thread's content (DOM, the reference overlay) is a **separate** compositor
  frame source.

### 2.3 One screen, but multiple independent producers feeding one aggregator

This is the crux. The screen image is produced by **one aggregator** (Viz), ticking
at a stable vsync. But that single aggregator **aggregates multiple independent
producers**:

- the main thread's renderer (DOM, reference overlay), and
- the worker's OffscreenCanvas (the square), via its own sink.

Viz draws whatever each producer has **ready at the aggregation deadline**. A Chrome
graphics-dev thread confirms the two paths are not frame-synchronized: a committed
OffscreenCanvas buffer "may present on screen in advance of the new DOM content (maybe
lead one frame), they are not synchronization."

So "same screen" does **not** imply "same frame-production clock." There is one
consumer clock (vsync, never desynced from the display) and two producer loops feeding
it.

---

## 3. What is and isn't avoidable

Two distinct effects get conflated. Keep them separate.

### 3.1 A fixed sub-frame offset — avoidable down to ≤1 frame, not zero

With two producers both genuinely locked to the same vsync, you get a **fixed**
sub-frame phase offset between them. That's ≤1 frame of lag and is not a beat. A fixed
offset is fine and is, in fact, the goal. It is the normal cost of crossing into a
separate compositor source.

### 3.2 A beat — NOT structural, it's a symptom of a clock error

If you observe a **periodic, multi-second beat**, that is by definition **two
near-but-unequal frequencies** (e.g. 60.0 Hz vs 59.94 Hz). A pure phase offset between
two loops locked to the *same* clock is constant — it does not produce a periodic
in-and-out. A beat means one of the loops is **not actually on the true display vsync**.

This reframes the whole problem:

> The beat you're seeing is evidence that one producer loop is being clocked at a rate
> slightly off the real display refresh. That is a findable, often fixable bug — not an
> immutable property of the architecture.

Known mechanisms that produce exactly this kind of off-by-a-fraction clock:

- A worker animation loop driven by a **timer-derived or estimated cadence** hardcoded
  near 60.0 Hz while the actual display is 59.94 / 60.05 / etc. This is a documented
  class of Chrome rAF/vsync bug historically.
- A worker loop re-armed via a fallback (`setTimeout`) or a **throttled / estimated
  BeginFrame** rather than the genuine vsync signal.
- Multi-monitor or VRR setups where the assumed refresh differs from the panel driving
  the window.

None of these are "the architecture forbids low lag." They're "one loop is on the wrong
clock."

### 3.3 The structural limit that *is* real

A **fixed** offset between two producers is unavoidable while there are two producers.
The only way to remove the phase degree of freedom entirely is to **collapse to one
producer**. With a single producer, "phase relative to the other producer" stops
existing — the one producer's frame is the frame, every vsync.

---

## 4. Why this is genuinely solvable for a 4 ms render

A Bevy scene that renders in ~4 ms sits inside a ~16.6 ms frame budget with enormous
headroom. If the only question were "does the work fit before the deadline," ≤1 frame
of lag would be trivial. It fits four times over.

So the latency badness is **not** a "work doesn't fit" problem. It's a combination of:

1. **Phase**: *when* in the frame the worker is triggered relative to the aggregation
   deadline. If the worker doesn't start until 2 ms before the deadline, it misses and
   slips a frame. If it starts right after a composite with ~14 ms of runway, it makes
   the deadline reliably. Early-starting is a real, worthwhile win — it widens the
   margin so the loop has to wander much further before it crosses the deadline.
2. **Clock**: if a producer loop is on a slightly-wrong frequency (section 3.2), you get
   the periodic beat regardless of how much margin you have.

Early-triggering attacks (1). It is a **probabilistic mitigation**: it pushes the
worker's typical finish time further left of the deadline, making misses rarer. It does
**not** pin the phase and does **not** fix a wrong clock. Worth doing, not sufficient
alone.

---

## 5. The WebGPU-specific trap: the ImageBitmap readback

A tempting "fix" is to take presentation away from the worker: have the worker render
into a detached OffscreenCanvas, call `transferToImageBitmap()`, post the bitmap to the
main thread, and display it via a `bitmaprenderer` context. This makes the **main
thread the sole compositor producer** (collapsing to one producer — section 3.3).

For **2D and WebGL**, this is genuinely zero-copy: `transferToImageBitmap` moves the
rendered texture **by reference**. The pixels never leave the GPU. This is a legitimate
single-producer fix for those backends.

For **WebGPU, this forces a readback and is a regression.** Getting an ImageBitmap out
of a WebGPU swapchain texture requires copying the contents out (the gpuweb spec
describes copying "the contents out of the texture... using a copyTextureToTexture
command"; Firefox's tracker states plainly that when the data lives on the GPU,
"transfers to ImageBitmap always cause readback"). For WebGPU this risks the absurd
round trip **GPU → CPU → GPU** every frame. A 4K frame is tens of MB; do not do this on
WebGPU.

> Net: the bitmap+`bitmaprenderer` single-producer trick is correct for 2D/WebGL and a
> performance regression for WebGPU. Since the target is `wgpu`/WebGPU, treat this route
> as off the table unless measurement shows the readback is somehow cheap on the target
> platform (unlikely — verify only if curious).

Also note: there is **no present-timing hook** exposed for worker WebGPU. The compositor
drives the worker's BeginFrame directly; you cannot insert yourself between the worker's
submit and Viz's aggregation deadline. So for worker WebGPU you cannot collapse the
phase from the main thread without the readback.

---

## 6. Candidate architectures

Ranked roughly by how directly they attack the root cause.

### A. Render on the main thread during interaction; worker during idle ("two modes")

**This is the recommended direction.** The game simulation can live wherever, but the
*render + present* happens:

- **On the main thread during active pointer interaction** (drag, mouseover). This gives
  you **one producer**: the square and the reference overlay both go through the main
  thread's single compositor frame, on true vsync, sampled at one deadline. The entire
  phase-drift degree of freedom disappears — structurally, not probabilistically. Input
  arrives, main RAF fires with the freshest coalesced pointer sample, you render the
  4 ms frame, it composites that same frame. ≤1 frame of lag, no beat, because there is
  nothing to beat against.
- **In the worker during idle / ambient / video-like animation**, where main-thread
  isolation is valuable and input latency is irrelevant.

Why this mapping is natural: the worker's isolation is **least** valuable exactly when
the user is interacting (they're doing one thing — dragging — so the main thread isn't
also busy), and **most** valuable during idle background animation. The switching idea
lines up with where each mode's strength actually matters.

**Cost / risk — the handoff is where this lives or dies.** You need:

- Game state renderable from either thread (render path + simulation state portable
  across the thread boundary).
- A clean mode switch with **no dropped or doubled frame**. Prefer a brief **overlap**
  over a brief **gap**: have the incoming producer render its first frame *before* the
  outgoing producer stops, then stop the outgoing one. A gap at `pointerdown` /
  `pointerup` produces a visible hitch at the worst possible moment.

This is real engineering work, but it has a known shape and it attacks the producer
count, which is the actual root cause.

### B. Always-in-worker, switch the *trigger* (worker RAF ↔ main-thread RAF message)

The worker always owns the canvas; you switch only what kicks off each frame:

- idle: worker's own RAF,
- interaction: a postMessage (or SAB flag) from the main thread's RAF.

**Honest assessment: weaker than A.** This does **not** change the producer count. In
both modes the worker is still a separate compositor producer with its own sink,
sampled at the aggregator's deadline on a clock you don't control. Main-thread
triggering only changes *when the worker starts* (the section-4 margin win). It does not
collapse the two-producer structure and does not put the worker on the true vsync if it
was on a wrong clock. So it can reduce misses but cannot, on its own, eliminate the beat.

Use B only if the handoff in A proves too costly and you're willing to accept a
probabilistic improvement rather than a structural fix.

### C. Everything in the worker, including an in-worker reference/overlay

If the *reason* you have two producers is that the square is in the worker and the
reference overlay is on the main thread, another single-producer option is to move the
overlay **into the worker** too. Then only one compositor source feeds aggregation and
there's nothing to drift against. Whether this is acceptable depends on whether the
overlay needs to be DOM/main-thread for other reasons. (In the test harness it's a
reference baseline, so this may or may not be meaningful — but for a *shipping* game
with a single worker-rendered world and no main-thread visual competing on the same
screen region, "just render everything in the worker" is the clean answer and the beat
becomes invisible because there's no second producer to compare against.)

### D. Main-thread WebGPU permanently

Render the whole game on the main thread always. One producer, always on true vsync,
freshest input at RAF. Lowest and most consistent input-to-photon latency. You lose
main-thread isolation: heavy DOM/UI work can now jank the render. For a 4 ms render and
a game that isn't doing much else on the main thread, this may be entirely acceptable
and is the simplest thing that gives ≤1 frame reliably. Worth including as a baseline to
measure against even if you don't ship it.

### E. Early-trigger optimization (applies on top of B, or any worker-render mode)

Trigger the worker as **early in the frame as possible** — ideally right after a
composite — to maximize the runway before the worker's deadline. Reduces missed-deadline
slips. Probabilistic, not structural. Cheap. Stack it under whatever else you do.

---

## 7. On SharedArrayBuffer

SAB is frequently cited as "the fix." Be precise about what it does:

- **What it fixes:** stale pointer data and message-queue jitter. The worker reads the
  latest pointer state atomically at the moment it starts rendering, with no postMessage
  round trip. Each worker frame renders from up-to-the-moment coordinates.
- **What it does NOT fix:** worker RAF phase drift / the beat. SAB is a faster data
  channel, not a synchronization primitive between the worker loop and vsync.

So in any worker-render mode, feed input via SAB so that whatever frame the worker
produces is rendered from the freshest input — this minimizes the *visible* lag even
when phase drift remains. But do not expect SAB to remove the beat. If SAB variants show
the same periodic drift as non-SAB variants (with less noise around it), that's the
expected signature: SAB removed the message jitter, the underlying phase/clock issue
remains.

---

## 8. Why you cannot just block the main thread to wait for the worker

For completeness, since it comes up: you cannot make main-thread RAF block until the
worker finishes rendering "this frame's" input.

- The main thread's rendering pipeline is run-to-completion. Blocking inside RAF freezes
  style/layout/paint/composite/input for the whole tab.
- `Atomics.wait` is forbidden on the main thread for exactly this reason.
- There is no "pause the compositor, run the worker, resume" primitive.

So every viable approach is fire-and-continue: either collect the worker result later in
the same frame (only cheap for 2D/WebGL via bitmap), or present the freshest
already-completed worker frame, or render on the main thread directly.

---

## 9. What to actually measure (the diagnostic that settles it)

The single most important measurement is **frequency, not latency**.

### 9.1 Frequency fit — is there a real clock mismatch?

Log, over ~10 seconds:

- main-thread RAF timestamps (every frame),
- worker frame timestamps (every frame),

and fit the slope (effective Hz) of each.

- If **both are 60.000 Hz** (or both exactly the true display refresh): there is no
  frequency error. You should then see a **constant** offset between them, ≤1 frame.
  Any remaining lag is phase/margin — attack with early-trigger and single-producer.
- If **one is, say, 59.9 and the other 60.0**: there's your beat. The beat period is
  `1 / |f_main − f_worker|` seconds. The fix is forcing the off-clock loop onto the
  true display vsync, not rerouting pixels. **This is the most likely culprit given the
  reported periodic symptom.**

### 9.2 Phase trace — where in the frame does the worker run?

Per frame, record:

- main RAF timestamp,
- the time the worker was triggered (message send, or SAB flag set),
- the time the worker started rendering,
- the time the worker finished / submitted,
- delta between worker finish and the next vsync estimate.

This tells you whether the worker is starting deep into the cycle (margin problem,
section 4/6E) versus starting early but still slipping (clock problem, section 3.2/9.1).

### 9.3 Suggested probe variants (controls)

- Worker render driven by **worker RAF** (baseline).
- Worker render driven by **main-thread RAF message** (tests section 6B / margin).
- Worker render driven by **`setTimeout(0)`** or a fixed timer (control: if this beats
  identically to worker RAF, the cause is generic task/clock timing, not RAF-specific
  scheduling).
- **SAB input + worker RAF** (tests section 7).
- **SAB input + main-thread-driven worker render** (margin + fresh input together).
- **Main-thread WebGPU** with identical render work (section 6D baseline — isolates
  "OffscreenCanvas worker effects" from "render cost").
- Worker renders every frame but **samples latency only on main-thread RAF** (isolates
  the measurement-sampling phase from the render phase).

### 9.4 Measurement-harness caveat

The harness samples worker latency when the worker posts a `rendered` message back to
the main thread. That measured number includes pointer arrival, main→worker (or SAB)
delivery, worker RAF timing, render, present scheduling, worker→main postMessage, and
main-thread handling of the callback. The **visual** experience also includes compositor
timing not captured by the `rendered` timestamp. So treat the `rendered`-message latency
as a proxy, and cross-check against the visual beat (e.g. capture both squares with a
high-speed/screen capture and compare positions), since the thing you ultimately care
about is photons, not the callback time.

---

## 10. Recommended plan

1. **First, measure frequency (9.1).** This is the fork in the road. The reported
   periodic beat strongly suggests a real clock mismatch, which would mean the fix is
   "put the worker loop on true vsync," not an architectural rebuild. Settle this before
   building anything.

2. **Confirm Chrome's worker-WebGPU BeginFrame source on the target platform** — is the
   worker's OffscreenCanvas BeginFrame the true display vsync, or an estimated/throttled
   cadence? This single fact decides whether the beat is a clock bug. (Reasoned here from
   architecture; verify on real hardware.)

3. **If it's a clock mismatch:** focus on getting the worker loop onto the genuine vsync
   signal. No rebuild needed.

4. **If frequencies match and lag is still too high:** it's phase/margin. Then:
   - apply early-trigger (6E) + SAB input (7) as cheap wins, and
   - pursue **single-producer during interaction** (6A, or 6C/6D depending on product
     constraints) as the structural fix. This is the only thing that removes the phase
     degree of freedom rather than narrowing it.

5. **Avoid the WebGPU ImageBitmap route (section 5)** — it's a readback regression on
   WebGPU.

6. **Validate the handoff (6A) in isolation** before integrating: build the
   mode-switch with an overlap (not a gap) and confirm no hitch at `pointerdown` /
   `pointerup` with a frame-accurate capture.

---

## 11. One-paragraph summary for the impatient

The periodic beat is almost certainly **not** an unavoidable cost of using a worker —
it's a **symptom that one render loop is on a slightly-wrong clock** (e.g. an estimated
~60 Hz cadence vs the true 59.94 Hz display), which produces a `1/Δf` beat. A *fixed*
sub-frame offset between the worker producer and the main-thread producer is structural
and unavoidable, but that's only ≤1 frame and was never the problem. The fix path:
**measure each loop's effective frequency first**; if they differ, get the worker onto
true vsync; if they match and lag persists, it's phase/margin, so early-trigger + SAB
input help probabilistically while the real structural fix is collapsing to **one
compositor producer** — which on WebGPU means rendering on the **main thread during
interaction** (or rendering everything, including any overlay, in one place), **not** the
ImageBitmap trick (a GPU→CPU→GPU readback regression on WebGPU) and **not** merely
triggering the worker from the main thread (which leaves two producers intact and only
buys margin).