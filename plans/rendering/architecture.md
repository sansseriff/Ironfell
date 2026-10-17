# 2D rendering architecture

**Status:** current; 2026-10-14. The one document describing how Iron's 2D rendering
works now and the decisions that constrain what comes next. The **order of work** lives
in [`handoff-render.md`](../handoff-render.md) §4; measurements live in
[`render-benchmark.md`](render-benchmark.md) and
[`renderer-sanity-checks.md`](renderer-sanity-checks.md).

It consolidates, and supersedes as a current-state record, vision 07 §2 and §10 and
three archived research documents: the [migration scope](../archive/vello-hybrid-migration-scope.md),
the [retention audit](../archive/vello-retention-and-render-plan.md) and the
[retained subtree renderer proposal](../archive/vello-retained-subtree-renderer-proposal.md).
Read those for the source citations and Zulip quotes behind each claim here.
The reasons for the architecture are in [vision 01](../vision/01_layered-architecture-overview.md)
and [vision 07](../vision/07_current-2d-backends-and-hybrid-architecture.md).

## 1. The pipeline as built

```
iron_document transaction / preview / playhead
  → DocumentChanges (changed NodeIds + structure flag), GeometryRevision (global counter)
  → Bevy entities, transform propagation
  → SpatialScene (document_bridge/spatial.rs + iron_spatial):
      geometry / painted / interaction bounds per node, one understory_index R-tree
      per canonical composition, old/new painted damage
  → render_document_layer (document_bridge/render.rs): per 2D panel and per plane view,
      walk indexed visible candidates in painter order into a DisplayList; unchanged
      lists compare equal and stop here
  → hybrid backend (vector/hybrid.rs): one retained vello_hybrid Scene per view target;
      a dirty target is reset and re-emits every visible command
  → Renderer::render into the view's own bucketed texture (one submit per target)
  → editor feedback (bevy_app/select.rs draw_select_overlay): one Overlay view per
      2D panel above its content view; hover/selection change encodes only this
  → the window composite pastes each view; 3D views and planes sample textures
```

Supporting facts:

- **Render on demand.** An idle frame renders nothing (`RedrawNeeded`); resting 10k
  frames cost ~0.9 ms of worker CPU.
- **View textures** ([`view-textures.md`](../specs/view-textures.md)): every panel, embed
  and plane renders into its own texture. A view whose content did not change keeps
  its pixels. `Offscreen` views render at a density derived from their on-screen use
  (√2 steps). `DrawCmd::Texture` draws another view's texture in painter order.
- **The draw vocabulary** (`vector/display_list.rs`): solid fills and strokes, clip
  scopes (emitted as `push_clip_layer`), textures, and shape kinds kept distinct for
  fast paths. No gradients, images, text, opacity groups, blends or filters yet.
- **Colour:** Vello writes premultiplied sRGB-encoded bytes into UNORM textures; every
  consumer un-premultiplies and converts (`composite_material.rs`, `view_composite.rs`).

## 2. What is retained, and at what granularity

| Level | Retained | Invalidated by |
|---|---|---|
| Document | Nodes, hierarchy, geometry, properties | Authored change |
| Spatial projection | Bounds and indexes per composition | Changed leaves (sparse), or a packed rebuild for dense motion |
| Selection/hover geometry | `SelectionGeometryCache` (boxes, grips, lit outlines) | Global `GeometryRevision` (conservative) |
| Editor feedback pixels | One `Overlay` view texture per 2D panel | Any feedback change; never content's |
| Display list | Per panel/plane view | Any visible change in that view |
| Vello `Scene` | Per view target, including generated strips | Any change in that target: **whole-view re-encode** |
| Pixels | Per view texture | Any re-render of that view |

The gap is the content rows: one changed shape re-encodes every visible shape in its
view. Editor feedback has its own target since 2026-10-14.

## 3. What Vello Hybrid can and cannot retain

`vello_hybrid` (renamed `vello_gpu` upstream) does its expensive work on the CPU, in
**device space**, at recording time:

```
fill_path(path) → transform to device px → flatten → 4×4 tiles → sparse strips + AA coverage
render(scene)   → schedule (every call) → upload alphas/paints/strips → GPU passes
```

