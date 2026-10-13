# Handoff: 2D render performance (roadmap stage H)

**Date:** 2026-10-13 · **Branch:** `document-spine` (last commit `1285030`) · **Read first:**
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
  `render`, `pick`, `frame`, `changed_frame`. Spans are placed in
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

Each step: change, re-run the benchmark at 1k and 10k, run the smoke script, update
`plans/render-benchmark.md` (add an "after" table), `plans/roadmap.md`'s H row, commit.

1. **Stop the overlay recomputing per frame (finding 1).** Cache the overlay's box,
   parts summary and grip colour per (selection, hover, document version, resolved-change
   generation, view mapping); rebuild only when one changes. Compute "resizable" once per
   selection change, not per frame. Prefer reading bounds from one derived bounds source
   (step 3) so this isn't a throwaway cache; if step 3 isn't ready, a version-keyed cache
   is an acceptable stopgap. Exit: `select-group` and `hover-group` `frame` within 0.5 ms
   of `idle` at 10k.
2. **Pick only when needed (finding 3).** Run picking when the pointer moved, the
   document or resolved values changed, or the view mapping changed; otherwise keep last
   frame's hits. Exit: `idle` `pick` n ≈ 0 with a still pointer.
3. **Bounds and spatial index outside the draw walk (findings 1, 3, 4).** This is
   `infinite-canvas-architecture-patterns.md` Phase 3. `Bounds2d` is filled during
   drawing today (`render.rs`); move it to its own system driven by `DocumentChanges`
   and resolved changes, per composition, with geometry vs painted vs interaction
   bounds, and a spatial index (`understory_index`, git-pinned, or a small native
   grid/BVH; measure). Use it for picking candidates, marquee, fit-to-content and the
   overlay. Watch: embeds (one node, many occurrences via chains), clips, 2D-on-3D
   picking, painter order (hits still sort by z), and the background-hit rule.
4. **Cull per view (finding 4).** In `render_document_layer`'s walk, skip nodes whose
   painted bounds don't touch the view's visible rect (through each embed's transform
   and clip). Exit: `pan` and `drag-one` encode at 10k, zoom 1, drop by roughly the
   off-screen fraction; `*-zoomed` unchanged; smoke output unchanged.
5. **Move editor feedback off the content layer (finding 2).** Phase 4 of the other
   plan: hover/selection/cross-probe outlines draw in the overlay layer (already a
   separate `VectorLayer` in the same view, order `DOCUMENT_2D + 1`), not as extra
   strokes in the document walk. Exit: hovering or selecting leaves the document layer's
   display list unchanged (`commands` doesn't rise; `encode` n = 0 while hovering).
   Note the overlay shares the view's texture, so a change still re-renders that view;
   a separate overlay target is a later refinement.
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
