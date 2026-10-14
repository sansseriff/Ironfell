# Renderer sanity checks and architecture decision

Date: 2026-10-13. Working-tree experiments after the SIMD/selection/spatial work.
Read with `render-benchmark.md` §6–7 and `infinite-canvas-architecture-patterns.md` §13.

## Decision

A development-build result of ~19 FPS moving 1,000 ordinary mixed shapes is not
an acceptable editor target. It is also not evidence that Rust or Bevy imposes
that ceiling. The optimized SIMD build moves the **whole** 1k Stress field near
60 FPS in these local tests, including a 5120×2880 backing store. The owner
confirmed that reloading the optimized build made the scene noticeably smoother
in Helium, their Chromium-based browser.

Do not declare the architecture finished or guarantee perfectly smooth 60 FPS.
There are occasional long hover/selection frames, limited margin during group
movement, and known costs that grow with descendants. The standalone same-version Vello
control is now built and measured, including changes to every shape every frame
(§7). It shows substantial outline cost even without Bevy. Prioritize selection
feedback and repeated derived geometry before a large raster-cache project.

## 1. Why the owner saw much worse performance

The localhost link sent after the previous implementation served the restored
**`dev-opt`** artifact. The reported performance tables used **`wasm-bench` with
`+simd128`**. They were different builds:

| Build | Optimization | SIMD configuration |
|---|---|---|
| `dev-opt` | level 1, 256 codegen units | harness does not enable SIMD |
| `wasm-bench` | level 3, 16 codegen units | harness enables `+simd128` |

These settings are in `Cargo.toml` and `.claude/skills/run-iron/build.sh`.
`.cargo/config.toml` does not add SIMD. The published size-oriented
`wasm-release` profile is yet another configuration; it was not tested here.
This experiment changes optimization and SIMD together, so it must **not** be
reported as a SIMD-only improvement. The earlier SIMD-only A/B is in benchmark §6.

The earlier nine-scenario benchmark also omitted **dragging the selected whole
field**. It tested moving one leaf and rotating via a slider with the group
unselected. Cached selection geometry solves unchanged-frame work, but its
conservative invalidation still rebuilds group bounds and resize eligibility
when a selected group moves.

## 2. Full-app interaction results

Chrome 155.0.8059.40, Apple M5, ANGLE Metal, headless hardware acceleration;
normal rAF pacing, **no** `UNTHROTTLED`. These automate Chrome, not Helium itself.
Each fixture contains N requested shapes plus the extra sample rectangle.
Input is delivered through the normal app bridge. Identity assertions verify
that the field—not a cell or leaf—is moved, and that subsequent scenarios start
with the intended selection.

| 1k whole-field drag | Callback FPS | Changed-frame CPU p50 | p95 |
|---|---:|---:|---:|
| dev-opt, 1600×1000 DPR 1 | 19.49 | 51.1 ms | 52.1 ms |
| dev-opt, 1600×1000 DPR 2 | 19.38 | 51.3 ms | 52.2 ms |
| optimized SIMD, 1600×1000 DPR 1 | 59.69 | 11.7 ms | 12.7 ms |
| optimized SIMD, 1600×1000 DPR 2 | 60.00 | 11.4 ms | 12.3 ms |
| optimized SIMD, 2560×1440 DPR 2, worker | 60.00 | 12.4 ms | 14.1 ms |
| same 5K backing, worker, no LogPlugin | 59.41 | 11.7 ms | 13.0 ms |
| same 5K backing, main thread, no LogPlugin | 60.00 | 13.4 ms | 15.0 ms |

In the development group-drag case, encoding is ~42.5 ms p50. In the optimized
DPR 2 case it is ~5.3 ms; overlay geometry still costs ~2.5 ms. That locates a
large part of this particular problem in build/encoding and repeated derived
geometry, not spatial querying.

Optimized leaf dragging is ~6.0 ms changed-frame p50 at DPR 2. Hover transitions
are more uneven: changed-frame p50 ~10.2 ms, p95 ~26.8 ms. A resting hover is
cheap, but entering a large group can still hitch. Average callback FPS does not
make those transition costs disappear.

