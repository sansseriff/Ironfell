# Render cache: chunks, raster tiles, and when to use which

**Status:** approved 2026-10-23. Phases 0 and 1 done the same day (§10); the inspector's phase A, then phase 2, next.
**Relates to:** [`rendering/architecture.md`](../rendering/architecture.md) §4–8 (the
three levels of retention, the contracts and acceptance tests this spec builds on),
[`handoff-render.md`](../handoff-render.md) §4 items 3–4, [`view-textures.md`](view-textures.md)
§3–4 (density buckets, the pooled `(texture, uv rect)` allocation),
[`render-inspector.md`](render-inspector.md) (the view that shows all of this).

**Decisions (user, 2026-10-23):**
- **Stretched zoom.** During a zoom gesture, cached pixels are drawn stretched and
  re-rendered sharp once the gesture settles, as Chrome does.
- **Automatic.** The renderer decides what to cache. There is no per-group switch; the
  render inspector shows what it decided.

## 1. What this is for

A large static drawing should pan and zoom at frame rate, and editing one shape inside
it should cost about what that shape costs, not what the drawing costs. A small
document should look and cost exactly what it does today.

## 2. Where the time goes now

The Prussia coat of arms (42,667 nodes, 28,083 paths, 603k anchors) in the Controls
tab, `wasm-bench`, GPU, 1600×1000 at DPR 1. Times are p50 worker CPU, ms per frame.
The scenario is `.claude/skills/run-iron/bench-large-svg.txt`.

| Scenario | Changed frame | `display_list` | `extract` | `encode` | Drawn nodes / commands | fps |
|---|---|---|---|---|---|---|
| Idle, fitted (Shift+1) | 67.6 (×4/s) | 33.1 | 0.0 | 0.1 | 29,200 / – | 86 |
| Pan, fitted | 254.3 | 26.1 | 21.2 | **170.7** | 29,200 / 42,839 | 5 |
| Cmd-drag one deep shape, fitted | 241.4 | 33.7 | 22.1 | **175.3** | 29,200 / 42,839 | 43* |
| Zoom gesture (Ctrl-wheel) | 195.2 | 12.6 | 8.7 | **96.9** | – / 42,832 | 10 |
| Idle, zoomed in (≈3×) | 141.6 | 9.7 | 0.0 | 0.1 | 7,514 / – | 52 |
| Pan, zoomed in | 86.3 | 9.3 | 6.9 | **42.0** | 7,514 / 11,720 | 9 |

\* fps counts frames in which nothing changed. Picking is 0.7 ms throughout, and the
spatial index keeps it there (no change needed). Four findings:

1. **Encoding is the cost of motion**: about 4 µs per command, every command, every frame
   the view moves. 171 ms is ten frames' budget. Vello's prepared work is in device
   pixels, so any pan or zoom re-encodes everything visible (architecture.md §3).
   Only reusing *pixels* removes this for pan and zoom.
2. **One changed shape re-encodes the whole view** (the drag row): the scene is one unit.
3. **Building and copying the display list costs 30–55 ms on its own**, because it is
   rebuilt and extracted whole, in view space, whenever anything changes.
4. **Idle is not idle.** The Stats view's text (a session node, `#JMAIjp5`) changes
   about four times a second, and every change rebuilds *every* panel's display list:
   33 ms for a panel the Stats text is not in. Zoomed in, the hover outline's
   `overlay_geometry` also recomputes (55 ms) on each of those ticks. It keys on the
   global `GeometryRevision`, which is the open half of handoff §4 item 3.

## 3. The model

```
composition ──partition──▶ chunks ──layerize (per view)──▶ layers ──▶ view texture
  (store)                (vector subgroups:   live layer:  encoded each render
                          own display list,   raster layer: tiles at a scale
                          own revision)                     bucket, drawn as quads
```

- **Chunk.** A painter-contiguous piece of a composition's content: an authored
  subtree, or a run of small siblings. It has its own display list in *composition
  space* and a revision, and it is the unit of change. Chunks are per composition and
  shared by every view of it. These are the "vector subgroups" the inspector tints.
