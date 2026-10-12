# View textures: everything renders to a texture sized to where it is seen

**Status:** Proposed 2026-10-11, decided with the user: spike and measure first, then
migrate renderer by renderer, converting each only where measurement shows no loss at
reasonable screen sizes.
**Relates to:** `entity-composition-model.md` §7.4 (embedded compositions, step 5),
`bevy-vello-hybrid-design.md` (the vector backend), `roadmap.md` stage H (render
invalidation), the 5K frame-pacing notes (`perf-grid-runbook.md`).

---

## 1. Why

Read from the source (Bevy 0.19.1, `src/vector/hybrid.rs`) on 2026-10-11:

- **Every Bevy camera's buffers are window-sized.** `prepare_view_targets` allocates
  `main_texture_a`, `main_texture_b` (and a sampled texture under MSAA) at
  `camera.physical_target_size`, the whole render target; the 3D depth texture is sized
  the same way. A viewport only restricts where the camera draws. A 3D panel a fifth of
  the window pays for a full-window buffer set; at 5K, each one is 5120×2880.
- **The vector backend renders a window-sized texture every frame.** All 2D layers merge
  into one `vello_hybrid` scene drawn into a window-sized offscreen image and composited
  by a full-window quad. The CPU scene rebuild is skipped when nothing changed, but
  `render_scene` still clears and draws the whole texture each frame.

So per-frame GPU work and memory scale with window area times view count, not with what
is visible, and idle frames are not free. The same structure is also what blocks
embedding: a composition cannot be drawn into anything but the shared screen.

## 2. The model

- **A view** is one source (a 2D or 3D composition, the timeline, a debug overlay) seen
  one way (a camera or pose for 3D; a content-space rectangle and scale for 2D) at a
  resolution. It renders into **its own texture**. Several views may show one
  composition (two panels, two embeds, a front and a side camera).
- **Uses** of a view's texture: pasted 1:1 into a panel by the compositor; drawn inside
  a 2D view with an affine transform (`vello_hybrid`'s `draw_texture_rects`, which takes
  external texture views); sampled on a plane in a 3D view, with mipmaps.
- **The view graph.** A view that uses another's texture depends on it; dependencies
  render first. Embed loops are refused by the store, so the graph is acyclic.
