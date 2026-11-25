# Handoff: Building the Unified Engine

**Date:** 2026-11-23 *(revised after reverting universal patch algebra `EN-3`–`EN-5c`)* · **Branch:** `master` · **Keys:** `EN-n` (`plans/README.md`, "Work keys") · **Status:** Active.

**What changed on 2026-11-23:** The universal N-axis patch algebra (`EN-3`–`EN-5c`, preserved on `archive/universal-patch-experiment`) added +11,800 net lines without deleting `Docs::copies` or shipping user-facing variation features, and was reverted in `cc9f380`. Read [`notes/postmortem-universal-patch-algebra.md`](notes/postmortem-universal-patch-algebra.md) first so we never repeat that mistake. Retired keys `EN-3`–`EN-6b` are never reused; new work continues under `EN-12`–`EN-16` alongside existing keys `EN-7`–`EN-11`.

## 0. Read first

1. This file.
2. [`notes/postmortem-universal-patch-algebra.md`](notes/postmortem-universal-patch-algebra.md) — why the universal patch algebra was reverted and the guardrails it establishes.
3. [`specs/engine.md`](specs/engine.md) — target architecture (4 orthogonal layers, recorded-read graph & `settle(mode)`, demand/time/host, settled decisions E1–E11).
4. [`specs/patches.md`](specs/patches.md) — the grounded discrete-variation, semantic-provenance, and node-inspectability plan (`EN-14`–`EN-16`).
5. [`.claude/skills/run-iron/SKILL.md`](../.claude/skills/run-iron/SKILL.md) — build, test, smoke, and durations.

## 1. Baseline (Stage 1 complete at `05df865` / `cc9f380`)

Everything in Stage 0 and Stage 1 is built, green, and stays:

- **In-place journaled transactions & read boundary** (`Document` rollback journal, `Tree`, `Changes`, `Store::take_changes()`).
- **Recorded-read graph, single write channel & `settle(mode)` (`EN-1`):** `structure::Key`, journaled per-root `ScopedIndexes`, keyed `Wiring` (`Cx::graph()` deleted), `sync_touched`, `MakerCx`, choke-point canaries.
- **Per-root Makers (`EN-2a`, `EN-2b`, `EN-2c`):** Layout & Pins makers, Constraints & Contexts makers, Network & DerivedBinding makers — all legacy layout/grouping/network hand rules deleted.
- **No refusal after derivation** (`18e3e61`) & **Order-free producer priority** (`95ff421`, `b0e2c68`).
- **Composition clocks & visibility gate v1** (`clocks.rs`, `run = always` override) + `work` choke-point & scaling probes.

## 2. Master Work Order

Every phase lands committed, pushed, and recorded in §2 here and `roadmap.md`'s Status table. Major milestones go through the council (`AGENTS.md`). Per `plans/README.md`, keys are never reused or renumbered.

### Completed & Retired Keys

