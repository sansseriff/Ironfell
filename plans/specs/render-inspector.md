# Render inspector: watching another view's culling and caching

**Status:** approved 2026-10-23. Phases A and B built the same day (§8). Its legend
also lists the selected component instance's parts, each static, keyed or live and why
(2026-10-24, [`instance-cache.md`](instance-cache.md) phase A).
**Relates to:** [`render-cache.md`](render-cache.md) (what it shows),
[`panels-and-session.md`](panels-and-session.md) ("Views and presenters": a view is a
source, a notation and a target), [`free-running.md`](free-running.md) §4 (the Stats
composition, the precedent for a built-in session view).

**Request (user, 2026-10-23):** a 2D view like the debug cameras game studios use to
show frustum culling and streaming: a box for the "main" camera's view, while the
inspector itself is zoomed out so culling can be seen happening. Chunks (vector
subgroups) are tinted different colours, and raster tiles are outlined.

## 1. What it is

A **view** whose source is another 2D panel, the *observed* view. Its notation is
`render-inspector`, and it has its own pan and zoom. It draws the observed view's
composition as the render cache sees it *for the observed view*, with that view's
decisions drawn over it. It never changes those decisions.

## 2. What it draws

Content first, then annotations over it. Annotations are in the inspector's own overlay
view, so they redraw without re-encoding content (architecture.md §5).

| Element | Drawn as | Changes when |
|---|---|---|
| **Content, chunk-tinted** | the composition with each chunk's brushes mixed 35% toward its hue. Hue is a golden-ratio step from the chunk id, so neighbours differ and a chunk keeps its colour | a chunk's revision; the partition; the inspector's camera |
| **Culled content** | the same, at 30% opacity, outside the observed view's visible rect | the observed view moves (an overlay-only mask, §3) |
| **Observed viewport** | a solid rectangle, the observed view's visible region in composition space, rotated with any embed transform; its prefetch margin dashed | every observed pan or zoom |
| **Live layers** | the hot chunk's painted bounds, magenta outline, "live" tag | hotness |
| **Raster tiles** (render-cache phase 2) | 1 px outlines in composition space; colour by state: **green** at the ideal bucket, **amber** shown stretched from another bucket, **dashed grey** queued, **red** drawn by live fallback; a tile fades out over 1 s when evicted | each tile's state |
| **Bucket labels** | the tile's scale (`×1.41`) in its corner once tiles are larger than 40 px on screen | inspector zoom |
| **Damage** | the last 10 frames' damage rects, fading | every change |

The **legend** is the HTML part of the view (a hybrid view, panels-and-session.md):
- chunks;
- layers, live and raster;
- tiles resident and queued;
- atlas memory used out of the budget;
- bake ms this frame and its budget;
- live fallbacks this second;
- the observed view's last changed frame and encode, in ms.

## 3. How it stays cheap

The inspector must not make the observed view slower. It runs on the same worker, so a
slow inspector frame is a slow frame for everything.

- **Its content is cached like any view's.** Its tinted content goes through the same
  chunks and tiles, under a **style key** (`tinted`) in the tile key, so tinted and
  untinted tiles never mix. Its tiles come from a separate budget, by default 64 MB, so
  inspecting does not change what the observed view can keep resident.
- **Observed motion is overlay-only.** Tiles are in composition space, so a pan or zoom
  of the observed view moves only the viewport box and recolours tile outlines. The
  "culled" dimming is a mask in the overlay: a translucent fill outside the viewport
  rect, not a re-render of content.
- **It bakes last.** Inspector bakes use only the bake budget left after the observed
  view's, so its own pixels may lag while the observed view is busy.
- **Untinted fallback.** Before render-cache phase 2 there are no tiles, so tinted content
  is encoded live. That happens only when the inspector's own camera moves or content
  changes, not when the observed view pans, because chunk lists are in composition
  space (render-cache.md §4.2).

## 4. Using it

- **Opening:** the **+** menu's *Views* section has "Render inspector", next to Stats. It
  opens docked beside the active 2D panel and observes it.
- **The observed view:** a dropdown in the view's settings menu (top right, as the
  composition settings menu is) lists the open 2D panels.
- **Toggles** in the same menu: chunk tint, tiles, culled dimming, damage, legend.
- **Freeze:** keeps the inspector's picture of the plan (layers, tiles, viewport) as it
  was, so a moment can be examined while the observed view carries on. This is the
  debug camera's "detach".
