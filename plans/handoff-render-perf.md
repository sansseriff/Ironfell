# Handoff: 2D render performance (roadmap stage H)

**Date:** 2026-10-13 · **Branch:** `document-spine` (benchmark/SIMD base `e08faac`) · **Read first:**
`plans/render-benchmark.md` (the measurements), then this file, then
`plans/infinite-canvas-architecture-patterns.md` §6–§8 (the target architecture for the
same problems). Background on the rest of the system: `plans/guide2/index.html`.

This is for an agent starting fresh. It says what was measured, what is wrong, what to do
in what order, and the traps the measuring agent fell into.

## 1. Rules of the repo (from AGENTS.md and the user)

- Every document mutation is an op in a transaction (`iron_document`); nothing else writes.
- Run the smoke script before committing; update `.claude/skills/run-iron/SKILL.md`'s
  expected output and the owning plan's status when behaviour changes.
- Don't push. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Worktrees, if used, go in `.agents/worktrees/<name>` (see AGENTS.md; symlink `target`).
- Native store tests: `cargo test -p iron_document --target aarch64-apple-darwin`.
- `plans/infinite-canvas-architecture-patterns.md` was written by another agent and is
  **untracked**; the user has not asked for it to be committed or implemented wholesale.

## 2. What exists now

- `?stress=N` / `await __iron.stress(N)`: a `stress` composition of N shapes in cells
  of 100 inside one `field` group, with a `spin` slider bound to the field's rotation and
  a 20×20 `sample` rect at panel (50,90)–(70,110) (`src/document_bridge/stress.rs`,
  FFI `document_stress` in `src/web_ffi.rs`, tab added in `src-ui/views/workspace.ts`
  when the URL has `stress`).
- Phase timers, `src/perf.rs`, read with `await __iron.timings(reset)` (FFI
  `document_timings`): `sync`, `gesture_reconcile`, `display_list`, `extract`, `encode`,
  `render`, `pick`, `overlay_geometry`, `bounds`, `frame`, `changed_frame`. Spans are placed in
  `document_bridge/mod.rs` (sync, reconcile_gesture), `render.rs`
  (render_document_layer), `vector/hybrid.rs` (extract_layers, prepare_scene,
  render_scene), `bevy_app/picking.rs` (pick_document_2d_system), `src/lib.rs`
  (`WorkerApp::frame`). A phase records only frames on which it did work (`discard()`).
- `.claude/skills/run-iron/bench-render.sh [counts…]` + `bench-render-table.py`, and
  driver commands `pmove`, `psweep`, `pwheel`, `measure-reset`, `measure` in `drive.ts`.
- Cargo profile `wasm-bench` (opt-level 3); `PROFILE=wasm-bench build.sh`.

Run it:

```sh
PROFILE=wasm-bench .claude/skills/run-iron/build.sh
.claude/skills/run-iron/serve.sh start
.claude/skills/run-iron/bench-render.sh 1000 10000 | python3 .claude/skills/run-iron/bench-render-table.py
.claude/skills/run-iron/build.sh          # back to dev-opt before the smoke script
```

One count takes ~2.5 min; four counts ~10–12 min, so run it in the background.

## 3. Findings (numbers in `plans/render-benchmark.md`)

