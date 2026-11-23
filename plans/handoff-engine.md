# Handoff: Building the Unified Engine

**Date:** 2026-11-22 · **Branch:** `master` · **Keys:** `EN-n` (`plans/README.md`, "Work
keys") · **Status:** Active. Consolidates the former `RE`, `DS`, and `PT` engine tracks
into one ordered plan to turn `iron_document` into one cohesive engine first (Stages 1–2),
then flatten all remaining O(document) scaling costs (Stage 3) and add chunked loading
(Stage 4).

## 0. Read first

1. This file.
2. [`specs/engine.md`](specs/engine.md) — the single target architecture (data/patches/views,
   recorded-read graph & `settle(mode)`, demand/time/host, settled decisions E1–E10).
3. [`specs/patches.md`](specs/patches.md) — the formal patch algebra (needed starting at `EN-3`;
   model in `src/iron_patch`).
4. [`.claude/skills/run-iron/SKILL.md`](../.claude/skills/run-iron/SKILL.md) — build,
   test, smoke, and durations.

## 1. Baseline (already built and kept)

Before this consolidation, the following foundation landed on `master` and stays:

- **In-place journaled transactions & read boundary** (`Document` rollback journal,
  `Tree`, `Changes`, `Store::take_changes()`).
- **Reactive cell graph & native cells** (`graph.rs`: height-ordered evaluation, equality
  cutoff, native cells for layout, network islands, constraint grouping/watchers, repeats).
- **No refusal after derivation** (`18e3e61`: `Saved` deleted, failed constraint solves
  apply and report derived problems, `exact` edits dry-run on `scratch()`, +3 constant
  edit 9.63 → 5.08 ms).
- **Order-free producer priority** (`95ff421`, `b0e2c68`: fixed `ProducerId` rank with
  layout over clips, automatic waiting-request promotion, structural-field formula errors).
- **Recording `Tree` & structural keys pilot** (`structure.rs`: `Node`, `Children`,
  `FacetSet`, `Facet` keys piloted on unpinned layout natives, +8% on 1,000-item stack).
- **Per-frame & per-pipeline scaling** (`work` choke-point + allocator counters, scaling
  test in `document_bridge::scaling`, idle/drag/playback/scrubbing at zero slope).
- **Composition clocks & visibility gate v1** (`clocks.rs`, `run = always` override).
- **Pure patch algebra model** (`src/iron_patch`: 19 properties × 10,000 scripts + 16
  worked examples, green).

## 2. Master Work Order (`EN-0` to `EN-11`)

Every phase lands committed, pushed, and recorded in §2 here and `roadmap.md`'s Status
table. Major design/code milestones go through the council (AGENTS.md).

| Key | Stage | Builds | Deletes | Gate / Exit | State |
|---|---|---|---|---|---|
| **EN-0** | 0 · Stabilize | Round-3 review check on `DS-7`/`DS-10a-1` fixes (`b0e2c68`..`18e3e61`); fix any remaining defects | — | Clean review; plain + differential + smoke pass | **Done 2026-11-22** |
| **EN-1** | 1 · Spine | **One write channel, locked-down `Cx`, one `settle(mode)` driver:** leaf/relation/index/wiring keys; graph drains touched log; `Cx::graph()` deleted; unkeyed `Tree` reads asserted; scoped per-root indexes `In`/`With`; one `settle(mode)` replacing 6 entry points | Op-built `changed` lists, manual `drain_touched` sites, 6 partial pipeline sequences, `apply_session` structural classifier | All suites pass; choke-point canaries fail when silenced; no unkeyed `Cx` reads | **Done 2026-11-22** |
| **EN-2a** | 1 · Spine | **Layout pilot:** composition maker over `With(root, Layout)` for layout roots, per-composition `Pins`, pinned parts, glide targets; batched claim handoff | `sync_layout`, `layout_touched`, `layout_roots`, `pinned_by_use`, `prime_layout`, `prime_reorders`, `regraph` `shaped`, `glided` sweep | Per-family layout from-scratch check & canary pass; `cells_demo` within 1.3×; **user checkpoint** before EN-2b | **Done 2026-11-22** |
| **EN-2b** | 1 · Spine | **Constraints & Contexts:** structural partition maker, per-candidate-component effective-cluster cells, derived cluster dep edges, per-composition attachment check, contexts library cell, `constraint.enabled`/`kind` evaluated with mates | `prime_grouping`, `grouping_reach`, `regroup`, `in_reach`, `moved`, `Clusters` special rule, `sync_cluster_natives`, Kahn's global sort, `resolve_contexts` scan | Retargeted binding, claim-move free-leaf, and ineligible-mate tests pass; per-family check & canary pass | **Done 2026-11-22** |
| **EN-2c** | 1 · Spine | **Network, Connections & Anchors:** per-island makers/cells, connection signals & anchor followers as cells, scoped network `deref` | `network_shaped`, `rewires_network`, `sync_islands`, `restructure` `rederive`, `chains`, `rest` | Network & anchor tests + per-family check & canary pass | **Next** |
| **EN-3** | 2 · Patches & Views | **Design & spike — Patches + Copies as Views:** apply round-3 fixes to `specs/patches.md`; design & prototype virtual views (`Tree` over shared templates + instance-axis patches), solver patch write-backs, and repeat item representation; council review | — | Working prototype of stable ids, view `Tree` recording, copy-targeted edits & undo; **user approves** before EN-4 | Open |
| **EN-4** | 2 · Patches & Views | **Patches & point evaluation in `iron_document`:** axes, patches, regions, ops, schema bump, in-memory patch index, session layer (scratch + gesture previews), evaluation at a point | Ad-hoc `overlay` precedence (once moved onto session layer in EN-5) | Differential parity with `src/iron_patch`; axis-growth properties; patch edit at 700 meters ≤ 2× constant edit | Open |
| **EN-5** | 2 · Patches & Views | **Instances as virtual views:** overrides become instance-axis patches, variants become layers, `use.notation` resolved with structure, solver writes to instance patches | `Docs::copies`, `override` nodes, `stale_overrides`, `Owners::affected`/`made`/`solved`, `keep_solved`, `release`, `apply_recorded`, bulk of `expand.rs` | All instance, override, variant & constraint tests pass; `Docs::copies` gone | Open |
| **EN-6a** | 2 · Patches & Views | **Machines on axes:** unordered state axes + pose patches; `std:plot` `vis` migrated; state editor edits pose patches | `machines.md` pose storage & custom pose resolution in `machine.rs` | Machine & plot tests + `plot-view.txt` pass | Open |
| **EN-6b** | 2 · Patches & Views | **Clips & ordered axes:** clips as patches on time axes; ordered axes for slides/takes/narrative extents; `now()` & `clock()` | `touches_clips`, structural `resample` rules, `clips_end` scans, narrative extent kinds | Clip/timeline/machine tests pass; deck & take examples pass | Open |
| **EN-7** | 2 · Patches & Views | **Remaining wiring & repeats on the unified `Tree`:** per-node binding makers, `@name` scope & boundary checks, theme token cells, blocked retries via `Wiring` keys, selection/state publish, repeat makers & recorded inputs, per-root expansion budget | `Rescope`, `regraph` (completely deleted), `after_transaction` classifier, `tokens_shaped`, `retokens`, `token_leaves`, `theme_at`, `REPEAT_INPUTS`, `update_drawn`, `ensure_drawn`, `scope_stamp` | `regraph` deleted; full suite + per-family checks + canaries pass | Open |
| **EN-8** | 3 · Scaling | **Need counts, scoped pulls & incremental heights:** per-edge need counts, output reservation, needed-only stale propagation, `pull()` / `current()` replacing `exact()`, cone `raise` for heights | `Store::necessity`, `Reactive::necessity`, `relaxed`, `pinned`, `relax`, stale set, `exact()`, `compute_heights` (non-test) | Need-count reachability oracle passes with `observe_all(false)`; constant/structural edit necessity counters flat | Open |
| **EN-9** | 3 · Scaling | **Visibility gate & retiring `ROOTED`:** cached node owners, cross-owner read counts, delta clock gating; retire `ROOTED`; captures observe what they draw | `Store::paused` scans, `Clocks::gated`, `Reactive::reader_nodes`, `Docs::reshaped`, `Reactive::wired`, `op::reshapes`, `ROOTED` | All gate & capture tests pass; §8.8 consistency with paused pages passes; playing & gate counters flat | Open |
| **EN-10** | 3 · Scaling | **Host-side scaling:** per-composition embed/control/3D indexes, touched spatial scopes only, per-composition paint order, incremental timeline lane planning, `gas::find` & `interaction` indexes | Host whole-document scans | Every row of `document_bridge::scaling` flat across both axes (store + host); `document_scaling_time` +6 ≤ 1.2× +0 | Open |
| **EN-11** | 4 · Storage | **Chunked snapshots & point loading:** revise `specs/patch-storage.md` (cut premature Postgres index tables), implement content-addressed chunks, Merkle manifests, point loading, arrow prefetching, `Pending` reads | Whole-scope snapshot requirement for large scopes | `patch-storage.md` exits met | Open |

*Product UX follow-ups decoupled from the engine track (can run after `EN-6` in `handoff-product.md`):*
search & provenance UI (former `PT-8`) and clock operators / ghosts / fixed-step accumulators (former `PT-10`).

---

## 3. Checkpoints

| When | Decision / Gate |
|---|---|
| End of `EN-0` | Confirm `DS-7` + `DS-10a-1` fixes are clean before refactoring `lib.rs` in `EN-1`. |
| Before `EN-1` | Short implementation plan for `EN-1` (write channel, `Cx`, `settle(mode)`) reviewed. |
| End of `EN-2a` | Verify layout pilot cleanly deletes all layout rules and stays within performance budget. |
| End of `EN-3` | **User approval** of `specs/patches.md` (round-3 polished) + Copies-as-Views design before starting Stage 2 builds. |
| End of `EN-7` | **Cohesive Engine milestone:** `regraph`, `Docs::copies`, legacy overrides/machines/clips, and all hand refresh rules are gone. |
| End of `EN-10` | **Zero-Slope Scaling milestone:** all store and host scaling probes flat on both axes. |

---

## 4. Immediate Next Steps (`EN-0` and `EN-1`)

### EN-0: Close out recent `DS-7` / `DS-10a-1` review
- Run a focused review check over commits `242798b..18e3e61` (constraint non-refusal,
  producer priority ranks, waiting requests, settled reads, NaN residuals) to confirm no
  defects remain before `EN-1` restructures the write channel and driver.

### EN-1: Detailed breakdown (One write channel, locked-down `Cx`, one `settle(mode)`)
1. **Expand `structure::Key` and `Document` scoped indexes:**
   - Add `Leaf(Leaf)`, `In(NodeId, TypeId)`, `With(NodeId, FacetKind)`, `Relations(NodeId)`,
     `Relation(RelationId)`, `Roots`, and `Wiring(Leaf)` to `structure::Key`.
   - Maintain per-top-level-root type and facet indexes in `Document` (updating old & new
     roots on moves; journaled for rollback).
   - Report `Leaf` keys on value writes and on node create/delete/restore; report relation,
     root, and index keys at `Document`'s mutation choke points.
   - Report `Wiring(leaf)` keys in `Reactive` wherever a binding, claim, or tracked read
     changes.
2. **Lock down `Cx` and recording `Tree`:**
   - Record keys for every `Tree` read used by cells (`relations_from`/`to`, `relation`,
     `roots`, scoped `nodes_of_type_in`/`nodes_with_in`, absent lookups, `version`).
   - Key `Cx::is_bound`, `Cx::driven`, `Cx::transitive_deps`.
   - Delete `Cx::graph()`; put `Cx::previous()` behind `Native::history`; assert on any
     unkeyed `Tree` read (including `scope_stamp`).
3. **One write channel into the graph:**
   - Have `Reactive` consume `Docs`'s touched-key log directly at the start of evaluation.
   - Delete the op-built `changed` lists in `lib.rs` and the hand-placed `drain_touched`
     call sites.
4. **One driver: `StoreCore::settle(mode)`:**
   - Unify `after_transaction`, `apply_session`, `post_apply`, `pipeline_from`/`derive`,
     `relayout`, and `advance`'s settle steps into `settle(SettleMode)` following
     `specs/engine.md` §3.3.
   - Introduce the typed Maker reader boundary (structure & authored constants only — no
     evaluated leaf reads) and batched native reconciliation (`set_natives` with claim
     handoff), ready for `EN-2a`.
5. **Verify:**
   - Plain + `differential` + `IRON_OWNER_CHECK` + `iron_spatial` + `iron_svg` + `bun test`
     + run-iron smoke.
   - Choke-point canary test verifying silencing each key emitter breaks at least one test.

---

## 5. Rules

- **One track, one set of keys (`EN-n`).** Do not revive retired `RE`, `DS`, or `PT` keys
  for new work.
- **Delete rules as you convert.** A phase is not done until its "Deletes" column is
  actually gone.
- **No new op classifiers or stamps.** Until a family is converted to cells, it refreshes
  coarsely by touched keys in its scope.
- **Commit and push** each completed phase, updating §2 here and `roadmap.md`'s Status
  table (`AGENTS.md`).