A retained, unchanged `Scene` skips the first line, roughly 90% of the CPU cost (native
50k paths: ~17.5 ms rebuild vs ~1.5 ms re-render). Prepared strips stay valid only while
the device-space placement is unchanged:

| Change to a shape | Strips reusable? |
|---|---|
| None | Yes |
| Whole-pixel horizontal move | In principle |
| Vertical move by a multiple of 4 px | In principle |
| Any other vertical, sub-pixel move, rotation, scale or zoom | No; coverage changes |

No public API reuses strips across scenes; the experimental `Recording` API that did was
removed upstream (vello#1611) and its envelope was mainly integer translation.

**Version facts that shape the plan:**

| | `vello_hybrid` 0.2 (Iron, wgpu 29) | `vello_gpu` 0.3 (upstream `db90a73`, wgpu 30) |
|---|---|---|
| Target handling | `render` always clears its target | `TargetInit::SrcOver` composes onto existing pixels; `ClearSettings::Rects` clears only listed rects |
| Clips | `push_clip_path`, `push_clip_layer` | adds `push_clip_rect` |
| Textures in painter order | `draw_texture_rects` | external-texture image paints |
| Uploads | Strips buffer per pass, whole alpha texture | Persistent strip arena, used rows only |
| Several `render` calls before one submit | Unsafe: shared upload resources are overwritten (Iron's per-target `own` renderer works around it) | Same hazard |

Upstream churn is high (the wgpu backend had 29 commits in the three months to
`db90a73`), and upstream is itself landing upload and target-handling improvements.

## 4. Three levels of retention

1. **Shape-level vector retention.** Keep each shape's strips by id; regenerate only
   changed shapes. Needs Vello core changes, helps only edits that leave placement
   unchanged, and repeats what upstream removed. **Not pursued.**
2. **Chunk-level vector retention.** One `Scene` per painter-contiguous chunk; reset
   and re-record only dirty chunks; render all chunks in order into the view with
   `TargetInit::SrcOver`. Public API, **0.3 only**. Waits for Bevy on wgpu 30.
3. **Raster surfaces (baking).** Render a static subtree once into an `Offscreen`
   texture at a scale bucket and draw it with `DrawCmd::Texture`. Makes rigid moves,
   rotation and zoomed-out views cheap; costs memory and resampling quality.
   **The path available on 0.2.**

Levels 2 and 3 both need per-subtree revisions from Iron, which is why those come first.

## 5. Decisions

- **Do not fork Vello.** Everything levels 2–3 need is public API. The only core
  change discussed (several jobs in one submission) is optional: use separate
  submissions, or on 0.3 one shared atlas target cleared with `ClearSettings::Rects`.
  Propose per-job staging upstream if measurement ever requires it.
- **The render plan is Iron-owned and backend-neutral** (vision 01 §2.4). It extends
  the view-texture graph, takes `NodeId`-mapped partitions, revisions and resolved
  affines, and could drive another backend.
- **No new transform hierarchy.** Partitions receive resolved composition-space affines
  from the store's own transforms (since 2026-10-17 `iron_spatial::Scene` reads
  `Store::local_affine` and the store's revisions, not Bevy globals; vision 10 §3);
  parent edges express compositing containment only. `understory_box_tree` is not adopted (roadmap §5, CS I5).
- **Understory:** `understory_index` is adopted inside `iron_spatial` with private
  handles and `NodeId` payloads. `understory_presentation` would duplicate
  `DisplayList` and is not adopted. Its roadmap lists a future render-tree crate.
- **Four hierarchies stay separate** (roadmap §5): model tree, composition layout tree,
  presentation chunks, raster cache entries. Regrouping one never changes those above.
- **Subtree surfaces before world tiles.** Measured costs are rigid group motion and
  zoomed-out encoding, which surfaces address; pan is already bounded by culling.
- **Editor feedback is separate from content**, in its own retained target
  (built 2026-10-14). A lit group or embed shows its frame, a lit leaf its outline,
  as in Figma; feedback never strokes every descendant of a group. Plane views,
  which have no overlay, still outline lit nodes in their content.

## 6. Retention and invalidation contracts

Keep separate revisions for local content, placement, effects, source pixels, view
quality and device resources. A surface key includes source identity, content/effect
revisions, raster scale, bounds/origin, format/alpha convention and any baked
clip/backdrop dependency. Occurrences share a surface only when all of these agree.

| Change | Expected behaviour |
|---|---|
| Clean frame | No recording, no raster job; view textures unchanged |
| Move a baked group | Update placement while quality is acceptable; keep contents |
| Change one leaf | Rebuild affected partitions and dependent ancestors; keep independent siblings |
| Child texture changes | Re-rasterize dependants; keep unchanged geometry preparation where possible |
| Zoom or non-uniform scale | Reuse within the sampling-quality envelope; rebake or draw live beyond it |
| Backdrop-dependent blend | Invalidate the backdrop dependency or draw live |
| Delete, history, load, device loss | Invalidate generations and epochs; never reuse a stale handle |

Selection alone does not force a baked subtree back to live vectors. Editor outlines
have their own dependencies; their screen-space stroke width can invalidate an outline
cache under scale while the content surface stays valid.

## 7. Correctness constraints

- **Painter order.** A → moving B → C cannot become cached(A+C) then B. Keep partitions
  painter-contiguous, or redraw every contributor to a damaged region in order.
- **Isolation.** Group opacity, masks and filters need the group rendered together;
  some blends need the external backdrop. Start with source-over content and clips
  (all Iron draws today); classify compositing barriers in the draw vocabulary before
  effects arrive (vision 07 §7.3).
- **A clear is not a redraw.** Clearing a damage rect does not restore what was behind
  it. Bake complete subtrees first; add damage redraw only with a reconstruction plan.
- **Coordinates and bounds.** Bake local content with a recorded origin and conservative
  stroke/filter extent, never just the visible fragment. Keep world-space identity
  separate from a view's density, clip and occurrence transform.
- **Resource lifetime.** Protect surfaces sampled by in-flight work; retire on eviction
  with submission-aware ordering; account resident, pending and transient memory.
  Memory pressure falls back to live rendering, never to stale or missing content.
  Surfaces beyond texture limits tile or fall back.
- **Colour and alpha.** Premultiplied sRGB-encoded bytes at every surface boundary;
  test translucent edges against the live path and declare resampling tolerances.
- **Geometry queries** use editable geometry, never pixels; picking into a baked group
  does not need readback.
- **Old pixels go.** Move, delete, hide and reparent remove previous pixels; damage
  inflates for strokes, antialiasing and gutters.
- **Every full-view fallback is counted and timed**, so it cannot become the normal path.

## 8. Acceptance tests for caching

| Test | Evidence required |
|---|---|
| Idle and hover-only update | Content recording/raster counters stay zero when only the overlay changes |
| One leaf edit among 10k | Unchanged partitions keep preparation and surfaces; dependent ancestors update |
| Translation, rotation, scale | Approved resampling distinguished from exact live rendering; scale policy changes residency |
| A/B/C overlap, opacity, clips | Matches the full live reference; no painter-order change |
| Multiple jobs per submission | Batched and separate outputs agree; no overwritten uploads |
| Resource replacement/eviction | No stale ids, use after retirement, blank content or unbounded residency |
| Multiple views/occurrences | Density and occurrence keys correct; sources not confused with placement |
| All geometry changes every frame | Reports the live floor; no claimed cache benefit; planning overhead quantified |
| Resize/device recreation | Correct content on the next valid frame |

Keep exact pixel tests where the representation should agree; use explicit tolerances
where resampling is intended. A cache hit is not evidence of correctness.

## 9. Renderer alternatives

Object-space renderers (Vello Classic's compute pipeline, tessellating renderers such
as Rive's) make moving unchanged shapes cheap on the CPU. They are experiments for
`iron_renderer_probe`, listed in [`renderer-sanity-checks.md`](renderer-sanity-checks.md)
§10, not scheduled work. Any backend must render into a wgpu texture on Bevy's device.
