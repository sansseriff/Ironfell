# Perf Grid Runbook — 5K frame-skip investigation

Goal: prove (or kill) each remaining hypothesis for why the Bevy app skips frames on
the 5K monitor while a minimal wgpu/vello app at the same resolution holds 60fps.

Established so far (from source, not speculation):
- Surface config (format/alpha/usage) is equivalent between Bevy-on-web and vello's
  demo — that axis is dead.
- With no cameras, Bevy's GPU work is one clear pass + present. GPU throughput is not
  a plausible cause.
- Measured app CPU is ~5–6ms/frame even while stuttering → the failure is in frame
  *pacing* (who gets to produce a frame, when), not throughput.

Remaining suspects:
- **H1 worker present path**: worker rAF is paced by compositor BeginFrame grants with
  backpressure; large canvas commits change the grant pattern.
- **H2 tracing-wasm tax**: LogPlugin on wasm emits performance.mark/measure for every
  system span every frame (JS calls + string garbage + unbounded User Timing buffer);
  GC pauses / late commits interact with H1.
- **H3 something else Bevy does per frame on any thread** (falsifies H1/H2 if the
  main-thread cells stutter too).

## One build, URL-selected variants

Build once with the release pipeline, then serve the UI:

```sh
./build-wasm.sh
bun run dev   # or however you serve src-ui
```

Variant is chosen per page load via `?bevy=` (handled in `wasm_loader.ts` →
`init_bevy_app(flags)`):

| URL param        | App contents |
|------------------|--------------|
| *(none)*         | whatever `init_app` currently enables (vello, fps overlay, inspector) |
| `?bevy=empty`      | DefaultPlugins only, no cameras — "renders nothing" |
| `?bevy=empty,nolog`| same, LogPlugin (tracing-wasm) removed |
| `?bevy=nolog`      | normal app, LogPlugin removed |
| `?bevy=min`        | minimal plugin floor (window+render only, no log) — if this panics at startup, skip it; B0/B2 carry the signal |

Worker vs main thread: use the existing mode switch (worker is default; switch to
main-thread mode in the UI, or however `PanelManager.boot`/`switchMode` is driven).
The console prints `init_bevy_app variant_flags=N` at startup — confirm the cell
you think you're running.

## The cells

Run every cell at **two sizes on the 5K monitor**: small window (~1200×800 CSS) and
full screen. 30–60s per cell. Record cadence stats (below) + notes.

| Cell | App | Thread | What it discriminates |
|------|-----|--------|----------------------|
| B0 | `?bevy=empty` | worker | baseline repro of "nothing renders, still skips" |
| B1 | `?bevy=empty` | main | **run this first** — H1 lives or dies here |
| B2 | `?bevy=empty,nolog` | worker | H2 |
| B2m| `?bevy=empty,nolog` | main | H2×H1 interaction |
| B3 | `?bevy=min` | worker | schedule breadth (H3) |
| C1 | testing_latency `/test/webgpu-raf?size=full` | main | TS control, trivial WebGPU |
| C2 | testing_latency `/test/worker-webgpu?size=full` | worker | **key control**: worker+OffscreenCanvas+5K with zero Bevy |
| C3 | vello demo: `cd ../vello && cargo run_wasm -p with_winit --bin with_winit_bin --release` | main | known-good reference |

Decision tree:
1. **B1 smooth, B0 stutters** → worker present path (H1). Then C2: if C2 *also*
   stutters at 5K, Bevy is exonerated — it's a Chromium worker-canvas behavior
   (file upstream with the Perfetto trace; ship main-thread mode). If C2 is smooth,
   something Bevy-specific interacts with the worker pipeline → step 2.
2. **B2 fixes B0** → tracing-wasm (H2). Corollary check: does B0 get *worse* the
   longer the page is open? (unbounded mark/measure buffer). Keep LogPlugin off on
   wasm or configure a custom subscriber.
3. **B0 ≈ B1 (both stutter)** with C1/C3 fine → Bevy-side on any thread (H3);
   compare B3, and read the Perfetto GPU track for where the frame actually dies.

## Measuring cadence (in-page)

Both adapters now record every engine tick into a ring buffer (~2 min):
rAF timestamp + wasm tick duration. Zero-cost: two typed-array writes per frame.

From the app page console:

```js
__probeReset()   // start a clean window (e.g. after going fullscreen)
// ... let it run 30-60s ...
__probe()        // logs JSON; also stored in window.__lastProbeStats
```