- **Layer.** A painter-contiguous run of chunks drawn one way in one view:
  - a **live layer** is encoded into a Vello scene whenever the view re-renders (today's
    behaviour, for everything);
  - a **raster layer** is a sparse set of tiles holding its pixels at a scale bucket,
    drawn as textured quads.
- **Tile.** A 256×256 px square of a raster layer at one scale bucket, in composition
  space, stored in a shared atlas.

Painter order is the order of layers. A layer is a run of chunks, chunks are runs of
preorder, and tiles hold a whole layer's pixels for their square. So no arrangement of
caching can reorder anything (architecture.md §7, "A → moving B → C").

### 3.1 Why these units

- **Raster pays for pan and zoom; vector retention does not.** A retained Vello scene is
  valid only while device-space placement is unchanged (architecture.md §3). Panning
  and zooming change it for every chunk, so the vector half helps only edits, with the
  view still. The raster half is what the slow rows in §2 need.
- **Tiles, in composition space, per layer** (Chrome's design, researched 2026-10-23:
  `cc` keeps a sparse `PictureLayerTiling` per layer per raster scale, pans by
  compositing existing tiles, and stretches stale-scale tiles during a pinch). Tiles
  bound memory to what is on or near screen at any zoom. A whole-chunk surface at 8×
  zoom would be 8k×12k px.
- **Cold runs share one layer.** Baking each chunk separately would store mostly
  transparent pixels once per overlapping chunk. One layer per run of cold chunks is
  one set of tiles per screen area, as in Chrome's layers.

## 4. Chunks (phase 1)

### 4.1 Partition

Per composition, from the document tree, recomputed only when structure changes, with
hysteresis so ids stay stable:

