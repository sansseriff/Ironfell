# `bevy_vello_hybrid` — open questions register

**Companion to:** [`bevy-vello-hybrid-design.md`](./bevy-vello-hybrid-design.md)
**Status:** ACTIVE — Tier A and desktop Tier B were resolved on 2026-07-24. They produced
a **no-go on the current public API**. Tier C browser testing and the upstream/API path
remain open.
**Opened:** 2026-07-24

---

## Why this document exists

The original design doc was drafted from partial source reading. A subsequent full source
trace, build spikes, pixel-readback test, and native benchmarks resolved the disputed
claims:

1. Claimed `vello_hybrid` re-encodes every frame → wrong; `render()` takes `&Scene`, scenes
   retain strips, but render still rebuilds schedules and uploads/draws the retained data.
2. Claimed the per-entity scene model cannot port → revised to "it can, via multiple
   `render()` calls" → **the original “cannot port” conclusion was correct**: each public
   render clears the target.
3. Claimed pan *and* zoom force a full re-record → revised to "only zoom" → **now partly
   undercut → **both require full rebuilds today**; a future integer offset only works
   inside a record-time overscan margin.

Every one of those errors shared a failure mode: **reasoning from prose descriptions of
the architecture instead of reading the relevant struct or function body.**

The source/API questions now exceed the threshold needed to reject the original
implementation plan. Remaining work asks whether an upstream-enhanced or forked design
can pass the same performance goal.

## How to use this document

- Questions are tiered by **what it takes to answer them**, cheapest first.
- **Answer Tier A before Tier B, and Tier B before Tier C.** Tier C costs human time on
  real hardware; do not spend it on things a code read would have settled.
- When answering: **cite the file and line or the command you ran.** Do not answer from
  recollection or from prose docs — this register exists because prose misled us.
- Record negative and surprising results too. Move resolved items to §5 with their citation.
- If an answer invalidates part of the design doc, **edit the design doc in the same pass**
  and note it in §6.

---

## 1. Tier A — resolvable by reading source (no build required)

Resolved on 2026-07-24; the prompts below are retained as the audit checklist and the
answers/evidence are in §5. Source of truth:
`https://github.com/linebender/vello` @ `main`, `sparse_strips/vello_hybrid/`.

### Q-A1. Can multiple retained `Scene`s composite into one `TextureView`? ✅ NO

- Which render pass actually writes into the caller's `view` argument, and what is its
  `LoadOp`? (Prior reading saw `LoadOp::Load` on several passes and a `Clear` inside a
  separate `clear_view` helper — but the final composite pass was never traced.)
- `GpuStrip.depth_index` is "painter's-order index used to compute z-depth for early-z
  rejection", and there is conditional depth `Clear(1.0)` vs `Load` logic (~line 2808 of
  `render/wgpu.rs`). **Does a second `render()` call reset depth and thus mis-occlude
  against the first?**
- Does `render()` assume it owns the whole target (e.g. full-viewport clears, scissor
  assumptions)?

The answer was no. §4 was rewritten around one compiled scene per view.

### Q-A2. Does recording clip content to the scene's `u16` viewport? ✅ YES
Determines whether "pan a retained scene" is even coherent.

- `GpuStrip.x/y` are `u16` and scene-local; a code comment mentions draws "clipped to the
  u16 viewport". **Is geometry outside `Scene`'s width/height discarded at record time?**
- If yes, panning a retained scene exposes blank regions, so a pan needs either a
  re-record or an oversized recording margin. Quantify: what does recording an N%
  oversized scene cost?
- Does `Scene` support negative coordinates at all, given `u16` positions?

### Q-A3. Is the zoom re-record conclusion actually right? ✅ YES
Verified from the record-time affine/subdivision path and fixed device-space tolerance.

- Confirm flattening tolerance is chosen from the record-time transform
  (`vello_common::flatten`, `strip_generator`).
- Is there any level-of-detail or tolerance override that would let a scene be reused
  across a limited zoom range within acceptable error?

### Q-A4. `ViewTarget` compatibility ✅ CONDITIONAL
`RenderTargetConfig` takes a single `format: wgpu::TextureFormat`.

- What formats does bevy's `ViewTarget` present in this project's WebGPU config, and does
  it match what `vello_hybrid` expects? sRGB vs linear handling?
