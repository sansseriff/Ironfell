# Instance cache: drawing components by what they depend on

**Status:** approved 2026-10-24 (with §6's recommendations); phases 0, A, B, C and D
built the same day (§7). Phase D was built differently from §3: references for SVG
import, not unexpanded component instances (§7, "Phase D").
**Implements:** [`component-model.md`](component-model.md) §11 (render analysis) and its
phase 6, [`render-cache.md`](render-cache.md) §7's first item (a group moved rigidly as
its own surface), and [`packed-paths.md`](packed-paths.md) §9's "`<use>` as linked
instances".
**Builds on:** the chunks and raster tiles of `render-cache.md` phases 1–2;
[`rendering/architecture.md`](../rendering/architecture.md) §6 ("Move a baked group").

This came out of a conversation on 2026-10-23/24. It started from the Prussia SVG's
1,178 `<use>`s and asked how components could cache their pictures by their input
ports' state, their animation state and the document's history (frames, undo). The
answer is that those are one question: **which values does each part of a component
depend on?** A part keyed on those values covers all three.

---

## 1. One mechanism for inputs, animation and history

Every part of a definition's template depends on a static set of things: some of the
definition's inputs, and possibly time. The dependencies are known from the
expressions (`Expr::deps`), not by watching (component-model §11):

| Class | Depends on | Drawn as | Shared by |
|---|---|---|---|
| **Definition-static** | nothing | one baked surface | every instance |
| **Instance-static** | inputs that are constant at this instance | one surface per distinct value of those inputs | instances whose relevant inputs are equal |
| **Dynamic** | an input that is live here (connected to something that changes, driven by time or a clip) | live, as now | nothing |

What this does for each of the three concerns:

- **Input ports.** An input that is wired but steady is still a constant value. Only
  the parts that depend on it go into the key. A thousand fibers of three lengths share
  three surfaces for each part that depends on length, and one for every other part.
- **Animation.** No frame cache. Only the parts that depend on time are dynamic, and
  they are drawn live. A voltmeter's needle rotates live over a dial face baked once.
  Storing frames would mean full-size pixel buffers per frame (a 2048² page is 16 MB),
  and it would only help animations that exactly repeat.
- **History.** No undo-specific retention. Surfaces are keyed by content: the
  definition's version and the input values. Undo returns to a key that was drawn
  before, so it is a cache hit while the entry survives the cache's normal LRU, and a
  rebake if not. The cache is never saved and never enters history.

**What sharing actually saves.** Vello builds strips in device space, so sharing a
part's *vector* recording between instances at different places saves recording and
memory, but not encode or strip generation. Those run once per placement. Sharing a
**baked surface**, drawn N times as a texture, does save them (component-model §11).
So the shared thing is a surface, not a recording.

## 2. The primitive: a baked group surface

Today's tiles are in view space: a composition's chunks, baked at the view's scale on
whole-pixel origins. Sharing across instances, and moving a group without redrawing
it, both need the same new thing: **a group baked in its own local space at a scale
bucket, drawn by its placement.**

- Keyed by content: for an instance's part, (definition, definition version, part
  run, the values it depends on, scale bucket). For an ordinary group, its subtree's
  revision.
- Drawn as a textured quad through the placement's transform. Translation and uniform
  scale within the existing ½–2× envelope draw the surface directly. Rotation or skew
  draws it resampled. Beyond the envelope, or for a rotated surface at rest, rebake
  or draw live (§6, question 2).
- **Painter order splits it into runs**: dial face (static), needle (dynamic), glass
  (static) are two surfaces with the live needle between them, the same split the raster
  planner already does for a hot chunk.
- It lives in the existing atlas pages and budget, alongside the view tiles.

This primitive is also what fixes the whole-drawing drag (236 ms, render-cache §10):
the dragged group is baked once and moved by placement.

## 3. Expanded or not

Building the primitive keeps every instance expanded, so the document doesn't
change. The document-sparse step, where an instance holds no derived nodes, comes
last and only if measured to be worth it (§4, phase D):

- An instance stays **unexpanded** while it is definition- or instance-static
  throughout, has no connected ports, isn't entered for editing, and has nothing
  bound to its insides. It is then one `use` node; its picture comes from the
  surfaces above. Hit-testing, bounds and snapping go through its transform into
  the definition.
- It is **promoted** (expanded as today) when any of that stops holding: you
  double-click into it, a port gets connected, or an input goes live. Expanded ids are
  computed from (instance, definition node), so promotion gives every node the id it
  would always have had. Selections, references and undo don't notice.
- A picture for non-default inputs is built in a **scratch store**: a private
  document holding one expansion with those inputs, solved and recorded, then
  dropped. At default inputs the definition's own template is the picture, with no
  scratch store.