1. Estimate each node's **cost** from its drawable: commands it emits, weighted by path
   segments (a packed path's anchor count is on its `Geometry`). Sum it up the tree.
2. Walk down from the composition. A subtree whose cost is at most `CHUNK_MAX` becomes
   one chunk. A larger group splits into its children.
3. Merge consecutive small siblings (each under `CHUNK_MIN`) into run chunks, up to
   `CHUNK_MAX`. Runs never cross a parent, so they stay painter-contiguous.
4. **Boundaries that are forced:**
   - an embed (2D or 3D) is a chunk of its own;
   - anything that changes every frame regardless of edits is its own chunk: a 3D embed's
     texture, a free-running source's output (free-running.md);
   - a chunk never contains part of an isolation scope (group opacity, a clip frame)
     unless it contains the whole group: a layer boundary must not cut a `push_layer`
     in two.

Starting values: `CHUNK_MAX` ≈ 2,000 commands (about 8 ms to encode), `CHUNK_MIN` ≈ 50.
The Prussia file becomes roughly 20–40 chunks. A demo-sized composition is one or two.

**Chunk id:** the first node's id, plus a run marker for run chunks. **Revision:** the
largest of its nodes' local and placement revisions (`geom::Revisions`), plus any
binding inputs read from outside it.

### 4.2 Per-chunk display lists in composition space

`render_document_layer` today walks each panel's visible nodes into one list, with the
view's transform baked into every command. That is why a pan rebuilds the list (26 ms)
and extracts it (21 ms) even though nothing in the document changed. Instead:

- Each chunk keeps its own `DisplayList` in composition space, rebuilt only when its
  revision changes. Embeds inside it draw under their occurrence transform, as today.
- A view's frame is its **layer plan**: the visible chunks, from the spatial index, as
  layers. Each layer carries the view transform, applied at emit time
  (`emit_layer`'s `base`).
- Extract copies only chunk lists that changed (shared `Arc`s), plus the layer plan.

### 4.3 Phase 0, before this

Two fixes that change no design, so that measurements mean what they say:

1. **A panel rebuilds only for changes it shows**: its composition, what it embeds, and
   loose nodes for the viewer. A Stats tick then rebuilds the Stats view alone. Expect
   idle-fitted `changed_frame` to go from 67.6 ms to under 1 ms.
2. **`overlay_geometry` keys on the lit and selected nodes' subtree revisions**, not the
   global `GeometryRevision` (handoff §4 item 3, the open half).

**Phase 1 exit:**
- Panning no longer rebuilds or extracts any chunk list: `display_list` and `extract`
  under 1 ms in the pan rows.
- A one-shape drag rebuilds one chunk's list.
- `encode` is unchanged; that is phase 2.
- Pixels identical to today: the smoke's screenshots, and a full-view pixel compare on
  the stress and Prussia scenes.

## 5. Raster layers (phase 2)

### 5.1 Layerization: which chunks are raster (the heuristic)

Decided per view, each frame, from measured costs, with hysteresis:

| Rule | Why |
|---|---|
| **Hot chunks are live.** A chunk is hot from the moment a gesture watches any of its nodes, or one is in PathEdit, until 500 ms after its last revision change. A chunk with a live source is always hot | Never bake what is being edited; entering editing takes it off the cache at once |
| **Only expensive views bake.** If the view's cold visible content would encode in under `LIVE_OK` (≈4 ms), everything stays live | Small documents keep exact vector pixels and use no texture memory: today's look and cost |
| **Cold runs bake once cold for `SETTLE` (≈300 ms)** | A chunk edited in bursts does not bake between strokes |
| **Over budget, the least recently visible tiles go first; a layer without tiles draws live** | Memory pressure never shows stale or missing content (architecture.md §7) |
| **A chunk whose content cannot be baked stays live**: backdrop-dependent blends (none exist yet), anything with a per-frame source | Correctness, not cost |

"Would encode in" uses the per-chunk encode times the backend already measures (§4),
falling back to the cost estimate for a chunk not yet encoded.

The result for the measured scenes:
- **Prussia, idle:** one or two raster layers.
- **Prussia, dragging one shape:** raster below, the dragged chunk live, raster above.
- **The Controls demo alone:** all live, as today.

### 5.2 Tiles, buckets, and the stretched zoom

- **Scale bucket:** √2 steps of the view's device scale (px per composition unit, DPR
  included), rounded *up*. A tile is then sampled at magnification ≤ 1, a bilinear
  downsample of at most √2, which stays sharp. Vello's image paint is bilinear with no
  mipmaps (`peniko::ImageQuality::Medium`), which is why buckets and rebakes are needed.
- **Tile:** 256×256 px of one layer at one bucket, with a 1 px gutter against bilinear
  seams. Keyed by `(composition, layer, layer revision, bucket, tx, ty)`, so two views at
  the same bucket share tiles.
- **During a zoom gesture** (scale changing in the last 150 ms), draw the existing tiles
  stretched, finding the nearest bucket for each tile area. Bake nothing new unless
  magnification passes 2 (zooming in) or ½ (zooming out). In that case, bake the next
  bucket for visible tiles, centre first, and swap each tile in as it lands. When
  zooming out, bake the lower bucket ahead of need, as `cc` does. Lower buckets are
  cheap: fewer pixels and the same commands culled harder.
- **After the gesture settles:** bake the ideal bucket for visible tiles, progressively.
  Keep the old bucket as a fallback until the view is covered, then evict tilings that
  aren't seen (`cc`: "Aggressively remove any tilings that are not seen").
- **Panning** composites existing tiles. Baking prefetches a one-tile margin, biased in
  the direction of motion.
- **Fallback for a tile that is missing**, in order:
  1. the same layer at another bucket, stretched;
  2. the layer drawn live, clipped to the missing area.
  The second is correct but costly, and it is counted (architecture.md §7: "Every
  full-view fallback is counted and timed").

### 5.3 Baking

- **A tile bake** is the layer's chunk lists, culled to the tile's square through the
  spatial index, encoded under `scale(bucket)` and a translate to the tile's origin,
  into the tile's atlas rect.
- **Budget:** at most `BAKE_MS` (≈6 ms) of encode per frame, measured. Visible tiles first,
  then the prefetch margin. A dense tile that alone exceeds the budget still bakes,
  and its time is reported.
- **Atlas:** 2048² RGBA8 pages (16 MB, 64 tiles each), from a pool, within
  `RASTER_BUDGET` (default 256 MB, lowered to fit `max_texture_dimension` and the
  device). One Vello scene per page per frame covers every tile baked onto that page,
  so baking costs one `render` and one submission per touched page. This works around
  the shared-upload hazard: the 0.3 source still submits between renders that share
  resources.
- **Format:** the atlas holds Vello's own output (premultiplied, sRGB-encoded UNORM).
  It is drawn with `ImageSource::external_texture(id, rect, premultiplied)` *without*
  the `Convert` pass, which exists for Bevy cameras' images.

### 5.4 Edits to a baked layer

- **Damage, as Chrome does:** a revision change in a cold chunk of a raster layer (a
  binding updates, an undo) invalidates the tiles under the change's old and new painted
  bounds. Those tiles draw live, clipped, until rebaked. Every other tile is kept.
- **Promotion:** a chunk that turns hot leaves its layer. The layer splits into
  *below* / *hot chunk live* / *above*. Until the new layers' tiles exist, the tiles
  under the hot chunk's starting and current bounds draw live, clipped. That is correct
  because they redraw everything in that square in order. All other tiles keep the old
  layer's pixels, which are still right outside that area. The new layers bake within
  the budget, then replace the old one.
- **Demotion:** a chunk that has been cold for `SETTLE` rejoins a neighbouring raster
  layer when the merged layer's bake would fit the budget within about a second.
  Otherwise it waits.

**Phase 2 exit**, on the §2 scenarios:
- Pan, fitted and zoomed in: `changed_frame` ≤ 16 ms once tiles are resident.
- Zoom gesture: ≤ 16 ms while stretched, with rebakes inside the budget.
- Idle: zero content encode.
- Drag one deep shape: ≤ 16 ms after promotion settles, measured with its settling time.
- No raster layer in the default demo document.
- Raster output within a declared tolerance of live output at the bucket scale
  (translucent edges included, architecture.md §7 "Colour and alpha").
- architecture.md §8's acceptance tests that apply: idle/hover, one leaf among 10k,
  overlap and opacity order, eviction, two views, every geometry changing per frame
  (the live floor, no claimed benefit).

## 6. Vector retention for live layers (phase 3, if measurement asks)

After phase 2 the live layers are the hot chunks, which change anyway. A retained Vello
scene per live layer saves encode only when a *different* live layer changes while the
view stands still: two things animating at once, or a free-running readout beside a
drag. Build it when the inspector shows such frames costing more than budget:

- One `Scene` per live layer, re-encoded only on its own revision or a view change.
- The layers render in order into the view texture: `Clear` for the first, `SrcOver` after.
  Each render gets its own submission until upstream offers several jobs per submission
  (architecture.md §5 decision).
- Cap the layer count per view (≈8). Merge the least recently changed neighbours past it.

**Exit:** two independently changing live chunks cost the sum of their own encodes, not
the view's.

## 7. Later, not in this spec

- **A hot chunk moved rigidly as its own surface**, by placement alone (architecture.md §6,
  "Move a baked group"), so a large group's drag costs no encode.
- **Embedded 2D compositions' own tilings**: v1 treats an embed as one chunk drawn live
  inside its layer.
- **Planes** (2D on 3D) keep their own offscreen views, with no tiling.
- **The custom final pass** and the shared atlas for view textures themselves
  (view-textures.md §4).

## 8. Phases

| Phase | Builds | Exit |
|---|---|---|
| 0 | Per-panel rebuild filtering; `overlay_geometry` on subtree revisions; the §2 baseline in `render-benchmark.md` | Idle-fitted changed frame under 1 ms; hover at zoom ≈ 0 `overlay_geometry` |
| 1 | Chunk partition, per-chunk lists in composition space, layer plan, per-chunk extract | §4.3 |
| 2 | Raster layers: layerization heuristic, tiles, atlas, stretched zoom, damage, promotion | §5.4 |
| 3 | Vector retention for live layers, if the inspector shows the need | §6 |

The render inspector ([`render-inspector.md`](render-inspector.md)) lands with phase 1,
showing chunks, and grows tiles in phase 2. Each phase: the benchmark at 1k/10k and the
large SVG, the smoke, an "after" table in `render-benchmark.md`, roadmap stage H's row.

## 9. Open questions for the user

None blocking. The starting values (`CHUNK_MAX`, `LIVE_OK`, `SETTLE`, `BAKE_MS`,
`RASTER_BUDGET`) are set from the §2 measurements and will be retuned with the
inspector's numbers in front of us.

## 10. Done

**Phase 0 (2026-10-23).** The large-SVG scenario at `wasm-bench`:

| | Before | After |
|---|---|---|
| Idle, fitted: changed frame (each Stats tick) / frame / fps | 67.6 / 10.7 / 86 | 2.4 / 0.3 / 880 |
| Idle, zoomed in: changed frame / `overlay_geometry` | 141.6 / 55 | 2.2 / 0 |
| Pan, fitted: changed frame / `encode` | 254.3 / 170.7 | 214.7 / 166.3 (phases 1–2) |

Four fixes, two more than §4.3 named, because idle frames turned out to walk the content
every frame:

1. **Panels rebuild only for what they show** (`render.rs`): each changed node's
   home composition (`nest::home`, which counts tombstones) against the compositions a
   panel draws through its embeds (`nest::shown_compositions`). The redraw line's
   drawn count is now that of the panels that rebuilt, so the smoke reads `2 of 36`
   (was `2 of 44`).
2. **Group frames from the spatial scene** (`Scene::group_content`, O(1)) instead of a
   walk of the subtree. Leaves count by tight geometry, as snapping does, so a group of
   curves frames slightly tighter than its paths' control boxes did. The
   selection/hover cache's stamp is now per view (`GeometryRevision::of_view`: the
   revisions of the compositions the view shows), so a Stats tick no longer
   recomputes a selected group's resize eligibility (`resize_capabilities`, a
   descendant walk).
