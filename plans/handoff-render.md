# Handoff: 2D render performance (roadmap stage H)

**Date:** 2026-10-13, work order revised 2026-10-14 · **Branch:** `document-spine` · **Read first:**
this file, then [`rendering/architecture.md`](rendering/architecture.md) (how rendering
works now, the decisions, and the correctness rules for caching), then
[`rendering/render-benchmark.md`](rendering/render-benchmark.md) §6–7 (the measurements).
Background on the rest of the system: [`guide2/index.html`](guide2/index.html). Product
work has its own handoff, [`handoff-product.md`](handoff-product.md).

This is for an agent starting fresh. It says what was measured, what is wrong, what to do
in what order (§4, the only place the order of rendering work is kept), and the traps
the measuring agent fell into. §7–9 are verification records of finished increments.

## 1. Rules of the repo (from AGENTS.md and the user)

- Every document mutation is an op in a transaction (`iron_document`); nothing else writes.
- Run the smoke script before committing; update `.claude/skills/run-iron/SKILL.md`'s
  expected output and the owning plan's status when behaviour changes.
- Don't push. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Ask the user whether substantial work should use a worktree; small-scale work
  may use the main checkout without asking. If chosen, worktrees go in
  `.agents/worktrees/<name>` (see `AGENTS.md`).
- Native store tests: `cargo test -p iron_document --target aarch64-apple-darwin`.
- `plans/specs/infinite-canvas-architecture-patterns.md` was written by another agent and is
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

## 3. Findings (numbers in `plans/rendering/render-benchmark.md`)

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

**Revised 2026-10-14.** Items 1–4 of the previous list (overlay geometry cache,
change-driven picking, independent bounds/index, per-view culling) are done and
committed (`87a9a8e`); the DPR question (old item 7) is verified. The list below
replaces the remaining items. It follows from the measurements in
`render-benchmark.md` §7 and `renderer-sanity-checks.md` §7, and from the review
of the retained-renderer proposal (decisions now in `rendering/architecture.md` §5).

Where the remaining time goes at 10k (worker CPU p50, optimized SIMD):

| Scenario | Changed frame | Encode | Bounds | Rest (mostly untimed) |
|---|---|---|---|---|
| pan, zoom 1 | 5.3 | 2.5 | 0 | ~3 |
| drag one | 6.0 | 2.9 | 0 | ~3 |
| rotate field, zoom 1 | 17.9 | 2.1 | 4.4 | ~11 |
| rotate field, zoomed out | 32.7 | 13.7 | 4.3 | ~14 |
| pan, zoomed out | 19.9 | 13.1 | 0 | ~7 |

Plus the standalone control: all-leaf selection outlines take Vello alone from
14.1 to 36.5 ms. Three separate problems: editor feedback, group motion touching
every descendant, and zoomed-out encoding of many visible paths.

**Constraint while on `vello_hybrid` 0.2.** Its `Renderer::render` always clears
its target (no `TargetInit`). Several retained scenes therefore cannot be drawn
into one view texture, so chunk-level *vector* retention (one `Scene` per chunk,
re-recording only dirty chunks) is not available. On 0.2 a chunk saves encoding
only when it is **baked** into a texture. `vello_gpu` 0.3 adds `TargetInit::SrcOver`
and `ClearSettings::Rects`, but needs wgpu 30, i.e. a Bevy release on wgpu 30.

1. ~~**Retained overlay target and a dense-outline policy.**~~ **Done
   2026-10-14** (`render-benchmark.md` §9). Feedback draws into one `Overlay`
   view per 2D panel; the owner chose Figma's policy (a lit group shows its
   frame, a lit leaf its outline); `paint_pad` lost its outline allowance.
   `hover-sweep` at 10k: content `encode.n` = 0, changed frame 2.1 ms; 1k
   selected-group drag at 5K 11.8 → 8.7 ms, its remaining 2.5 ms
   `overlay_geometry` belongs to item 3.