- The text view the model reads is unchanged at every step: an instance is one line.

**The law:** an instance drawn from a cached surface or unexpanded looks exactly like its
expansion (the raster-pixels check, extended), and selecting, snapping and exporting it
behave the same.

## 4. Phases

| Phase | Builds | Exit |
|---|---|---|
| 0 | Benchmarks: N library voltmeters (static, one driven by a slider); the Prussia file; a drag of its near-whole group | Baselines in `render-benchmark.md` |
| A | The analysis (component-model §11): per definition, each template node's dependencies per channel (shape, placement, effects); per instance, which inputs are live. In `iron_document`, tested natively. The render inspector shows each part's class and its reason ("needle depends on `in`") | The classes are right for the library parts; nothing drawn differently |
| B | Baked group surfaces moved by placement (§2), for ordinary groups first: a dragged group large enough to bake moves without redrawing | The Prussia near-whole drag under 16 ms p50, pixel-equal at rest |
| C | Instances key their static runs on the analysis (§1), so equal instances share surfaces | Component-model phase 6's exit: two voltmeters share one dial surface; driving `in` from a slider rebakes nothing and redraws only the needle |
| D | *If phase 0 shows the cost of expanded nodes matters:* unexpanded instances (§3), and SVG import keeping repeated `<use>` targets as definitions with their uses as instances, and exporting them as `<use>` | Prussia imports as about 8.7k paths plus 1.2k instances; edits, selection and export unchanged |

A is pure document work and can run alongside B. C needs A and B. D needs C, and
comes with the reader changes (geometry index, hit-testing, bounds, snapping, export,
the layers panel when it exists).

## 5. Not in this spec

- **Frame caches** for looping or scrubbed animation (§1).
- **Retaining surfaces for undo** beyond the LRU (§1).
- **Memoizing a definition's outputs**, so identical instances with equal inputs share
  one solve. It's the same key applied to the graph instead of the renderer; measure first.
- **A direct template evaluator** that skips nodes even in the scratch store: a second
  implementation of expansion would drift from the first.
- **Embedded compositions and 3D planes** as keyed surfaces (render-cache §7). They fit
  §2's primitive once it exists.

## 6. Decisions (user, 2026-10-24)

The user approved the plan and asked for it to be carried through; the
recommendations below were taken as the answers.

1. **The order** A, B, C, with D conditional on phase 0. B pays off on its own
   (whole-group drags) before any component work lands.
2. **Rotated surfaces.** Rebaked at their angle once they settle, as zoom is. As built,
   one that is turning draws live meanwhile (§7, phase B).
3. **Phase D for all components**, not an SVG-only type. As built, references are a
   `use` with its own `instance` facet, the component model's standalone `use` (§5.1),
   which any definition can have; only SVG import makes them so far (§7, phase D).

## 7. Done (2026-10-24)

Measured at `wasm-bench`, `GPU=1 UNTHROTTLED=1 PANEL_PIXELS=physical SIZE=1600x1000@1`,
changed frames in ms (`bench-instances.txt`); the store natively, release.

**Phase 0.** `bench-instances.txt` (a `?meters=N` scene: N standard dial meters, the
first driven by a slider; the Prussia near-whole drag) and `instance_cost` (ignored,
`iron_document`).

| | p50 |
|---|---|
| 100 meters idle / scrub one / pan | 1.4 / 2.3 / 1.6 |
| Prussia near-whole group drag (g93169, 38,901 nodes) | 235.5 (encode 179, record 31, selection box 21) |

| Meters (60 ticks each) | Live nodes | Any structural edit | Each placement | A constant edit |
|---|---|---|---|---|
| 100 | 6,972 | 13.9 | 7.0 | 1.0 |
| 1,000 | 69,072 | 161.7 | 79.9 | 14.1 |

Expanded instances cost real time: every structural edit re-expands every instance.

**Phase A.** `iron_document::analysis`: each part of an instance's picture, per channel
(shape, placement, effects), is static, keyed or live, with its reasons, and `runs`
cuts the drawing parts by class. Dependencies are read from the reactive graph on
the instance's copies, not from the definition's stored slots: the graph also holds
what connections and anchors derive, so a fiber plugged into a laser reads its
partner and is live, which the definition alone would not show. Six native tests
(`tests/analysis.rs`). The render inspector's legend lists the selected instance's
parts and reasons ("live, moved needle: placement depends on `in`, bound to …").