3. **Embeds from the type index, not a walk:** `collect_embed_views` ran `preorder`
   over every 2D panel's whole tree each frame (about 4 ms at 42k nodes);
   `composition::embeds_in` walked descendants for every view's liveness each frame.
   Both now filter `nodes_of_type(embed)` and sort by the new `Document::order_path`.
4. **The Live set counts only to its budget:** `decide_modes` collected a hovered
   group's whole subtree each frame to compare it with `LIVE_BUDGET`; the new
   `Document::descendants_within` stops at the budget.

**Phase 1 (2026-10-23).** Chunks, in `document_bridge/chunks.rs`. Same scenario:

| | Phase 0 | Phase 1 |
|---|---|---|
| Pan, fitted: changed frame / `display_list` / `extract` / `encode` | 214.7 / 24.7 / 19.7 / 166.3 | 176.9 / 0.2 / 0.0 / 172.8 |
| Drag one shape, fitted: `display_list` / `extract` | 30.1 / 21.9 | 0.6 / 0.0 |
| Pan, zoomed in: changed frame / `encode` | 58.8 / 41.0 | 39.2 / 37.2 |

- **The cut** packs consecutive siblings greedily up to `CHUNK_MAX` = 2,000 (a path's own
  cost is 2 plus its outline elements over 8), and splits a larger subtree into its
  children. It keeps whole a group with an `appearance` facet or a clip frame, a node
  that draws and has children, and anything holding an embed. An embed chunk is
  recorded per panel (`Chunks::per_panel`). A home is cut again on a structure change
  or a change to a split group. `CHUNK_MIN` was not needed: greedy packing already
  merges small siblings.
- **Recordings are shared:** `Recorded` (commands, per-command bounds, their union)
  lives in an `Arc` held by the chunk, by each panel's layer and by the backend's
  mirror. `Shape::Path` holds a `SharedPath` (`Arc<BezPath>`, equal by pointer first),
  and the encoder passes paths to Vello without copying them.
- **Layers:** a panel has one `VectorLayer` per chunk (`ChunkLayer`), ordered by
  `VectorLayer::sub` and placed by `VectorLayer::affine`, its pan and zoom. The panel's
  own list is empty and its clip to the panel rect is gone: the composite shows only
  the panel's part of its texture.
- **Culling** per chunk (layer bounds) and per command inside a partly visible chunk
  (`emit_layer`), which is why zoomed-in panning got cheaper.
- **Pixels:** the fitted view is identical. Zoomed in, 754 of 699,360 panel pixels
  differ, 741 by one level, at scattered antialiased edges, from no longer
  compositing through the panel's clip layer. Planes keep the old walk.