| Key | Stage | Summary | State |
|---|---|---|---|
| **EN-0** | 0 · Stabilize | Round-3 review fixes (`b908d7b`) | **Done 2026-11-22** |
| **EN-1** | 1 · Spine | One write channel, locked-down `Cx`, scoped indexes, `settle(mode)` (`7a5840e`) | **Done 2026-11-22** |
| **EN-2a** | 1 · Spine | Per-root Layout & Pins makers (`6e5511e`) | **Done 2026-11-22** |
| **EN-2b** | 1 · Spine | Per-component Constraints & Contexts makers (`a26395a`) | **Done 2026-11-22** |
| **EN-2c** | 1 · Spine | Per-root Network & DerivedBinding makers (`05df865`) | **Done 2026-11-22** |
| **EN-3**..**EN-6b** | *(Retired)* | Universal N-axis patch algebra experiment (`14e4534`..`85c4c45`, archived on `archive/universal-patch-experiment`, reverted in `cc9f380`; see post-mortem) | **Retired 2026-11-23** |
| **EN-12** | 2 · Lean Core | Flat `Document::solved` (`Op::Solve`, schema 16, solver-free undo/redo/replay/load), split `gesture_preview` & `live_solve_preview`, zero-alloc `NodeId::expanded_parts()`, recorded-read `TemplateCache` (deleted `Document::scopes`, `Owners::solved`/`edited`/`wanted_slots`, `keep_solved`, `apply_recorded`, `release`, `solve::writes`, and `check_owners` solver exemption). **Net +334 lines in `src`: the "net-negative" checkpoint was missed** (§6). | **Done 2026-11-23** |
| **EN-13** | 2 · Lean Core | **Lean Lexical Virtual Views + minimal `ItemTable`:** virtual `Tree` over shared `Arc<Template>` + sparse facets + keyed `ItemTable` (`Arc<[ItemRecord]>`) + derived relations (`Arc<[Relation]>`) + in-place piece updates + single transaction-wide net `ViewDelta` + independent per-owner `NODE_BUDGET` safety cap (deleted `Docs::copies`, `apart`, `with_copies_in_document`, static `Owners` tables, and cross-owner budget accounting). **With EN-7, net +3,355 lines in `src`; the views subsystem is ≈ +2,400 net against the post-mortem's ~500, so the "net LOC dropped significantly" checkpoint was missed, and `views_spike` was never re-measured.** Measured at the council review (single runs, `instance_cost`): memory per meter 141.3 → 102.8 KiB (−27%), edit plan at 700 meters 26.7 → 18.0 ms, but a definition edit at 700 meters 19.8 → 31.3 ms (+58%) — see `EN-17`. `NODE_BUDGET` is now per owner, so the document total is uncapped (1,000 meters hold 67,000 expanded nodes; the old global cap was 50,000). | **Done 2026-11-24** |
| **EN-7** | 2 · Lean Core | **Deleted `regraph` & remaining wiring/repeats on virtual `Tree`:** per-parent `Key::LetsIn` `@name` lookup, `ThemeMaker` with cached light/dark tables, keyed `embeds_in` + post-commit 2D/3D `EmbedTopology` with O(1) `Docs::cyclic_embeds`, `ScopedIndexes` virtual global indexes, O(1) repeat root lookup, and `Scene` empty-source unframed embed restoration (deleted `regraph`, `Rescope`, `after_transaction` classifier, `theme_at`, `REPEAT_INPUTS`, `update_drawn`/`ensure_drawn`; removed 7 rows from `document_bridge::scaling`) | **Done 2026-11-24** |
| **EN-8a** | 3 · Scaling | **Epoch version stamps + local topological pass scheduling:** monotonic `epoch` with `leaf_changed_at` / `input_changed_at` / `key_changed_at` + per-cell `verified_at`/`changed_at`/`must_run` + `unneeded_may_be_behind` fast guard + stack-safe O(\|dirty\| + \|E(dirty)\|) `compute_local_ranks` & local `raise_queued` (deleted `Reactive::stale`, `GRAPH_STALE`, `IDLE_SWEEP`/`sweep`, `evaluated_at`/`propagations`, and global `height`/`raise`/`compute_heights`; fixed `run_native` setting `checked` before cycle-deferral `return 0`). Net +387 lines; the stamps sit on top of the cell-level `unneeded` set rather than replacing it (`EN-8c` removes it); `changed_at` is written and never read. | **Done 2026-11-24** |
| **EN-8b** | 3 · Scaling | **Owner-level `OwnerIndex` + `ActiveOwners` + delta clock gating:** `Owner` (`Composition(c)` / `Entity(e)` / `Free`), O(1) `Tree::root_of`, incremental `OwnerIndex` (compositions, `run = always`, figures, traces/accumulators/queries/entities/clips, and cross-owner reactive read counts), O(\|active\| + \|cross-owner edges\|) `compute_active`, delta `Clocks::pause`/`resume`/`forget`, `BTreeSet` `running_steps`/`running_clips`, O(1) `Steps::forget`/`Queries::forget`/`Player::forget`, and narrowed `sync_touched` clip refresh (deleted `StoreCore::gate`/`paused`, `Clocks::gated`/`GateKey`, `Reactive::reader_nodes`/`wired`, `Docs::reshaped`, `op::reshapes`, `Steps::prune`/`Queries::prune`, and `stamp()`; removed 62 rows from `document_bridge::scaling`). Net +1,119 lines. **Broke §5's "no new op classifiers" rule:** `sync_owners` (480 lines) classifies every forward and inverse op by hand into ~20 cached maps with no check against `OwnerIndex::rebuild`, and the council found four bugs in it (§6). `EN-8c.0` replaces it. | **Done 2026-11-24** |

