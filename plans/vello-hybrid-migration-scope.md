# Migrating to Vello sparse strips: scope, complexity, and what caching actually buys

**Status:** Research findings — supersedes parts of [`bevy-vello-hybrid-design.md`](./bevy-vello-hybrid-design.md)
**Date:** 2026-08-15
**Audited against:** `vello` @ `main` (post-`v0.10.0` / `vello_hybrid` 0.2.0), `bevy` 0.19.1, `bevy_vello` 0.13.1 + `main`
**Predecessors:** [`bevy-vello-hybrid-design.md`](./bevy-vello-hybrid-design.md), [`bevy-vello-hybrid-open-questions.md`](./bevy-vello-hybrid-open-questions.md), [`vision/01_layered-architecture-overview.md`](./vision/01_layered-architecture-overview.md)

---

## 0. Headline

Three things changed in the three weeks since the 2026-07-24 audit, and one important thing did not.

**Changed — the compile-time blocker is gone.** `vello_hybrid` 0.2.0 (2026-08-07) and Bevy 0.19.1 both depend on `wgpu ^29.0.3`. `app-surface` 1.12.0 is the matching release. The "Bevy 27 vs hybrid 29" gate that produced the previous NO-GO no longer exists.

**Changed — hybrid got a core rewrite.** [vello#1759](https://github.com/linebender/vello/pull/1759) ("a complete rewrite of the core logic", shipped in 0.1.0) removed coarse rasterization, removed `SceneConstraints`, and rewrote the scheduler. Layer/filter performance improved and memory allocation became lazy. The device-space, viewport-culled, CPU-strip-generating character of the renderer is unchanged.

**Changed — Bevy↔Vello texture sharing is now a supported path.** `Scene::draw_texture_rect(ExternalTextureRect)` plus `TextureBindings` (0.1.0/0.2.0) let a vello scene sample an arbitrary externally-owned `wgpu::TextureView`. This is the primitive a tile cache needs, and it did not exist in the version previously audited.

**Did not change — there is no cross-frame caching primitive, and there will not be one upstream.** The `Recording` API that provided cached sparse strips was removed in [vello#1611](https://github.com/linebender/vello/pull/1611) (2026-05-04). Both the removal thread and the June 2026 roadmap thread make clear it is not coming back in that form. `Scene::strip_storage` and `Scene::recorder` remain `pub(crate)`. `Config::strip_offset_x/y` still exist in the shader and are still hardcoded to `0` in both backends.

**The conclusion is therefore inverted from the previous document, but not for the reason you might expect.** The migration is now *possible*. It is not thereby *cheap*, and the caching you want is still something you build, not something you adopt. What is newly true is that the pieces you need to build it with are all public.

---

## 1. Version reality, verified

| Crate | Version | wgpu | Verified by |
|---|---|---|---|
| `bevy` / `bevy_render` | 0.19.1 (2026-08-13) | `^29.0.3` | crates.io deps API |
| `vello_hybrid` | 0.2.0 (2026-08-07) | `^29.0.3` | crates.io deps API |
| `vello` (classic) | 0.10.0 | `^29.0.3` | crates.io deps API |
| `app-surface` | 1.12.0 (2026-06-17) | `^29` | crates.io deps API |
| **Current app** | bevy 0.18.1 | 27.0.1 | `Cargo.lock` |

`app-surface` 1.13.0 has already moved to wgpu 30, so `=1.12.0` is the pin, exactly as `=1.8.1` is today. `vello` pins `wasm-bindgen = "0.2.114"` as a caret requirement, so this project's `0.2.126` pin resolves cleanly.

**The bevy 0.19 upgrade is now a prerequisite, not an option.** That is a real cost item and it is on the critical path: it is the same upgrade the [[bevy-018-upgrade]] note deferred because `bevy_vello` 0.13.1 required bevy `^0.18`. Note that `bevy_vello` `main` has since moved to bevy 0.19 + vello 0.9 ([`223`](https://github.com/linebender/bevy_vello/pull/223), 2026-07-30, unreleased), so upgrading bevy *without* migrating renderers is also possible via a git dependency — useful as a de-risking step (see §8, Phase 0).

---

## 2. What a frame actually costs in `vello_hybrid` today

This is the part that determines everything else, so it is stated with citations. All paths are relative to `sparse_strips/` in the vello checkout.

### 2.1 The pipeline

```
BezPath ──flatten──▶ line segments ──tile──▶ tiles ──sort──▶ strips + alpha coverage bytes
   (CPU, device space, per record)                                       │
                                                                         ▼
                                              CommandRecorder (retained in Scene)
                                                                         │
                                     ┌───────────────────────────────────┘
                                     ▼
                          Schedule::try_new  (CPU, EVERY render call)
                                     │
                                     ▼
             upload alphas + encoded paints + strips  (GPU, EVERY render call)
                                     │
                                     ▼
                        render passes (vertex/fragment only, no compute)
```

### 2.2 What is retained across frames

Holding a `Scene` and not touching it retains **flattening, tiling, strip generation, and antialiasing coverage**. That is the expensive half. Native M5 measurements from the previous audit: rebuild+render of 50k paths ≈ 17.5–18.7 ms, retained render of the same ≈ 1.2–1.8 ms. **Roughly a 10× difference, available for free** — simply don't rebuild the scene.

### 2.3 What is *not* retained — the per-frame floor

Every call to `Renderer::render` redoes all of this, even for a scene that has not changed by one byte:

| Work | Where | Cost shape |
|---|---|---|
| Paint preparation | `vello_hybrid/src/render/wgpu/mod.rs:465` | O(encoded paints) |
| **Schedule construction** | `:479-488` → `Schedule::try_new` | O(recorded draws × wide tiles) |
| **Full alpha texture upload** | `:2546-2584` | O(alpha texture size), *not* O(changed) |
| **Full encoded-paints texture upload** | `:2587-2613` | O(paints texture size) |
| **Fresh strips `Buffer` allocation + upload, per draw pass** | `:2688-2719` | O(strips), plus an allocation |
| Render passes | `schedule::execute` | O(strips) |
| Target clear | `:505-507`, `clear_view` at `:534-551` | full view |

Two of these deserve emphasis, because they are exactly the "repeated loading of GPU buffers every frame" you asked about, and both are acknowledged as unfinished in the source:

**The alpha texture is re-uploaded in full, every frame.** `upload_alpha_texture` pads `alphas` out to the *entire* texture extent and issues one `queue.write_texture` covering the whole thing. The texture is `max_texture_dimension_2d` wide at 16 bytes per texel — one row is 128 KB at 8192 wide, 256 KB at 16384 — and the code **grows but never shrinks** it (`maybe_resize_alphas_tex`, `:2292-2325`). So a scene that momentarily needed a tall alpha texture keeps paying that full upload on every subsequent frame, forever. The inline comment concedes the design:

> `// TODO: For the time being, we upload the entire alpha buffer as one big chunk. As a future refinement, we could have a bounded alpha buffer, and break draws when the alpha buffer fills.` — `:491-493`

**The strips vertex buffer is allocated fresh for every draw pass of every frame.**

> `// TODO: We currently allocate a new strips buffer for each render pass. A more efficient approach would be to re-use buffers or slices of a larger buffer.` — `:2771-2772`

Neither is architecturally hard to fix; both are small, local, forkable patches (see §6.4). Neither is likely to be your bottleneck at the current app's shape count. Both become significant at editor scale.

### 2.4 Structural constraints that survived the rewrite

- **Device space, viewport culled.** `StripGenerator::generate_filled_path` culls to `RectU16::new(0, 0, width, height)` unless a clip narrows it further (`vello_common/src/strip_generator.rs:118-168`). Geometry outside the scene rectangle is discarded at record time. A retained scene therefore cannot be panned to reveal content it never recorded.
- **Flattening tolerance is device-space and fixed.** Scale changes change the generated geometry and the coverage. There is no public LOD or tolerance override.
- **Every public `render()` clears the target.** `render` → `render_scene(..., clear = true, ...)` (`:305-318`). `render_scene` takes a `clear: bool`, but it is private. Two scenes still cannot composite into one view.
- **No `Scene::append`, no public strip injection.** `strip_storage` and `recorder` are `pub(crate)` (`scene.rs:207-228`).
- **`strip_offset_x/y` remain dead.** The shader reads them (`vello_sparse_shaders/shaders/render.wesl:102,267`); the wgpu backend writes `0` (`:1973`, `:2420`), as does WebGL.

---

## 3. What efficiencies are actually possible — the transform taxonomy

You asked to be reminded what is possible for shapes that have only translated, rotated, or scaled. The honest answer is stratified, and the strata behave very differently.

### 3.1 Tier 0 — retained scene (available now, free)

Don't rebuild the `Scene` when nothing changed. Saves the ~10× flatten/tile/strip cost. Still pays the §2.3 floor. **This is the single highest value-per-effort item in the entire document.** Bevy change detection gives it to you almost directly.

### 3.2 Tier 1 — culling (available now, cheap, no invalidation hazards)

Only record what is inside the viewport. For an editor, this is usually the whole game: if a spatial index keeps the visible shape count in the hundreds, a full rebuild per frame is ~0.3 ms and the caching problem never arises. Nico Burns made exactly this point on Zulip and it is the correct ordering:

> most people are just creating new Scene(s) "on-demand" when rendering the frame … Cached scenes for unchanged subtrees can be cheaper, but runs into all the usual problems around cache invalidation.

Culling has no correctness hazards. Caching has many. **Do culling first and measure before building anything in Tier 3.**

### 3.3 Tier 2 — strip-level caching (does not exist; narrow even if it did)

This is what `Recording` was. Its reuse envelope is much narrower than "translate, rotate, scale" implies, and the constraints are physical, not API accidents. From the "Translating Cached Path Sparse Strips" Zulip thread (Tom Churchman, Laurenz Stampfl, Daniel McNab, Nov–Dec 2025):

| Transform delta | Can cached strips be reused? | Why |
|---|---|---|
| None | **Yes** | Identical device-space output |
| Integer X translation | **Yes, in principle** | X is the winding direction; strips translate freely. Negative offsets need `i32` coords instead of `u16` |
| Integer Y translation, multiple of 4 | **Yes, in principle** | 4 is the strip height |
| Integer Y translation, not multiple of 4 | **No** — must regenerate | The `fill_gap` winding flag goes stale. A wide rectangle aligned to a strip edge is one sparse row; shifted 1px it becomes two dense rows |
| Sub-pixel translation | **No** | Analytic AA coverage changes at every edge |
| Rotation | **No** | Geometry re-flattens; coverage is entirely different |
| Scale (zoom) | **No** | Flattening tolerance is device-space; subdivision and stroke tolerance both change |

So Tier 2 buys you **integer translation and nothing else**. Alex Gemberg measured ~10× on the naive translation implementation, which is real, but the applicable workload is scrolling and glyph placement — not an editor where the interesting gestures are zoom and free transform. Daniel McNab's framing on the same thread: the motivation is glyph caching, and "I don't think we particularly care to do non-integer translation."

**Upstream position:** not on the roadmap. Laurenz Stampfl's removal rationale on Zulip cites three reasons — memory cost, incompatibility with the glifo-based text setup, and, most durably:

> I think it will become much harder to support recordings for Vello Hybrid in a (potential) future where most stuff moves onto the GPU, since the strip alphas won't be calculated on the CPU anymore and thus cannot be cached conveniently anymore.

Olivier Faure's June 2026 roadmap post proposes reintroducing a `Recording` type but explicitly as "pure lists of commands storing no cached data" — the opposite of what you want. Tom Churchman's comment is the one worth keeping, because it describes the shape of the thing that *would* be useful:

> Path caching definitely still seems useful, but especially as a generalized cache that can also handle some classes of transform. Recordings/scenes as a concept do make sense to me if they're transformable, which seems like it could be handled by an abstraction above.

**"An abstraction above" is you.**

### 3.4 Tier 3 — raster / tile caching (available now, and the only tier that makes rotate and scale cheap)

Render content once into a texture; thereafter any affine transform is a textured quad. CPU cost per cached unit per frame ≈ zero. This is what Figma, Sketch, and every browser compositor actually do, and it is the only mechanism in this document that makes rotation and scale cheap.

Three public primitives now support it:

1. **`Renderer::render_to_atlas`** (`#[doc(hidden)]`, used internally for glyphs) — renders a scene directly into an atlas layer at an allocated offset, then `queue.submit`s immediately.
2. **`Scene::draw_texture_rect(ExternalTextureRect)` + `TextureBindings::insert(TextureId, TextureView)`** — new in 0.1/0.2. Binds *any* externally-owned `wgpu::TextureView` and samples a sub-rect of it into a destination rect in scene coordinates. This is the seam that lets Bevy own the tile atlas.
3. **`Renderer::upload_image` / `destroy_image` / `atlas_texture`** — the vello-owned image cache path.

Two composition strategies fall out, and they are genuinely different:

| | **Bevy-side composite** | **Vello-side composite** |
|---|---|---|
| Cached tiles live in | Bevy textures / a Bevy-owned atlas | vello's image cache or an external texture |
| Composited by | A Bevy mesh/material pass, one quad per tile | `draw_texture_rect` inside the vello scene |
| Vello sees per frame | Dirty tiles + live overlay only | Dirty tiles + live overlay + N texture rects |
| Painter-order interleaving of cached and live content | Not possible — cached block draws as one layer | **Correct** — rects sit in document order |
| CPU cost for clean content | ~0 | ~0, plus one recorded draw per tile |

The right answer is probably both: Bevy-side for the background tile grid, vello-side for cached subtree images that must interleave with live vector content. Design the seam so "emit this subtree" has two implementations — live traversal or cached rect — exactly as the earlier design brief recommended.

**Quality policy is the real work here, not the plumbing.** Tiles are keyed by a discrete zoom bucket; continuous zoom scales stale tiles as a proxy and progressively refills the new bucket; rotation of a cached subtree is fine if it was rasterized at adequate resolution, and needs re-rasterization when the effective scale crosses a threshold. Section 5 of the vision doc already specifies this correctly.

### 3.5 The summary you asked for

> For content that only **translates by integer pixels**, strip caching would give ~10× — but the API does not exist and upstream is not building it.
> For content that **rotates or scales**, no strip- or geometry-level cache can help; the coverage genuinely changes. Only a raster cache makes those cheap.
> For content that **does not change at all**, you already get ~10× for free by not rebuilding the `Scene`, and you get the rest by never invoking the renderer on it — which is a tile cache.

The corollary is that the cache tier worth building is Tier 3, and the tier that generated all the excitement about sparse strips (Tier 2) is the one you should probably skip.

---

## 4. Upstream trajectory and its risks

Worth understanding before committing, because two of these are strategic rather than technical.

**Sparse strips *is* Vello now.** From the June 2026 roadmap thread, Olivier Faure proposes officially putting classic Vello in maintenance mode, dropping the `0.0.x` versioning for sparse strips, and retiring the name "Vello Classic". Raph Levien confirmed the resourcing reality: "It is difficult for me to justify work time on Vello classic because there is no clear path to shipping it at Canva." Migrating *toward* sparse strips is migrating toward where the maintenance is.

**`bevy_vello`'s own future is uncertain.** Its maintainer, Spencer Imbleau, posted in that same thread:

> I'm really hoping GPU gets a clear story. Is there benchmarks comparing hybrid to GPU? I'm not sure switching my stack (or others) to hybrid will be performant enough. And if it's not, I won't need vello anymore - and that would potentially jeopardize bevy_vello.

You are currently one release behind on a crate whose maintainer is publicly unsure whether to continue. That is an argument *for* owning the integration layer, independent of any performance question.

**Hybrid performance is under active work, in a direction that helps you.** Canva ran a two-prong perf sprint: optimizing the strip pipeline, and a "blit rect" fast path (Zulip, Feb 2026, Taj Pereira) that bypasses strip generation entirely for unclipped, unblended textured rects — "scenes that used to take multiple milliseconds now take a few hundred microseconds." Raph and Nico both pushed for axis-aligned rect clips to be in scope. **If that lands, it is the fast path a tile compositor wants**, and it makes Tier 3 cheaper still. It does not appear to be in `main` yet.

**The #936 startup stall is unverified and still open.** [vello#936](https://github.com/linebender/vello/issues/936) has not been updated since 2025-08-29. DJMcNab's statement that sparse strips would "incidentally resolve" it remains a hypothesis. **You have the only machine that reproduces it at 1.5 s.** Measuring this is cheap, is the stated premise of the entire migration, and the maintainers have asked for exactly that data.

**Relevant prior art that appeared since the last audit:** [`forest-rs/cachet`](https://github.com/forest-rs/cachet) (Bruce Mitchener, Apr 2026) — `cachet_residency` / `cachet_storage` / `cachet_atlas` / `cachet_surface`, a bounded-residency and eviction kernel explicitly aimed at glyph caches, tiled surfaces, and zoomable canvases. It is early and exploratory. More useful than the crate, right now, is Nicolas Silva's reply on that thread describing WebRender's three-layer architecture (atlas allocator → `TextureCache` with LRU and compaction → `ResourceCache` glue), plus his recommendation of `etagere` over `guillotiere` for long-lived caches because of fragmentation behaviour. That reply is the best available blueprint for your Layer 5, and it comes with a warning worth heeding: he notes he has "not felt the need in WebRender for as much abstraction as is presented in cachet" — plain composed structs, no traits.

---

## 5. What `bevy_vello` does for you today, and what replacing it costs

Current usage in this repo is **small**: 9 `VelloScene2d` entities across `overlay2d.rs`, `timeline.rs`, `ui_panels.rs`, `vello_world_demo.rs`, and roughly 10 systems that call `reset()` / `fill()` / `stroke()` / `push_layer()` / `pop_layer()`. The drawing-call translation is nearly mechanical (`scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, affine, &clip)` becomes `scene.push_clip_layer(&path)`; `scene.fill(rule, affine, color, None, &shape)` becomes `set_transform` + `set_paint` + `fill_path`).

What you lose and must rebuild:

| `bevy_vello` provides | Replacement cost |
|---|---|
| `VelloPlugin`, render-app wiring, `RenderSet` ordering | Small — pattern is copyable from `render/plugin.rs` |
| Extract world/UI scenes with `RenderLayers` intersection | Small — copy the intersection test |
| World→vello affine chain, incl. Y-flip and skew negation | **Small but treacherous.** `model_matrix.w_axis.y *= -1.0`. Getting it wrong is silent |
| Render-to-`Image` + `VelloCanvasMaterial` fullscreen quad composite | Medium — or replace with a render-graph node, which you want anyway |
| Screen-space vs world-space scene handling | Medium — hybrid's `Scene` is inherently pixel-sized, so the bias inverts |
| Resize handling | Small — `Scene::reset_and_resize` is new in 0.1.0 |
| SVG / Lottie / text integrations (~2,250 of its 5,316 lines) | **Not needed.** Out of scope |
| `Aabb` / frustum-culling workarounds | Improvement: a real scene graph computes true bounds from `BezPath` |

What you gain immediately: a **render-graph node** instead of a material quad. `Renderer::render` takes `&mut CommandEncoder` and a `&TextureView`, and Bevy's `RenderContext::command_encoder()` hands you exactly that; `RenderDevice::wgpu_device()` and `RenderQueue` give the other two. This is a cleaner fit than `bevy_vello`'s system-based approach, which predates needing it. The one constraint: because `render()` clears its target, the node must own its target texture (or own the whole 2D layer), not draw into a populated `ViewTarget`.

Also gained: the dependency floor. `vello_hybrid` needs `bytemuck, thiserror, vello_common, log, hashbrown` + `wgpu`; `vello_common` is `no_std` + alloc. The previous audit's spike confirmed `bevy_sprite_render`, `bevy_text`, `bevy_ui`, `bevy_winit`, `bevy_pbr`, `bevy_gizmos_render`, and `bevy_state` can all stay out of the graph — though it also measured that removing `bevy_text` alone saved only 13.8 KB brotli, so do not oversell the wasm-size argument.

---

## 6. What you have to build

Mapped onto the layers in [`vision/01`](./vision/01_layered-architecture-overview.md). Everything below Layer 4 is renderer-independent and is worth building whether or not the renderer migration happens.

### 6.1 The renderer control layer (`iron_vector_hybrid`)

The thin part. A `Renderer` + `Resources` pair held as a render-world resource, a render-graph node, target management, and a `VectorRasterizer`-shaped interface:

```rust
trait VectorRasterizer {
    fn prepare_resources(&mut self, changes: &[ResourceChange]);
    fn rasterize(&mut self, list: &DisplayList, target: RasterTarget,
                 clip: DeviceRect, transform: Affine) -> RasterResult;
}
```

The hybrid implementation of `rasterize` is: reset a pooled `Scene` to the target size, walk the display list emitting `set_transform`/`set_paint`/`fill_path`/`draw_texture_rect`, then `render()` into the target view. **Note the shape of that interface: it takes a bounded workload and a target.** That is deliberate — it is the shape that lets one `Scene` serve as a tile rasterizer rather than a document, which is the only role hybrid's public API supports well.

### 6.2 Display list and coalescing (`iron_presentation`)

Renderer-neutral ordered primitives with stable document IDs, grouped into chunks by document or spatial boundary. This is where shape coalescing lives — batching adjacent primitives that share paint state so the scene builder emits fewer state changes, and grouping by chunk so invalidation has a unit. Vello has nothing to offer here; it is entirely yours.

### 6.3 Spatial index, damage, and cache management (`iron_spatial`, `iron_canvas_cache`)

The bulk of the work, and the part with the real risk. Bounds, AABB index, dirty queues with deduplicated IDs, old∪new damage rects inflated for stroke and filters, tile keys by zoom bucket, atlas allocation, LRU eviction, memory budget, and a zoom proxy policy.

Do not invent the architecture. Use Nicolas Silva's WebRender three-layer description (§4) as the blueprint, `etagere` as the allocator, and `understory_index` / `understory_view2d` as either dependencies or references. Note that `understory_tiling`, despite the name, is about **docking and pane layout**, not raster tiles — it is interesting for this app's panel system and irrelevant to the cache.

### 6.4 Optional: a forked hybrid with the per-frame floor fixed

Three small, well-localized patches, in ascending order of value and difficulty:

1. **Bounded alpha upload.** Track a dirty row range and `write_texture` only those rows instead of the whole texture. Removes a per-frame multi-hundred-KB-to-MB upload that scales with the high-water mark rather than with content. Low risk, self-contained.
2. **Persistent / suballocated strips buffer.** Replaces a per-pass `create_buffer` with a growable pool. The code's own TODO.
3. **Expose `clear: bool` and `strip_offset_x/y`.** Both already exist internally; the shader already reads the offsets. Turns "one scene per view" into "compose several scenes", and enables integer-pan reuse *within a recorded overscan margin*.

Patch 3 is the one that changes the architecture, and it is also the one most likely to conflict with upstream churn. Patches 1 and 2 are the kind of thing that could be upstreamed rather than forked, and probably should be attempted that way first. **Do not fork to reintroduce `Recording`** — §3.3 explains why the payoff is narrow, and you would own it against a codebase that is actively moving work toward the GPU.

---

## 7. Scope and complexity, priced in four independent tranches

These are sized for one focused engineer. They are sequential in dependency but each has standalone value, and each has a real stopping point.

### Tranche A — Bevy 0.19 upgrade *(prerequisite, 3–7 days)*

Bevy 0.18 → 0.19, `app-surface` `=1.8.1` → `=1.12.0`, wgpu 27 → 29, `bevy_vello` via git (its `main` already supports 0.19) or crates.io when released. **Ship this on its own, with the renderer unchanged.** It de-risks everything downstream and is independently useful. Verify with `cargo tree -i wgpu` — exactly one version.

### Tranche B — replace `bevy_vello` with a thin hybrid integration *(2–4 weeks)*

Plugin, render-graph node, target texture, extract + `RenderLayers` filtering, the affine chain including the Y-flip, screen/world space handling, resize, and porting the 9 scenes and ~10 draw systems.

**Buys:** control of the render path; a possible fix for the #936 startup stall; a smaller dependency graph; no dependency on a crate whose maintainer is publicly wavering.
**Does not buy:** any cross-frame efficiency. Frame cost is unchanged or slightly worse until Tranche C.
**Risk:** low and bounded. The API surface you need is small and stable-ish. `vello_hybrid` is 0.2.0 and still self-describes as having "parts of the API and its documentation … still suboptimal" — pin exactly and budget for upgrade churn.

### Tranche C — culling, change detection, and damage *(4–8 weeks)*

Spatial index, viewport + zoom culling, revision stamps, dirty queues, display-list chunking, and the discipline that an idle frame rebuilds nothing.

**Buys:** the largest measured win available, at the lowest risk. Idle frames become free. Edit cost becomes proportional to the *visible* set rather than the document.
**Risk:** low. No invalidation hazards, because nothing is cached yet.
**This is the tranche most likely to make Tranche D unnecessary.** Measure at 10k shapes before starting D.

### Tranche D — tile / subtree raster cache *(2–4 months)*

Atlas allocation and eviction, tile keys by zoom bucket, painter-correct per-tile display lists, zoom proxy and refinement, memory budget and instrumentation, plus the composite path (Bevy-side, vello-side, or both).

**Buys:** cost proportional to actual damage; cheap pan; cheap zoom via proxies; cheap rotation and scale of static complex subtrees.
**Risk:** high, and it is *correctness* risk, not performance risk. Painter order across a partially-cached document is the classic place this goes wrong — an edited shape between two unchanged ones cannot be drawn as a topmost overlay, which is why tiles rebuild their full display list. Budget for a visual regression suite before, not after.

**Total for a credible tiled editor architecture: roughly 4–7 months.** Tranches A+B alone: about a month. The temptation to price only A+B and assume the rest follows is the main way this estimate goes wrong.

---

## 8. Recommended sequence, with gates

**Phase 0 — measure the premise before spending anything else (days, not weeks).**

1. Build `sparse_strips/vello_hybrid/examples/wgpu_webgl` for wasm. Measure GPU-process time to first paint in Chrome on the **Windows / NVIDIA 4080** machine. Baseline is ~1.5 s. Post the numbers to [vello#936](https://github.com/linebender/vello/issues/936).
2. Enable `-Ctarget-feature=+simd128` in `build-wasm.sh` and verify `fearless_simd` actually selects a SIMD path — the current build silently uses the scalar fallback, so any benchmark run before this is not measuring the real thing.
3. Do Tranche A (bevy 0.19) with `bevy_vello` still in place.

> **Gate 1 result, 2026-08-15 — PASSED.** Measured on the Windows / NVIDIA 4080 machine with
> the harness in `../vello-startup-test`, 2000 shapes, cold browser, AA area-only:
> **vello classic 1370 ms worst gap → vello_hybrid 281 ms.** Time to first frame 60 ms vs
> 152 ms (hybrid pays more CPU up front; see below). The GPU track shows the classic stall
> as one continuous block. This is the first measurement from affected hardware on
> [#936](https://github.com/linebender/vello/issues/936) and is worth posting there.
>
> **Counter-result from the same harness — throughput is the opposite way round.** At 25 000
> shapes with a full scene rebuild every frame, classic holds 16.7 ms (vsync) while hybrid
> takes 100.7 ms. Startup and throughput have opposite winners, and the harness's
> rebuild-everything-with-a-rotation loop is hybrid's worst case by construction. This is
> the measured form of §3: hybrid is only viable with the work-avoidance layers, and the
> ordering in Tranches C and D is therefore not optional.

**Gate 1:** if #936 does not improve meaningfully, the startup argument for the migration is dead. The other arguments — maintenance direction, dependency floor, integration control — may still carry it, but they are much weaker, and you should say so explicitly rather than letting the original premise quietly survive its own falsification.

**Phase 1 — Tranche B**, with `overlay2d` / `timeline` / `ui_panels` as the acceptance test. Instrument from day one: record-time, schedule-time, upload bytes, GPU time, and composite time as *separate* counters. "Frame time" will not tell you which of §2.3's line items is hurting.

**Gate 2:** a 10k-shape benchmark document, measured in four conditions separately — static, continuous pan, continuous zoom, edit-one-shape — against the current `bevy_vello` build. Do not presume the outcome. With today's API, pan, zoom, and single-shape edits all trigger a full visible-scene rebuild, so hybrid can plausibly lose three of four.

**Phase 2 — Tranche C.** Re-run the four-condition benchmark. **Gate 3:** if culling alone brings all four conditions inside frame budget at realistic zoom levels, stop. Do not build Tranche D because the architecture document says the architecture has a cache in it.

**Phase 3 — Tranche D**, only if Gate 3 fails, and only in the regime where it fails (which will be zoomed-out views of large documents — which is also, conveniently, the regime where a raster proxy is visually near-lossless).

---

## 9. Open questions

| # | Question | How to answer | Blocking? |
|---|---|---|---|
| 1 | Does #936 improve under sparse strips? | Phase 0.1 — Windows/NVIDIA browser test | **Yes** — it is the premise |
| 2 | What does hybrid cost at 10k shapes *in the browser with SIMD on*? | Phase 0.2 + Gate 2 | Yes for Tranche C sizing |
| 3 | Will the blit-rect fast path land, and does it cover clipped rects? | Watch vello `main`; ask on Zulip `#vello` | No, but it changes Tranche D's cost |
| 4 | Are patches 6.4/1 and 6.4/2 acceptable upstream? | Open a Zulip thread before writing code | No — but asking first is much cheaper than forking |
| 5 | `render_to_atlas` is `#[doc(hidden)]` — is it stable enough to build a tile cache on? | Ask upstream; else use external textures + Bevy-owned atlas | Yes for Tranche D's composite strategy |
| 6 | Does hybrid's depth/early-z path interact correctly with a Bevy-owned target? | Spike; `depth_view` is now caller-owned ([#1810](https://github.com/linebender/vello/pull/1810)) | Yes for Tranche B |
| 7 | Bevy-side or vello-side tile composite? | Decide at Tranche D from painter-order requirements | No — design the seam to allow both |

---

## Appendix A — reading the Vello Zulip without a browser

You asked how to get at this. Zulip's web app is JS-rendered, but `xi.zulipchat.com` has `realm_web_public_access_enabled: true`, and its **spectator API works over plain HTTP with no account** — no headless browser needed. The trick is that the narrow must include `{"operator":"streams","operand":"web-public"}`; without it every request 401s, including ones that name a channel explicitly.

```bash
# 1. Get a CSRF token and cookie jar
curl -sS -c cj.txt -o page.html 'https://xi.zulipchat.com/'
TOK=$(grep -o 'name="csrfmiddlewaretoken" value="[^"]*"' page.html | sed 's/.*value="//;s/"//')

# 2. Register anonymously (client_gravatar MUST be false for anonymous requests)
curl -sS -c cj.txt -b cj.txt -X POST -H "X-CSRFToken: $TOK" \
     -H "Referer: https://xi.zulipchat.com/" -d 'client_gravatar=false' \
     'https://xi.zulipchat.com/json/register'

# 3. Fetch messages — note the mandatory web-public operand
NARROW='[{"operator":"streams","operand":"web-public"},
         {"operator":"topic","operand":"Linebender 2026 roadmap - Vello"}]'
curl -sS -c cj.txt -b cj.txt -G -H "X-CSRFToken: $TOK" \
     -H "Referer: https://xi.zulipchat.com/" \
     --data-urlencode "narrow=$NARROW" \
     --data 'anchor=newest&num_before=200&num_after=0&apply_markdown=false&client_gravatar=false' \
     'https://xi.zulipchat.com/json/messages'
```

Substituting `{"operator":"search","operand":"path caching"}` for the topic operand gives full-text search across every web-public channel, which is how the threads below were found. A working script is at `scratchpad/zulip.sh` in this session's scratch directory; it is four lines and worth keeping.

## Appendix B — source index

**Vello PRs and issues**

| Ref | What |
|---|---|
| [#1137](https://github.com/linebender/vello/pull/1137) | Recording API / path sparse-strip caching added, 2025-08-13. Read the description for the benchmark table and the y-misalignment caveat |
| [#1155](https://github.com/linebender/vello/pull/1155) | Spatio-temporal compositing — the wide-tile scheduler and slot ping-pong. Best available explanation of how hybrid schedules layers |
| [#1611](https://github.com/linebender/vello/pull/1611) | Recordings removed, 2026-05-04 |
| [#1759](https://github.com/linebender/vello/pull/1759) | Hybrid core rewrite; coarse rasterization removed; `SceneConstraints` gone |
| [#1789](https://github.com/linebender/vello/issues/1789) | Open request for offset/region rendering for atlas-style caching — same use case as Tranche D |
| [#936](https://github.com/linebender/vello/issues/936) | The startup stall. Open, stale since 2025-08-29, awaiting your data |
| [#1810](https://github.com/linebender/vello/pull/1810) / [#1808](https://github.com/linebender/vello/pull/1808) | Depth texture ownership moved to callers; depth can be disabled |

**Zulip threads (`#vello`, all web-public)**

- *Translating Cached Path Sparse Strips* — the transform taxonomy in §3.3, straight from the maintainers
- *Removing recordings from vello_common* — the removal rationale
- *Linebender 2026 roadmap - Vello* — strategic direction, classic-vs-hybrid, `bevy_vello`'s exposure
- *Cachet: glyph caching, tiled surfaces, and bounded residency* — plus Nicolas Silva's WebRender architecture reply
- *Blit rect pipeline* — Canva's fast path for texture/text-heavy scenes
- *A graphic editor example via WASM* — [infinitecanvas.cc/experiment/vello](https://infinitecanvas.cc/experiment/vello), an editor built on classic vello with viewport culling and dirty flags. Closest existing prior art to this project

**Local checkouts**

- `/Users/andrew/Documents/PROGRAM_LOCAL/vello` — `main`, post-0.10.0. Note it contains a nested `bevy_vello/` checkout on bevy 0.19 + vello 0.9
- `/Users/andrew/Documents/PROGRAM_LOCAL/bevy_vello` — older, 0.10.2-era. Prefer the nested copy for reference