| # | Problem | Evidence | Where |
|---|---|---|---|
| 1 | A selected group costs ~12 µs per descendant **every frame**, idle (119 ms at 10k shapes, 11 ms at 1k). A hovered group ~1.3 µs per descendant (13 ms at 10k). | `select-group`, `hover-group` rows | `draw_select_overlay` in `src/bevy_app/select.rs`: `Geo::frame` (box over descendants), `Geo::selection_frame`, `Geo::parts` (recurses every leaf for resize), `Geo::resizable` → `free` → `Store::handle` per leaf just to colour the grips |
| 2 | Hovering or cross-highlighting a group adds one outline command per descendant **to the content layer**, so the whole view re-encodes (+N commands). | `commands` rose 13,004 → 23,004 when a group was hovered or selected | `document_bridge/highlight.rs` (`lit`) feeding `render.rs`'s walk, which strokes highlighted nodes in the document layer |
| 3 | Picking walks every node every frame while the pointer is over a 2D view, even idle (1.6 ms at 10k, 3.3 ms at 20k). | `pick` in `idle` rows | `pick_document_2d_system`, `bevy_app/picking.rs` |
| 4 | Encode is 50–65% of a changed frame and is paid for every path, visible or not (10k: 13 ms at zoom 1 with ~1/6 visible, 26 ms zoomed out). Nothing culls before Vello. | `encode` column | `render_document_layer` walks all nodes; `prepare_scene`/`emit_layer` in `vector/hybrid.rs` (culls whole *layers* only) |
| 5 | Any change re-encodes the whole view: one moved rect costs as much as panning. | `drag-one` ≈ `pan` | `DisplayList::rebuild` per panel in `render.rs`; the comment at its top says so |
| 6 | ~10 ms of a 10k `spin` frame is outside every timed phase, probably Bevy transform propagation over one entity per shape (not timed directly). | `changed_frame` minus phases | one entity per 2D node (`materialize.rs`) |
| 7 | At device scale 2, document px appear to map to device px, not CSS px: panel coordinates in the script miss, shapes look half size. Unverified; check before trusting any 5K number. | 5K rows in `render-benchmark.md` §5 | `PanelView::affine`, panel rect plumbing (`set_panel_viewport`) |

## 4. What to do, in order

**Current update 2026-10-13:** SIMD and the selection/hover cache are implemented,
then the user authorized change-driven picking, independent bounds/indexing, and
indexed picking/per-view culling (items 2–4 of this handoff). Those increments are
implemented in the working tree. All resting 2D `pick.n`, `bounds.n`, and
`overlay_geometry.n` are zero. At 10k, idle is 0.9 ms; zoom-1 changed frames:
pan 18.7→5.3 ms, drag 19.0→6.0 ms, rotation 27.0→17.9 ms. Zoomed-out rotation is
flat at ~32.7 ms. Full tables/raw runs are in `render-benchmark.md` §7.

`src/iron_spatial` pins `understory_index` at
`2ac7efbc010c8ee4daa316c34eadc0bafacfe83b`; `SpatialScene` consumes propagated Bevy
globals and maintains canonical composition bounds/indexes even for hidden
sources. Iron ids are payloads; Understory handles stay private. Query candidates
retain exact shape/clip tests, occurrence chains, painter order and 3D occlusion.
Dense motion uses a packed rebuild; sparse edits update individual entries.
Do not add `understory_box_tree` simply to get bounds: this adapter deliberately
avoids a third transform hierarchy. `understory_presentation` is also not added.

**Remaining:** marquee/fit/selected-group geometry still use exact `Geo` queries;
shared subtree summaries and finer cache dependency revisions are follow-ups.
Retained overlay targets, painter-contiguous presentation chunks, partial raster
invalidation and selective subtree baking remain unfinished. Stage H is partial.

Implementation: `SelectionGeometryCache` shared by overlay drawing and idle grip
cursor testing; lightweight eligibility summary, detailed gesture parts on press;
`GeometryRevision` survives render drains and covers preview reconciliation and
same-version loads. Keys also cover authored version, reaches and playhead. Before-
propagation entries are finalized after propagation for embed bounds. Invalidation
is globally conservative until the bounds subsystem has subtree revisions.

DPR mapping is verified with named-target and slider-value assertions at DPR 1/2.
Benchmark panel offsets now use `PANEL_PIXELS=physical`; the default smoke driver
remains in CSS pixels. No application coordinate convention was changed. The
corrected 5K pointer addressing has not yet been used for a full 5K matrix.

Each step: change, re-run the benchmark at 1k and 10k, run the smoke script, update
`plans/render-benchmark.md` (add an "after" table), `plans/roadmap.md`'s H row, commit.