### Continuous input check

WebDriver pointer calls and wheel delivery are not exact 60 Hz producers. To
avoid treating idle ticks as proof of continuous smooth motion,
`bench-group-raf.txt` holds a real press and sends pointer moves each main-thread
rAF through the ordinary DOM input bridge.

At 5K backing, the producer sends 181 updates over 3016.5 ms; the worker records
180 ticks and 177 changed frames. Callback cadence is 59.34 FPS, with 177
intervals below 18 ms and two ~33 ms intervals. Changed-frame CPU p50/p95 is
11.8/13.3 ms, maximum 27.8 ms. This is near the target with occasional hitches;
it is not a zero-dropped-frame result.

None of these FPS figures is a GPU-completion or input-to-photon measurement.
For a claim about actual displayed frames, use Chromium FrameSequenceMetrics /
PipelineReporter traces as described in `perf-grid-runbook.md`. The worker vs
main/no-log comparison did not reproduce a persistent 5K pacing failure here;
that does not resolve every browser/configuration issue in the older runbook.

### A pitfall caught by the assertions

A second press soon after selecting the field was interpreted as a double-click
and drilled into a 100-shape cell. The target assertion failed. Those runs were
discarded. The reusable test now lets the double-click window expire and checks
selection identity again after motion. This is why target identity and bounds
update counts matter when publishing performance figures.

## 3. Independent renderer controls

`renderer-sanity.html` and `src-ui/bench/renderer-sanity.js` replay the actual
Stress field exported from a canonical Iron save. The exporter preserves painter
order, transforms relative to the field, rectangle dimensions, circle radii,
colors, stroke widths, and cubic Bézier control points. It checks N+1 primitives
and sample identity. Frozen fixtures and SHA-256 hashes are recorded below.

The target is **2040×1490 physical pixels**, matching the DPR 2 Stress panel in
the 1600×1000 app. 1k uses zoom 1; 10k uses zoom 0.37. Every requested primitive
is submitted; the Canvas2D control has no host spatial culling. Tests warm up for
500 ms and measure three seconds per scenario, one renderer at a time.