- **MSAA:** bevy cameras may render multisampled. Can `vello_hybrid` render into an MSAA
  target, or must the node run post-resolve? (Note this project already forces `Msaa::Off`
  on its vello camera — but the crate should not silently require that.)
- Where must the render-graph node sit relative to `Node2d`/`Node3d` and tonemapping?

### Q-A5. What is `Resources` and who owns its lifetime? ✅ RESOLVED
`render()` takes `&mut Resources` alongside `&mut self`. Undocumented in our notes.

- What does it hold (glyph atlas, scratch buffers, layer textures)? Per-view or global?
- What is the resize story when the window changes size — full rebuild, or incremental?

### Q-A6. Cost model of `reset()` + re-record ✅ RESOLVED
The core performance question, partially answerable statically.

- What exactly does `reset()` free vs. retain (it clears `strip_storage`, `encoded_paints`,
  `recorder`, `root_transforms`)? Are allocations reused across frames?
- Is strip generation single-threaded? `vello_cpu` advertises multithreading — is any of
  that reachable from `vello_hybrid`, and does it work on single-threaded wasm?
- Confirm `fearless_simd` actually enables **wasm SIMD128** in this project's build
  configuration (this matters a great deal and has only been read about, not verified).

### Q-A7. Does `bevy_vello`'s current design have a reason we're missing? ✅ YES
Before discarding render-to-texture + `Material2d` quad for a direct `ViewTarget` node.

- Is the intermediate texture load-bearing for blending, ordering, or `RenderLayers`?
- Check `bevy_vello` git history/issues for whether a direct-to-target approach was tried
  and abandoned.

---

## 2. Tier B — resolvable by a small local spike (build, no special hardware)

Desktop/compile-time portions resolved on 2026-07-24; the wasm runtime measurements remain
part of Tier C. Results and commands are in §5.

### Q-B1. Does `vello_hybrid` build for `wasm32-unknown-unknown` in this project's config? ✅ VERSION BLOCKER
With `default-features = false`, `wgpu` feature on, WebGPU only, against **wgpu 27**.
Note `vello_hybrid` pins its own wgpu — **check for a version conflict with bevy 0.18's
wgpu 27**, which would be fatal to the whole plan and is cheap to discover.

### Q-B2. Minimal harness: two retained scenes into one target ✅ FAILS
The empirical counterpart to Q-A1. Headless via `render_to_file` if simpler. Render scene A
(red square) then scene B (blue square, overlapping) and confirm both appear with correct
ordering.

### Q-B3. Re-record cost, synthetic ✅ NATIVE BASELINE CAPTURED
Time `reset()` + re-record for 1k / 10k / 50k simple paths on desktop. Establishes the
shape of the curve (linear? superlinear?) without needing the browser. Do the same on
wasm if feasible.

### Q-B4. Actual dependency floor ✅ VALIDATED
Build a skeleton crate depending on `bevy` (minimal features) + `vello_hybrid`; run
`cargo tree` and confirm none of `bevy_sprite_render`, `bevy_text`, `bevy_ui`,
`bevy_winit` appear. This validated design doc §8.

---

## 3. Tier C — requires human + real hardware/browser

These need a person watching a real browser on real GPUs. **Do not start these until
Tier A and B are burned down.**