**Phase B.** A chunk is recorded relative to where its roots' parent sits and drawn
through that position (`Chunk::frame`), so moving a group split into chunks changes
frames, not recordings. Tilings are keyed by the layer's linear part and its sub-pixel
phase (`raster::At`): at rest texel for pixel, while a group moves its tiles are drawn
resampled, and the exact tiling bakes once it settles. The near-whole drag:
**235.5 → 0.6 ms p50, 1.6 p95**; `raster-pixels.txt` gains the drag (`rp-group`) and
passes. Deviations:

- **The frame is the parent's position only.** A turned or scaled split group is
  recorded again, as before; a run whose rotation or skew is changing draws live until
  it settles rather than resampled (decision 2).
- **The selection box's resizability is cached per document version.** It walked every
  leaf of a selected group each frame: the remaining 21 ms of the drag.
- **The pixel check's tolerance.** Each frame is a raster group of its own, composited
  separately, so overlapping antialiased edges round once per group: 1–2 levels on
  about 2% of pixels (47 groups at the Prussia fit). The check now bounds pixels off by
  more than 2 (0.2%) and by more than 8 (0.01%).
- The 10k stress scenarios are unchanged (pan 0.7, drag-one 1.1, spin 8.0).

**Phase C.** A linked instance is a chunk of its own with one layer per run
(`chunks::instance_runs`). Static and keyed runs are recorded relative to the
instance, from the template's local transforms, and keyed by the analysis plus the
instance's turn and scale, so equal instances share one recording and the raster bakes
it once and draws its tiles at each placement. With 100 meters given 60 ticks each,
**2 raster groups and 8 tiles serve every visible meter**; scrubbing the driven one a
second time **bakes nothing**, encodes 0.1 ms, 0.4 ms p50; `rp-meters` passes (worst 1
level). Deviations:

- **Shared recordings are content-addressed** (`chunks::Shared`): among instances the
  analysis says may be equal, each distinct recording is a variant with a layer key
  of its own, so one key always means one content. A readout first recorded before its
  font loaded showed why: a stale shared entry otherwise keys every later instance alone.
- **Live runs are drawn uncached.** A wired needle cooling between scrubs otherwise
  joined a baked group and was regrouped on the next scrub.
- **A shared run counts all its uses** toward the raster's smallest worthwhile group,
  and each run's motion is tracked per occurrence of its first key.

**Phase D, built as references.** §3's unexpanded instances would not have reached the
measured cost: library parts keep anchors, which plugging and connection lines read as
nodes, so they would stay expanded. The repetition phase D was for is SVG `<use>`, whose
pieces have no ports. So:

- **A reference** (`expand::reference_template`) is a `use` with its own `instance`
  facet (`linked`) and no entity: drawn from its definition's template wherever that
  is, never expanded. Template content is materialized; the spatial scene indexes
  templates first, inner before outer, and a reference is an entry over its template's
  union; picking, `space()`, snapping and the selection box go through it as through an
  embed; drawing records the template in place (`Draw::content`); export writes the
  template's content in place.
- **SVG import shares repeated pieces by content**, after usvg resolves them: each
  subtree hashed bottom-up (names and its root's placement aside, path geometry by its
  anchors), one definition per piece drawn twice or more (at least 3 nodes or 32
  anchors), its occurrences references; definitions nest, and one referenced only once
  is put back. A `<use>` that inherits another colour is simply another piece.
  `iron_svg::import_with(text, false)` and `__iron.importSvg(t, {share: false})` import
  without sharing.
- **Prussia:** 42,667 → **14,407 nodes**, paths 28,083 → **5,924**, anchors 603k → 108k,
  534 definitions and 1,832 references; a document clone 9 → 2.2 ms; paste and apply
  30 ms. Drawn live it is **pixel-identical** to the unshared import, fitted and zoomed
  in (`instance-pixels.txt`, after a page reload each so the views match); its raster
  tiles pass `raster-pixels.txt`; `references_draw_what_copies_draw` (ignored,
  `iron_svg`) walks both imports side by side and finds every node's type, placement and
  bounds equal. The near-whole drag is 7.0 ms p50 (8.5 p95): references sit at their own
  sub-pixel phases, so there is more to bake.
- **A reference is a layer of its own only above 256 commands.** Lighter ones draw in
  ordinary chunks: at arbitrary phases their tiles are seldom shared, and 1,832 of them
  each taking a 256 px slot starved the atlas.
- **Entering a reference edits its definition**, every copy at once, as a main does.
  Per-reference overrides, detach, and promoting one to an expanded instance are not
  built; nor is writing `<defs>`/`<use>` on export.

**Open:** the measured cost of expanded instances (161.7 ms a structural edit at 1,000
meters) is untouched: expansion recomputes every instance's copies on every structural
transaction, and making it incremental is the fix. Phase C's sharing for component
instances depends on equal sub-pixel phases, as at whole-pixel zooms.