- **Canvas2D:** creates retained `Path2D` geometry once, clears the target and
  fills/strokes paths each frame under changing transforms. There is no authored
  bitmap cache or dirty-region rendering. `Path2D` is geometric reuse, not an
  assertion that the browser performs no internal caching.
  [MDN's Path2D contract](https://developer.mozilla.org/en-US/docs/Web/API/Path2D).
- **ThorVG WebCanvas 1.1.2:** retained `Shape`/`Scene` objects, `engineOption=None`
  to disable optional smart/partial rendering, explicit physical target size with
  automatic DPR scaling disabled. Both `gl` and `wg` are tested. Shapes are
  updated and rendered through the published web API; no custom bitmap cache.
  The pinned NPM package is checksum-verified and kept in gitignored `.agents`,
  rather than changing Iron's runtime dependencies.
  [Official web API tutorial](https://www.thorvg.org/web-tutorial) and
  [official integration repository](https://github.com/thorvg/thorvg.web).

CPU figures below time the synchronous drawing/update/submission call. They do
not include asynchronous GPU completion. The control does not include document
solving, Bevy scheduling/propagation, picking, inspector/session synchronization,
resize eligibility, or final full-window composition.

| Field size / scenario | Canvas2D CPU p50 / FPS | ThorVG GL CPU p50 / FPS | ThorVG WG CPU p50 / FPS |
|---|---|---|---|
| 1k group movement + outlines | 2.1 ms / 60 | 3.8 ms / 60 | 5.0 ms / 60 |
| 1k rotation | 2.6 ms / 60 | 2.6 ms / 60 | 4.1 ms / 60 |
| 1k leaf movement | 2.6 ms / 60 | 1.8 ms / 60 | 4.1 ms / 60 |
| 1k hover outlines | 2.7 ms / 60 | 2.8 ms / 60 | 4.5 ms / 60 |
| 10k group movement + outlines | 5.4 ms / 60 | 33.7 ms / 29.7 | 25.3 ms / 39.1 |
| 10k rotation | 4.7 ms / 60 | 9.7 ms / 60 | 19.8 ms / 50.3 |
| 10k leaf movement | 4.6 ms / 60 | 10.2 ms / 60 | 19.9 ms / 50.1 |
| 10k hover outlines | 5.1 ms / 60 | 10.9 ms / 44.1 | 20.7 ms / 44.1 |

The control outlines use 2 px half-alpha blue. The app's selected outlines are
opaque, and its hover/selection treatment includes extra group frames/grips.
Motion trajectories and delivery also differ. Consequently these are useful
isolated reference workloads, **not** a full-editor renderer ranking or a ratio
that can be applied directly to Iron's full-frame timings.

### Rendered-output checks

Screenshots of Iron, Canvas2D and both ThorVG backends were inspected. The first
ThorVG run accidentally used an obsolete stroke overload and omitted strokes;
its results were rejected and rerun with the documented stroke-options object.
The retained results include strokes and outlines. Static Canvas2D and ThorVG
field output agree closely in geometry/color; antialiasing differs. This is not
pixel-identity certification across different renderers or a comprehensive test
of masks, text, effects, gradients, clipping or color management.

## 4. What this establishes—and what it does not

- The current stack can deliver useful near-60-FPS 1k interaction on this machine
  with the correct build. That is evidence against an inevitable 1k Rust/Bevy
  ceiling, and the owner's Helium observation supports the build explanation.
- Canvas2D is a credible inexpensive reference and draws this simple scene
  efficiently without an application-authored raster cache. Its 10k result is
  reason to investigate the difference rather than explaining every slowdown as
  “we just need Figma's caches.”
- ThorVG is not uniformly faster, and WebGPU does not automatically win over
  WebGL. Shape/stroke mix, retained geometry, renderer implementation and workload
  matter. One short test is not a general ranking of graphics libraries.
- The original Canvas2D/ThorVG controls did **not** isolate Bevy's overhead.
  The subsequent standalone Vello control (§7) establishes a renderer-only
  baseline, but does not yield an exact subtraction for the complete editor:
  host timing, overlays, and final composition differ.
- CanvasKit/Skia was not tested. Browser Canvas2D is not a benchmark of a native
  desktop Skia application or its separately compiled WASM bindings.

Figma's published architecture is a custom optimized tiled renderer, and its
current WebGPU implementation batches resource updates and draws. Its engineering
article explicitly describes why simply adopting WebGPU can regress performance.
These are architectural practices to learn from, not evidence for what an
uncached Figma would do on this scene.
[Original renderer architecture](https://www.figma.com/blog/building-a-professional-design-tool-on-the-web/),
[WebGPU renderer engineering](https://www.figma.com/blog/figma-rendering-powered-by-webgpu/).

## 5. Next tests and implementation priorities

### Completed: isolate Vello without Bevy

The standalone replay of **`vello_hybrid 0.2.0`** now exists as
`src/iron_renderer_probe`, with both wgpu WebGL2 and WebGPU backends. It uses
optimized SIMD, the same frozen fixtures and physical target, and resets one
complete scene each frame. See §7 for the checked results and limits.

The renderer alone spends ~6.6 ms submitting a 1k group with outlines, and
~36.5 ms at 10k on WebGL2. Changing every shape's geometry **without** additional
selection outlines costs ~3.9 ms / ~13.8 ms respectively. This means both the
renderer's workload and the editor's extra work matter; moving out of Bevy alone
would not solve the dense outlined case. It also means bitmap caching is not a
prerequisite for this particular 10k unselected animated field to reach 60 Hz
callback cadence on the M5.

Keep using Chromium presentation traces for claims about displayed frames. A
callback interval and a synchronous API call do not measure GPU completion.

### Alongside it: fix avoidable editor work

1. Split cached local/subtree geometry and eligibility from world placement.
   Moving a group does not normally change every descendant's resize policy or
   local bounds. Reuse local bounds and apply the gesture/world affine. Retain
   invalidation for descendant edits, reparenting, bindings, handles, constraints,
   point changes, undo/load and embedded occurrence mapping.
2. Put hover/selection feedback in an independently retained **overlay target**.
   A separate layer in the same dirty target still rebuilds all content. Confirm
   that a hover transition leaves content-target encoding/rasterization unchanged.
3. Measure 1k and 10k *selected* group movement/rotation, enter/leave transitions,
   leaf dragging and dense-group resizing at display cadence. Report p95/p99 and
   actual changed-frame rate, not only idle-inclusive FPS or CPU p50.
4. Then form painter-contiguous presentation chunks and measure selective subtree
   baking where work remains expensive. Baking saves raster/encoding work but
   does not automatically remove descendant Bevy propagation/index updates.

`iron_document` and `iron_spatial` are renderer-independent foundations. A future
Canvas2D/ThorVG/Skia adapter or standalone Vello host could consume derived drawing
primitives without moving authority into Svelte. Bevy can remain the 3D host even
if a measured future design uses a different 2D presentation path. That is a
possible design, not an implemented migration.

### Versions and additional references

The app currently uses Vello Hybrid **0.2.0**, not Vello Research/Classic. Current
upstream sparse-strips 0.3 renamed Hybrid to Vello GPU and moved to wgpu 30;
Iron/Bevy currently uses wgpu 29. Compare newer Vello in the standalone control
before attempting a coupled dependency upgrade. The upstream architecture makes
clear that the sparse-strips GPU renderer still does CPU preprocessing, whereas
Vello Research uses a different, compute-based pipeline.
[Upstream architecture](https://github.com/linebender/vello/blob/main/ARCHITECTURE.md),
[Sparse Strips 0.3 release](https://github.com/linebender/vello/releases/tag/sparse-strips-v0.3.0).

If an additional Skia baseline is useful after Vello isolation, CanvasKit is the
official Skia-to-WASM route. It should be a separate control, not inferred from
Canvas2D's result. [CanvasKit documentation](https://skia.org/docs/user/modules/canvaskit/).

## 6. Reproduce and inspect

The optimized build is deliberately left served for the owner; a development
rebuild will replace it again. Reload an existing browser tab after switching
artifacts. Do not build/rebind WASM while a driver run is navigating.

```sh
PROFILE=wasm-bench .claude/skills/run-iron/build.sh
.claude/skills/run-iron/serve.sh start
GPU=1 PANEL_PIXELS=physical SIZE=1600x1000@2 \
  URL='http://localhost:5173/Ironfell/?gfx=webgl2&layout=default&stress=1000' \
  bun run .claude/skills/run-iron/drive.ts < .claude/skills/run-iron/bench-interaction.txt
GPU=1 PANEL_PIXELS=physical SIZE=2560x1440@2 \
  URL='http://localhost:5173/Ironfell/?gfx=webgl2&layout=default&stress=1000' \
  bun run .claude/skills/run-iron/drive.ts < .claude/skills/run-iron/bench-group-raf.txt

# Uses archived DPR-2 fixtures if no local export exists; downloads checksum-pinned
# ThorVG into .agents. It does not add ThorVG to the application dependencies.
sh .claude/skills/run-iron/build-renderer-probe.sh
DPR=2 COUNTS=1000,10000 SCENARIOS=group-plain,group-drag,all-transform,all-geometry \
  bun run .claude/skills/run-iron/compare-renderers.ts run
# To regenerate the fixtures from the current Iron document first:
DPR=2 COUNTS=1000,10000 bun run .claude/skills/run-iron/compare-renderers.ts capture
# Quick static output check without another timing run:
DPR=2 COUNTS=1000 bun run .claude/skills/run-iron/compare-renderers.ts preview
```

Open `http://localhost:5173/Ironfell/renderer-sanity.html` to try the isolated
scene manually. Change `shapes=10000&zoom=0.37` for the large field; select
Canvas2D, ThorVG WebGL/WebGPU or Vello Hybrid WebGL/WebGPU with the dropdown.
The two new buttons move every shape and change every shape's local geometry.

Raw checked intervals and frozen fixtures:

- [36 full-app intervals](benchmarks/render-interaction-paced-2026-10-13.jsonl).
- [24 independent renderer intervals](benchmarks/render-independent-controls-2026-10-13.jsonl).
- [1k frozen fixture](benchmarks/stress-1000-fixture-2026-10-13.json.gz),
  SHA-256 of decompressed JSON:
  `51371d38e9e8b75667da051c2c89759a0c3fbb706d5cfc83f4f41168cf56eab2`.
- [10k frozen fixture](benchmarks/stress-10000-fixture-2026-10-13.json.gz),
  SHA-256 of decompressed JSON:
  `043fa928df14e2571751cea1892175392ae3b6dc3b58355c9ec564af017e1db1`.

Existing `bun run check` failures remain at 5,009 errors in the same five files:
`Github.svelte`, `utils.ts`, and three generated wasm JS files. No errors are
reported in the new isolated benchmark file. The driver is outside the app
typecheck scope and was validated through successful browser runs. No application
Rust behavior or runtime dependency was changed. The follow-up adds a separate
Bevy-free diagnostic workspace crate, with build artifacts under `.agents`; its
build does not overwrite the served Iron WASM.


## 7. Follow-up: all shapes change, and one standalone Vello scene

The owner's cache concern was correct: the original group/leaf tests reused
unchanged local geometry. That is useful for editors but did not answer what
happens when **every** shape's geometry changes. The additional controls use the
same fixture, target, zoom, painter order, paints, strokes and trajectory across
all five backends. These results supersede the original test's scope for this
question; the original data remains preserved separately.

### What changes every frame

- `all-transform`: each shape has independent continuously changing x/y offsets
  (amplitudes 12/8 document pixels, distinct phases). Local geometry is retained.
- `all-geometry`: those same independent offsets **plus** a distinct continuously
  changing size factor, `1 + 0.2*sin(2*t + i*0.618)`, applied to each rectangle's
  width (height uses `2-factor`), circle radius, and local path/control-point
  coordinates. Paths also displace their two cubic controls in opposite y
  directions by `10*(factor-1)`: this changes curvature beyond an affine
  placement/scale of the original path.
  Canvas2D constructs a **new Path2D for every shape on every frame**. ThorVG
  calls `Shape.reset()` and rewrites path commands while retaining style/identity.
  Vello copies/resizes the local coordinates into a reusable scratch path and
  regenerates the scene's strips. The factor is applied to geometry, not merely
  left as a scale matrix around unchanged paths.
- `group-plain` / `group-drag`: identical group movement without/with extra
  outlines. The matched A/B changes only outline visibility. In `group-drag`,
  the group moves with 2 px half-alpha blue outlines on every leaf,
  allowing comparison with the previous dense feedback workload. The two
  independent-animation cases have ordinary document strokes but no added
  selection outlines.

Shared JS motion-buffer generation is inside the measured call for all engines.
The Vello control includes copying those buffers across WASM, geometry mutation,
scene preprocessing, surface acquisition and submission. No precomputed frame,
application raster/subtree cache, dirty-region policy, or host culling is used.
All backends draw the same solid background; this avoids WebGL surface-alpha
support differences. ThorVG smart rendering remains disabled. Hidden outline
objects are not rewritten during geometry animation or subjected to redundant
transform setters; retained renderer APIs are used normally.

Independent motion and geometry edits invalidate an unchanged-image bake of the
whole subtree and repeatedly damage tiles across the field. Some individual
primitive fills could still reuse resampled pixels; this harness provides no such
cache. It does **not** disable legitimate renderer optimizations, and we
cannot claim that Chromium has no internal caches. Rectangles remain rectangles,
circles remain circles, and path topology is unchanged (curvature changes).
This is a workload in which our proposed static-image caches have little reuse, **not** the universal
worst case of arbitrary huge paths, changing topology, blur, masks or overlap.

### Checked results

Same Chrome/M5, 2040×1490 physical target, DPR 2, 500 ms warm-up / 3 s samples,
normal vsync. Zoom is 1 at 1k and 0.37 at 10k, fitting the whole field; the
10k shapes are smaller on screen. This does not establish throughput for 10k
shapes at the 1k per-shape pixel size. N includes one extra sample shape.
CPU = synchronous host API work and submission, **not GPU completion**. FPS = measured rAF callback cadence.

| 10k requested shapes | Canvas2D CPU p50 / Hz | ThorVG GL | ThorVG WG | Vello Hybrid GL | Vello Hybrid WG |
|---|---|---|---|---|---|
| All independent transforms | 5.1 ms / 60 | 11.6 ms / 60 | 20.7 ms / 48.1 | 14.2 ms / 60 | 14.9 ms / 60 |
| All geometry + transforms | 6.4 ms / 60 | 13.2 ms / 60 | 22.0 ms / 45.4 | 13.8 ms / 60 | 14.6 ms / 60 |
| Group movement, no extra outlines | 4.9 ms / 60 | 10.8 ms / 60 | 20.0 ms / 49.7 | 14.1 ms / 60 | 14.8 ms / 60 |
| Identical movement + all outlines | 5.6 ms / 60 | 34.3 ms / 29.2 | 25.9 ms / 38.3 | 36.5 ms / 27.2 | 38.3 ms / 26.0 |

All five sustain 60 Hz callback cadence at 1k in all three scenarios. For 1k
all-geometry, CPU p50 is Canvas2D 2.0 ms, ThorVG GL 2.6 ms, ThorVG WG 4.3 ms,
Vello GL/WG 3.9/4.2 ms. At 1k group+outlines, Vello GL/WG is 6.6/6.7 ms.
Differences of fractions of a millisecond in short runs should not be treated as
a strong ranking.

At 10k all-geometry, CPU p95 is 7.2 ms Canvas2D, 13.4 ms ThorVG GL, 22.3 ms
ThorVG WG, 14.2 ms Vello GL, 15.2 ms Vello WG. The 60-Hz cases have zero measured
rAF intervals above 24 ms; ThorVG WG has 44. Vello GL still has one
23.1 ms synchronous call: callback cadence alone is not a tail-latency guarantee. Vello has limited headroom for the editor's document/bounds/picking/feedback work. Do not promise that the full app
can animate/edit every one of 10k nodes at 60 FPS on the basis of this control.

Standalone WASM SHA-256: `12de1517c8c7f459e92c3467dd7efc9b4a1459d60e31a69a517897d0e65f5791`.
The served Iron artifact remains `57450a52ae35d5965a640c2faf64fba604d58ab540c20ab2f4407754d5554ac0`.
The control build does not rebind Iron.

[40 retained intervals, with versions and GPU metadata](benchmarks/render-dynamic-controls-2026-10-13.jsonl).
All five backends were rerun after introducing non-affine curve changes and
removing redundant hidden-outline setter work; only the final geometry run and subsequent paired outline A/B are archived.
Static and animated snapshots at t=0.731 were inspected/compared across all backends. Dimensions match; mean RGB absolute
difference from Canvas2D over the upper-left 1098×848 physical-pixel field area
is at most 3.061/255. Animated output differs substantially from static output
for every backend. This checks that the geometry mutation is visible and that
strokes/order/placement agree broadly; antialiasing is not identical. Artifacts
are under `.agents/render-perf/renderer-*-geometry-*-dpr2.png`.

### Interpretation and next work

1. Editing does not rule out caches. Retained geometry and style survive ordinary
   placement changes; unaffected subtrees can remain baked while the edited
   subtree draws live. The new all-geometry case deliberately removes that
   static-subtree advantage. `Path2D` retention is not a bitmap cache.
   [MDN Path2D](https://developer.mozilla.org/en-US/docs/Web/API/Path2D).
2. Browser Canvas2D handles this all-changing simple field efficiently. Do not
   explain its advantage solely by unmeasured hidden raster caching. Conversely,
   this browser API result is not a direct CanvasKit/native Skia benchmark.
   [Skia's separate CanvasKit API](https://skia.org/docs/user/modules/canvaskit/).
3. The Vello control is the **same Hybrid 0.2 renderer Iron uses**, doing CPU
   preprocessing and GPU rasterization, not compute-based Vello Research/Classic
   and not the newer sparse-strips release. A single complete scene answers the
   host-overhead question more closely than a renderer swap does, but full-editor
   timing still encloses extra work. Do not attribute the standalone outlined
   slowdown to Bevy.
   [Pinned release architecture/API](https://github.com/linebender/vello/releases/tag/sparse-strips-v0.2.0).
4. The paired outline A/B isolates a substantial cost: Vello GL rises from
   14.1 to 36.5 ms for identical 10k group movement. Outline cost is a concrete
   priority. Isolate feedback into a retained target,
   split selection bounds/eligibility from placement, and consider an interaction
   policy that does not unnecessarily stroke thousands of descendants. An overlay
   target stops hover from rebuilding content, but does not by itself make
   10k individually changing outlines cheap. Measure this separately before
   claiming the overlay split solves dense selected-group movement.
   Tutorial inspiration:
   [RenderHighlighter](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/RenderHighlighter.ts#L53),
   [ComputeBounds](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/ComputeBounds.ts#L82).
5. Newness is not a performance ordering. ThorVG offers retained scenes,
   SVG/Lottie support, portability, a small footprint and CPU as well as GPU
   backends. Our test exercises its published WASM GPU API, not its native CPU
   implementation. Vello's families have different preprocessing strategies.
   Features, bindings, hardware and workload determine the useful comparison;
   existence of either library is not evidence that it beats Skia on every case.
   [ThorVG design and supported backends](https://github.com/thorvg/thorvg#readme),
   [Vello renderer families](https://github.com/linebender/vello/blob/main/ARCHITECTURE.md).

The priorities in §5 therefore remain local/subtree revisions and independent
feedback, followed by measured presentation chunks/bakes. Keep `iron_document`
as authority and `iron_spatial`/Understory as bounds/query infrastructure. Static
subtree baking has little reuse in the deliberately all-changing case; reducing
host work and renderer preprocessing are necessary there. Compare newer Vello or Research in
this isolated harness if the required full-app workload still exceeds budget,
before coupling a renderer/dependency migration to the rest of the app.


## 8. Would Canvas2D still need application caches?

For the measured flat-shape workload, do not preemptively build a general raster
cache on Canvas2D. Retain local paths/bounds, cull, skip unchanged frames, and
separate content from feedback. Add raster caching only where measured reuse
outweighs baking, image composition, memory and invalidation costs. Simple
rectangles/circles can be cheaper to draw than to manage as individual cached
images. Expensive stable paths, text/effects, and groups viewed at small scale
are stronger candidates. The same measurement rule applies to Vello.

Canvas2D can support a retained document renderer implemented above its drawing
API. Cache a complex unchanged group into an offscreen canvas and place that
image using `drawImage`; retain the underlying document for editing. Rendering
a group's children together is also necessary for some visual semantics:
half-opacity on a group of overlapping opaque shapes differs from half-opacity
on every child draw. An isolated intermediate image can provide the former,
and can be retained when its inputs do not change. Canvas2D does not remove
this application-level composition/invalidation problem.
[Canvas optimization guidance](https://developer.mozilla.org/en-US/docs/Web/API/Canvas_API/Tutorial/Optimizing_canvas),
[Per-draw alpha semantics](https://developer.mozilla.org/en-US/docs/Web/API/CanvasRenderingContext2D/globalAlpha).

A cache must preserve painter order. In an A → animated B → C sequence, flattening
A and C together and drawing B afterward changes the result. Independent overlay
feedback is simpler because it is deliberately above content. General content
caches need order-aware chunks or correct damage redraw, whatever renderer draws
the pixels.

Figma's original 2015 explanation explicitly identified repeated geometry
submission and lack of control over compositing as limitations motivating its
custom tiled renderer. Its 2025 account demonstrates continued control over
resource-upload batching, bind-group reuse and render-pass batching. Those are
concrete architectural capabilities, rather than proof that every custom renderer
outperforms modern Canvas2D. The 2015 claim about geometry uploads is historical;
do not repeat it as a verified description of current Chromium internals.
[Original rendering decision](https://www.figma.com/blog/building-a-professional-design-tool-on-the-web/#rendering),
[Current resource management](https://www.figma.com/blog/figma-rendering-powered-by-webgpu/).

Architectural interpretation: Canvas2D lets us express draws and geometric reuse,
but does not expose document-node GPU allocations, explicit geometry residency,
shader selection, or a document-aware render graph. A custom GPU renderer lets
an application control those choices and specialize their cost. Its advantage
must still be implemented and measured. Our current Vello integration rebuilding
one full view scene does not automatically acquire Figma's retained-resource
behavior simply because both eventually use a low-level graphics API.
[Canvas2D contract](https://html.spec.whatwg.org/multipage/canvas.html#the-2d-rendering-context).

**Measurement qualification:** Chromium has deferred paint records, and its
`Canvas2DResourceProvider::RasterRecord` routes recorded operations through
software Skia or an accelerated shared-image raster interface. The timed
Canvas2D API loop does not enclose all work on other browser threads/processes.
Vello's timed call includes its local CPU scene preprocessing. Thus §7 is a
calling-thread API-work comparison, not a total CPU-engine-cost comparison;
callback cadence is likewise not presented-frame proof. This strengthens the
requirement for Chromium presentation/latency traces before a renderer migration.
[Chromium paint-record resource provider](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/platform/graphics/canvas_2d_resource_provider.cc#527),
[Deferred paint record](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/platform/graphics/canvas_deferred_paint_record.h).

Iron's vision prioritizes explanatory 2D/3D composition and animation, rather than
all professional visual-authoring features (`vision/01_layered-architecture-overview.md`
§1). Canvas2D therefore remains a serious possible 2D backend. Before choosing
it, test the intended corpus: realistic imported paths/text, nested clips/group
opacity, sparse edits versus whole-field animation, and 2D/3D texture handoff.
Keep document authority and spatial indexing independent of that decision. Do
not build Figma's entire cache/compositor architecture merely because Figma needed
it, or treat raster baking as the inevitable solution to expensive Vello encoding.


## 9. Retention terminology and upstream intent

See [Vello retention and the render-plan layer](vello-retention-and-render-plan.md)
for the pinned API audit and newly read public Zulip discussions. Iron has
whole-view scene/pixel retention today; its dirty-view path still resets and
re-encodes visible content. Raster bakes add a finer retained representation,
without changing the backend API into an object-addressable scene graph.
The maintainers explicitly discuss retained orchestration above Vello and connect
it to Understory. The old migration-scope assertion that upstream would never
provide cross-frame caching was too strong and has been corrected.


## 10. Renderer-family experiments (not scheduled)

Vello Hybrid computes antialiased coverage on the CPU in device space, so its
prepared strips survive only an unchanged transform (or whole-pixel horizontal,
4-pixel vertical moves). Renderers that keep geometry in object space and
rasterize on the GPU (Vello Classic's compute pipeline; tessellating renderers
such as Rive's) make moving unchanged shapes nearly free on the CPU, at other
costs. If the work in `handoff-render-perf.md` §4 still misses budget, compare
these in this harness before any migration:

- **Tessellated mesh baseline:** `lyon` fills/strokes tessellated once into
  retained meshes, moved by transform, MSAA for antialiasing. Measures the
  object-space-retention benefit directly; expect weaker AA and re-tessellation
  at zoom thresholds.
- **Vello Classic on WebGPU:** same `kurbo`/`peniko` inputs; the open question
  remains the compute-shader startup stall on affected hardware (#936).
- **Rive's WebGL2 build as a reference only**, with and without the draft
  `WEBGL_shader_pixel_local_storage` extension. Rive's best modes need pixel
  local storage or fragment-shader interlock, which wgpu does not expose; a port
  running on Bevy's wgpu would get its MSAA or atomic fallbacks. Its runtime is
  MIT C++ (`rive-app/rive-runtime`); `nuxieai/nuxie-runtime` is an independent
  pure-Rust reimplementation, unreleased as of 2026-10-14.

Any backend must render into a wgpu texture on Bevy's device to compose with 3D
views, planes and embeds; Canvas2D and the C++ Rive runtime cannot do that cheaply
on the web.