1. **Stop the overlay recomputing per frame (finding 1).** Cache the overlay's box,
   parts summary and grip colour per (selection, hover, document version, resolved-change
   generation, view mapping); rebuild only when one changes. Compute "resizable" once per
   selection change, not per frame. Prefer reading bounds from one derived bounds source
   (step 3) so this isn't a throwaway cache; if step 3 isn't ready, a version-keyed cache
   is an acceptable stopgap. Exit: `select-group` and `hover-group` `frame` within 0.5 ms
   of `idle` at 10k.
2. **Pick only when needed (finding 3; implemented for 2D).** Run picking when the pointer moved, the
   document or resolved values changed, or the view mapping changed; otherwise keep last
   frame's hits. Exit: `idle` `pick` n ≈ 0 with a still pointer.
3. **Bounds and spatial index outside the draw walk (findings 1, 3, 4).** This is
   `infinite-canvas-architecture-patterns.md` Phase 3. **Foundation now implemented:**
   `Bounds2d` is filled independently after propagation; the remaining goal is
   sharing subtree summaries with marquee/fit and selection. Preparation is driven by `DocumentChanges`
   and resolved changes, per composition, with geometry vs painted vs interaction
   bounds, and a spatial index (`understory_index`, git-pinned, or a small native
   grid/BVH; measure). Use it for picking candidates, marquee, fit-to-content and the
   overlay. Watch: embeds (one node, many occurrences via chains), clips, 2D-on-3D
   picking, painter order (hits still sort by z), and the background-hit rule.
4. **Cull per view (finding 4; implemented).** In `render_document_layer`'s walk, skip nodes whose
   painted bounds don't touch the view's visible rect (through each embed's transform
   and clip). Exit: `pan` and `drag-one` encode at 10k, zoom 1, drop by roughly the
   off-screen fraction; `*-zoomed` unchanged; smoke output unchanged.
5. **Move editor feedback off the content layer (finding 2).** Phase 4 of the other
   plan: hover/selection/cross-probe outlines draw in the overlay layer (already a
   separate `VectorLayer` in the same view, order `DOCUMENT_2D + 1`), not as extra
   strokes in the document walk. Exit: hovering or selecting leaves the document layer's
   display list unchanged. **Correction:** a separate layer in the same view
   does not make `encode` n = 0: `prepare_scene` resets the shared dirty target and
   emits every layer. A separate retained overlay target is needed to leave content
   encoding/rasterization untouched. Measure content-target and overlay-target work
   separately; the overlay itself still encodes when it changes.
6. **Only then, the subtree raster cache (findings 4–5, zoomed out).** Design notes:
   `plans/vello-hybrid-migration-scope.md` §3.4 and §6.3, `plans/view-textures.md`,
   vision 07, roadmap §5, Phase 5 of the other plan. Most plumbing exists: offscreen
   `VectorView`s render into their own textures (used by 2D-on-plane,
   `document_bridge/planes.rs`, with √2 density steps from on-screen scale), and a 2D
   view can draw another texture in painter order (`DrawCmd::Texture`, used for 3D
   embeds). A first version: promote a group to its own offscreen view when static for N
   frames, not selected or under a gesture, and small on screen or complex; draw its
   texture in its place; invalidate when `DocumentChanges` names a node under it; key by
   scale bucket; drop back to live drawing when edited. Exit: `spin-zoomed` and
   `drag-one` at 10k fall well under 16 ms. Preserve clip/blend semantics.
7. Separately, verify finding 7 and fix the script's coordinates (or the mapping) so 5K
   runs are valid.

Phases 1–2 of the other plan (gesture ownership, typed UI protocol) are correctness work
independent of this list; don't mix them into the performance commits.

## 5. Traps (each one cost the measuring agent a run)

- **The numbers in `render-benchmark.md` were taken without wasm SIMD.** The shipped
  build (`build-wasm.sh`) compiles with `-Ctarget-feature=+simd128`; without it
  `vello_common`'s `fearless_simd` silently uses a scalar fallback. `build.sh` now adds
  the flag for `PROFILE=wasm-bench` (a full rebuild). **Done for 1k/10k (render-benchmark §6).** The 5k/20k scalar rows remain historical; the `encode` column is expected to shrink, the overlay, pick
  and display-list numbers (no Vello involved) should not.