- **Its own camera:** pan and zoom like any 2D view, and Shift+1 fits the observed
  composition. It does not select or edit: a click shows the chunk under the cursor (id,
  cost, revision, layer, hot or cold) in the legend, and nothing else.
- **Session tier:** never saved, like Stats. Its settings are session ops.

## 5. Where it lives

- **The plan it reads** is the render cache's per-view layer plan and tile table
  (render-cache.md §3). It reads them one frame late, from the main world, and they
  are the cache's own records, not a copy built for the inspector. The plan has to be
  inspectable anyway, so the inspector costs no extra instrumentation.
- **Drawing:** the content is a document layer like any panel's, with a tint applied at
  emit (brushes mixed toward the hue; gradient stops likewise). The annotations are an
  `Overlay` view like the select tool's. The legend is a Svelte component registered as
  the notation's HTML part.

## 6. Phases

| Phase | Lands with | Shows |
|---|---|---|
| A | render-cache phase 1 | chunk tint, viewport box, culled dimming, live layers (all layers live then), damage, legend without tiles |
| B | render-cache phase 2 | tiles and their states, bucket labels, eviction fades, memory and bake legend |
| C | render-cache phase 3, if built | retained vector layers outlined, with their re-encode counts |

**Exit for each:**
- With the inspector open on the Prussia scene, the observed view's §2 scenarios
  (render-cache.md) cost within 1 ms of the same runs with it closed.
- A screenshot of the inspector during the zoom-gesture scenario shows stretched
  (amber) tiles turning green as rebakes land.
- The smoke opens it once and reads its legend.

## 7. Open questions for the user

- **Tint strength and dimming**, 35% and 30%: a guess to be tuned by eye.
- **Should it also observe 3D views' frusta** (which 3D embeds are culled or rendered)?
  Not planned. The 2D cache is the subject here, and the same viewport-box idea would
  carry over.

## 8. Done

**Phase A (2026-10-23).** Open it from the **+** menu's *Render inspector* section, which
has one "Watch <tab>" item per open composition view. Its view spec is
`{source: <panel>, notation: "render-inspector"}`, and it posts `inspect:<panel>` as its
canvas part's composition (`views.ts`, `canvasSource`).

- **Rust:** `PanelView::observes`; the inspector shows the watched panel's composition
  and loose nodes and is skipped by picking. Its chunk layers use tinted copies of the
  shared recordings (`Recorded::tinted`, cached per recording in `Chunks`). Its
  overlay view (`document_bridge/inspector.rs`) draws the watched viewport (blue),
  dims what is outside it, outlines every chunk in its colour, and fills chunks
  re-recorded in the last second, fading.
- **Legend** (`RenderInspector.svelte`, engine calls `render_inspector` and
  `render_inspector_options`): chunks and how many are in the watched view, nodes,
  the largest chunk's cost, chunks re-recorded in the last second, the last encode's
  commands, the watched zoom; toggles for colours, bounds and dimming.
- **Cost:** a Controls pan of the Prussia file is 177.3 ms with an inspector beside it
  and 177.2 ms with an ordinary view in the same split.
- **Smoke:** opens one watching Controls and reads its legend.
- **Not built yet:** freeze, the click-to-inspect chunk readout, the damage trail,
  and choosing the watched panel from a dropdown (each inspector watches the panel
  it was opened for). The driver opens views through `window.__dock`, the dock API.

**Phase B (2026-10-23).** With render-cache phase 2.

- **Squares:** the overlay outlines every raster square the watched view drew in its
  last build (`raster::picture`, by the panel's view entity): green from its own tile,
  amber stretched from another scale, red live because nothing covered it, magenta
  split around a hot chunk (the hot chunk live, its neighbours from their own tiles).
  Drawn over the chunk bounds, under the viewport box; toggled by *Raster squares*.
- **Legend:** whether the view rasters, its squares by kind with the same colours, the
  tile scale, atlas tiles and memory against the budget, tiles baked and their time,
  and live squares, over the last second (`raster::summary`).
- **Its own content** is tinted chunks under their own cache keys, so it rasters like
  any view and never shares tiles with the observed one. It shares the observed view's
  atlas budget and bake budget rather than a separate 64 MB, and does not bake last.
- **Cost:** with the inspector beside it, the observed Controls view's fitted Prussia
  pan is 1.4 ms a changed frame (1.1 beside an ordinary view) and a deep drag 2.7 ms
  (2.7), p50 at `wasm-bench`.
- **Not built:** bucket labels and eviction fades (display lists draw no text yet), the
  queued state (a square is drawn, or baked the same frame), and the prefetch margin.