- **Retained textures.** A view re-renders only when its content changed (the
  reconciler's changed-node set, filtered to its composition and what it embeds), its
  size or camera changed, or a dependency re-rendered. A static panel then costs one
  composite quad per frame. The view texture is the cache stage H was going to build,
  at the granularity that already exists.
- **The compositor** clears the window once and draws each panel's view texture as a
  1:1 quad (reusing `composite_material`'s premultiplied, UNORM, clip-space blit), then
  `bevy_ui` on top.
- **Fits `vello_hybrid`.** Its `render` clears its target, which is why every 2D layer
  had to merge into one scene. With one target per view that constraint disappears: each
  view's scene renders into its own texture.
- **Inline is still allowed.** A 2D embed inside a 2D view can keep drawing as vectors
  inline (crisper, no texture) and switch to its view texture when that is cheaper
  (large and static). Which one is a per-use policy, decided by measurement.

## 3. Resolution

Derived from on-screen footprint (decided):

- A panel's view: the panel's pixel size.
- A view used in 2D: its footprint after the use's transform.
- A view on a 3D plane: the texel density its **nearest visible part** needs (project the
  plane's visible corners; the largest screen-space scale governs), capped.
- Sizes round up to coarse buckets (multiples of 64 px; for scaled uses, steps of √2) with
  hysteresis, so dragging a pane divider or a slow zoom does not reallocate every frame.
  Capped by the device's maximum texture size and a memory budget; over budget, the
  least recently visible views drop resolution first.

## 4. Foundation for demand-driven, tiled view textures (extension, later)

The user's extension: when a 3D camera sees part of a 2D texture up close and the rest
far away or off screen, render each region at the resolution it needs, and nothing for
what is not visible. This is virtual texturing (clipmaps, sparse virtual textures) and
it fits planes well, because a plane's screen-space texel demand can be computed exactly
on the CPU (no GPU feedback buffer, which WebGL2 makes hard). What to lay now so it
slots in without redesign:

1. **Views are described in content space, not pixels.** A view request is "this
   content-space region at this density (texels per unit)", and pixel size follows.
   Today every request is one region at one density.
2. **Demand is a step in the frame.** Each use reports its demand on the view (region,
   density). Today the view takes the union region and the maximum density. Later the
   same reports, per tile, decide which tiles exist at which level, and frustum-culled
   tiles report zero.
3. **Allocation goes through a pool that returns (texture, uv rect),** and every use
   samples through that uv rect, never assuming it owns a whole texture. Later a tile is
   one such allocation in an atlas.
4. **Rendering a region is an affine on the scene.** A tile is the view's scene under
   translate-and-scale, culled to the tile; `vello_hybrid` rasterizes per target in
   device space, so a tile costs about what its pixels contain.

The extension then adds: a quadtree of tiles per view with a level per tile, chosen from
projected density per tile; tiled sampling, first as the plane split into sub-quads, one
per resident tile, each with its own texture (no custom sampler), later as a page table
with shader indirection; and progressive refinement (show the coarse level while a fine
tile renders). Direct perspective rasterization of a plane's vectors (projecting
flattened geometry through the homography, with the plane mesh as the depth proxy) stays
the alternative quality path; it uses the same plane, depth, and picking.

## 5. Spike and measurement (first)

Measure before migrating anything:

- **S1. A 3D panel into a panel-sized texture.** Its camera targets an `Image` the
  panel's size; the compositor pastes it 1:1. Compare against today's viewport camera.
- **S2. The 2D layer into panel-sized textures.** One `vello_hybrid` scene and target per
  2D panel (finding out whether one `Renderer` can serve several targets and sizes, or
  each needs its own), composited 1:1.
- **S3. Retention.** Skip a view's render when nothing it shows changed, for both kinds,
  so the idle cost is the composite only.

Method: headless Chrome on the GPU (`GPU=1`) with frame-rate limits off, so throughput
shows instead of being clamped at 60 fps; at 1600×1000 and at 5K (2560×1440 at device
scale 2); idle and dragging; 1, 2 and 4 panels; old path and new. Record frames per
second, worker tick time, and allocated texture bytes. A renderer moves to view textures
only if it is no slower at 1600×1000 and 2560×1440.

### Spike results (2026-10-11)

Built behind `?bevy=views` (`VARIANT_VIEW_TEXTURES`): S1 (every 3D camera, panel or
embed, renders into its own image sized to its rectangle in 64 px buckets, pasted by a
`ViewCompositeMaterial` quad texel for pixel; picking and drags map window px into the
camera's own px) and S3 (a texture-backed 3D camera renders only when its rectangle,
pose, lens, or 3D content on its layers changed, staying awake three frames after a
change; the vector backend skips `render_scene` when the scene was not rebuilt). S2
(per-view vector targets) is not built: the vector texture is still window-sized, but
no longer re-rendered when nothing changed.

Measured with headless Chrome on the GPU, vsync and frame-rate limit off, the main
layout plus `?panels=`; frames per second, idle (the scrub phase is short, so its
numbers mostly repeat idle and are omitted). Both paths in the same session:

| window | panels | old | views | change |
|---|---|---|---|---|
| 1600×1000 | 1 | 677 | 1143 | +69% |
| 1600×1000 | 2 | 555 | 910 | +64% |
| 1600×1000 | 4 | 500 | 845 | +69% |
| 2560×1440 @2 (5K) | 1 | 233 | 303 | +30% |
| 2560×1440 @2 (5K) | 2 | 191 | 237 | +24% |
| 2560×1440 @2 (5K) | 4 | 183 | 261 | +43% |

Worker CPU per tick also falls (5K, 4 panels: p50 2.9 → 1.9 ms). Run-to-run variation
is roughly ±10% (the old path measured 730 and 677 fps in two runs of the same case).
The smoke script passes on the views path with identical output. Two bugs found and
fixed: a camera woken for one frame rendered with per-view state from before it slept
(dark, no ground) until the system ran before frusta and visibility and kept woken views
awake a few frames; and (both paths) a clipped embed reframes its camera to the clipped
strip instead of cropping the full view (not yet fixed).

**What remains window-sized:** the background camera's clear, the vector composite
camera's main textures and full-window composite quad, and the window present. At 5K
an idle frame still costs ≈3.3 ms with nothing re-rendering, which is that overhead; S2
and a single compositor pass are where it goes.

**Conclusion:** convert. Retention is the largest single win, and it only exists
because each view keeps its own texture.

## 6. Migration order (after the spike)

1. Composition panels (2D and 3D) onto views and the compositor; drop window-sized
   targets for them.
2. 3D embeds in 2D: their camera renders to a view texture drawn with
   `draw_texture_rects` under the embed's affine. This lifts the viewport limitations
   (rotation; 2D content over and under the 3D view in painter order).
3. 2D in 3D: a plane mesh sampling the composition's view texture with mipmaps; picking
   and drags through the plane's inverse mapping into the composition.
4. The timeline panel as a 2D view.
5. Anything else (the FPS overlay, debug UI) only if measured not slower.

## 7. Risks

- **Per-camera CPU cost.** Each Bevy camera runs the full view pipeline (extraction,
  preparation, its render graph). Many small views may cost more CPU than they save GPU.
  Retention (skip unchanged views) is the mitigation; the spike measures it.
- **WebGL2.** Render-to-texture formats, the maximum texture size, mipmap generation (no
  compute: a blit chain), and the number of passes per frame.
- **Colour.** View textures must agree on encoding and premultiplication; the composite
  material's lessons (premultiplied, UNORM sampling, un-premultiply before converting)
  apply to every use, not only the compositor.
- **`vello_hybrid` renderer per target.** If a `Renderer` is bound to one target size,
  many views need many renderers, each with its own GPU resources.