- **Measure `changed_frame`, not fps.** Unthrottled, the app renders ~1000 fps while the
  driver moves the pointer ~25 times a second, so fps mostly counts idle frames.
- **The probe's `tick` excludes rendering**; use `frame` / `changed_frame` from `perf`.
- **`dev-opt` triples encode** (opt-level 1); benchmark only `wasm-bench`. Rebuild
  dev-opt (`build.sh`) before the smoke script.
- **Hover and selection contaminate later scenarios** (finding 1 makes them expensive
  every frame). Hover updates only on pointer *move*: content scrolling under a still
  pointer, or a pointer left over content after a drag, keeps a stale group hover. The
  script parks the pointer at panel (600, 50) and moves it before each scenario.
- **Escape walks up one level** (shape → cell → field → nothing), and with slow frames a
  burst of Escapes can lose one: pace them (`sleep 300`). A click on "empty" canvas can
  land on a selected group's box grip (at 1k the field box reaches panel y≈52) and not
  deselect. The script prints `STILL SELECTED` if deselection failed; treat any such run
  as contaminated.
- **Pan must keep content on screen**: Vello's cost follows coverage, so scrolling the
  content away makes encode look cheap. The script pans 5 px a step, 200 px total.
- **Timer resolution is 0.1 ms** in the worker (not cross-origin isolated).
- **Smoke variance:** the pendulum's under-converged solve sometimes logs
  `"link" … moved 1 node(s): #W` and one more undo op, and the save size moves by a byte
  or two. Documented in SKILL.md; not a regression.

## 6. Open questions for the user

- Whether to adopt Understory crates (unpublished; git-pin or vendor) or write a small
  native index. Measure both against the full traversal first.
- Whether 2D shapes should stop being one Bevy entity each (finding 6). Large change;
  only if transform propagation proves to be the remaining cost after steps 1–6.
- Whether to commit `plans/infinite-canvas-architecture-patterns.md`.


## 7. Verification for the selection/hover cache

- `PROFILE=wasm-bench build.sh` with SIMD, final 1k/10k matrix: nine scenarios
  per count, target/deselection assertions, no scenario failures.
- `selection-cache.txt`: idle reuse, descendant edit, undo, same-version reload,
  pan/zoom reuse, and slider/sample identity at DPR 1/2. Driver assertions now fail
  the process; Rust panics are failures even when caught by the frame wrapper.
- Native `iron_document` suite: 90 tests passed.
- **Passed:** final dev-opt build, complete standard smoke on stable artifacts,
  no page errors or panics; inspected bound mid-drag, rotated selection, and cache
  view-mapping screenshots. Both final DPR runs passed all 11 cache assertions.
  Never rebuild/rebind wasm during a running driver: reloading can
  mix cached glue with replacement wasm and invalidate the run.
- `bun run check` remains blocked by existing CSS diagnostics in
  `src-ui/lib/Github.svelte`, an implicit parameter type in `src-ui/lib/utils.ts`,
  and generated wasm glue diagnostics (5,005 errors across five files in this
  checkout). No Svelte application source was changed for this fix.

Next action is the change-driven picker, followed by independent bounds and
spatial queries. The cache is not a substitute for those systems.


## 8. Spatial implementation handoff

- `src/document_bridge/spatial.rs`: prepare after propagated transforms, before
  document rendering and selection-overlay drawing. `GeometryRevision` and
  `PropagatedGeometryRevision` survive render drains; the pick key includes both
  plus the spatial generation, so a newly reconciled pose is picked again after
  it settles. Base 2D hits are restored before the per-frame 3D pass.
