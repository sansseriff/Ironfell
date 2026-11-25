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
| **EN-8c** | 3 · Scaling | **One demand notion: owner-level demand + pull on read** (engine spec §4.1). A cell is needed when its owner is in the demand set (shown compositions, `run = always`, `Free`, owners holding a constraint cluster, closed over cross-owner reads and `Figures`) — a rule, not a stored set. Owners entering demand bring their behind cells up to date; store code that reads a resting cell pulls it (evaluate its behind cone, leave it resting) and re-runs. No bridge cone in the first cut: a scaling row "a shown page reads one hidden value" decides whether one is needed. | `Store::necessity`, `Reactive::necessity`, `Reactive::unneeded`, `set_unneeded`, `relaxed`, `pinned`, `candidates`, `need`, `relax`, `exact()` (export uses `exact_all()`), `ROOTED`, `advance`'s necessity re-check | **Net negative** across `EN-8c.0` + `EN-8c`. Exact parity under `observe_all(false)` (`reactivity::*`, `differential_random`); a `debug_assert!` on any read of a resting cell outside a pull; new scaling rows (cross-owner read, unrelated relation edit) | **Done 2026-11-24** (`569ce7d`). As built: `StoreCore::sync_demand` (owner-level demand: `compute_active(…, held, demand: true)` seeded by every shown composition, `run = always` compositions, the free owner and cluster owners, closed over cross-owner reads and `Figures`); owners leaving it `rest` a whole owner's cells (`Reactive::cells_of`), owners entering it `wake_cells`; moved cells follow their new owner (`place_moved`); store reads of resting cells pull without pinning (`Reactive::pull`); host commands pull the nodes they act on (`Store::pull_nodes`); export, the agent's whole-document view, `author`/`table`/`link`, a binding checked for cycles and the rare fallbacks call `exact_all()`. Deleted `Store::necessity`, `exact()`, `Reactive::necessity`/`set_unneeded`/`relax`/`relaxed`, `pinned`, `candidates` (and `work::GRAPH_CANDIDATES`), `ROOTED`, `advance`'s necessity check and the `exact()` at the start of every transaction and action. **Production +29 lines (EN-8c.0 + EN-8c: −22).** The scaling table lost 85 rows (every `graph.nodes`/`natives`/`readers`/`descendants`/`relations`/`roots` row) and gained two under `EN-9` (`scratch()` still wakes every resting cell of its clone). `differential_random`'s partial-observation script keeps only the sheet `always` and reads the viewer back in half the seeds (a read back keeps the viewer in demand). Plain 522, differential with `IRON_OWNER_CHECK=1` 530, the 1,000-seed partial-observation run, scaling probe and fast smoke pass. **Council round 2026-11-24 (§6), both 6/10, fixed:** cells that moved, were made, or were woken by a read in a resting owner are placed with their owner (`StoreCore::place`, at the end of each transaction and in `sync_gate`), so a move is judged by the cell's owner, not the final tree; a pull rests again everything it woke; owners made outside demand rest; a query pulls its inputs before it asks; only an `Animates` link made or gone resamples clips; `apply_session` no longer wakes everything (the cycle check is structural); the settle fallback decides demand again; Convert to Path and the inspector's scrub read through `Store::current`; natives spanning owners never rest; the `unneeded_may_be_behind` guard is deleted (every rest set it). `IRON_OWNER_CHECK` now also checks demand (`check_demand`: kept demand equals a fresh decision, resting cells are exactly the cells of owners outside it, no awake cell reads a resting one, rooted natives excepted) and found two of these bugs itself. The partial-observation script moves random content to and from a second page that rests; scaling rows `cross-owner read` and `unrelated relation edit` added (only `allocations`/`store.clusters` grow, as for every transaction). The fixes are −11 lines; the test-only check +67 (production +56 net). Five regression tests. Plain 526, differential with `IRON_OWNER_CHECK=1` 530 (the 20-seed partial-observation run included), scaling probe and fast smoke pass; the 1,000-seed run fails at seed 334 on the structure lag below. A resting page's derived structure lags (repeat items are made by rooted natives from what its cells held when it rested; `ROOTED` used to keep repeat inputs evaluated), so the derived node set depended on what is observed. **Decided 2026-11-25 (user): pull on ask**, not per-cell demand for structure inputs. Built: a binding's read of a derived node that does not exist brings up to date what makes it (its authored instance or repeat) when that rests (`Reactive::take_missing`, `StoreCore::pull_makers` in `settle_reads`); a gesture commit does the same for what it previews; the Layers panel pulls the page it lists, the inspector the node it shows, and narrative value marks read through `Store::current`; the random script lists nodes as a host does, pulling first. Also fixed: an entity pictured only by an `on_demand` composition held still only if the composition itself held an accumulator — entity clocks now run only when pictured on an open path or by a `run = always` composition (free-running.md §2). Verified 2026-11-25: plain 528, differential with `IRON_OWNER_CHECK=1` 532, the 1,000-seed run (all but seed 634, below), scaling probe and fast smoke. Seed 634: a cycle through `deref` closed while its page rested reports "has no value" where the observed store reports "a cycle" — the same leaves fail, and in either order the evaluation is valid for a cycle, only the message depends on it (`IRON_SEED_FROM` resumes the run past a seed). **Council round 2 (2026-11-25, §6; Opus 7/10, Sol 6/10), fixed:** an ask now brings the whole resting page up to date (`StoreCore::pull_owners`, replacing `pull_makers` and the per-node cone in `pull_nodes`), so a node that should exist or should be gone, and what a rooted layout places, are current; `Store::current` records every node it reads and pulls their pages; a transaction pulls the pages of the derived nodes its ops name (what `exact()` at the start of `run` did, scoped); demand is decided at the end of every transaction inside the net view window (`reconcile_rest`, so a wake never reports an item made and dropped) and at the end of the outermost live settle when a cross-owner read changed; a run-mode or embed change marks demand for a fresh decision; a shown composition off an open path seeds no clock (deleting `comp_accumulators` and the figures gate), so a figure reading its entity no longer runs it; the edit plan's `scratch()` clone has no demand (everything in it is awake); `check_demand` also runs at the end of every transaction and exempts only rooted natives whose page rests. Six regression tests, each failing before. Round 3 (§6): clips follow demand rather than clocks; Figures followed only from open or `run = always` compositions; `scratch()` never decides demand; nested asks; four deletions. Open from round 2: a genuine `Animates` edit still resamples every clip, moving a paused one (`resample()`, `EN-9`); the 1,000-seed script lists nodes by pulling every page, so structure lag lasting longer than a step is covered only by targeted tests (Opus); a binding that reads an item that legitimately does not exist pulls its page on every evaluation while something rests. Not done: panels re-asking when a value they showed moves (they show what they read when they last asked); the narrative "window" for values that run on time (observe the page while the block is in view). |
| **EN-17** | 3 · Scaling | **Why virtual views saved less than expected:** account for the whole-store memory per meter (−27% measured against the spike's "copies at 2.5% of materialized") and for the definition-edit slowdown at 700 meters (+58%); fix or re-baseline, and re-run `views_spike`-style numbers into this table | — | Measured explanation recorded here; regressions fixed or accepted by the user. Also: the differential suite takes ~155 s at `63a8199` (154 s measured 2026-11-24) against ~27 s recorded on 2026-11-22 — find which tests grew. The per-owner memory accounting built here is what the demand diagnostics proposal needs (`notes/demand-diagnostics.md`) | **Done 2026-11-25.** Explained and fixed. **Memory:** the spike measured only what views replaced (`Docs::copies`); views now cost 3.8 KiB a meter, as the spike promised. The rest of the store was never in the spike: each instance's graph cells and their expressions, derived anchor expressions, scoped indexes, the path registry. A new probe drops the store part by part (`Store::drop_in_parts`, `instance_cost::memory_by_part`; run it alone). What remains, per meter: derived makers 16.6 KiB (the anchor expressions, now held once), scoped indexes 10.4, graph dependents 7.7, bound cells 5.1, owner index and clocks 4.2, views 3.8, the path registry 67 paths (never freed; about 4.6 KiB). Fixed: derived expressions were held three times (the derived makers, `Reactive::derived`, each cell), now one shared `Arc<Expr>`; the path registry stored each path twice and took a lock on every read, now one step per path, read without a lock (its "no owner" is not an id, so `#0/5` reads back). **104.2 → 70.0 KiB a meter** (141.3 before `EN-13`: −50%). **Definition edit:** every instance got a new template, so the view diff compared every node of every instance, twice; now each pair of template revisions is diffed once and each view checks only the changed indices and its own overrides (`TemplateDiffs`); the differential reference turns this off and walks every node, so it checks the shortcut instead of sharing it (council: a mutation of `changed_nodes` fails 31 tests, 2 before the fix). **700 meters 30.1 → 5.4 ms** (19.8 before `EN-13`), 1,000 meters 43.6 → 8.2 ms, edit plan 16.7 → 7.1 ms. **Differential suite:** two tests are nearly all of it (`two_hundred_random_scripts`, ~1,500 CPU-s; `observing_part_of_the_time`, ~330 CPU-s; the other 535 tests take 6 s). The cost was the harness's snapshot asking `context_reads` and `hover_listeners` of every live node, each scanning every cell or `on` node (quadratic per step), plus the registry lock contended across threads. Now asked only of nodes that can have an answer, and the registry read without a lock: the full differential suite with `IRON_ATOMICITY=1 IRON_OWNER_CHECK=1` **212 → 25 s** (541 tests), the plain suite 2.3 → 1.2 s. Production +160 net lines, of which the probe is +45; harness and tests +72. Plain 538, differential 541, scaling probe and fast smoke pass. Not fixed: anchor positions are exponential in nesting depth (`anchor::outward` copies x and y into both of the next level's expressions); a `with` form would be linear but `handle::linear` does not invert through `with`. Placing a meter costs O(meters) (4.8 ms at 700: per-root makers re-run the whole composition), `EN-9` |
| **EN-9** | 3 · Scaling | **Store-side `scan_all_*` boundary & remaining scaling burn-down:** explicit `scan_all_*` naming + `clippy::disallowed_methods` on hot store paths; clear remaining `store.clusters`, `machines`, and store-side rows in `scaling.rs`; captures observe what they draw. Includes: editing a machine `state`/`channel`/`transition`/`tween`/`definition` scans every entity (`machines_full` → `machine.rs:330`), untested by the probe; decide whether `NODE_BUDGET` needs a document-wide cap again; `scratch()` (every edit plan) still wakes every resting cell of its clone — plans should pull what they read; host commands without a fixed scope (`author`, `table`, `link`, the agent's view) call `exact_all()`, and each `exact_all()` makes the next demand decision rest every hidden owner again; host commands that pull fixed nodes (`Store::pull_nodes`) could read through `Store::current` instead, as Convert to Path does | Remaining store whole-document scans in `iron_document` | All store rows in `document_bridge::scaling` flat across both axes | **In progress (2026-11-25).** Done:
- **Constraint problems found by index:** `Reactive` keeps the cluster natives whose last run found a violated constraint, so `cluster_problems` visits only those. A transaction compares only the nodes in trouble before it and writes messages only for the new ones. This removed every `store.clusters` row (12) and the `allocations` row of every ordinary transaction (9).
- **Paste:** copied roots are sorted by their place among their ancestors' children, not by a preorder walk of the document. Connections, constraints and clips are found from the relations into the copy, not from every one in the document. The model and library are found by type, and the model's last entity only when a main is pasted. All four paste rows are gone.
- **Host commands:**
  - the agent's whole-document view and the SVG export read through `Store::current`, so only the pages they read are brought up to date;
  - `author`, `table` and `link` pull the nodes their arguments name.
  - `exact_all()` is left only as `current`'s fallback. Test `the_agent_s_view_of_a_resting_page_is_current`.
- **An allocation tracer:** `WORK_TRACE=allocations` in `scaling::trace_one_count` groups allocations by store stack.

Left:
- `scratch()` clones the whole store for every edit plan (the `edit plan` and `let made` rows). Waking its clone is the same order of cost as the clone, so the real fix is planning without a clone (the real store and a rollback), an API change to ask the user about.
- `fire` scans every `on` node (an `on.node` can name any node, so finding listeners needs an index).
- A theme switch clones the token map.
- Machine edits scan every entity (`machines_full`).
- `NODE_BUDGET`'s document cap.
- `resample()`'s whole-document path; a meter's placement is O(meters).
- The `scan_all_*` naming and lint. |
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
| End of `EN-8c` | **Met 2026-11-24** (production −22 lines across `EN-8c.0` + `EN-8c`; +34 with the council round's fixes, of which the test-only demand check is +67). **One demand notion:** `unneeded`, `necessity`, `relax`, `exact()` and `ROOTED` are gone, owner-level demand is the only answer to "is this cell needed", and `EN-8c.0` + `EN-8c` together are net negative. If not, stop and re-plan with the user before `EN-17`/`EN-9`. |
| End of `EN-10` | **Zero-Slope Scaling milestone:** all store and host scaling probes flat on both axes. |
| End of `EN-14` | Confirm `inspect(node)` / `<look connections/>` makes derivations, animations, bindings, and consumers legible in one query. |
| End of `EN-15` | Confirm discrete variation core stays small (~350–500 lines) and delivers cyclic shared-property machines cleanly before building decks/takes in `EN-16`. |

Every checkpoint records the phase's **measured** net lines (`git diff --shortstat` over `src`) and any performance numbers it promised, in its §2 row. A checkpoint that is missed says so in the row; it is not marked as met.

---

## 4. Immediate Next Steps (`EN-9`; `EN-8c.0`, `EN-8c` and `EN-17` done)

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

### 2026-11-24: review of `EN-8c.0` + `EN-8c` (`f4ef731`, `569ce7d`)

| Delegate | Verdict | Score |
|---|---|---|
| Claude Opus 5.5 | A real step forward, not circling; the architecture is right, owner moves and Convert to Path still stale, coverage trimmed | 6/10 |
| GPT 6.2 Sol | `EN-8c.0` a real simplification; owner-level demand needs a correctness pass | 6/10 |

Both: keep the per-root summaries (no bugs found in them; `IRON_OWNER_CHECK` sound for what it covers). Every bug found sat where rest state is kept by transitions (an owner entering or leaving demand, a move, a pull) rather than derived, the same weakness as the classifier `EN-8c.0` replaced. So the round added a check of the rule itself (`check_demand`) rather than more reviews, and it found two more bugs of the class on its first run.

| Finding | From | Outcome |
|---|---|---|
| A rect moved out of a group that moves into the same composition in one transaction stays stale (owner change judged from the final tree) | both | Fixed: every moved subtree is placed; placed at the end of the transaction (the 1,000-seed run found it shown stale until the next advance) |
| A hidden query asks with its resting input's old value | Sol | Fixed: inputs pulled before `Queries::advance` |
| Convert to Path bakes a hidden reader's stale value into the document | both | Fixed: `Store::current` |
| Pulled and newly made cells of a hidden owner stay awake | both | Fixed: a pull rests what it woke; new and read-woken cells are placed |
| A link at a running clip resamples every clip, advancing a paused one | Sol | Fixed |
| Selection changes wake the whole document (`apply_session`'s `exact_all`) | both | Fixed: removed |
| The settle fallback leaves every owner awake (`demand_dirty` not set) | Opus | Fixed |
| A native spanning owners can be rested by a move | Opus | Fixed |
| `unneeded_may_be_behind` no longer guards anything | Opus | Deleted |
| A constraint made on a hidden page may solve from stale inputs | Opus | Not a bug: test added, passes |
| An owner made outside demand never rests; a cell woken by a new cell's read stays awake | `check_demand` | Fixed |
| An accumulator pictured only by a shown On Demand composition keeps stepping | Sol | Fixed 2026-11-25: entity clocks follow the open path |
| `differential_random` never rests random content | Opus | Fixed: content moves to and from a page that rests (what runs on time is not moved: its clock pauses by design). It found two more bugs (a move shown stale until the next advance; fixed) and the structure lag below |
| A resting page's repeat items lag its values, so the derived node set depends on what is observed (`deref` by id reads the fallback; node lists differ; a gesture commit writes to an item that went) | 1,000-seed run | Fixed 2026-11-25: pull on ask (user's decision) |

### 2026-11-25: round 2, the fixes (`ea9d466`, `24c5233`)

| Delegate | Round 1 | Round 2 |
|---|---|---|
| Claude Opus 5.5 | 6/10 | 7/10 |
| GPT 6.2 Sol | 6/10 | 6/10 |

Both: round 1's fixes are real, with no regressions; the placement step and checking the rule (not the transitions) are the right direction; production stays small. Not ready for `EN-17` until pull on ask covered every asker.

| Finding | From | Outcome |
|---|---|---|
| A transaction's wake ran after its net view delta closed: an item made and dropped is reported as both | Sol | Fixed: demand decided inside the window |
| A targeted ask (`pull_nodes`, `current`) misses items that should exist or be gone, and a rooted layout's output (it pulls a cone, which stops at the awake native) | both | Fixed: an ask pulls the whole page |
| A transaction naming an item of a resting page sees its old items (a set fails as a bad id, or succeeds on a dropped item) | Opus | Fixed: `run` pulls the pages its ops name |
| A figure that reads its entity runs it from an `on_demand` composition | both | Fixed: only open compositions seed clocks |
| A run-mode or embed change, or a publish pointing at a dropped item, is stale until the next advance | Sol | Fixed: demand decided at the end of every transaction and live settle |
| A gesture on an item that went commits through its binding's source | Sol | Not a demand bug: the observed reference commits the same |
| A genuine `Animates` edit resamples every clip | Sol | Open: `EN-9` (`resample()`) |
| The 1,000-seed script pulls every page each step, hiding lag that lasts longer | Opus | Accepted: targeted tests instead (its picks filter on each store's own tree) |
| `check_demand` skips the end of a transaction; exempts every rooted native | Opus | Fixed |
| Reconciling inside a settle nested in another's stages | Opus | Fixed: only the outermost settle reconciles |

### 2026-11-25: review of `EN-17` (`45a09ae`)

| Delegate | Score |
|---|---|
| Claude Opus 5.5 | 8/10 |
| GPT 6.2 Sol | no score (ChatGPT usage limit reached before it reported) |

Opus: the three optimizations are correct, with no wrong output found. It checked:
- the template fast path reads only what `changed_nodes` compares or what each view's overrides hold;
- revisions only ever grow;
- the registry's ordering and bucket arithmetic, with a stress test on 8 threads;
- derived `Arc`s are compared by value.

All timings reproduced.

| Finding | Outcome |
|---|---|
| The differential reference (`full_views`) took the same template fast path as the store it checks, so a broken `changed_nodes` failed only 2 tests | Fixed: `TemplateDiffs::off` in the reference; the same mutation fails 31 |
| `memory_by_part` cannot see the registries (never freed); `memory_per_meter` reads 65.4 after another test warms them | Fixed: the probe prints paths per meter; the row says to run it alone |
| A path of one step used owner 0, so `#0/5` read back as `#5` | Fixed: a "no owner" value that is not an id; test |
| `collect_changed_nodes` lost its doc comment and `allow` | Fixed |
| The snapshot's context reads could ask non-live nodes | Fixed: live only |

Opus's registry stress test is kept (`id_registry.rs`).

### 2026-11-25: round 3, round 2's fixes (`e5584b9`)

| Delegate | Round 1 | Round 2 | Round 3 |
|---|---|---|---|
| Claude Opus 5.5 | 6/10 | 7/10 | 7/10 ("converging") |
| GPT 6.2 Sol | 6/10 | 6/10 | no score (usage limit reached mid-review; its nine reproductions were triaged) |

Opus: round 2's fixes are sound; round 3 found one rule edit with two side effects plus bookkeeping to delete, so the work is converging; ready for `EN-17` once those two are fixed. Triage by the intended use (lessons with pages, simulations, narrative, a timeline, an agent editing through edit plans), as the user asked: fix what is likely or possible; for what is rare or very rare, only check the engine does not panic (each fails on the test's own assertion).

| Finding | From | How likely | Outcome |
|---|---|---|---|
| A scrub does not move a clip on a shown `on_demand` page (clips followed the clocks) | both | Likely | Fixed: clips follow demand (`run_clips` in `sync_demand`), deleting their handling from `sync_gate` |
| An edit plan that makes a page and edits a new item fails: the plan's clone decided demand and rested hidden pages | both | Likely (agent edits) | Fixed: `scratch()` sets `planning`; demand is never decided in it |
| Reading one value of a hidden page runs every entity it pictures | Opus | Possible | Fixed: Figures followed only from open or `run = always` compositions |
| An ask inside an ask (`current` in `current`) loses the outer ask's record | Sol | Possible | Fixed: the inner one runs plainly |
| A shown `on_demand` page's trace shows no first output | Sol | Rare | Not fixed; no panic |
| Two hidden pages chained through a layout, asked through `current`, read the old layout | Sol | Rare | Not fixed; no panic |
| A tree read (not a value read) inside `current` sees a repeat's old items | Sol | Rare | Not fixed; no panic |
| A page embedded inside a hidden page's repeat, newly shown, is stale for a frame (and trips `check_demand`) | Sol | Very rare | Not fixed; no panic in release |
| An embed change leaves `check_demand` skipped for that transaction | Opus | Rare (test coverage) | Not fixed |
| A long headless session without `advance` rests new owners again every transaction (quadratic) | Opus | Rare | Not fixed |
| A refused delete of an item publishes the pull's view changes | Sol | — | Not a bug: the observed reference has the item too |

Deleted as duplicates (Opus): `sync_gate`'s own demand block (it calls `reconcile_rest`), `reconciling` (folded into `settle_depth`), `pull_missing`, the free-owner filter in `pull_owners`. Four regression tests, each failing before.
| Missing scaling rows (cross-owner read, unrelated relation edit) | both | Added |

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