### Q-C1. Does the #936 startup stall actually go away? ⚠️ THE PREMISE
Build `sparse_strips/vello_hybrid/examples/wgpu_webgl` for wasm; measure GPU-process time
to first paint in Chrome's performance panel.
- **Windows / NVIDIA 4080 / AMD 3900x** — baseline ~1.5 s. This is the machine that matters.
- **M2 MacBook Air** — baseline ~75 ms.
- Post results to [vello#936](https://github.com/linebender/vello/issues/936); maintainers
  have asked for exactly this data.

**If this shows no meaningful improvement, stop the entire project.**

### Q-C2. Four-condition performance comparison
~10,000 shapes, hybrid vs current `bevy_vello`, measured **separately**:
static · pan · zoom · edit-one-shape.
Do not presume an outcome: with today's API, pan, zoom, and edit-one-shape all trigger a
full visible-scene rebuild. Needs human-driven interaction, hence Tier C.

### Q-C3. Frame pacing at 5K
If a supported direct/no-clear path becomes available, does removing the intermediate
full-window texture affect the existing 5K frame-skip problem? See
`plans/perf-grid-runbook.md` and the `CadenceProbe` harness already built for this.
Ground truth via Perfetto, not DevTools.

---

## 4. Meta-questions for the project owner

- **Q-M1.** If Q-C1 succeeds but Q-C2 shows a bad zoom regression, is a zoom-gesture
  raster proxy (draw the stale recording scaled, re-record on settle) acceptable UX?
- **Q-M2.** How much upstream churn is tolerable? `vello_hybrid` is 0.0.9, self-described
  as "roughly beta quality"; sparse strips is intended to *become* Vello, so APIs will move.
- **Q-M3.** Is it worth opening the §6c upstream requests — first a compound/batch
  render API, then offset/overscan and incremental strip reuse — before building, so the
  answer informs the design?

---

## 5. Settled — with citations

Only move items here with a file:line or command. Read these claims as **verified**;
remaining Tier C outcomes are unknown.

| Claim | Evidence |
|---|---|
| Audited revisions | Vello `f7e0fcc00a26552b8def2f587409c482be32435f`; `bevy_vello` v0.13.1 `4bc056c6d685b8d3421cf91a6f536afc9b8803de`; Bevy v0.18.1 `f667c282dad2c1419afb5836ded22a3ec263970e` |
| `render()` takes `&Scene`; recorded strips survive across frames | `sparse_strips/vello_hybrid/src/render/wgpu.rs:222-276` |
| Every public `render()` clears the caller's view; multiple calls do **not** composite | `render()` calls `render_scene(..., true, ...)`, `render/wgpu.rs:222-276`; caller-view clear at `:460-462`; `clear_view` uses `LoadOp::Clear(TRANSPARENT)` at `:488-505` |
| A two-scene pixel test leaves only scene B | `target/upstream-analysis/render-bench-current/src/bin/two_scenes.rs`; observed red-only `[0,0,0,0]`, overlap and blue-only `[0,0,255,255]` |
| Retained rendering still rebuilds CPU scheduling and uploads strip/alpha data | paint preparation and `Schedule::try_new` at `render/wgpu.rs:421-443`; scheduler storage clear/rebuild at `schedule/mod.rs:167-204,986-990`; alpha upload at `render/wgpu.rs:2195-2224`; new strip buffer/upload at `:2668-2699` |
| No `Scene::append` exists | grep for `fn (append\|extend\|push_scene\|concat)` over `scene.rs` — no matches |
| `vello` 0.7 *does* have `Scene::append`, implemented as `extend_from_slice` + offset patching | `vello-0.7.0/src/scene.rs:461`, `vello_encoding-0.7.0/src/encoding.rs:94` |
| `Scene::new(width: u16, height: u16)` — pixel-sized | `scene.rs:232` |
| `GpuStrip` = integer device-pixel `x/y/width` + alpha-texture index + `depth_index` | `render/common.rs:362` |
| Recording clips/culls to the scene viewport | `vello_common/src/strip_generator.rs:119-167,204-238`; `vello_common/src/flatten_simd.rs:83-86,122-130,171-180`; `vello_common/src/strip.rs:132-158` |
| Negative input coordinates work for crossing/winding geometry, but fully out-of-view content is not retained for later pan | same strip-generator, flatten, and strip clipping paths above |
| `Config` has `strip_offset_x/y`, hardcoded to `0` in the wgpu backend | `render/common.rs:331`; call sites `render/wgpu.rs:1946, 2394` |
| No public render-time root transform (`RootTransforms` is a private field) | `scene.rs:216`; `vello_common/src/transforms.rs:81` |
| Zoom affects generated geometry/coverage; no public tolerance/LOD setting | affine applied before subdivision at `vello_common/src/flatten_simd.rs:92,105`; fill tolerance `TOL = 0.25` at `flatten.rs:15-19`; stroke tolerance varies with scale at `flatten.rs:231-238`; `RenderSettings` surface at `scene.rs:109-199` |
| `Scene::reset()` reuses capacities rather than freeing all allocations | `scene.rs:937-952`; `vello_common/src/strip_generator.rs:51-55,241-248`; `vello_common/src/record.rs:239-255` |
| Hybrid recording is synchronous/single-threaded | no parallel dispatch in `vello_hybrid`; recorded draw uses `thread_idx: 0`; hybrid's `vello_common` dependency enables `std`, not its optional multithreading path |
| Ironfell wasm currently selects scalar `fearless_simd`, not SIMD128 | `.cargo/config.toml` has no target feature; `build-wasm.sh` replaces `RUSTFLAGS` without `-Ctarget-feature=+simd128`; `rustc --print cfg --target wasm32-unknown-unknown` lacks `target_feature=\"simd128\"`; `fearless_simd` requires that flag |
| Bevy 0.18's main `ViewTarget` is single-sampled; MSAA uses a separate attachment resolved into it | `bevy_render/src/view/mod.rs:1107-1114,1135-1152` |
| Normal Bevy target format is `Rgba8UnormSrgb`; HDR is `Rgba16Float` | `bevy_render/src/view/mod.rs:737-739,1092-1096`; `bevy_image/src/image.rs:35-38` |
| `Resources` persists image cache and, with text, glyph preparation/atlas state; resize is handled by render config/private depth recreation, while the pixel-sized `Scene` must be rebuilt | `vello_hybrid/src/resources.rs:15-54`; `render/wgpu.rs:2379-2410` |
| Dep floor: `vello_common` = `bytemuck, peniko, fearless_simd, smallvec, thiserror, guillotiere, log`; `no_std` + alloc | `sparse_strips/vello_common/Cargo.toml` |
| Text is via `glifo` (optional feature), **not** `skrifa` | `sparse_strips/vello_hybrid/Cargo.toml` |
| `bevy_vello` 0.13.1 is 5,316 lines; ~2,250 are svg + lottie | `find src -name '*.rs' \| xargs wc -l` |
| `bevy_vello` renders to an `Image` then composites via `Material2d` fullscreen quad | `bevy_vello-0.13.1/src/render/{mod.rs,plugin.rs,systems.rs}` |
| The intermediate texture supplies ordinary Bevy 2D ordering/render-layer semantics; no evidence a direct path was tried and abandoned | `bevy_vello/src/render/systems.rs:40-67,232-257,446-461,527-575`; [bevy_vello#15](https://github.com/linebender/bevy_vello/issues/15) |
| Bevy 0.18.1 + `vello_hybrid` 0.0.9 fails because Bevy uses `wgpu` 27 and hybrid uses 29; hybrid 0.0.6 shares 27 and checks successfully for wasm | minimal spike in `target/upstream-analysis/compat-spike`; `cargo check --target wasm32-unknown-unknown --features hybrid-0-0-9` fails with mismatched `Device`/`TextureFormat`; the `hybrid-0-0-6` check succeeds |
| Synthetic native record cost is roughly linear in path count and grows with zoom | Apple M5 release harness `target/upstream-analysis/record-bench-current`: 1k ~0.3–0.5 ms, 10k ~3.2–3.8 ms, 50k ~16.3–16.7 ms; 10k at 8× ~12.8 ms |
| Retained render is cheap but nonzero | Apple M5 Metal release harness `target/upstream-analysis/render-bench-current`: retained render 1k ~0.2–0.3 ms, 10k ~0.3–0.7 ms, 50k ~1.2–1.8 ms; rebuild+render 50k ~17.5–18.7 ms |
| Minimal Bevy render + compatible hybrid dependency floor excludes the unwanted high-level Bevy crates | `cargo tree` in `target/upstream-analysis/compat-spike`; absent: `bevy_sprite_render`, `bevy_text`, `bevy_ui`, `bevy_winit`, `bevy_pbr`, `bevy_gizmos_render`, `bevy_state` |
| Maintainer expects sparse strips to incidentally fix #936 | [vello#936](https://github.com/linebender/vello/issues/936), DJMcNab 2025-08-26 |
| `bevy_text` is force-enabled by `bevy_vello` via `2d_api`; removing it from our features saved only 13.8 KB brotli (0.35%) | measured 2026-07-24; `cargo tree -e features -i bevy_text` |

---

## 6. Change log

| Date | Change |
|---|---|
| 2026-07-24 | Register opened. Design doc marked provisional. Q-A1 and Q-A2 raised after noticing two unverified claims in the design doc's §2. |
| 2026-07-24 | Completed source audit of pinned Vello, `bevy_vello`, and Bevy revisions; ran wasm compatibility, two-scene pixel, dependency-floor, recording, and retained-render spikes. Q-A1 disproved the per-entity scene design; Q-B1 found the `wgpu` 27/29 blocker. Design status changed to no-go on current APIs. |
