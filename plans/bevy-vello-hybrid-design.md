# `bevy_vello_hybrid` — design brief

**Status:** ⛔ **NO-GO on the current public API — Phase 0 / upstream work only.**
**Date:** 2026-07-24
**Author context:** written for the `ironfell` project (bevy 0.18.1 / wgpu 27.0.1 / WebGPU wasm)

> **Read [`bevy-vello-hybrid-open-questions.md`](./bevy-vello-hybrid-open-questions.md) first.**
> A source audit and native microbenchmarks on 2026-07-24 invalidated two load-bearing
> assumptions in the original draft: public `Renderer::render()` calls clear rather than
> composite, and recorded geometry is viewport-clipped. The latest published
> `vello_hybrid` also cannot share Bevy 0.18's `wgpu` device because they use different
> major versions. Tier C browser measurements remain open, but these API facts are enough
> to stop implementation of the proposed crate as designed.

---

## 1. Why this exists

The motivating problem is [linebender/vello#936](https://github.com/linebender/vello/issues/936):
a multi-second GPU-process freeze on first paint in the browser. Measured on this
project: **~1.5 s on Windows (NVIDIA 4080 / AMD 3900x), ~75 ms on an M2 MacBook Air.**

Root cause per Vello maintainer `DJMcNab` (working hypothesis, not a confirmed fix):
shader compilation is slow because Vello is "a long pipeline of prefix sums, which
require a lot of workgroup memory," and browser WebGPU implementations polyfill
workgroup-memory zero-initialisation as thousands of individual `val=0` stores.
He states: *"There isn't really any feasible way for us to fix this in Vello. It would
incidentally be resolved by some of the sparse strips work we're doing."*

`vello_hybrid` is that sparse-strips work. Its stated design goal is to run "on GPUs
without compute shader support, using only fragment and vertex shaders" — which removes
the trigger at source rather than working around it.

Secondary goal: a purpose-built crate can depend on a far narrower slice of bevy than
`bevy_vello` does (see §8), which materially reduces wasm size.

### Decision

Do **not** build the proposed reusable crate on `vello_hybrid`'s current public API. It
cannot guarantee the desired lower per-frame cost for the defining editor workload:
continuous pan, continuous zoom, and small edits surrounded by unchanged complex shapes.

A narrow prototype is still justified to test whether sparse strips fixes #936 and to
develop the missing upstream primitives. Reconsider a crate after a compatible release
can batch or incrementally compose independently cached document chunks. If startup time
and dependency size are valuable even without edit-local invalidation, describe that as
a different, narrower product; do not market it as a Figma-style performance solution.

---

## 2. Read this before writing code: the two renderers are architecturally opposite

This is the single most important thing to understand, and it is a **risk to the project**.

| | `vello` 0.7 (what `bevy_vello` uses) | `vello_hybrid` 0.0.9 |
|---|---|---|
| Where paths are flattened/tiled | **GPU**, in compute shaders, **every frame** | **CPU**, once at record time |
| `Scene` contents | retained `Encoding` (flat byte streams) | retained `CommandRecorder` of generated strips |
| Cross-frame retention | encoding retained; GPU redoes flattening each frame | strips retained, but every `render()` rebuilds a schedule, repacks/uploads strips and alpha data, and issues passes |
| `Scene::append(&other, affine)` | **exists** — `extend_from_slice` + offset patching, O(bytes) | **does not exist** |
| Composing multiple scenes | one merged encoding → one draw | **unsupported by the public API**; every `render()` starts by clearing the target |
| Coordinate space of stored data | resolution-independent | **device space** — strips baked at the record-time transform |
| `Scene` sizing | resolution-independent | `Scene::new(width: u16, height: u16)` — pixel-sized |
| Drawing API | immediate: `fill(rule, affine, color, None, &shape)` | stateful: `set_transform()`, `set_paint()`, then `fill_path()` |

### Consequences

1. **`bevy_vello`'s per-entity retained-scene model does not port.** The public
   `Renderer::render()` path calls `render_scene(..., clear = true, ...)`, so a second
   scene erases the first. A pixel-readback test confirmed this. One compiled `Scene`
   must therefore contain the whole view unless Vello adds a batch/append/no-clear API.
   Merely exposing `clear = false` would make composition possible, but rendering N
   object scenes would still rebuild N schedules, upload N strip sets, and issue N groups
   of passes; that is not a credible Figma-style cost model.
2. **A `Scene` retains expensive geometry work, not a finished GPU display list.**
   Reusing it avoids CPU flattening, tiling, and antialiasing generation, which is useful.
   It does not make a frame free: the renderer walks all encoded paints and strips,
   rebuilds its schedule, uploads the alpha texture and strip buffers, and renders them
   again. Native measurements on an Apple M5 put retained rendering at roughly
   0.2–0.3 ms for 1k synthetic paths, 0.3–0.7 ms for 10k, and 1.2–1.8 ms for 50k.
3. **Pan reuse is bounded by the record-time viewport.** A `GpuStrip` is
   `{ x: u16, y: u16, width: u16, …, col_idx: u32, … }`: an integer device-pixel position
   plus an index into an alpha texture of CPU-computed antialiasing coverage. What
   matters is whether that *coverage* stays valid:
   - **Integer-pixel pan — coverage of already-recorded strips is unchanged.** The shader
     supports `strip_offset_x/y`, but the wgpu backend hardcodes both to zero. More
     importantly, recording culls/clips geometry to `Scene::new(width, height)`. An
     exposed offset can only pan within a deliberately recorded overscan margin; moving
     beyond it reveals blank space and requires a rebuild.
   - **Subpixel pan** — edge coverage changes, so strictly needs a re-record, unless the
     app snaps to integer pixels during the gesture.
   - **Zoom — a correct vector result requires a re-record.** Paths are transformed before
     subdivision against a fixed device-space fill tolerance, and stroke tolerance also
     changes with affine scale. There is no public LOD/tolerance control.

   There is also no public render-time root transform (`RootTransforms` is private, used
   for layer-relative roots), so arbitrary affine changes mean re-recording today.
4. **A small edit also rebuilds the whole visible scene.** The crate exposes neither
   `Scene::append` nor public recorded-strip injection, and multiple scenes cannot be
   composed. ECS change detection can identify the edited object, but cannot turn that
   knowledge into incremental renderer work through the current API.

### Where the risk actually lives

For the target Figma-style workload, the current API has the wrong invalidation
granularity:

- idle frames reuse CPU-generated strips but still schedule, upload, and draw the entire
  retained scene;
- pan rebuilds the entire visible scene unless it stays inside a future overscan/offset
  cache;
- zoom rebuilds the entire visible scene for correct coverage;
- editing one shape rebuilds the entire visible scene, including unchanged complex
  neighbours.

Synthetic native recording measurements were approximately 0.3–0.5 ms for 1k paths,
3.2–3.8 ms for 10k, and 16.3–16.7 ms for 50k at 1×. For 10k paths, increasing scale from
1× to 8× increased recording from about 3.5 ms to about 12.8 ms. These are not browser
or production-scene results, but they show the expected linear shape-count cost and the
additional device-space complexity from zoom.

The static case remains promising, and sparse strips may solve the startup stall. That is
enough to justify a focused experiment and upstream API work, but **not** a production
`bevy_vello_hybrid` crate promising cheap pan/zoom/small-edit frames. See §9 for the gate.

---

## 2c. Has upstream considered CPU cost? Yes — it is the central concern

Worth recording, because "the CPU does the flattening" sounds alarming out of context.

- `vello_cpu` is benchmarked against a fork of the **Blend2D** benchmark harness and is,
  per Linebender, "likely the fastest CPU-only renderer in Rust." Blend2D is a highly
  optimised C++ rasteriser; that is a serious bar.
- SIMD is explicit and portable via **`fearless_simd`**, including a WASM SIMD path.
  However, Ironfell's current wasm build does **not** pass
  `-Ctarget-feature=+simd128`, so `fearless_simd` selects its scalar fallback. Phase 0
  must fix that flag (or deliberately ship scalar and SIMD bundles) before measuring.
- Sustained optimisation work: fast paths that bypass full coarse rasterisation, rectangle
  special-casing, glyph caching, gradient-heavy fixes, skipped layer operations, opaque
  full-tile image handling, and overdraw elimination (~30% on one benchmark).
- Linebender's stated plan is to **move the sparse-strips crates to the top level of the
  repository once they are ready** — i.e. sparse strips is intended to *become* Vello,
  not to remain a sidecar.

The design premise (Raph Levien's *"Potato: a hybrid CPU/GPU 2D renderer design"*) is that
moving this work to the CPU buys compatibility with GPUs lacking compute shaders, removes
performance cliffs, and copes with tighter memory budgets — and that with good SIMD the
CPU cost is acceptable. The startup stall in #936 is a direct consequence of the
compute-shader path that this design removes.

**What upstream has *not* published** is a like-for-like benchmark against compute-based
Vello, or any statement about the pan/zoom re-record cost specifically. That gap is
exactly what Phase 0 measures.

---

## 3. Confirmed `vello_hybrid` API surface

Verified against `main` @ 2026-07-24 (crates.io `vello_hybrid` 0.0.9).

```rust
// Construction — takes an EXISTING wgpu device. This is the key bevy integration point.
Renderer::new(device: &Device, config: &RenderTargetConfig) -> Self
Renderer::new_with(device: &Device, config: &RenderTargetConfig, settings: RenderSettings) -> Self

pub struct RenderTargetConfig { pub format: wgpu::TextureFormat, pub width: u32, pub height: u32 }

// Rendering — writes into an EXISTING texture view via an EXISTING encoder.
Renderer::render(
    &mut self,
    scene: &Scene,
    resources: &mut Resources,
    device: &Device,
    queue: &Queue,
    encoder: &mut CommandEncoder,
    render_size: &RenderSize,
    view: &TextureView,
    texture_bindings: &TextureBindings,
) -> Result<(), RenderError>

// Raster caching primitives (see §6)
Renderer::render_to_atlas(..)
Renderer::upload_image<T: AtlasWriter>(..)
Renderer::destroy_image(..)
Renderer::atlas_texture(&self) -> &Texture
```

`Scene` (stateful builder):

```rust
Scene::new(width: u16, height: u16)
// state
set_transform(Affine) / reset_transform()
set_paint(impl Into<PaintType>) / set_fill_rule(Fill) / set_stroke(Stroke)
set_blend_mode(BlendMode) / set_paint_transform(Affine) / set_tint(Option<Tint>)
// geometry
fill_path(&BezPath) / stroke_path(&BezPath)
fill_rect(&Rect) / stroke_rect(&Rect)
fill_blurred_rounded_rect(..)
draw_texture_rects(..)          // <- how baked/cached content is drawn
glyph_run(..)                   // requires `text` feature
// layers — note these are MUCH simpler than vello 0.7's push_layer
push_clip_layer(&BezPath) / push_blend_layer(BlendMode)
push_opacity_layer(f32) / push_mask_layer(Mask) / push_filter_layer(Filter)
pop_layer()
reset()
```

### Compatibility gate

The API shape fits Bevy, but the current dependency graph does not:

- Bevy 0.18.1 uses `wgpu` 27.
- `vello_hybrid` 0.0.9 and current Vello `main` use `wgpu` 29.
- A minimal wasm compatibility crate fails with mismatched `wgpu::Device` and
  `TextureFormat` types when it combines Bevy 0.18.1 with `vello_hybrid` 0.0.9.
- The same crate succeeds with `vello_hybrid` 0.0.6, which uses `wgpu` 27.

The implementation must therefore either pin the older hybrid release, maintain a
backport/fork, or move Bevy to a version using the same `wgpu` generation. This is a
compile-time blocker, not an adapter-layer inconvenience.

**Migration note for existing app code:** `scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, affine, &clip)`
becomes simply `scene.push_clip_layer(&path)`.

### Dependency footprint (the size win)

- `vello_hybrid`: `bytemuck, thiserror, vello_common, log, hashbrown`, optional
  `wgpu`, `vello_sparse_shaders`, `glifo` (text), `js-sys`/`web-sys` (webgl).
- `vello_common`: `bytemuck, peniko, fearless_simd, smallvec, thiserror, guillotiere, log`. **`no_std` + alloc.**
- Text is via **`glifo`**, not `skrifa`. `vello` 0.7 pulls `skrifa` 0.40 — a third copy
  of the font stack in this project today (alongside `cosmic-text`→`swash`→`skrifa` 0.31
  and `cosmic-text`→`skrifa` 0.39).

---

## 4. Scene / ECS model

### Decision: entity per *editable object*; hierarchy = groups

Use entity granularity for editing ergonomics and invalidation tracking. Do not equate an
entity with a retained hybrid `Scene`: current APIs require one compiled scene per view.
Entity and group boundaries remain valuable seams for future batching or raster caches.

- A **group** is an entity with `Children`. Bevy's `ChildOf`/`Children`, `Transform`
  propagation, picking, change detection and reflection give Figma-style group semantics
  for free.
- A **leaf shape** is an entity holding geometry + paint components.
- **Anchor/handle-level geometry stays as data** (`BezPath` in a component). Individual
  anchors become transient handle entities *only while in edit mode*.
- **Do not restructure the hierarchy when entering edit mode.** Edit mode is a marker
  component (+ transient handles), never a structural mutation. Structural change on
  mode-switch breaks entity identity, undo/redo, and animation bindings.

### Rendering is a hierarchy traversal

One cached `Scene` per view, sized to the view. Reuse it while neither content nor the
effective view transform changes. When dirty, reset it, cull to the view, traverse visible
roots in document order, and emit `set_transform` / `set_paint` / `fill_path` calls.
Paint order is traversal order.

On the current API, pan, zoom, resize, or editing any visible shape dirties that whole
compiled scene. ECS change detection avoids unnecessary rebuilds on idle frames but
cannot incrementally replace a single object's recorded strips.

This incidentally fixes a real bug class seen in this project: with `bevy_vello`, all
screen-space scenes sit at `z = 0` and their relative draw order is not well defined.

### Suggested component sketch (non-binding)

```rust
#[derive(Component)] pub struct VelloShape { pub path: BezPath, pub fill_rule: Fill }
#[derive(Component)] pub struct VelloFill(pub PaintType);
#[derive(Component)] pub struct VelloStroke { pub style: Stroke, pub paint: PaintType }
#[derive(Component)] pub struct VelloGroup;              // has Children
#[derive(Component)] pub struct VelloClip(pub BezPath);  // push_clip_layer for subtree
#[derive(Component)] pub struct VelloOpacity(pub f32);
#[derive(Component)] pub struct VelloEditMode;           // marker, no structural change
#[derive(Component)] pub struct VelloBaked { /* atlas handle, baked scale */ }  // §6
```

---

## 5. Compositing: keep the intermediate target for the current API

**Current decision: retain `bevy_vello`'s render-to-`Image` + fullscreen `Material2d`
composition until Vello exposes a supported no-clear/batch path.**

`bevy_vello` today: renders vello into a full-window `Image`, then draws that image as a
`Mesh2d` + `VelloCanvasMaterial` quad on a configurable render layer.

Although `Renderer::render()` accepts a `&TextureView` and `&mut CommandEncoder`, it
clears that view to transparent at the start of every public call. Pointing it at Bevy's
`ViewTarget` would erase earlier Bevy draws. The existing offscreen texture is therefore
load-bearing, not merely historical convenience. It also lets the whole vector block
participate in Bevy's normal `Transparent2d` ordering and `RenderLayers` filtering.

Direct rendering remains a worthwhile conditional optimisation because it would:

- remove a full-window texture (**~59 MB at 5120×2880 RGBA8**);
- remove a full-screen blit;
- drop the `Material2d`/`Mesh2d` path and its `bevy_sprite_render` dependency.

If Vello gains a supported no-clear/batch entry point, run a direct node after Bevy has
resolved its main MSAA attachment. Bevy's `ViewTarget` main texture is single-sampled even
when MSAA is enabled, so `Msaa::Off` is not inherently required. Use the actual target
format (`Rgba8UnormSrgb` for the usual non-HDR main target, `Rgba16Float` for HDR) and
visually validate blending and colour semantics. Place world/HDR vectors before tonemapping
and display-space UI late in the graph. A direct node can order only the whole vector
block; it cannot interleave individual vector entities with native `Transparent2d` items.

Coordinate note: `Scene` is inherently pixel-sized and screen-space, so **screen-space is
the natural model here** and world-space content requires applying the camera transform —
the inverse of `bevy_vello`'s bias. Expose both explicitly and document which is which;
conflating them has already cost this project a debugging session.

---

## 6. Caching strategy (design the seam now)

There is **no public cross-frame caching primitive** in `vello_hybrid` today:

- `Scene::recorder` is `pub(crate)`; `push_draw` is not public, so pre-generated strips
  cannot be injected.
- Even if it were public, strips are transform-dependent (flattening tolerance is in
  device space), so reuse is only valid while a transform is unchanged.

**What is available now: raster-level baking**, via `render_to_atlas` / `upload_image` /
`draw_texture_rects`. This is the standard editor strategy (Figma/Sketch group flattening)
and maps cleanly onto the group model:

> **grouped ⇒ baked to atlas; edit mode ⇒ live re-encode**

Rules:
- Bake a static subtree once into an atlas image; draw it with `draw_texture_rects`.
- Invalidate on content change, or when zoom crosses a resolution threshold (baked content
  is resolution-dependent).
- Never bake a subtree currently in edit mode.
- Track atlas memory; `guillotiere` (in `vello_common`) is the allocator upstream uses.

**Implementation guidance:** structure encoding so that "emit this subtree" is a single
seam with two implementations (live traversal vs. baked rect). That keeps the door open
for an upstream strip-level cache without a redesign.

### 6c. Upstream asks, in priority order

1. **A compound-scene or batch render API** that preserves painter order, clears once,
   and avoids one schedule build/upload/pass group per editable object. A bare
   `clear: false` switch would prove composition but is insufficient by itself.
2. **Expose `strip_offset_x/y` per render call** (or per-scene integer translation) and
   define how recording overscan is requested. This can avoid strip regeneration while
   panning inside the recorded margin; retained scheduling/upload/draw work remains.
3. **A render-time root transform** for gesture-time translation/scaling, explicitly
   allowing approximate AA/tolerance until a high-quality re-record settles.
4. **Public incremental command/strip reuse** so an integration can replace a dirty
   object's recorded contribution without rebuilding unchanged complex neighbours.

### Cull before you cache

From the [Vello Zulip thread](https://xi.zulipchat.com/#narrow/channel/197075-vello/topic/Hierarchical.20scene.20compilation.20system.20for.20editing.20workflows.3F):

> **Nico Burns:** most people are just creating new Scene(s) "on-demand" when rendering the
> frame (if you can render everything into a single Scene or reuse a single Scene (resetting
> it between uses) that'll be cheaper if you're not caching them). Cached scenes for unchanged
> subtrees can be cheaper, but runs into all the usual problems around cache invalidation.

This yields an ordering that the implementer should follow strictly:

1. **Viewport + zoom culling first.** If a spatial index keeps the *visible* shape count in
   the hundreds, one rebuilt-per-frame `Scene` is fine and **the caching problem never
   arises**. Culling has no invalidation hazards; caching has many.
2. **Raster baking only where culling cannot help** — i.e. genuinely zoomed *out* over a
   10k-element SVG, where everything really is on screen at once.

Note the pleasing property: the regime where shape count explodes (zoomed out) is exactly
the regime where shapes are near-pixel-sized and a **baked raster is both necessary and
visually near-lossless**. Zoomed in, live encoding is both affordable and required for
crisp vectors. Bake aggressively at low zoom, never at high zoom.

**Do not build a cache in Phase 1.** Measure with culling alone first.

---

## 6b. Understory — recommended prior art

Raph Levien pointed at [`forest-rs/understory`](https://github.com/forest-rs/understory) in
the Zulip thread as solving "a fairly similar set of issues, in particular having a
mostly-retained document and trying to minimize the amount of work needed to render each
frame." It is worth studying before designing Phase 2.

**What it is:** ~35 headless, renderer-agnostic, `no_std` + alloc crates providing the
*document* substrate for editors and CAD viewers. Apache-2.0. Created 2025-11, actively
developed (last push 2026-07-12), ~41 stars. Only 4 crates published to crates.io so far
(`understory_view2d`, `understory_virtual_list`, `understory_timing`, plus `overstory`);
the rest are repo-only.

**Three-tree architecture:** widget tree (state/interaction) → box tree (geometry, spatial
indexing) → presentation tree (resolved drawing intent). Directly relevant crates:

| Crate | Relevance |
|---|---|
| `understory_index` | 2D AABB index, pluggable FlatVec/R-tree/BVH backends, batched `commit()` with coarse damage (added/removed/moved). **This is the culling primitive from §6.** |
| `understory_box_tree` | Kurbo-native spatially indexed box tree: local bounds, transforms, clips, z-order; computes world AABBs and syncs into `understory_index`. Explicitly *not* a layout engine. |
| `understory_presentation` | Retained resolved drawing primitives keyed by caller-owned geometry ids, with **deduped dirty keys**. `PathPrimitive` carries path geometry + fill/stroke intent. |
| `understory_selection`, `understory_precise_hit`, `understory_event_state` | Selection anchors, precise hit-testing, hover/drag state machines — the "edit mode" machinery. |
| `understory_view2d` | Pan/zoom, coordinate conversion, view fitting — the zoom signal driving bake decisions. |
| `understory_animation_timeline`, `understory_timeline_model` | Relevant to this project's existing timeline work, independent of vello. |

**Critical caveat:** every crate's docs repeat that it does **not** own "renderer command
emission." Understory gives you a retained document and tells you *what changed* and
*what is visible* — it does not compile vello scenes. The subtree-compilation step
(Andrew's item 2) remains yours to build; Understory supplies the invalidation substrate
that makes it tractable, which is precisely Nico's flagged risk.

### Recommendation: borrow, don't adopt wholesale

Understory is a **parallel scene graph**. Adopting the full three-tree model alongside
bevy means running its box tree and presentation store next to bevy's ECS hierarchy,
`Transform` propagation, visibility and picking — duplicated state with a synchronisation
burden.

- **Preferred (bevy-native):** keep bevy ECS as the document model (§4). Borrow
  Understory's *ideas* — dirty keys, batched commit with coarse damage, spatial index for
  culling — and optionally the leaf crates that make no tree assumptions, chiefly
  `understory_index` and `understory_view2d`.
- **Alternative (Understory-as-document):** bevy becomes render host + input only; the
  document lives in Understory. Conceptually cleaner for a Figma-class editor and better
  aligned long-term, but surrenders bevy ECS ergonomics for document state and bets on a
  young, mostly-unpublished crate family. Only take this if the editor is *the* product
  and the bevy 3D/scene side is secondary.

Given this project already has picking, interaction, transforms and an inspector built on
bevy ECS, the preferred path is bevy-native with selective borrowing.

---

## 7. Scope decisions (confirmed with project owner)

| Area | Decision |
|---|---|
| Backends | **WebGPU only.** No WebGL2 path in v1 (avoids `vello_sparse_shaders/glsl` and a second bevy render path). |
| Text | **Included, behind an optional off-by-default `text` feature** using `glifo`. Requires glyph-atlas resource management. |
| Compositing | Intermediate `Image` + Bevy 2D material on the current API; direct `ViewTarget` only after a supported no-clear/batch API (§5). |
| Scene model | Entity-per-editable-object hierarchy traversal (§4). |
| Caching | One compiled scene per view; full visible-scene re-record on any dirty object or view-transform change. **Cull first (§6).** Do not promise incremental small-edit cost. |
| SVG / Lottie | **Out of scope.** (`bevy_vello` spends ~2,250 of its 5,316 lines on these.) |

---

## 8. Target dependency floor

`bevy_vello` 0.13 requests `default_app`, `2d_bevy_render`, `ui_bevy_render` from bevy.
Because Cargo features are **additive across the whole graph**, this overrides a
downstream crate's `default-features = false` and force-enables `bevy_text`, `bevy_winit`,
`bevy_state`, `bevy_input_focus`, `custom_cursor`, `reflect_auto_register`,
`bevy_post_process`, `bevy_sprite_render`, `bevy_gizmos_render`, `bevy_ui_render`.

This crate should require **only**: `bevy_app`, `bevy_ecs`, `bevy_render`, `bevy_camera`,
`bevy_math`, `bevy_transform`, `bevy_asset`. Explicitly **not** `bevy_sprite_render`,
`bevy_text`, `bevy_ui`, or `bevy_winit`.

Verify with `cargo tree -i <crate>` and treat any of the above appearing in the graph as a
bug in this crate's manifest.

A minimal dependency-floor spike using Bevy's render crates plus the `wgpu`-compatible
`vello_hybrid` 0.0.6 confirmed that `bevy_sprite_render`, `bevy_text`, `bevy_ui`,
`bevy_winit`, `bevy_pbr`, `bevy_gizmos_render`, and `bevy_state` need not appear. This
validates the size premise independently of the renderer API blockers.

---

## 9. Phasing

### Phase 0 — Validate the premise (**do this first; do not skip**)

The source audit produced a **no-go for the originally proposed wrapper**. Phase 0 is now
a bounded feasibility project, not the first step of an assumed implementation:

1. **Resolve the API/version gate.** Choose one controlled prototype path:
   - backport the required hybrid APIs to the `wgpu` 27-compatible 0.0.6 line;
   - update Bevy to a matching `wgpu` generation; or
   - get an upstream batch/no-clear/incremental API and then align versions.

   The prototype must demonstrate composition of independently cached document chunks
   without clearing and without a full renderer schedule/upload cycle per shape.
2. **Does sparse strips fix the stall?** Build the matching `vello_hybrid` web example for
   wasm, run on the **Windows/NVIDIA machine** (the Mac is already fine at ~75 ms).
   Measure GPU-process time to first paint in Chrome's performance panel. Compare against
   the existing ~1.5 s. Enable `simd128` deliberately and record the exact browser/GPU.
   Attach numbers to issue #936 — the maintainers want this data.
3. **What does the editor workload cost?** Load ~10,000 shapes (an SVG
   comparable to the one already tested) and measure CPU time per frame in **four**
   separate conditions, versus the current `bevy_vello` build:
   - **static retained render**;
   - **continuous pan**, including crossing the recorded overscan boundary;
   - **continuous zoom**;
   - **editing one shape** among unchanged complex neighbours.

   Do not presume three conditions favour hybrid. On today's API, the latter three all
   trigger a full visible-scene re-record. Report CPU recording, CPU render preparation,
   GPU time, allocations/uploads, and frame pacing separately.

**Gate:** proceed to a reusable crate only if all are true:

- startup time improves substantially on the affected Windows/NVIDIA browser path;
- Bevy and hybrid share one `wgpu` type universe;
- the integration can update a bounded dirty region/chunk rather than recompiling all
  visible unchanged shapes;
- pan and zoom have an explicit cache/quality policy with measured frame-time bounds;
- the four-condition browser comparison beats or acceptably trades against current
  `bevy_vello`.

If the incremental API is not available, the work may still be worthwhile as a private
startup-stall prototype or a small dependency experiment, but it does **not** satisfy the
Figma-style performance goal stated for this crate.

Note that upstream considers this a live area: Vello Hybrid is described by Linebender as
"roughly beta quality; there are some rough edges still and performance work to be done,
but it should be usable," and CPU-side throughput is the central engineering focus of the
sparse-strips project (see §2c).

### Phase 1 — Minimal viable crate

**Blocked until the Phase 0 gate passes.**

Cover only what `ironfell` uses today: filled/stroked `BezPath`s, clip layers, screen-space
overlays, `RenderLayers` filtering, resize handling. Use the compositing path validated in
Phase 0. No text, no groups, no baking. Port `overlay2d`, `timeline`, `ui_panels` as the
acceptance test.

### Phase 2 — Hierarchy, groups, and culling

Scene-graph components, traversal-order painting, edit-mode markers, transient handles.
**Add spatial culling here** (`understory_index` or equivalent) — it is the highest-value
performance work and has none of caching's invalidation hazards. Re-measure the Phase 0
10k-shape benchmark with culling active before considering Phase 3.

### Phase 3 — Baking and text (only if Phase 2 measurement demands it)

Atlas baking behind the §6 seam, driven by a zoom threshold; optional `text` feature.

---

## 10. Reference: how `bevy_vello` is structured

5,316 lines total. Worth reading, but note §2 — much of its design is a consequence of
`vello`'s retained encoding and will not transfer.

| Area | Lines | Port? |
|---|---|---|
| `integrations/lottie/**` | ~1,700 | No |
| `integrations/text/**` | ~950 | Phase 3, different (glifo) |
| `render/systems.rs` | 589 | Concepts only — it targets an offscreen Image |
| `integrations/svg/**` | ~550 | No |
| `render/mod.rs` | 265 | Concepts only |
| `integrations/scene/render.rs` | 276 | **Yes** — extract + affine logic is the closest analogue |
| `picking.rs` | 179 | Later |
| `render/plugin.rs` | 93 | **Yes** — plugin/system-set wiring pattern |

Two specific things worth stealing:
- `extract_world_scenes` / `extract_ui_scenes` render-layer intersection test.
- `prepare_scene_affines`' world→vello affine chain, including the Y-flip
  (`model_matrix.w_axis.y *= -1.0` — vello is y-down, bevy world is y-up) and the
  skew negation. Getting this wrong is subtle and silent.

## 11. Known gotchas

- A `vello` scene has **no measurable bounds** — `bevy_vello` warns that `Aabb` stays
  default and content gets frustum-culled unless `NoFrustumCulling` is added. Any
  equivalent here needs an explicit bounds story (a scene graph *can* compute real bounds
  from `BezPath`s, which is an improvement worth taking).
- `Scene::new` takes `u16` dimensions — max 65535, fine for 5K but assert on it.
- `vello_hybrid` is **0.0.9 and explicitly "not yet suitable for production use."** Expect
  API churn; pin exactly and budget for upgrade work.
- Bevy 0.18.1 is on `wgpu` 27 while `vello_hybrid` 0.0.9/current main are on `wgpu` 29.
  Passing Bevy's device, texture format, or view across that boundary does not compile.
- Ironfell's current wasm `RUSTFLAGS` do not enable `+simd128`; `fearless_simd` therefore
  uses its scalar fallback. Do not describe a benchmark as “WASM SIMD” without verifying
  the target cfg and generated bundle.
- `Renderer::render()` clears the supplied target. Never call it directly on a populated
  Bevy `ViewTarget` unless the API contract has changed and a pixel-readback regression
  test proves prior content survives.