- `src/iron_spatial`: three bounds kinds, private Understory handles, conservative
  old/new painted damage even for same-bounds style changes; canonical painter
  order. Call `commit()` before querying a batch. A dense packed rebuild remaps
  private handles by payload; tests cover subsequent sparse edits/deletion.
- `render.rs`: inverse-map device-expanded view/occurrence clips; query painted
  candidates; preserve exact clip commands and recursive painter ordering. A
  dirty view still rebuilds all its visible commands. Damage unions are diagnostic
  across canonical spaces, not yet partial raster invalidation.
- `__iron.spatial(false/true)`: reference/indexed walks, with current exact hit
  chains in its diagnostic reply. Allow frames to settle after toggling. Bounds
  update in both modes; this switch therefore does not remove maintenance cost.
- Initial repeated lookup/sorting plus per-leaf incremental tree updates regressed
  10k low-zoom rotation to 42.1 ms. Cached sort keys, one sort per canonical scope,
  and packed dense updates reduce it to 32.7 ms. Both raw runs are retained in
  `plans/benchmarks/`; tune the ≥256 / >50% batch policy from further workloads.
- Do not conflate faster picking with a constant-time large-group transform.
  Bevy globals and ~4.3 ms of bounds/index maintenance still scale with descendants.
  Raster baking saves encoding but does not automatically eliminate these costs.

Verification commands and the reusable comparison fixtures are in
`.claude/skills/run-iron/SKILL.md` under “Spatial and picking regression checks”.
`spatial.txt` covers repeated/nested hidden sources and 2D-on-3D plane hits;
`check-spatial-images.py` compares complete canvas/editor pixels, excluding the
live FPS HUD. `picking.txt` checks stationary invalidation; `selection-cache.txt`
checks retained selection geometry and physical offsets at DPR 1/2.


Final validation: 95 native tests, both build profiles, full smoke, 10 picking
assertions, 37 spatial assertions + four pixel comparisons at both DPRs, and
11 selection-cache assertions at both DPRs all pass. Final served wasm is
`dev-opt`. Svelte checking retains errors in the same five pre-existing files
(5,009 now, with four extra generated-glue errors from the new FFI export);
no errors point at the changed document client. See benchmark §7 for caveats,
including root-enumeration cost not represented by the candidate counter.


## 9. Subsequent renderer sanity investigation

Read `renderer-sanity-checks.md` before explaining poor interactive performance
as a renderer/Bevy ceiling. The owner was viewing dev-opt; whole-selected-group
1k dragging measured ~19 FPS / 51 ms, versus ~60 FPS / 11–12 ms with optimized
SIMD. The owner confirmed the optimized reload felt smoother. `wasm-bench` is
now left served deliberately, overriding the previous final dev-opt state.

New display-paced tests assert the exact group/leaf target and wait out the
second-press double-click window. A continuous rAF pointer replay verifies that
near-60 callback cadence is not solely idle ticks between WebDriver events.
Canvas2D and ThorVG controls replay canonical frozen geometry but exclude the
editor/store/Bevy; they are not a full-editor renderer ranking. 36 app intervals,
24 renderer intervals and compressed hashed fixtures are retained in `plans/benchmarks`.

The standalone same-version Vello control is now complete; see
`renderer-sanity-checks.md` §7 and the 40 dynamic intervals. At 10k, all geometry
and independent transforms changing each frame reaches 60-Hz callback cadence
in Canvas2D, ThorVG GL and Vello Hybrid GL/WG, with no authored bitmap cache.
Vello GL spends ~13.8 ms p50 there, but ~36.5 ms with all-leaf selection outlines.
The latter slowdown exists without Bevy. Canvas2D is ~6.4 / 5.6 ms respectively.
These are synchronous submissions and callback cadence, not GPU-completion or
full-editor guarantees.

Priority: split selection local-bounds/eligibility invalidation from placement;
retained overlay targets and measured dense-outline policy. Then decide
chunks/bakes or a newer/Classic renderer experiment from remaining costs. The
probe is a separate diagnostic workspace crate; no Vello/Bevy version upgrade
or new application runtime dependency was made.