How to read it:
- `rafDeltaMs.p50 ≈ 16.7` with tight p95/p99 → healthy.
- `rafDeltaBuckets.d24to36` dominating → locked to ~30fps; `d36to60` → ~20fps.
- **The money signal**: bad rAF deltas while `tickMs.p95` stays ~6ms proves frames
  are being *withheld from* the app, not blocked *by* the app. That plus which cell
  it happens in localizes the cause.
- Copy each cell's JSON into `testing_latency/manual_results/perf-grid-<cell>-<size>.json`
  (or a scratch doc) so we can diff them afterward.

For C1/C3 (main-thread pages without the probe), paste this in the console:

```js
(() => { const d = []; let last; const f = (t) => {
  if (last !== undefined) d.push(t - last); last = t;
  if (d.length < 1800) requestAnimationFrame(f); else {
    d.sort((a,b)=>a-b); const q = p => +d[Math.floor(p*d.length)].toFixed(2);
    console.log({ frames: d.length, p50: q(.5), p95: q(.95), p99: q(.99),
      max: +d[d.length-1].toFixed(2), over24: d.filter(x=>x>24).length,
      over36: d.filter(x=>x>36).length });
  } }; requestAnimationFrame(f); })()
```

(C2's render loop is in its worker; judge it via Perfetto + the page's own trial
recording rather than this snippet.)

## Perfetto: seeing what DevTools can't

DevTools' performance panel shows JS/renderer threads only. The decision about
whether your frame gets displayed — BeginFrame scheduling, compositor frame
aggregation, GPU process work — happens in other processes. Perfetto records all of
Chrome (renderer, workers, viz/display compositor, GPU main), which is exactly where
the "jumbled rAF cadence" is decided. chrome://tracing is deprecated in favor of this.

Recording (per failing cell + one healthy cell for contrast, ~15s each):
1. Go to https://ui.perfetto.dev → **Record new trace** (first time: it will prompt to
   install the "Perfetto UI" Chrome extension).
2. Target platform: **Chrome**.
3. Categories: enable the rendering/graphics tags, and additionally type in:
   `viz`, `cc`, `gpu`, `toplevel`, `benchmark`, `input`.
4. Start recording, reproduce the stutter for ~10–15s, stop. Keep the Perfetto tab
   open the whole time. Save the trace file per cell.

What to look at (Ctrl/Cmd-K to search track names):
- **DedicatedWorker thread** (your app's worker): `AnimationFrame` slices = worker rAF
  ticks. Measure their spacing directly. Between them you'll see the wasm tick and
  the WebGPU submit.
- **VizCompositorThread (GPU process)**: `Graphics.Pipeline` / `PipelineReporter`
  async slices — one per frame, showing each stage and the frame's fate. Search for
  `DidNotProduceFrame` and `MissedBeginFrame`. This is the direct answer to "why did
  the compositor skip my frame".
- **CrGpuMain (GPU process)**: actual GPU-side work per frame. Compare small vs 5K:
  if per-frame GPU slices stay ~1ms at 5K while frames drop, GPU throughput is
  formally exonerated (on top of the vello argument).
- **CrRendererMain**: your main thread — check whether Svelte/UI work or GC
  (`MajorGC`/`MinorGC` slices, also on the worker thread) lines up with the gaps.

Signatures → verdicts:
- Worker `AnimationFrame` slices arrive late/irregular, but each one is short, and
  viz shows withheld/missed BeginFrames → **H1** (pacing/backpressure), exactly the
  "6ms chunks jumbled" symptom.
- GC slices or long `performance.measure` storms on the worker right before the gaps
  → **H2**.
- `PipelineReporter` shows frames produced but dropped at aggregation with long GPU
  import/composite stages at 5K only → browser-side per-pixel cost; compare C2's
  trace to see if it shares the signature.

## Optional follow-ups (only if the grid is ambiguous)

- **Speed-optimized release**: `wasm-release` uses `opt-level="z"` + `wasm-opt -Oz`
  (size-tuned). A one-off profile with `opt-level = 3` and `wasm-opt -O3` tells us
  what the size optimization costs per tick. Not expected to change cadence, but it
  shrinks `tickMs` and therefore sharpens the H1 signal.
- **rust-wgpu-worker control**: fill in the reserved `testing_latency` slot — a Rust
  wgpu clear-only loop in the same worker harness — to split "Rust/wasm-bindgen in a
  worker" from "Bevy in a worker".
- Note: `run-wasm.sh` already references bevy#13345 (worker frame-interval weirdness
  in debug builds) — prior art that this pipeline is pacing-sensitive.