### Active Work Order (`EN-8c.0` → `EN-8c` → `EN-17` → `EN-9` → `EN-10` → `EN-14` → `EN-15` → `EN-16` → `EN-11`)

| Key | Stage | Builds | Deletes | Net-LOC target / Exit | State |
|---|---|---|---|---|---|
| **EN-8c.0** | 3 · Scaling | **Owner index correct by construction:** a per-root runtime summary (`PerRootMakers<RootRuntime>`: the root's run mode, traces, accumulators, queries, entities, outgoing `Figures` and `Animates`) recomputed from scratch over the root's subtree with recorded structural reads; the global owner maps are aggregated from the old-versus-new summary of each re-made root; `IRON_OWNER_CHECK` compares the aggregate with `OwnerIndex::rebuild` after every transaction. Relation edits resample only clips whose `Animates` changed. | `sync_owners`' op/inverse classifier and its reconciliation steps; the hand-kept reverse maps it needs | **Net negative.** Fixes by construction: Auto ↔ On Demand leaving a simulation running; a `Figures` link+unlink in one transaction; a move away from a parent deleted in the same transaction (undo); a running clip gaining a target leaving `wake.live` stale. Regression tests for each; differential suite with `IRON_OWNER_CHECK=1` green | **Done 2026-11-24.** As built: `clocks::RootRuntime` + `root_runtime` (reads only through the recording `Tree`, so owners of clip targets and of nodes under pictured entities record exactly what decides them), `OwnerIndex::apply(old, new) -> RuntimeDiff`, `StoreCore::sync_owner_makers` / `apply_owner_diff` / `note_machine_ops`; `sync_owners` now only collects moved subtrees (their cells' cross-owner reads) and the machine player's revisits. Deleted `add_figure_rel`/`remove_figure_rel`/`add_animates_rel`/`remove_animates_rel`, `figure_rels`, `figure_edge_counts`, `clip_rels`, `OwnerIndex::rebuild` and the op/inverse classifier. **Production −51 net lines** (`clocks.rs` +317/−155, `lib.rs` +224/−437); tests +184: five regressions in `reactivity.rs`/`time.rs`, each failing on `63a8199`. Plain 522, differential with `IRON_OWNER_CHECK=1` 530, scaling probe, fast smoke pass. |
| **EN-8c** | 3 · Scaling | **One demand notion: owner-level demand + pull on read** (engine spec §4.1). A cell is needed when its owner is in the demand set (shown compositions, `run = always`, `Free`, owners holding a constraint cluster, closed over cross-owner reads and `Figures`) — a rule, not a stored set. Owners entering demand bring their behind cells up to date; store code that reads a resting cell pulls it (evaluate its behind cone, leave it resting) and re-runs. No bridge cone in the first cut: a scaling row "a shown page reads one hidden value" decides whether one is needed. | `Store::necessity`, `Reactive::necessity`, `Reactive::unneeded`, `set_unneeded`, `relaxed`, `pinned`, `candidates`, `need`, `relax`, `exact()` (export uses `exact_all()`), `ROOTED`, `advance`'s necessity re-check | **Net negative** across `EN-8c.0` + `EN-8c`. Exact parity under `observe_all(false)` (`reactivity::*`, `differential_random`); a `debug_assert!` on any read of a resting cell outside a pull; new scaling rows (cross-owner read, unrelated relation edit) | **Done 2026-11-24** (`569ce7d`). As built: `StoreCore::sync_demand` (owner-level demand: `compute_active(…, held, demand: true)` seeded by every shown composition, `run = always` compositions, the free owner and cluster owners, closed over cross-owner reads and `Figures`); owners leaving it `rest` a whole owner's cells (`Reactive::cells_of`), owners entering it `wake_cells`; moved cells follow their new owner (`place_moved`); store reads of resting cells pull without pinning (`Reactive::pull`); host commands pull the nodes they act on (`Store::pull_nodes`); export, the agent's whole-document view, `author`/`table`/`link`, a binding checked for cycles and the rare fallbacks call `exact_all()`. Deleted `Store::necessity`, `exact()`, `Reactive::necessity`/`set_unneeded`/`relax`/`relaxed`, `pinned`, `candidates` (and `work::GRAPH_CANDIDATES`), `ROOTED`, `advance`'s necessity check and the `exact()` at the start of every transaction and action. **Production +29 lines (EN-8c.0 + EN-8c: −22).** The scaling table lost 85 rows (every `graph.nodes`/`natives`/`readers`/`descendants`/`relations`/`roots` row) and gained two under `EN-9` (`scratch()` still wakes every resting cell of its clone). `differential_random`'s partial-observation script keeps only the sheet `always` and reads the viewer back in half the seeds (a read back keeps the viewer in demand). Plain 522, differential with `IRON_OWNER_CHECK=1` 530, the 1,000-seed partial-observation run, scaling probe and fast smoke pass. |
| **EN-17** | 3 · Scaling | **Why virtual views saved less than expected:** account for the whole-store memory per meter (−27% measured against the spike's "copies at 2.5% of materialized") and for the definition-edit slowdown at 700 meters (+58%); fix or re-baseline, and re-run `views_spike`-style numbers into this table | — | Measured explanation recorded here; regressions fixed or accepted by the user. Also: the differential suite takes ~155 s at `63a8199` (154 s measured 2026-11-24) against ~27 s recorded on 2026-11-22 — find which tests grew | **Next** |
| **EN-9** | 3 · Scaling | **Store-side `scan_all_*` boundary & remaining scaling burn-down:** explicit `scan_all_*` naming + `clippy::disallowed_methods` on hot store paths; clear remaining `store.clusters`, `machines`, and store-side rows in `scaling.rs`; captures observe what they draw. Includes: editing a machine `state`/`channel`/`transition`/`tween`/`definition` scans every entity (`machines_full` → `machine.rs:330`), untested by the probe; decide whether `NODE_BUDGET` needs a document-wide cap again; `scratch()` (every edit plan) still wakes every resting cell of its clone — plans should pull what they read; host commands without a fixed scope (`author`, `table`, `link`, the agent's view) call `exact_all()` | Remaining store whole-document scans in `iron_document` | All store rows in `document_bridge::scaling` flat across both axes | Open |
| **EN-10** | 3 · Scaling | **Host-side scaling:** per-composition embed/control/3D indexes, touched spatial scopes only, per-composition paint order, incremental timeline lane planning, `gas::find` & `interaction` indexes | Host whole-document scans | Every row of `document_bridge::scaling` flat across both axes (store + host); `document_scaling_time` +6 ≤ 1.2× +0 | Open |
| **EN-14** | 4 · Inspect & Vary | **First-class Node Inspectability & Provenance ("Ask the node"):** unified read-only `Store::inspect(node)` returning tree location, semantic derivation (`extends`/instance/slide/take), property provenance stack, incoming drivers (bindings, clips, machines, constraints, layouts), and outgoing consumers; expose in XML `<look id="#…" connections/>` and UI Inspector | Ad-hoc disconnected inspection helpers | Agent and UI can trace any animated/bound/derived component in one query; unit + agent-probe tests pass | Open |
| **EN-15** | 4 · Inspect & Vary | **Small Discrete Variation Layer (~350–500 lines) + Shared-property Machine States:** finite `Ordered`/`Unordered` axes (stable IDs + tombstones), values & `hidden` only, finite-set resolution/arrows/provenance, bubbling ops (`drag`, `widen`, `narrow`, `split`, `reset`); migrate `machine.rs` poses onto unordered local state patches so cyclic/partial-sharing machines work end-to-end | Legacy per-state channel-value storage in `machine.rs`; historical toy crate `src/iron_patch` | Cyclic 3-state triangle machine (`{A, B}` colour + `{B, C}` position) + smooth arrow transitions + bubbling ops + `plot-view.txt` pass; discrete variation core ≤ ~500 lines | Open |
| **EN-16** | 4 · Inspect & Vary | **Decks, Takes, Ghosts & Narrative Arrows:** ordered document axes (`slide`, `take`, narrative joints), `@slide`/`@take` readable by `when` for structural/topological changes, `<place … at="take=2" clock="…"/>` with isolated simulation state for ghosts/chaos comparisons, Inspector & State Graph bubbling/provenance UI | Narrative extent special-cases | Slide deck (`[B, ∞)` vs `{C}` + `when` schematic branch), pendulum take+ghost race on shared clock, chaos comparison, and bubbling UX checks pass | Open |
| **EN-11** | 5 · Storage | **Chunked snapshots & point loading:** revise `specs/patch-storage.md` for the grounded engine (cut premature Postgres index tables), implement content-addressed chunks, Merkle manifests, partial loading | Whole-scope snapshot requirement for large scopes | `patch-storage.md` exits met | Open |

---

## 3. Checkpoints

| When | Decision / Gate |
|---|---|
| End of `EN-12` | Confirm net-negative LOC, solver-free undo/redo/replay/load, strict `gesture_in_progress()`, and recorded-read `TemplateCache`. |
| End of `EN-13` | Confirm `Docs::copies` is completely deleted, net LOC dropped significantly, and read performance matches `views_spike` expectations. |
| End of `EN-7` | **Cohesive Engine milestone:** `regraph`, `Docs::copies`, and all legacy hand refresh rules are gone. |
| End of `EN-8c` | **Met 2026-11-24** (production −22 lines across `EN-8c.0` + `EN-8c`). **One demand notion:** `unneeded`, `necessity`, `relax`, `exact()` and `ROOTED` are gone, owner-level demand is the only answer to "is this cell needed", and `EN-8c.0` + `EN-8c` together are net negative. If not, stop and re-plan with the user before `EN-17`/`EN-9`. |
| End of `EN-10` | **Zero-Slope Scaling milestone:** all store and host scaling probes flat on both axes. |
| End of `EN-14` | Confirm `inspect(node)` / `<look connections/>` makes derivations, animations, bindings, and consumers legible in one query. |
| End of `EN-15` | Confirm discrete variation core stays small (~350–500 lines) and delivers cyclic shared-property machines cleanly before building decks/takes in `EN-16`. |

Every checkpoint records the phase's **measured** net lines (`git diff --shortstat` over `src`) and any performance numbers it promised, in its §2 row. A checkpoint that is missed says so in the row; it is not marked as met.

---

## 4. Immediate Next Steps (`EN-17`; `EN-8c.0` and `EN-8c` done)

### EN-8c.0: owner index correct by construction
1. A `RootRuntime` summary per top-level root, made from scratch by an `owner_makers: PerRootMakers<RootRuntime>` over a recording `Tree`, like the other per-root makers: whether the root is a composition and its run mode; its live traces, accumulators, queries and entities, each with its chain of entity ancestors; its outgoing `Figures` (composition → entity) and `Animates` (clip → target) relations.
2. When a root is re-made, subtract its old summary from the global owner maps and add the new one, then re-own what a change of `pictured_by` moves. The runtime systems (steps, queries, machines, `running_clips`/`animated`, clocks) take the same set differences.
3. Delete `sync_owners`' classification of forward and inverse ops and the reconciliation steps that follow it.
4. `IRON_OWNER_CHECK` compares the aggregate with `OwnerIndex::rebuild` after every transaction; regression tests for the four bugs in §6; relation edits resample only clips whose `Animates` changed.

### EN-8c: one demand notion (engine spec §4.1)
1. `needed(cell)` = everything observed, or the cell's owner is in the demand set. No stored `unneeded` set.
2. Owners entering demand bring their behind cells up to date (epoch stamps say which); owners leaving it simply rest.
3. Store code reads through the existing read recorder; a read of a resting cell pulls its behind cone and re-runs the reader (`current`), with no pinning. `run`, `scratch()`, `apply_session`, `fire_at` and the host's `exact()` calls stop making the document exact; export calls `exact_all()`.
4. Delete `ROOTED`; constraint clusters hold their owners in demand.
5. Verify: exact parity under `observe_all(false)`, a `debug_assert!` for a read of a resting cell outside a pull, new scaling rows, net negative.

---

## 5. Rules

- **One track, one set of keys (`EN-n`).** Never reuse or renumber retired keys (`EN-3`–`EN-6b`).
- **Scope-creep circuit breaker (`AGENTS.md`):** If a simplification phase reaches **+1,500 net lines** without having deleted the old mechanism it replaces, **stop immediately and check in with the user**.
- **Delete as you go.** A phase is not done until its "Deletes" column is actually gone.
- **One real consumer before generalizing.** Never generalize a new abstraction across 3+ subsystems before one concrete user-facing workflow uses it end-to-end.
- **No new op classifiers or stamps.** Until a family is converted to cells, it refreshes coarsely by touched keys in its scope.
- **Commit and push** each completed phase, updating §2 here and `roadmap.md`'s Status table (`AGENTS.md`).

---

## 6. Council reviews

### 2026-11-24: direction review of `EN-12`..`EN-8b` (`63a8199`)

Question: since the revert, is the engine going in a better direction, or in circles?

| Delegate | Verdict | Score |
|---|---|---|
| Claude Opus 5.5 | Sideways, leaning better; at risk of circles in `EN-8c` | 6/10 |
| GPT 6.2 Sol | Better | 6/10 |

Agreed: the "Deletes" columns are real, and the scaling probe's removed rows are genuinely flat (the test fails in both directions). The size checkpoints were missed: `iron_document/src` grew +4,715 (+10%) against the experiment's +6,904. `sync_owners` is a new hand-written op classifier, against §5. `EN-8c` should use owner-level demand rather than exact per-leaf need counts; its main risk is store code reading a resting cell and getting a stale value without an error.

Disagreed: Sol wanted a bridge cone (so one cross-owner read does not wake a whole hidden owner); Opus wanted none in the first cut. Decision: none in the first cut, plus a scaling row that would show the cost.

Verified findings (all checked against the code):

| Finding | From | Goes to |
|---|---|---|
| Auto ↔ On Demand on a shown composition does not dirty the gate, so its simulation keeps running (`lib.rs` `sync_owners` watches only `Always`) | Sol | `EN-8c.0` |
| A `Figures` link and unlink in one transaction is indexed as linked: the added-relation pass checks only that the endpoints are live; moving an entity's only figure out of a hidden composition leaves its accumulator frozen | Sol | `EN-8c.0` |
| The old owner of a reparented node is read from the post-transaction tree, so a move away from a parent deleted in the same transaction (an undo) keeps the subtree under a dead owner | Opus | `EN-8c.0` |
| A running clip gaining a target in another composition leaves `animated` and `wake.live` stale (recomputed only when `running_clips` changes) | Sol | `EN-8c.0` |
| Any relation edit resamples every clip (`sync_touched` treats `Key::Relation(s)` as clip structure): cost grows with hidden clips, and a paused hidden target moves | Sol | `EN-8c.0` |
| Editing a machine state or definition scans every entity in the document (`machines_full`) | Opus | `EN-9` |
| Definition edit at 700 meters +58% slower; memory saving well short of the spike's | Opus | `EN-17` |
| `NODE_BUDGET` per owner leaves the document total uncapped | Opus | `EN-9` (decide) |