2. ~~**Time the untimed remainder.**~~ **Done 2026-10-17** with vision 10 phase 2
   (`render-benchmark.md` §11): Bevy's propagation is timed as `propagate`, and
   removing document nodes from it took the 10k rotation's changed frame from 67
   to 27 ms on the container. Finding 6 is thereby decided: 2D nodes carry no
   transform; phase 3 drops their entities outside the working set.
3. **Per-subtree revisions and the local/placement split.** Replace the global
   `GeometryRevision` dependency of the selection cache and spatial prep with
   per-subtree revisions. Let a group's contents be indexed in group-local space
   with one placement transform, generalizing how embeds already work (one
   canonical index per composition, an affine per occurrence). Rotating the field
   should then update one placement, not 10,002 bounds. Do not add
   `understory_box_tree` or another transform hierarchy for this (roadmap §5).
   *2026-10-17:* **the spatial half is done** (`render-benchmark.md` §10): the
   store classifies placement, local and structure changes (`geom::Revisions`)
   and `iron_spatial::Scene` indexes each group in its own space, so rotating
   the 10k field rewrites one entry (was 10,002) and the bounds phase drops below
   the timer's 0.1 ms. This is phase 1 of `vision/10_engine-boundary.md` §7.1.
   **Still open:** the selection cache's `overlay_geometry` keys on the global
   `GeometryRevision`; key it on the selection's subtree revisions instead.
   *Vision 10 phases 2 and 3 followed the same day* (`render-benchmark.md` §11–12):
   Bevy propagates nothing for document nodes, and 2D nodes have entities only
   while in the Live working set.
4. **Chunks with selective subtree baking** (old item 6, unchanged in substance).
   Start with authored groups as the chunk unit. Bake a static, complex or
   small-on-screen group into an `Offscreen` view at a √2 scale bucket, draw it
   with `DrawCmd::Texture` in painter order, return to live drawing when its
   subtree revision changes. Gate on the A → B → C painter-order scene
   and the acceptance tests in `rendering/architecture.md` §8. Targets: zoomed-out rotate and pan encode.
   Baking does not remove propagation/bounds cost; item 3 does.
5. **Small experiment, any time:** panel and embed clips use `push_clip_layer`,
   which routes a whole panel through an intermediate layer texture. 0.2 also has
   `push_clip_path`. Measure before switching.

Bevy 0.20 (wgpu 30) shipped 2026-10-08 and `vello_gpu` 0.3 on 2026-10-02, so the
upgrade is available now: `vello_gpu` 0.3 (renamed from `vello_hybrid`;
`draw_texture_rects` became external-texture image paints), `app-surface` 1.13,
and `bevy_vello` dropped (its `main` is still on Bevy 0.19; Classic is rebuilt
later on the shared view-texture path, probe crate first). Then try chunk-level
vector retention with `TargetInit::SrcOver`. Item 3's revisions
are what it needs. Renderer alternatives (Vello Classic on WebGPU, a tessellated
mesh baseline, Rive as a reference) are experiments for the
`iron_renderer_probe` harness, not priorities; see `renderer-sanity-checks.md` §10.

Each step: change, re-run the benchmark at 1k and 10k, run the smoke script, update
`plans/rendering/render-benchmark.md` (add an "after" table), `plans/roadmap.md`'s H row, commit.

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

- Settled 2026-10-14: a hovered, selected or cross-probed group outlines only its
  frame, following Figma; a leaf outlines its own shape (§4 item 1).
- Whether 2D shapes should stop being one Bevy entity each (finding 6). Large change;
  decide from §4 item 2's propagation timing, after item 3.
- Settled 2026-10-13/14: `understory_index` is adopted (git-pinned, inside
  `iron_spatial`); `box_tree` and `presentation` are not. The infinite-canvas plan
  is committed. Vello is not forked (`rendering/architecture.md` §5).


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
  `plans/rendering/benchmarks/`; tune the ≥256 / >50% batch policy from further workloads.
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
