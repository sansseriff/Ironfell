# Handoff: building the new reactive engine

**Date:** 2026-11-19 · **Branch:** `master` · **Keys:** `RE-n`, `RE-n.m` for a
phase's step (plans/README.md, "Work keys") · **Closed 2026-11-21 and archived:**
phases 0 to 5 are done as §2 says, each with named items deferred or not met and
the reasons where each phase is (phase 5's in §4.10, "RE-5.6: phase end"). RE-4.3 is in
[`handoff-render.md`](../handoff-render.md) §10; the rest moved to
[`handoff-patches.md`](../handoff-patches.md) §1. Nothing here is open work.
**Status when open:** the design
([`specs/reactive-engine.md`](../specs/reactive-engine.md), "the spec" below) is approved
and its decisions settled. **Phases 0 to 4 are done** as §2's table says (2b's swap
removed the old engine; 2c, 3 and 4 each left named steps deferred, with the reasons
recorded where each phase is). **Phase 5** is in progress: its first step (§4.8) and
its scan measurements (§4.8, step 2) are done; its cells are not built. **This plan
ends with phase 5's layout, network and solver cells** (2026-11-20): its clock
providers, clips as cells, parking and phase 6 move to
[`handoff-patches.md`](../handoff-patches.md) §1. RE-4.3 (Dormant by view) goes to
`handoff-render.md` then, and this file to `archive/`.
This file is for whoever builds next: what to read, the order of work, the phases as tasks, and
the rules.

## 0. Read first, in this order

1. This file.
2. The spec, whole. Its §3 is the vocabulary; §14 lists what is assumed and not yet
   verified; §17 is the phase table; §19 the settled decisions.
3. `src/iron_engine_kernel` (`src/lib.rs`, `src/model.rs`, then `src/engine.rs`,
   `src/reference.rs` and `tests/kernel.rs`): the core pass of the spec's §6 and §7 as
   about 900 lines of running code, tested against recomputation from scratch.
   `lib.rs` says what it simplifies. It is a model to learn the
   algorithm from, not code to extend: the real engine is a new module,
   `iron_document::engine`, written in phase 2b.
4. [`specs/reactivity.md`](../specs/reactivity.md) §3 and §13: the behaviour that must
   survive.
5. The code being replaced, enough to recognise it: `Store::settle_repeats`,
   `Store::run`, `after_transaction`, `post_apply` and `resolve_physics` in
   `src/iron_document/src/lib.rs`; `graph.rs`; `expand.rs`.
6. The run-iron skill ([`.claude/skills/run-iron/SKILL.md`](../../.claude/skills/run-iron/SKILL.md))
   for building, the smoke and timings.

[`specs/reactive-engine-review.md`](../specs/reactive-engine-review.md) is the design's
history (three council rounds, what each found). It is not needed to build.

**The change in one paragraph.** Today any change to a repeat's items, and any
structural transaction, re-derives every derived node in the document and rebuilds and
re-evaluates the whole reactive graph; every transaction also clones the document. The
new engine keeps a tree of *owners* (an instance's body, a picture, a repeat, one of
its items), each made from a shared compiled *template*, and a graph of *cells*
evaluated in one pass in height order. A repeat is a Structure cell that makes and
disposes its items inside that pass, so a change costs what it touches. Only cells
some *observer* needs are evaluated. It is built beside the old engine and compared
with it after every step of every test until the two agree; then the old one is
deleted.

## 1. The problem

The user saw a placed `std:plot` drop below 60 fps while dragging (panning) inside it or
zooming it with the wheel, worst when grid lines appear and disappear quickly.

Value changes in Iron are already incremental: a pan step that only moves values costs
the store about 0.12 ms. **Structure** is not. A step that changes the plot's tick count
adds or removes a tick label (each axis's labels are a repeat over its ticks), and any
change to any repeat's items does two whole-document things in
`Store::settle_repeats` (`src/iron_document/src/lib.rs`, ~1290):

1. `expand::expand` (`expand.rs`, ~733) computes every derived node of the document
   (every instance's copies, every repeat's and conditional's items, in every
   composition) and diffs it against what exists.
2. `ReactiveGraph::rebuild` (`graph.rs`, ~269) drops the graph, re-collects every
   binding, recomputes every height by a global fixpoint (`compute_heights`) and
   **re-evaluates every binding in the document** (`evaluate(doc, &all, &all)`).

Transactions do the same: any transaction with a structural op (create, delete,
reparent, add or remove a facet, link, unlink, write a formula, touch a context name)
re-expands the document when `expand::affected_by` says so (`Store::run`, lib.rs ~2660)
and rebuilds the graph (`after_transaction`, lib.rs ~2294). Only setting a constant is
incremental.

### Measurements

Native release, `tests/plot_perf.rs` (ignored; `PLOTS=n` for n plots):

| | 1 plot (69 nodes) | 10 plots (690 nodes) |
|---|---|---|
| Pan step, values only | 0.13 ms | 0.14 ms |
| Pan step, tick count changed | 1.3 ms | 7.3 ms |
| Zoom step, tick count changed (160 of 300 steps) | 1.2 ms | 7.5 ms (max 14.9) |

Split of a re-expansion (temporary instrumentation, 2026-11-14): about **60 % graph
rebuild, 40 % expansion**; both grow with the document.

In the app (`PROFILE=wasm-bench`, demo document, one 700 × 420 plot,
`plot-perf.txt`), after the NaN fix below:

| Gesture | changed frame p50 / p95 / max | `action` (store) p50 / p95 / max | Frames over 24 ms |
|---|---|---|---|
| Slow pan | 4.3 / 12.3 / 21.9 ms | 0.85 / 6.7 / 14.1 ms | 12 of 181 |
| Wheel zoom | 8.3 / 11.3 / 19 ms | 4.4 / 6.4 / 9.8 ms | 0 of 180 |

The same cost at scale, from instance-cache.md: at 1,000 placed meters (69,072 live
nodes) **any structural edit costs 161.7 ms** and each placement 79.9 ms, because every
structural transaction re-expands every instance.

### Phase 0 baselines (2026-11-16, native release, M-series)

| Measure | Result | Probe |
|---|---|---|
| A constant edit at 1,000 meters, **after phase 2a** | **1.57 ms** (1.54–1.59 over three runs; was 12.5): `run` 0.006 ms, journal included; `after_transaction` 0.97; `post_apply` and history 0.59; commit 0.000. A structural edit there is 123 ms p50 | `instance_cost.rs` `expanded_instances_cost`; temporary timers, removed |
| Memory per placed meter beyond the first | **92.4 KiB**, 69 nodes, history cleared (100 to 700 meters) | `instance_cost.rs` `memory_per_meter` |
| A constant edit at 1,000 meters | **12.5 ms**, of which the document clone 6.1 ms and dropping the old document 4.4 ms (84 %); `after_transaction` 1.3 ms (`rebuild_clusters` 0.81, `solve_network` 0.34, layout 0.04); `post_apply` 0.62 ms, all `solve::clusters`; `affected_by`, `attachment::validate`, `invalidate`, `resample`, `settle_repeats`, palette, contexts, selection and state each under 0.01 ms | temporary timers, removed |
| Solver, 1 / 10 / 100 pendulums at rest | drag step **0.16 / 0.30 / 1.90 ms**; a transaction setting an unrelated constant **0.23 / 0.66 / 5.77 ms** | `solve_cost.rs` |
| Interned id growth | a value-keyed repeat moved through 10,000 positions: **+40,662 paths, +20,331 keys, about 5.2 MiB** never freed (about 530 bytes per item made) | `id_growth.rs` |
| Loading the demo document | **65.8 ms** (scene and showcase, 3,712 nodes, 359 KB) | `demo.rs` `load_demo_time` (app crate) |

**Finding: the 1,000-meter numbers are truncated.** At 1,000 meters the document has
52,072 live nodes, 50,506 of them derived, not the 69,072 instance-cache.md recorded:
expansion stops at `expand::NODE_BUDGET` (50,000 derived nodes), so only about 740
meters are expanded and the rest are left as a problem. `instance_cost` now reports a
structural edit at 129.0 ms and a placement at 80.6 ms over that truncated document.
Memory is therefore measured from 100 to 700 meters. The §15 meter targets should be
read against an un-truncated document (raise the budget for the probe, or compare at
700).

**A10 (registry growth):** about 530 bytes per value-keyed item made, kept for the
process. A plot whose ticks are keyed by value (D7) and pans so that one tick enters
per frame would grow it by about 30 KiB a second, 110 MiB an hour of continuous panning.
Small per gesture; not small for a page left animating. Reported to the user; D5's
decision stands until they choose.

### Already done (2026-11-14, `639f07b`)

- `perf::Phase::Action` times `Store::pointer_action` (the store's share of a drag or
  wheel action, re-expansion included), reported as `action` by `__iron.timings` and
  the driver's `measure`.
- `Value` equality is sameness of data: NaN equals NaN (`value.rs`); formula `==` keeps
  IEEE numbers (`expr::same`). Before, an `auto` (NaN) constant never equalled itself, so
  each re-expansion rewrote it in every instance (10 plots: 102 ops per tick change,
  now 2) and a NaN-valued binding counted as changed every pass. Test:
  `tests/reactivity.rs` `a_plot_s_new_tick_writes_nothing_outside_it`.
- Probes: `.claude/skills/run-iron/plot-perf.txt` (in app) and
  `src/iron_document/tests/plot_perf.rs` (native), both described in the run-iron skill.

## 2. Work order

The spec's §17 has each phase's contents and exit. In order:

| Key | Phase | State |
|---|---|---|
| RE-0 | Baselines and the differential harness, run old against old | **Done 2026-11-16** (below) |
| RE-1 | The read contract (`Tree`, `take_changes()`) on the old engine; every caller moved | **Done 2026-11-16** (below) |
| RE-2a | Transactions in place on the old engine: the document journal | **Done 2026-11-16** (below; tasks in §4.3) |
| RE-2b | The new cell graph and owners, copies still stored. **This is the plot fix**; it closes the roadmap's "Plot pan and zoom performance" row | **Done 2026-11-17** (§4.4; swap `9f4ac59`) |
| RE-2c | Copies leave the document: instances as references | **Done 2026-11-19** as far as its exit (§4.6, "Where phase 2c ends"); lazy slots and validation from templates moved to phase 4 |
| RE-3 | Time: clocks, the scheduler, steppers as cells, recorded events | **Done 2026-11-19** (§4.7): one `advance`; step 4 (`now()`, clock providers) deferred, now PT-1 (`handoff-patches.md`) |
| RE-4 | Observation: Dormant, Unloaded, parking, extents | **Done 2026-11-19** (§4.9): observers, necessity, the idle sweep, three council rounds; steps 3 (Dormant by view) and 4 (Unloaded, lazy slots) deferred with the reasons. RE-4.4 is now PT-11 |
| RE-5 | The pipeline as cells: layout, the network solve, the constraint solver. Does not depend on 3 or 4; closes instance-cache.md's "Open" at 1,000 meters. Clips as cells moved to PT-9 (2026-11-20) | **Done 2026-11-21** (RE-5.6, §4.10's "RE-5.6: phase end"): RE-5.1 (a cluster solved only when something of it changed) and RE-5.2 (the scans) done (§4.8). The cells, on one general cell (the user's decision, 2026-11-20), are planned as RE-5.3 to RE-5.6 (§4.10). **RE-5.3 done 2026-11-20** (natives in the graph; the network per island on them). **RE-5.4 done 2026-11-21** (layout and repeats as natives, containment); **RE-5.5 done 2026-11-21** (grouping, validation and each cluster's change test as natives; §4.10); **RE-5.6 done 2026-11-21** (the old pipeline deleted; layout natives unheld; the phase's exits, met and not) |
| RE-6 | Value slices | Moved to PT-5 (2026-11-20): evaluation at a point |

After 2c, on the rendering track: **render-cache phase 3** (vector retention for live
layers, with GPU upload bytes measured), which the user wants built (2026-11-18;
handoff-render.md §4). It reads `Changes`, so it can start any time after 2b.

**Phase 0, as done (2026-11-16).** Exit: both harnesses green on old-vs-old.

- `cargo test -p iron_document --features differential --target aarch64-apple-darwin`:
  420 tests, 0 failed. `Store` is `StoreCore` without the feature; with it, the facade in
  `src/differential/` (59 forwarded `&mut` methods; the snapshot and its
  `NOT_COMPARED` list in `snapshot.rs`; `intended.rs`, whose `INTENDED` is empty and
  whose `NOT_ATOMIC` has four entries).
- Three faults in the reference alone each failed it (commit `91f0eba` has the output):
  skipping `settle_repeats`, at `pending.derived_ops`; dropping an emit, at
  `pending.emitted` (the suite had no `on.emit` test; one was added); leaving a change
  undrained, as a different `drain_changes` return.
- Atomicity, measured with `IRON_ATOMICITY=1`: four failures, each shown by a test in
  `tests/atomicity.rs`. The handoff's three are confirmed; the fourth is new: a
  transaction the solver refuses leaves a pending change, since `apply_now` rolls back
  by rebuilding the graph.
- `tests/differential_random.rs`: 10,000 seeds green in 359 s (release, 10 threads),
  each of the 15 situations reached at least 376 times; 200 seeds in the normal suite.
  A seed with `std:plot` reaches about 380 nodes, since placing it brings 281.
- `document_bridge/host_reference.rs`: green over the smoke's operations and a script
  of value- and op-driven structure. It found a host bug, fixed: a deleted node's
  derived descendants kept their drawables, since no op names them.
- A12 holds (`tests/id_order.rs`): with the document's derived ids numbered in
  reverse in a second process, the saved text and the full view are equal.
- Baselines in §1. Two findings for the user: the 1,000-meter probe is truncated by
  `expand::NODE_BUDGET`, and A10's growth (about 530 bytes per value-keyed item, never
  freed).

**Phase 1, as done (2026-11-16).** Exit: no behaviour change, every reader moved.

- `Tree` (`src/tree.rs`): `Store::tree()` and `Tree::over(&doc)`, forwarding to the
  document. `Store::document()` is now `authored()`; the uses were sorted by hand
  between the two (crate `src`: 84 `tree()`, 21 `authored()`; tests: 605 and 250; the
  host, `iron_svg`, `iron_spatial` and `src-native`: 158 and 71). Read-only
  `&Document` parameters in the crate and the host became `Tree`.
- `Changes` (`src/changes.rs`) and `Store::take_changes()` / `changes_pending()`
  replace `take_derived_ops`, `drain_changes`, `has_changes`, `take_touched` and
  `take_geometry_changes`, and `collect_ids`' chaining of `expanded` and `inverse`.
  The store notes changes at its four mutation points: transactions, `apply_session`,
  `settle_repeats` outside a transaction, and the solver's writes in `post_apply`.
  Two departures from the spec's sketch, now written into its §12.2: parents are
  `Option<NodeId>` (`None` for a root), and a `geometry` field carries the spatial
  classification. The consumers table is in §12.2.
- The host keeps one `ChangeInbox` (`document_bridge/mod.rs`): `geometry` waits for
  the spatial scene, the rest for the reconciler, so neither takes what the other
  needs. `iron_spatial::Scene::update` takes its changes as an argument. The host
  reference's scan for dead derived drawables is gone: `Changes::removed` names a
  deleted node's derived descendants.
- The snapshot reads through `Tree` (task 5 came with task 2) and compares
  `take_changes()` with `==`, through a non-consuming `peek_changes`.
- Checks: `iron_document` 426 tests with and without `differential`; `iron_spatial`
  and `iron_svg` 41; the host reference 2; wasm and native builds. `git grep` finds
  none of the five drains and no `.document()` call. `smoke.txt` and `regression.txt`
  print the same normalised output as on `8fc772d` (built and run as the baseline);
  both scripts' `ready` times (1.85 to 2.54 s) overlap across the two builds. The
  regression's expected block had drifted before this phase (16 nodes in the first
  redraw, "model edit" labels on the model's links); SKILL.md is updated.

**Phase 2a, as done (2026-11-16).** Exit: no document clone in `run`; a refused
transaction leaves the document bit-identical.

- `Engine { Old, InPlace }` (`lib.rs`): a store applies `InPlace` unless the
  differential facade makes it the reference (`Old`, the clone). Scratch stores follow
  their parent (`StoreCore::scratch`). `run` returns a `Rollback` token that
  `commit` or `abort` consumes.
- The journal (`document.rs`): each mutator records the write it made, and rollback
  undoes the records in reverse on the raw fields. Under `differential`, `begin` keeps
  a shadow copy and `rollback` asserts the document equals it.
- `expansion_problems` are handed back by `run` (`None` when nothing expanded) and
  stored only once it succeeds, in both strategies: one `NOT_ATOMIC` entry gone, its
  test turned round. `attachment::validate` takes the points attached before.
  `NOT_ATOMIC` gained the out-of-document state a failure leaves (`machines.shared`,
  `machines.snap`) and a fuller solver entry. `INTENDED` is empty.
- Checks:
  - `iron_document`: 445 tests with and without `differential`, including the
    journal tests in `document.rs` and `tests/rollback.rs` (13 refusals, one per
    stage). Two planted faults in the journal were each caught.
  - The 10,000 random scripts, 296 s, with 11,750 failing transactions.
  - `iron_spatial` and `iron_svg`: 41 tests. The host reference: 2 tests.
  - The wasm build. `smoke.txt` and `regression.txt` print the same normalised
    output as at `bf7d13f`.
- Measured: §1, a constant edit at 1,000 meters is 1.57 ms, down from 12.5.
- Not built: `undo` and `redo` refusal cases in `tests/rollback.rs`. Those calls fail
  only if something outside history changed the document, so they have no ordinary
  route there. Their path through `run` is the one `replay` and `apply` test.

At the end of each phase: its exit in the spec's §17 is met and shown by a command
whose output is pasted into the commit or the roadmap row; this table and the
roadmap's Status table are updated; the next phase's task list is written here in the
form of §4, from the spec, before starting it.

## 3. Verification, in one place

- **The old engine is the reference until the swap** (spec §16.1). From phase 0 on,
  every test in `src/iron_document/tests` is also a differential script (§4.1, task 2).
- **Differences from the old engine are allowed only if listed** in
  `src/iron_document/src/differential/intended.rs`. The spec's §16.1 gives the list
  that is already agreed.
- **Behaviour the old engine does not have** (dormancy, parking, the scheduler) is
  tested against the engine itself by the consistency property (spec §8.8, §16.3). The
  kernel's `random_observation_is_consistent` is that test in miniature.
- **Before committing a behaviour change:** `cargo test -p iron_document --target
  aarch64-apple-darwin`, `bun test`, and the fast smoke (run-iron skill). Rust builds
  and tests need no check-in; other steps over a minute do (AGENTS.md).

## 4. The phases as tasks

### 4.1 Phase 0: baselines and the harness

Nothing in this phase changes behaviour. Its purpose is that every later phase has a
number to beat and a test that cannot be passed by a wrong engine.

**Task 1. Baselines.** Each is a native, ignored, release test beside
`tests/plot_perf.rs` and `tests/instance_cost.rs`, printing one line per measure.
Record the numbers in §1 of this file and in the "Today" column of the spec's §15.

| Measure | How |
|---|---|
| Memory per placed meter beyond the first | In `instance_cost.rs`, a counting `#[global_allocator]` for that test binary; (live bytes at 1,000 meters − live bytes at 100) / 900. The allocator counts every test in the binary, so run that test alone with `--test-threads=1`. The id registries (`id.rs:57`, `:116`) live for the process and are never freed; they are part of the number |
| Where a **constant** edit at 1,000 meters spends its 14.1 ms | The edit is `instance_cost.rs`'s constant edit. Temporary timers around each step of `apply_now`, `Store::run`, `after_transaction` and `post_apply` for it: the document clone, `expand::affected_by`, `attachment::validate`, `invalidate`, `resample`, `rebuild_clusters`, `post_apply`'s `validate_all` and `solve::clusters`, and `resolve_physics`'s parts (`lib.rs:1796`: layout, network, palette, contexts, `publish_selection`, `settle_repeats`, `publish_state`). Report the split; remove the timers |
| Solver cost with clusters at rest | A new `tests/solve_cost.rs`: 1, 10 and 100 pendulums, then the time of one gesture step dragging one ball, and of a transaction setting an unrelated constant. `pendulum()` in `tests/constraints.rs:70` is private and builds one in a fresh store: copy it, and place N of them in one composition, offset so they do not touch |
| Growth of interned id paths | A repeat **keyed by value** (`repeat.key`), its values moved through 10,000 distinct numbers; the path and key registries' lengths before and after. A plot's ticks are keyed by index today, so panning a plot does not show this. The accessor is a `#[doc(hidden)] pub fn` in `id.rs`: an integration test cannot see a `#[cfg(test)]` item |
| Loading the demo document | The demo is built by `document_bridge::demo::demo_scene` in the app crate. Apply it, save, and time `Store::load` of the saved text, as a test in the app crate: `cargo test -p ironfell --lib --features native --target aarch64-apple-darwin` |

Run the baselines without the `differential` feature.

**Task 2. The differential facade.** Make every existing test a differential script
without rewriting any test.

- Rename today's `Store` to `StoreCore` (`lib.rs:197`, and the `impl crate::Store`
  blocks at `parts.rs:561` and `standard.rs:170`). Without the feature,
  `pub type Store = StoreCore`. The crate's own functions that take `&Store` (about
  70, `geom::local_bounds` for one) take `&StoreCore`; callers outside keep
  compiling, since `&Store` derefs to it.
- Add a cargo feature `differential` to `iron_document`. With it on, the public
  `Store` is a facade holding two `StoreCore`s, `subject` and `reference`. It
  implements `Deref` to `subject` for every `&self` method, and **not** `DerefMut`:
  each public `&mut self` method is forwarded by one macro to both, and after each
  call the facade compares the two return values and the two snapshots (task 3) and
  panics with the method's name and the first difference. On 2026-11-16 there are 58:
  56 in `lib.rs`, `place_part` (`parts.rs:565`) and `place_standard`
  (`standard.rs:174`). Also forward the consuming `into_document(self)` and the
  constructors `new`, `with_document`, `load` and `Default`. Every by-value argument
  type is `Clone`.
- **Return values are compared with `==`, all of them, through phase 2a.** That
  includes `ApplyError` messages, `SolveReport` counts and what the drains return: old
  against old they are equal. A comparison is relaxed only by an entry in the
  intended-differences file (task 4), in the phase that first needs it.
- This works because the store's public API takes and returns plain data: timestamps
  are arguments, producers are registered by name, there are no callbacks, no public
  fields and no interior mutability, and the crate reads no wall clock and no random
  source. If you find a method for which that is false, stop and report (§5).
- Two things are shared by both stores, and must be: the id registries (`id.rs`),
  so one path gets one id in both; and the text engine (`iron_text::engine()`), so
  register fonts before building the stores and not between steps.
- The free functions in `timeline_view.rs:1187` that take `&mut Store` call only
  public methods and keep taking the facade.
- `Store::new` and `Store::load` build both stores. A store is built with an engine
  choice (`Engine::Old` or `Engine::New`), which until phase 2b has one value. So in
  this phase `subject` and `reference` are both the old engine, and a green run proves
  the facade, the snapshot and the determinism of the old engine. From 2b, `subject`
  is the new engine.
- Both engines live in `iron_document`, chosen at run time inside the store, so the
  two stores share every type. Do not make a second copy of the crate: its types would
  not be comparable.
- Exit: `cargo test -p iron_document --features differential --target
  aarch64-apple-darwin` is green, and each of three faults put into the reference
  alone, behind a debug flag, makes it fail: skipping one `settle_repeats` call;
  dropping one emitted effect; leaving one pending change undrained. Show the three
  failures, then remove the flag.

**Task 3. The snapshot.** `fn snapshot(&StoreCore) -> Snapshot`, in
`src/differential/` behind the feature, with `PartialEq` and a `diff` that names the
first difference. It is the definition of "the same", so it is written out here and in
the spec's §16.1; do not weaken it to make something pass. It reads without draining:
where a buffer has only a draining accessor, add a crate-internal one that looks.

| Compared | Detail |
|---|---|
| The saved document | `serial`'s saved text of each store. This covers authored nodes, tombstones and the facets they keep, counters and the version |
| Nodes | The set of live ids, authored, derived and session; each one's type, parent, order key and every facet, slot for slot |
| Relations | Every relation: id, kind, ends, data |
| Values | For every declared field of every facet of every live node, and every leaf that has an overlay, runtime or published entry without a stored slot (a preview may have none, `lib.rs:1152`): the value as a view reads it **and** the error, both. A failed leaf keeps a value to draw (`graph.rs:116`) |
| Signals | `signals()` and `signal_errors()`: which producer holds which leaf, and its errors |
| Problems | (node, kind, message) |
| The view | What a `ViewQuery` of the whole document prints at full fidelity with every instance, table and narrative expanded (`ViewQuery::expand`, `view.rs:41`) |
| History | The whole undo and redo stacks: each transaction's `ops`, `derived_ops` and `inverse` |
| Other store state | Everything else a public `&self` method answers: the gesture, clocks and time, query states, `tables`, `clusters()`, the contexts library, the selection, a pinned state. Go through the 65 public `&self` methods; each is in the snapshot or on a short list in the code saying why not |
| Pending | What each drain would return now (`take_derived_ops`, `drain_changes`, `take_touched`, `take_geometry_changes`), `take_emitted`, query requests issued, session cells made and dropped, revisions reported |

Not compared: `Transaction::expanded`; evaluation counts; heights; timing. Message
text and ops for derived structure **are** compared in phases 0 to 2a; they are
relaxed from 2b by entries in task 4's file.

**Two checks on a failed call, kept apart.**

- *Agreement:* after a call that returns an error, the two stores still have equal
  snapshots and equal errors. Always on.
- *Atomicity:* after a call that returns an error, a store's snapshot equals its
  snapshot before the call. **The old engine does not meet this**, so in this phase it
  is measured, not required: run it over the existing suite and the random scripts,
  and record every method and situation that fails it in task 4's file, each with a
  test that shows it. Two are known: `apply_session` rolls back by inverse ops, which
  leaves a tombstone and an advanced session counter (`lib.rs:1235`,
  `document.rs:502`); and a transaction rejected after it expanded leaves the new
  `expansion_problems` behind (`lib.rs:2663`). One was reported in review and is to be
  confirmed: an action that fails on a later write has already queued its `emit`
  (`lib.rs:1666`). Atomicity becomes required of the new engine call by call as the
  spec's §11 takes each over (transactions in 2a), and its entry is then deleted.

**Task 4. The differences file.** `src/iron_document/src/differential/intended.rs`,
behind the feature, since the facade in `src` reads it. Two tables. *Intended
differences:* each entry has a name, the predicate that narrows it (which nodes,
leaves or return values, in which situation), and the name of the test that asserts
the new behaviour. Empty in this phase; the agreed entries, added in the phase that
first needs each, are the spec's §16.1 list, and any other is a stop-and-report.
*Known not atomic:* the old engine's failures of the atomicity check above, found in
this phase.

**Task 5. Random scripts.** `tests/differential_random.rs`: a seeded generator of
documents and of steps over them, run through the facade. It must reach, and count
that it reached, each of the situations the spec's §16.1 lists under "Scripts"
(repeats in instances and instances in repeats, conditionals toggling, keys changing
kind, overrides on items that leave and return, enter and exit in flight, providers
moved, connections to instance ports, definition edits, failing transactions, undo and
redo, previews, publishes, clock moves). Assert the counts at the end, as the kernel's
random test does: agreement over scripts that never reach the hard paths proves
nothing. Build documents from the pieces the existing tests build (the helpers in
`tests/repeat.rs`, `compose.rs`, `machines.rs`, `constraints.rs`, `standard.rs`),
combined and nested at random, not from random ops on an empty document: almost all
of those are rejected. Keep a document under about 200 nodes and a script to about 30
steps. The 10,000-seed run is a release, ignored test; 200 seeds run in the normal
suite. If the full run takes more than ten minutes, report the time; do not thin the
snapshot. Exit: 10,000 seeds green, with the counts printed.

**Task 6. The host reference.** In the app crate: a headless Bevy `World` (no window,
no GPU) with the document bridge's systems, a function that rebuilds the host's
derived state from scratch (drawables, paint order, the spatial scene), and a test
that after each step of a script compares the incrementally kept state with the
rebuilt one. `materialize::reconcile` takes `Commands` and mesh assets and is
`pub(super)` (`materialize.rs:352`), so this needs a `World` and is a `#[cfg(test)]`
module inside `document_bridge`, run with `cargo test -p ironfell --lib --features
native --target aarch64-apple-darwin`. Compare by document `NodeId`, geometry and
content, never by Bevy entity or asset handle. The browser smoke cannot run here:
"the fast smoke's document" means the demo scene and the same document operations the
smoke performs, written as store calls. Exit: green over that and a few written
scripts.

**Task 7. Order across processes** (spec §14, A12). A derived id's number is its
slot in a process-wide registry, in order of first use (`id.rs:255`), and ids order
by that number. The facade's two stores share the registry, so it cannot show whether
anything visible depends on that order. One test, run as two child processes of the
test binary: each builds the same document with instances and repeats, the second
after first interning a few hundred unrelated paths; both print the saved text and
the full view, which must be equal. If they differ, that is a finding to report, not
to fix in this phase.

**Phase 0 is done when** tasks 1 to 7 meet their exits, the numbers are recorded, and
this file's §2 and the roadmap say so.

### 4.2 Phase 1: the read contract, on the old engine

The point: after this phase no code outside the engine assumes a derived node is
stored in the `Document`, while the old engine still stores them there. So when
phase 2c takes the copies out, nothing else changes. **No behaviour changes in this
phase.** The spec's §12 is the contract.

**Task 1. `Tree`.** `Store::tree()` returns a read view with `Document`'s read methods
under the same names. Its inventory is every public `&self` method of `Document`
that any caller uses, not only those the spec's §12.1 names: roots, enumeration of
live nodes, descendants and ancestry helpers, `nodes_with`, relation lookup. In this
phase it forwards to the document. It is a type of its own, not `&Document`, so later
phases can change what is behind it. It must be buildable from a bare `&Document`
inside the crate (`Tree::over(&doc)`), not only from a `&Store`: `Store::run`
validates against a working document that is not the store's, and store methods read
the tree while they hold `&mut` to other fields.

**Task 2. Rename `Store::document()` to `Store::authored()`**, so every use is looked
at. On 2026-11-15 there are about 286 uses in the host, 82 inside the crate's `src`,
and 733 in its tests. For each: if the code could meet a derived id (it walks
children, reads a node found by picking, selection, a relation's end, a path id), it
takes the `Tree`; if it only ever reads authored nodes (saving, the layers panel,
minting ids), it keeps `authored()`. When unsure, use the `Tree`. Tests mostly use
the `Tree`.

**Task 3. The crate's own readers.** Functions that take `&Document` and can meet
derived nodes take a `Tree`: the spec's §12.1 names them (`layout::compute`,
`constrained`, `port::network`, the machine player, steps, queries, the palette,
`publish_selection`, `Store::selection`, `solve::clusters`, `check_created`,
`translate` and handles, `view`, `analysis`).

**Task 4. `take_changes()`.** `Changes` as in the spec's §12.2, filled by the old
engine from what it already knows (`drain_changes`, `take_geometry_changes`,
`take_touched`, the transaction's ops, `expanded` and `inverse`). First write, as a
table in the spec's §12.2, every consumer each field feeds (the spec asks for this);
then move `document_bridge::sync_document` and `collect_ids` onto it; then delete
`take_derived_ops`, `drain_changes`, `has_changes`, `take_touched` and
`take_geometry_changes`. Tests that call them (about 45 places) move to
`take_changes()`. The facade compares what `take_changes()` returns with `==` in this
phase. Today geometry and reconciliation keep separate pending sets on purpose
(`graph.rs:110`); `Changes` must not let one consumer take what another still needs.

**Task 5. The snapshot reads through the `Tree`.**

**Exit:** the differential and host harnesses are green; the fast smoke's printed
output is the same as before the phase, timings aside; `git grep` finds no caller of
the deleted drains and no call of `Store::document` (other types' `document()`
methods are not this); timings in the smoke are within noise.

### 4.3 Phase 2a: transactions in place, on the old engine

The point: `Store::run` stops cloning the document. That clone and dropping the
document it replaces are 10.5 of the 12.5 ms of a constant edit at 1,000 meters
(§1). Ops apply to the store's own document, and a failure rolls back a **journal**
that `Document`'s mutators write (the spec's §11.1, assumption A1). Nothing else in
the old engine changes, apart from one fix shared by both strategies (task 3).
Validation, expansion, `post_apply`, `apply_session` and the graph rebuild after a
failure stay as they are.

Written 2026-11-16 from the spec's §11 and §17 and the code at `bf7d13f`. Revised the
same day after a council review (Claude Opus 5.5 and GPT-6.1 Sol; round 1: 6 and
6/10, round 2: 8 and 9/10, round 3: 8 and 10/10; round 4, Opus only: 9/10, its two wording points applied). A1 was checked by both: no writer bypasses the mutators. `Document`'s tables
are private, `facet_mut` has one caller (`op.rs` `set`), `expand` and `settle_repeats`
write through `apply_op`, and load and `set_scope` never run inside a transaction.

**What the harness compares.** The spec's §16.1 keeps the old engine as the reference
until the swap. So the reference keeps today's clone-based `run` and the subject runs
in place. Otherwise the facade would compare the journal with itself. **2a has no
intended differences**: `INTENDED` stays empty and the facade stays exact, because
every behaviour this phase changes is changed for both strategies (task 3).

The harness has no way to relax a comparison yet. `Store::call` diffs whole snapshots
and never reads `INTENDED`, and a difference such as a reference-only tombstone would
last past the call that caused it. Building that mechanism is not 2a's work.

**Task 1. Two transaction strategies.**
- `Engine` moves out of `differential` (the store needs it without the feature) and
  gains `InPlace` beside `Old`. `Old` stays as it is at phase 1's end, with the clone
  in `run`.
- `StoreCore` holds which one it runs; only `run` and its callers (`apply_now`,
  `undo`, `redo`, `replay`) look at it. The facade sets it with `set_engine`, which
  `debug_assert!`s that no journal is open; `new`, `with_document` and `load` default
  to `InPlace`. `engine()` and `set_engine` are available without the feature. Scratch stores built
  from a store (`edit.rs`, `compose.rs`, and the two in `parts.rs` that call
  `StoreCore::with_document(store.authored().clone())`) take the parent's engine
  (`set_engine(store.engine())`), so the reference's scratch work stays `Old`. Without the `differential` feature, `Store`
  runs `InPlace`. With it, the facade builds the subject `InPlace` and the reference
  `Old`; `Store::pair`'s assertion that both are `Old` changes to match.
- `Old` is the reference engine, not a compatibility path. It goes with the rest of
  the old engine at the swap.
- `run` returns a rollback token in place of the working document:
  `#[must_use] struct Rollback(Option<Undo>)` with `enum Undo { Clone(Document),
  Journal }`, consumed exactly once by `commit(token)` or `abort(token)`, which
  `take()` the `Undo`. Its `Drop` `debug_assert!`s `self.0.is_none() ||
  std::thread::panicking()`. The `Option` lets `abort` move the document out, and the
  `panicking` test keeps a panic between `run` and `commit` from becoming a
  double panic that aborts the test binary.
  - **`Old`:** `run` swaps the working copy in and keeps the previous document in the
    token. `abort` puts it back; `commit` drops it.
  - **`InPlace`:** `abort` rolls the journal back and closes it; `commit` closes it.

  The callers then read the same for both.

**Task 2. The journal, in `Document`.** One transaction at a time, no nesting:
- **API:** `begin()` (`debug_assert!` that none is open), `rollback()` (restore and
  close) and `commit()` (close). With no journal open, nothing is recorded: load,
  `from_parts`, and `settle_repeats` outside a transaction.
- **Records are at the level of the write, made only when the write changed
  something.** For a set insert or remove, record only when it returned `true`; an
  `insert_facet` on a node already in `by_facet` changes nothing. They are:
  - `nodes[id]`, each `facets[kind][id]` and `relations[id]` as they were, or that
    each was absent;
  - each counter that moved (`next_ids` and `next_rels` per client, with whether the
    client had an entry; the two session counters) and `version`;
  - **every map key a write creates**: `facets[kind]` (outer), `by_type[ty]`,
    `by_facet[kind]`, `by_source[n]`, `by_target[n]`, `children[id]` (made by
    `insert_node` and `restore_node`; a tombstoned node loaded from a file has none);
  - for the indexes, the single change: an id into or out of a set, or into or out
    of a child list (`children`, `roots`) at its position.
- **Rollback restores raw fields in reverse order.** It does not call the ordinary
  mutators and does not rebuild indexes.
- **`restore_node` clears a tombstoned node's facets directly**; each facet it takes
  is recorded.
- **`facet_mut` records the facet before handing out `&mut`** (A1).
- `set_scope` and `set_client` `debug_assert!` that no journal is open.

Why records and not snapshots of whole index entries: `by_type` and `by_facet` sets
hold tens of thousands of ids at 1,000 meters, and cloning them per transaction would
bring back part of the cost this phase removes.

- Derive `PartialEq` for `Document`, so tests compare every field, indexes included.
  The journal and the shadow clone below are `Document` fields (the mutators write
  the journal) kept outside that comparison and outside `Clone`: a wrapper whose
  `PartialEq` is always true and whose `Clone` is empty. The shadow is boxed
  (`Option<Box<Document>>`, since it is a `Document` inside one), and the wrapper's
  `Debug` prints a short summary rather than a second document.
- **Under the `differential` feature, `begin` keeps a shadow clone** and `rollback`
  asserts `==` against it, and `check_invariants()`. Every failing call in the suite
  and in the 10,000 seeds then checks rollback at the level of the maps, which the
  snapshot cannot see.
- **Tests (in `document.rs`):**
  - random sequences of mutator calls (insert, restore, tombstone, reparent, reorder,
    facet insert, remove and `facet_mut`, relation insert and remove, counter
    advances, version), then rollback, then `==` with a clone taken at `begin`;
  - deterministic cases: a facet kind's first insertion; a facet table that exists
    but is empty; restoring a tombstoned node with no `children` entry; one node
    touched several times.

**Task 3. `run` in place.** For `InPlace`, following the spec's §11.2 steps 1 to 3 on
the old engine:
- Translate and `machine::record` as now, before `begin`. They can refuse, and then no
  journal is open.
- `begin`, then apply the ops to `self.doc` with the same per-op error collection,
  including `expand::refuse`.
- `expand::expand` runs as now, between the ops and the checks. Its writes go through
  `apply_op`, so they are journaled. The checks read `Tree::over(&self.doc)`.
- On any error, `abort` and return the errors. On success, `set_version(stamp)`
  (journaled); the token goes back to the caller.
- `apply_now`: `after_transaction` (which can `settle_repeats`) and `post_apply`'s
  solver writes are journaled too. On `post_apply`'s `Err`, `abort`, then rebuild the
  graph, resample and rebuild the clusters, as now. On success, `commit` after history
  is pushed.
- `undo`, `redo` and `replay` commit once `run` succeeds.
- **The shared fix: `expansion_problems`.**
  - Today `run` stores them before its checks (`lib.rs` `run`), so a refused
    transaction keeps them (`NOT_ATOMIC`).
  - In both strategies, `run` returns them as `Option<Vec<Problem>>`, `None` when
    `expand` did not run (today it stores them only inside the `affected_by`
    branch). The caller stores a `Some` as soon as `run` returns `Ok`, before
    `after_transaction`; a `None` leaves the old problems in place. `settle_repeats` there can replace
    them, as it does now, so the last expansion still wins.
  - A refusal by `run` leaves them as they were, in both. A `post_apply` refusal
    behaves as today, under the solver's `NOT_ATOMIC` entry.
  - This removes the second `NOT_ATOMIC` entry. `tests/atomicity.rs`'s test for it
    becomes a test that the call is atomic. The spec's §16.1 bullet that lists this
    as an intended difference is updated: it is fixed in both engines in 2a.

**Task 4. `attachment::validate` without the previous document.** Its first loop needs
the points that had an `Attachment` before the transaction. Both strategies collect
`nodes_with(FacetKind::Attachment)` (live, not derived, in id order) before applying,
and pass that list: `validate(before: &[NodeId], doc: Tree)`. The cost is the same
order as the second loop, which already scans every attachment. The order, and so
the first error reported, is the same as today's.

**`apply_session` stays as it is in 2a.** Its rollback by inverse ops leaves more than
`NOT_ATOMIC` says (Sol): besides a created node's tombstone and the session counter, a
failing op's own writes stay, since `reparent`, `set` and `add_facet` write before
their checks and return no inverse. Making it atomic would change the reference, or
need the harness mechanism above. It moves to 2b, where the spec's §11.3 takes
`apply_session` over. Here its `NOT_ATOMIC` text gains the partial-write case.

**Store state outside the document.** These change before a failure, identically in
both strategies, so the facade cannot see them and 2a does not fix them. Each gets a
`NOT_ATOMIC` entry, text only:
- `machines.shared`, incremented in `run` before any check;
- `machines.snap`, extended by `undo` and `redo` before `run`;
- the solver refusal's entry is broadened. `after_transaction` has already cleared
  the overlay, noted geometry, settled repeats (`repeats`, `presence`,
  `repeat_inputs`, `truncated`, the published items, and the `expansion_problems`
  from `run` and from `settle_repeats`) and stamped signals.

**Task 5. The exit test: a failing transaction leaves the document bit-identical.**
`tests/rollback.rs`, on a `StoreCore` running `InPlace` (not the facade). Each case
compares `authored()` with `==`, the saved text before and after, and
`check_invariants()`; then applies a successful transaction and checks that it mints
the same ids as on a store that never saw the failure, and that no journal is open.
`differential_random.rs`'s failing transactions do not reach expansion or anything
after it, so these cases need their own builders:
- refusals before the journal opens (translate, `machine::record`);
- an op error mid-list:
  - including `expand::refuse`;
  - the three ops that write and then fail a check (`reparent`, `set`, `add_facet`);
  - an op that writes other nodes before a later op fails (`new_input_args` when a
    `port_spec` is created, `catch_up_args`);
- `check_created`, `check_scope` and `check_context` refusals, and an attachment
  refusal;
- a refusal after expansion created and deleted derived nodes;
- `post_apply` refusals:
  - before any solver write (a malformed or duplicate constraint, a cycle through a
    cluster);
  - an unsolvable cluster after an earlier cluster wrote variables;
  - an exact request unmet after solver writes;
  - a refusal after `settle_repeats` re-expanded in `after_transaction` (change a
    repeat's count, then fail the solver);
- for `undo` (including undoing a delete, the `restore_node` path), `redo` and
  `replay`, the stages before and inside `run` above that each can reach. For example,
  undo never meets `expand::refuse` (its inverse holds no derived ids), and undoing
  the top entry fails only if something outside history changed the document. They never call `post_apply`
  (the spec's §11.3), so the solver refusals are tested on `apply` only;
- one node touched several times in a failing transaction.

Also tests that `expansion_problems` follow the last expansion on a successful
transaction that grows or shrinks a repeat, and that a transaction that does not
expand leaves them as they were (build the depth nest of `tests/atomicity.rs`, then
apply an unrelated `Set`).

**Task 6. Measure.** Re-run §1's constant edit at 1,000 meters (`instance_cost.rs`,
release, **without** the `differential` feature, whose shadow clone would put the
clone back) and the profile split, with the same temporary timers, removed afterwards.
Record the new number in §1 and the roadmap row. The expectation is that the clone
and the drop (10.5 ms) go and the journal costs well under 1 ms. Report it if not.

**Exit:** the spec's §17 for 2a, plus this phase's own:
- `git grep -n "doc.clone()" src/iron_document/src/lib.rs` shows a clone only on the
  `Old` path;
- task 5 and the `document.rs` journal tests are green;
- the differential suite (subject `InPlace`, reference `Old`, the shadow-clone
  assertion on) and the 10,000-seed run are green, with `INTENDED` empty;
- the host reference passes; `smoke.txt` and `regression.txt` print the same as at
  `bf7d13f`;
- the constant edit at 1,000 meters is measured and recorded.

**Stop and report** if:
- a write to `Document` is found that does not go through a mutator (A1 fails). The
  spec's fallback is then the copy-on-write document of §11.3, and choosing it is the
  user's decision;
- the journal's measured cost is more than a third of what the clone cost;
- a difference between the strategies appears that this section does not explain.

### 4.4 Phase 2b: the new graph, copies still stored

The point: the plot fix. A change to a repeat's items, or any structural transaction,
stops re-expanding the document and rebuilding the graph, and costs the owners and
cells it reaches (the spec's G1). The new engine replaces `graph.rs`, `expand::expand`
and `settle_repeats`. Derived nodes are still written into the document, so every
reader and the comparison with the old engine stay exact (the spec's §17).

Written 2026-11-16 from the spec (§5–§7, §11, §14, §15, §16) and the code at
`d9ec4ea`. Revised after a council review: Claude Opus 5.5 alone, since GPT Sol was
unavailable, with the user's agreement. Rounds 1 to 4 scored it 6, 7.5, 8.5 and
8.5/10; round 4's last points are applied.

**The new engine need not copy the old one's quirks** (the user, 2026-11-16). Where
the old behaviour is a quirk, the new engine does the reasonable, expected thing.
- **A quirk the old engine can drop cheaply is fixed in both** (stage 1). The
  comparison then stays exact, and the app, which runs the old graph until stage 5,
  gets the better behaviour sooner.
- **Any other case becomes a recorded intended difference:** an `INTENDED` entry
  (stage 1) with a test of the new behaviour, relaxed for exactly that case and
  listed at the end of its stage. It is no longer a stop-and-report.
- **Stop and ask** only when it is not clear which behaviour is right, or when the
  change alters something a user can see and may rely on.

**Two moves from 2b to 2c** (the review's findings 11 and 12, 2026-11-16). The spec's
§17 rows say so.
- **Validation from templates** (§11.1, A2, A3). While copies are stored, the checks
  see them in the document. In 2b, validating from templates would only produce
  §16.1's more permissive item-path rule, and it carries A3's risk. In 2c, 2b's
  validation over stored copies is an exact reference for it.
- **Cells evaluate from templates.** In 2b a derived cell evaluates the stored copy's
  expression, borrowed rather than cloned. Template-relative evaluation (`Ref`s
  resolved per owner) moves to 2c, where 2b is its reference. This removes a source
  of differences at no measurable cost: a copy's expression is already rewritten
  once, when it is written.

**Stages.** Five, each ending green on the differential suite, committed and pushed.

**Progress.**
- **Stage 1 done (2026-11-17).**
  - Signal stamps are deleted; `Changes`, expansion problems and `truncated` are
    sorted; a changed error message counts as a change.
  - The stale-derivation cases re-derive (`tests/rederive.rs`; each test fails
    without its fix).
  - Every `NOT_ATOMIC` entry is fixed, so the table is empty. The restore after a
    solver refusal applies only when the document holds a constraint.
  - The harness runs intended differences: normalisers and divergence tests, with
    counts (`tests/differential_harness.rs`).
  - Checks: `iron_document` 453 tests; the differential suite 457, with
    `IRON_ATOMICITY=1` reporting nothing; the host reference; `iron_spatial` and
    `iron_svg`; the wasm build. `smoke.txt` and `regression.txt` match a build of
    `HEAD` without stage 1.
  - Measured: the solver baseline's unrelated constant edit with 100 pendulums is
    6.2 ms (phase 0: 5.77; before 2a), within the threshold. The meter numbers were
    noisy (load average about 6 from an unrelated process): re-measure at stage 2.
- **Stage 2 done (2026-11-17).**
  - `src/engine/owners.rs` and `expand::expand_owners`: each op expands again only
    the owners it can change. The existing want functions run per owner, and the
    shared diff (`apply_wanted`) applies the result, so the copies are the same
    code's output. An owner's holdings are read from the document (derived
    descendants, and derived relations by `RelationId::root`). The definition index
    only grows. Reparents, and definitions or the library made or removed, fall
    back to every owner.
  - `Engine::New`, chosen at construction (`with_engine`, `load_with`;
    `IRON_ENGINE=new`; the `engine-new` feature in `iron_document` and the app;
    `build.sh`'s `EXTRA_FEATURES`). The facade pairs `New` with `Old`.
  - Shared fixes found on the way, in both engines:
    - a failed expansion op is retried once the rest of its pass has applied. The
      old engine relied on its next whole-document pass to repair an op that read
      a copy made later in the pass;
    - `reparent`, `set` and `add_facet` undo their own write when their check
      fails. An expansion op that failed used to leave a half-applied facet.
  - **For the user's approval: the node budget.** Past 50,000 derived nodes, the
    cut now falls per owner in id order, not in the old whole-document walk's
    order. At 1,000 meters this keeps 50,000 derived nodes where the old engine
    kept 50,506. Nothing changes below the budget.
  - Checks:
    - the differential suite, 457 tests, with `IRON_OWNER_CHECK=1` (the whole
      expansion run after every owner update finds nothing to do) and
      `IRON_ATOMICITY=1`;
    - the plain suite, 453 tests; the 10,000 random scripts (318 s); the host
      reference;
    - `smoke.txt` and `regression.txt` identical to `HEAD`'s on both `InPlace` and
      an `engine-new` app build.
  - Measured (load average about 15, so the ratio is what counts): at 1,000
    meters a structural edit goes from 134 to 38 ms and a placement from 85 to
    24 ms; at 100 meters, 15.2 to 4.4 ms. What is left is mostly the old graph's
    rebuild, which stage 3a removes.

- **Council review of stages 1 and 2 (2026-11-17):** Claude Opus 5.5 6.5/10, GPT Sol
  6/10. Fixed, each with a test in `tests/owners.rs` or `tests/atomicity.rs` (three
  of the owner tests fail on `b104047`):
  - deleting a container now re-expands the pictures of the entities it took;
  - a `use` in a repeat's template that pictures an outside entity follows it;
  - an unlinked derived relation is derived again by its owner;
  - an expansion op that failed is retried while a pass makes progress, not once;
  - a definition at the document root is indexed like one in a library;
  - owners whose expansion failed in part are expanded again on every update;
  - a `use` picturing another owner's derived entity is re-expanded in a second
    wave (`Owners::readers`);
  - past the node budget the update falls back to a full expansion in id order,
    so the cut does not depend on edit history;
  - `fire_at` writes session state and queues events only once its transaction is
    accepted;
  - the unused `StoreCore::set_engine` is gone.
- **Stage 3a, as built: the old graph updated in place, not a new cell engine.**
  Under `New`, a structural op no longer rebuilds the graph: `Reactive::restructure`
  re-reads only the bindings of the nodes the ops touched (with the full inverse, so
  a cascade's nodes are seen), compares the signals connections and anchors derive
  with what they were, raises heights locally (§6.3) and evaluates what changed and
  what reads it. `@name` readers are resolved again only when an op can move a
  provider. `settle_repeats` and `apply_session` take the same path. This departs
  from this section's plan (dense cell arrays ported from the kernel). The reason:
  the old graph's rebuild is what 2b's plot targets need gone, and an in-place update
  keeps every evaluation rule (tracked reads, cycles, boundary checks, messages)
  identical by construction. The dense engine stays the plan for where measurement
  shows the old graph's per-read cost matters, and for 2c, where cells evaluate from
  templates.
  - Measured (`plot_perf.rs`, native release, load about 2): a step that changes the
    tick count, `InPlace` against `New`:

    | | pan, 1 plot | zoom, 1 plot | pan, 10 plots | zoom, 10 plots |
    |---|---|---|---|---|
    | `InPlace` | 1.34 ms | 1.42 ms | 8.62 ms | 9.02 ms |
    | `New` | 0.41 ms | 0.60 ms | 0.57 ms | 0.82 ms |
    | values only (`New`) | 0.10 ms | 0.32 ms | 0.16 ms | 0.41 ms |

    The 2× target holds for zoom and not yet for pan (3.9× and 3.6×).
- **Stage 3b, first step: a settle round reconciles only the items that changed.**
  The profile of a tick change under `New` put most of it in expanding the whole
  owner around the repeat (the plot's picture, about 70 nodes) again.
  `Owners::expand_items` (with `expand::expand_items`) wants only the changed repeats'
  items, with the scopes nested in them, and diffs them against the item subtrees the
  repeat holds. A derived repeat's template is then read from the copies its owner's
  last expansion wrote. It is taken only where it matches a whole expansion: the owner
  has no problems recorded, no other owner reads it, and wanting the items meets no
  budget or nesting problem. Otherwise the owner expands whole, as before; an op that
  fails to apply is repaired that way in the same call. Under `IRON_OWNER_CHECK=1` the
  whole expansion is compared after every update: green over the differential suite
  (462) and the 10,000 random scripts (274 s).

  | tick change, `New` | pan, 1 plot | zoom, 1 plot | pan, 10 plots | zoom, 10 plots |
  |---|---|---|---|---|
  | owner expanded whole | 0.41 ms | 0.60 ms | 0.57 ms | 0.82 ms |
  | items only | 0.19 ms | 0.38 ms | 0.36 ms | 0.60 ms |
  | values only | 0.11 ms | 0.32 ms | 0.17 ms | 0.42 ms |

  Within 2× of values-only except pan at 10 plots (2.2×). What is left there is
  `settle_repeats` polling every drawn repeat each round (reading its inputs and
  rebuilding every item's published values), the per-Collection Structure cells of
  this stage's plan, and `solve_network` (phase 5).
- **Stage 3b, second step, and the plot targets (2026-11-17).** Three changes under
  `New`, each exact by construction and green under the harness:
  - `settle_repeats` keeps each repeat's settled items (keys, published values,
    problems) against its inputs, for repeats with no enter or exit timing, and
    publishes the items again only when one changed;
  - the across/through network (`port::network`, a whole-document scan) is cached
    on a count of document writes (`Document::edits`);
  - `restructure` recomputes what connections and anchors derive only when an op
    touches a port, an anchor, a connection, a relation or a transform's facet;
  - and a binding's expression is shared (`Arc`), not cloned per evaluation.

  Native (`plot_perf.rs`), a tick-count step against a values-only step:

  | | pan, 1 plot | zoom, 1 plot | pan, 10 plots | zoom, 10 plots |
  |---|---|---|---|---|
  | tick change | 0.20 ms | 0.35 ms | 0.33 ms | 0.51 ms |
  | values only | 0.11 ms | 0.29 ms | 0.12 ms | 0.33 ms |
  | ratio | 1.8× | 1.2× | 2.7× | 1.5× |

  Values-only steps no longer grow with the document (0.12 ms at 1 and at 10 plots;
  `InPlace` 0.11 and 0.16). In the app (wasm-bench, `plot-perf.txt`, `engine-new`
  against `InPlace` the same day): `action` p95 slow pan 6.85 → **1.45 ms**, fast pan
  5.74 → 1.29 ms, wheel zoom 7.96 → **1.63 ms**; changed frame p95 12.78 → 7.58,
  10.32 → 7.10, 11.97 → 6.89 ms. Measured before the second step's network cache.

  **The targets are met except pan at 10 plots natively (2.7× against 2×) and wheel
  zoom in the app (1.63 against 1.5 ms).** Where that time is (sampled, a 10-plot
  pan whose steps change the ticks): re-evaluating the plot's bindings is about half,
  and within it the plot's curve written as SVG path text with exact-precision float
  formatting and parsed back (`series::path_data`, `as_leaf_kind`) about a sixth of
  the step; whole-document `resolve_layout` about 8 %; the settle loop about 15 %. A
  zoom step natively is 0.29 ms of value work and 0.06 ms of structure. So what is
  left is the plot's own formulas, path text (the spec's §1.3 follow-up) and layout
  (phase 5), not the structure work 2b replaces. A frame-pacing note: the app's pan
  runs counted more rAF gaps over 24 ms on `New` (25 and 49 against 13 and 14) while
  every store and frame-time figure fell; to be re-checked at stage 5 on an idle
  machine.
  - Found on the way: a set on a port's `datatype` changes what its binding's
    boundary check accepts with no change of expression; a touched node's own
    bindings now always run again (`tests/rederive.rs`).

- **Council review of stages 3a and 3b (2026-11-17):** Claude Opus 5.5 6/10 (four
  findings confirmed by running), GPT-6.1 Sol 6/10. Fixed, with tests in
  `tests/restructure.rs` (each failed before its fix):
  - a cell that failed on a tracked-read cycle, or did not settle, is run again on
    every structural update (`Reactive::blocked`; the spec's §6.3), so a cycle that
    ends no longer leaves it failed;
  - a cell that tracks its reads starts again from none when `restructure` runs it,
    as the rebuild does, so a read that now closes a cycle is caught (found by the
    extended random scripts);
  - a binding's `tracks` is recomputed when its node is touched (a datatype set to
    `like(xs)` makes its check read the argument);
  - an edit inside a definition re-runs the argument bindings of every entity naming
    it (`types::declared` reads the definition's port_specs);
  - **shared fix, both engines:** a reorder of a `let` or colour token is structural
    and resolves `@name` again, since the first provider among siblings answers;
  - the item-level path keeps a running total of what owners hold, so two owners
    cannot spend the same room; it is not taken while any owner has a budget or
    expansion problem; a repeat nested in another changed repeat is expanded with it,
    not twice.

  The random scripts now also generate tracked reads that make and break cycles, a
  definition's port_spec changing type under a bound argument, sibling providers
  reordered, and nested repeats changing in one round: four more situations, each
  counted. Recorded, not fixed: heights only rise, so `New` can order a pass
  differently from a rebuild and spend its "did not settle" budget differently
  (`dirty × 4 + 1024` against `all × 4 + 1024`); no script has shown a difference.

- **Stage 5's other exit checks, run ahead of stage 4 (2026-11-17, `b717e13`),
  nothing swapped or deleted.** A dev-opt webgl2 app build with `engine-new` and one
  without, both from the same exported commit, each served on its own and driven by
  the same scripts: `smoke.txt`, `regression.txt`, `plot-view.txt`, `actions.txt`,
  `query.txt`, `fiber.txt` and `pulses.txt` all pass on `New`, with no page errors,
  and their output matches `InPlace`'s line for line except two sampled-in-time
  readings that differ from run to run on either engine (the pendulum's position in
  the sixth decimal, pulse phases a frame apart). The host reference
  (`IRON_ENGINE=new`, `engine-new`) and `bun test` (116) pass. With the differential
  suite and the 10,000 random scripts green, **the plot targets are the only stage 5
  exit not met** (above).

- **Stage 4, first part (2026-11-17).** `Transaction::expanded` is gone, in both
  engines: what expansion and settling write travels with the run (`Run::expanded`)
  to `after_transaction` and `Changes`, and history holds `ops`, `derived_ops` and
  `inverse` only, so the snapshot no longer strips it. Stage 4's exit already holds
  with stage 1's restore-by-copy: the differential suite (467) is green with
  `IRON_ATOMICITY=1` reporting nothing, and the plain suite (463, `rollback.rs`
  included) is green on `New`. Effects are already outside the trial: a query's
  request comes out of `advance` and `on.emit` out of `fire_at`, so a refused
  transaction releases neither (`tests/atomicity.rs`).
  **Not built: rollback by telling the engine the reverse changes, with held
  owners,** in place of `Saved`. It is a performance change only (`Saved` copies the
  graph and the owners' records on each transaction in a document with a
  constraint), and it needs decisions this engine's shape leaves open:
  - `Saved` also covers what is not the engine (clusters, geometry revisions,
    presence, tables, motion, contexts). Re-derived rather than copied, the
    geometry revisions would come back newer, which `Old`, the reference, does not
    do: a §16.1 entry, or the same change in `Old`.
  - "Held owners" assumes owners that keep cell state. Here an owner's state is
    the document's derived nodes, which the journal already restores, plus the
    records `store_found` applies only once a transaction is accepted. So it is
    unclear what holding would add.
  Measure the copy on a constraint-heavy document before building it.

- **The whole frame, re-measured (2026-11-17, `7af66ef`, wasm-bench webgl2, one
  700 × 420 plot, three runs per engine).** The frame-pacing note above is
  resolved: every rerun counts 13–14 rAF gaps over 24 ms per pan on both engines
  (none on zoom), so the earlier 25 and 49 were one-off runs, not the engine. Zoom,
  `New` (p50 per changed frame, ms): store `action` 1.29, spatial `bounds` 1.49,
  `display_list` 0.63, `encode` 0.37, `gesture_reconcile` 0.23, `render` 0.14,
  `pick` 0.06, unattributed about 2.0; worker frame 6.26 (`InPlace` 8.56, its
  `action` 4.70). `action` p95 across the runs: slow pan 1.53–1.62, fast pan
  1.10–1.22, zoom 1.68–1.81 (`InPlace` 6.3–6.8, 6.2, 5.3–6.8).

- **Playback, the Fourier page (2026-11-17, same build).** The Fourier tab shown,
  the timeline playing, a clip on its `t` slider: the worker frame is 9.3 ms p50 on
  either engine (`InPlace` 9.3–9.8; the engines differ by under 0.4 ms, since
  playback is values only). Removing clips one at a time:
  - the **Wave packet page's clip, on a hidden tab, costs about 2.6 ms a frame**.
    Its page is not drawn, but its plot is re-evaluated every frame and its curves
    written as path text. Natively (`playback_time` in `document_bridge/demo.rs`) a
    playing step is 3.6 ms with it and 0.39 ms without;
  - the Main tab's spin and slide clips cost about 0.3 ms;
  - the Fourier page's own formulas cost under 0.1 ms natively (its partial sum
    does not read `t`). Drawing its view costs about 2.9 ms (encode 1.5,
    display list 0.7, submit 0.3, reconcile and bounds 0.35), with both
    801-point curves re-encoded every frame although only the cursor, phasors
    and link move;
  - about 2 ms a frame is outside the named phases even when paused.
  Unseen pictures costing evaluation is phase 4's (a hidden tab's pure pictures
  cost 0 evaluations); 2b does not change it.

- **Four frame fixes, placed by the user (2026-11-17):**
  1. **Paths as geometry, before the stage 5 swap.** The helpers (`points`,
     `smooth`, …) build `Value::Geometry` directly instead of SVG text parsed back
     (plot.md §2.1). Shared by both engines, so the comparison stays exact. It is
     the largest piece of the plot-target shortfall and most of a hidden animated
     page's cost. Then the plot targets are measured again and the stop rule
     re-applied.
  2. **The fixed ~2 ms a frame, measured at stage 5** (its "a changed frame's
     time"), and fixed where it lands: phase 3 if it is the store's per-frame
     polling (`advance_*`, settle checks, steppers at rest), the rendering track if
     it is the host.
  3. **Unchanged paths re-encoded in a moving view: render-cache.md phase 3**
     (vector retention for live layers), after the swap, beside 2c. Measurement now
     asks for it: the Fourier view re-encodes two static 801-point curves each
     frame (~1.5 ms).
  4. **Hidden pages' evaluation: phase 4,** as planned; nothing partial before it.

- **Fix 1 built: paths as geometry (2026-11-17, `9ba2ed5`).** Green: `iron_document`
  464, differential 468, the app's 16; on an `engine-new` wasm-bench build, `smoke.txt`
  and `plot-view.txt` pass, and the rewritten gallery pages (Kepler, Interference,
  Oscillator, Fourier) draw as before. The plot targets again:

  | | `New` | `InPlace` | target |
  |---|---|---|---|
  | values-only step, native, pan / zoom, 1 plot | 0.065 / 0.122 ms | 0.041 / 0.121 | no slower than 0.13 (phase 0) |
  | values-only step, native, pan / zoom, 10 plots | 0.065 / 0.152 ms | 0.098 / 0.230 | |
  | tick change, native, pan / zoom, 1 plot | 0.166 / 0.190 ms (2.6× / 1.6×) | 0.90 / 1.01 | within 2× values-only, "so about 0.3 ms" |
  | tick change, native, pan / zoom, 10 plots | 0.257 / 0.322 ms (4.0× / 2.1×) | 7.58 / 7.66 | |
  | app `action` p95: slow pan, fast pan, zoom | **1.32, 1.05, 1.45 ms** | 5.26, 6.12, 6.39 | under 1.5 ms |

  - **The app target is met.**
  - **The native tick-change target is met as an absolute** (about 0.3 ms; 10-plot
    zoom 0.32), **not as a ratio:** values-only steps fell to 0.065 ms (from 0.11),
    so 2× is now 0.13 ms.
  - **Values-only steps:** under the phase 0 baseline at every size. At 1 plot,
    `New`'s pan is 0.024 ms slower than `InPlace`'s at the same commit (the settle
    poll); at 10 plots `New` is faster.

  Playback (Fourier tab, the timeline playing, both engines alike): frame 9.3 →
  7.2 ms; the hidden Wave packet page 2.6 → 0.7 ms; native store step 3.6 → 1.1 ms.
  Which reading of the tick-change target holds is the user's call before the swap.
  **The user's call (2026-11-17): the absolute, so the target is met; swap.**

- **Stage 5 done: the swap (2026-11-17, `9f4ac59`).** The exit, run before anything
  was deleted, on both engines where it compares them:
  - the 10,000 random differential scripts green (258 s), no intended entries;
  - the plot targets met (fix 1, above);
  - recorded, native release, `InPlace` → `New` (`tests/instance_cost.rs`, now at
    100, 700 and 1,000 meters; load about 14, so the ratios count):

    | | `InPlace` | `New` |
    |---|---|---|
    | structural edit, 700 meters | 116.5 ms | 5.6 ms |
    | placement, 700 meters | 56.5 ms | 3.4 ms |
    | structural edit, 1,000 meters (past the node budget) | 123 ms | 96 ms |
    | placement, 1,000 meters | 76 ms | 28 ms |
    | the demo's load | 150 ms | 78 ms |
    | memory per meter | 110.0 KiB | 110.5 KiB |

    Past the budget, `New` falls back to a whole expansion in id order on every
    update (the council's fix), which is why 1,000 meters gains little. The budget
    rule itself (the cut per owner in id order, 50,000 nodes kept where the old
    engine kept 50,506) is still the user's to approve.
  - **The changed frame's fixed cost (fix 2):** a paused, unchanged frame is about
    1.2 ms on the worker, and `?bevy=empty` (Bevy's default plugins, no Iron
    systems) is 1.17 of it. So it is the host's schedule, not the store's polling,
    and goes to the rendering track: [`handoff-render.md`](../handoff-render.md) §3,
    finding 8, with the per-schedule numbers. Phase 3 does not take it.

  Then deleted: `Engine` (`Old`, `InPlace`, `New`), `with_engine`, `load_with`, the
  `engine-new` feature and `IRON_ENGINE`, the clone rollback (`Rollback` is the
  journal's token), `regraph`'s whole rebuild, `Found::Problems`,
  `expand::affected_by`, and the whole-document expansion on the transaction and
  settle paths. **Kept, departing from the stage's list:** `graph.rs`, updated in
  place (stage 3a as built), and `settle_repeats` with its kept items (stage 3b as
  built), since they are the new engine; `expand::expand` and `wanted`, compiled
  only under `differential`, as `IRON_OWNER_CHECK`'s oracle. The facade pairs two
  stores of the one engine until 2c's engine is its subject.

  After the swap: `iron_document` 464; the differential suite 468 with
  `IRON_ATOMICITY=1 IRON_OWNER_CHECK=1`, nothing reported; the app's 16 (the host
  reference's 2 among them); `iron_spatial` and `iron_svg` 43; `bun test` 116; on a
  dev-opt webgl2 build, `smoke.txt`, `regression.txt`, `plot-view.txt`,
  `actions.txt`, `query.txt`, `fiber.txt` and `pulses.txt` pass with no page errors,
  their output as on `engine-new` but for the two sampled-in-time readings.
  The roadmap's "Plot pan and zoom performance" row is closed (2026-11-18).

**How the engines were chosen, until the swap.** (History: stage 5 deleted all of
this; one engine remains.)
- The engine is chosen **when a store is built**, not set afterwards:
  `StoreCore::with_engine(doc, engine)`. `New` builds differently from the others.
- `Old` is the clone and the old graph, the reference. `InPlace` is 2a's journal and
  the old graph, **frozen**: the app runs it until stage 5. `New` is whatever of the
  new engine has been built so far.
- Without the `differential` feature, `Store` builds `InPlace`. With it, the facade's
  subject is `New` and its reference `Old`.
- Two switches run `New` before stage 5, since `std::env` cannot be read on wasm:
  - `IRON_ENGINE=new`, read once, for `plot_perf.rs` and the native probes;
  - a cargo feature, `engine-new`, for a wasm-bench app build.
- `StoreCore::scratch` uses its parent's engine. Tests that read evaluation counts
  branch on `engine()` until stage 5.
- **"Frozen" means no new-engine work.** `InPlace` still takes stage 1's shared
  fixes, which change shared code.

**One type in front of both graphs.** `StoreCore`'s `reactive` becomes
`enum Graph { Old(graph::Reactive), New(engine::Engine) }`. Its methods are the read
surface the crate and host call on `Reactive` today, with the same meaning:
- `read`, `signal(s)`, `is_driven`, `driven_by`, `published_by`, `publish`, `retract`,
  `signal_errors`;
- `expr_of`, `is_bound`, `is_read`, `errors`, `contexts`, `transitive_deps`;
- `overlay`, `preview`, `unpreview`, `clear_overlay`, `mark`;
- the pending sets behind `take_changes`;
- what tests read today: `height`, and `evaluations` and `errors`, which become
  methods.

Callers do not branch.

**Differences the old engine would show, settled in both engines first.** A difference
the harness can predict, and that the old engine can drop cheaply, is removed by
changing shared code, as 2a did, rather than allowed for with a normaliser.

**Stage 1. Shared fixes, and intended differences the harness can run.**
- **Shared fixes, each with a test, in both engines:**
  - **Delete `Signal::stamp`.** It comes from one store-wide counter that every
    republish and `resample` bumps, so any change to how often things publish moves
    every later stamp. Its one reader is `engine_api.rs`'s diagnostic JSON. Nothing in
    `src-ui` reads it. Delete it there too.
  - **Sort `Changes`' lists** in `take_changes` and `peek_changes`: `created` parents
    first, `removed` deepest first, then by id; `moved` and `relations` by id. Today
    they are in op order, which follows how expansion happened to walk the document.
  - **Sort `Changes`' tie-breaks fully:**
    - `created` by depth, then by id;
    - `moved` and `relations` by the whole tuple;
    - `removed` deepest first, by the depth each node had where it was held when the
      change was noted, then by id.
  - **Sort `expansion_problems` and `truncated`** by (node, kind, message).
    `problems()` lists them before binding errors, and `problem_notes` keeps the
    first three per node, so the order can be seen.
  - **Treat a changed error message as a change.** Today `graph.rs` replaces a
    failing leaf's message without telling its readers (`errors.insert(..).is_none()`).
    So a reader keeps "depends on X, which failed: <the old reason>" until the next
    `rebuild`, which the new engine never does.
  - **Re-derive where the old engine leaves derived state stale** until its next
    whole-document pass. The new engine works per op, so it would be right early and
    differ. The known cases:
    - **any** `Set`, constant or expression, on an authored repeat's template child
      outside the library: `affected_by` is false, and `settle_repeats` re-expands
      only when counts change;
    - a set on a port's `kind` or `dir`, or on an anchor's `follow` or `port`: each
      is read by `derived_signals` or `anchor::derived`, but `after_transaction`
      treats a set on it as non-structural;
    - a set on a port's `datatype`, or on an argument a boundary check reads:
      `types::declared` and `ty.check` read them during evaluation without a
      dependency. They become dependencies of the boundary binding;
    - ~~`apply_session`, which never expands.~~ **Not a case** (found building
      stage 1): a derived id cannot sit under a session-tier id
      (`NodeId::from_path`), so session nodes derive nothing, and session ops touch
      only session nodes. `tests/rederive.rs` `a_session_use_derives_nothing`.

    Each becomes structural (or expanding, or a dependency) in shared code. Any
    further case found later is handled the same way and recorded.
  - **Each `NOT_ATOMIC` entry is fixed,** with its test turned round. None of these
    depends on the new engine:
    - `apply_session` rolls back by the journal;
    - `machines.shared` and `machines.snap` change only on success: once
      `post_apply` has accepted the transaction (counted at commit, not in `run`),
      and for undo and redo once `run` has;
    - an action's `emit` is queued only once its writes succeed;
    - **after a `post_apply` refusal, under `Old` and `InPlace`,** the state that
      `after_transaction`, `post_apply` and the pipeline changed is put back, with no
      rebuild. It is copied before `apply_now` stores `run`'s expansion problems,
      which happens just before `after_transaction`. The state is:
      - the whole `Reactive` (the graph's structure, its resolved values, errors,
        overlay, signals and pending sets);
      - `clusters`, and the geometry revisions with their pending structure and
        leaves;
      - `repeats`, `presence`, `repeat_inputs`, `truncated`, `expansion_problems`,
        `session_state`, `tables`, `motion`, `contexts`, `time_errors`,
        `physics_error`, `signal_stamp` (until it goes), and the pending `Changes`.

      `InPlace` takes the same restore (it is the same code in `apply_now`), so the
      plain suite passes the turned-round test and the app is atomic now.
      - **The copy is taken only when `post_apply` can refuse:** when the document
        holds a constraint. Without one, `post_apply` accepts every transaction.
      - Re-measure §1's constant edit at 1,000 meters on `InPlace` (meters hold no
        constraint), and the solver baseline's unrelated constant edit with 1 and
        100 pendulums, which do pay for the copy.
      - Stop and report if the copy adds more than half to either.

    Then `NOT_ATOMIC` is empty and the reference is atomic. **Until stage 4, `New`
    restores the same state the same way:** by copy, with its owners by their undo
    log and its cells by copy or re-derivation. Stage 4 replaces that copy with
    "tell the engine the reverse changes". The copy is taken before every
    transaction, so stages 2 and 3a time it separately and report each
    transaction's cost with and without it. The plot path applies no transaction
    and is unaffected.
- **Intended differences the harness can run.** `intended.rs`'s module comment is
  rewritten to the user's rule (the spec's §16.1). `INTENDED` is descriptive today:
  `Store::call` compares return values and then whole snapshots, and reads neither
  table. Each entry gets an executable form, one of two kinds:
  - a **normaliser** applied before every comparison, to return values, the snapshot
    and the `view` text alike. The new engine reproduces the old message strings
    where that is cheap; a message quirk that is costly to reproduce becomes an
    entry, under the user's rule, rather than a hard requirement;
  - a **divergence test** on one call's results. When it matches, the facade records
    the entry, stops comparing that script, and counts it, since the stores then
    legitimately differ (an id accepted by one store and refused by the other leaves
    different nodes behind it).

  Any difference that no entry explains still panics. The 10,000-seed run reports,
  per entry, how often a normaliser changed a comparison's outcome and how many
  scripts a divergence ended.
- **The entries 2b needs,** all on §16.1's list:
  - ops emitted for derived structure, once stage 4 removes `Transaction::expanded`;
  - an inner picture follows an override of its entity's `instance.def` (today the
    body does and the picture does not);
  - two Collections whose inputs read each other's items are a cycle with a problem,
    against the old engine's 129-round cut (stage 3b).

  The NaN text comparison in `settle_repeats` is dead code (`Value`'s `==` already
  treats NaN as equal) and is deleted as a shared fix. The machine loop is phase 3's.
- Tests: an injected unlisted difference panics; each listed one is relaxed only in
  its narrow form; a divergence ends comparison for that script only.

**Stage 2. Templates and owners, replacing the whole-document expansion.**
- **`engine/template.rs`.** Compile a `Template` per (definition, notation, sized, main)
  and per Collection, including Collections inside templates (today's
  `template_of_repeat`). What it produces, relocated per owner, must equal what
  `want_entity`, `want_use`, `rewritten`, `apply_overrides` and `template_of_repeat`
  produce:
  - `port_spec` becomes `port`;
  - `Value::Ref` constants are relocated;
  - a picture's references into the body go to the entity's body owner, or stay on
    the definition for a main;
  - references to the template root go to the `use` in a sized `use`;
  - machines are copied;
  - an override's facet is added where the type admits it.
- **`engine/owner.rs`.**
  - The owner tree: Root, Composition, Body, Picture, Collection and Item.
  - An index from template to owners, and overrides indexed per owner by target.
  - Nested owners are found from effective slots (§5.4).
  - Depth limits as `Wanted::enter` applies them.
- **Owners write their copies with `expand`'s own diff,** run on that owner's subtree:
  one `Set` per changed field, remove-then-add for a dynamic facet, deletes deepest
  first. The ops go through `apply_op` with the journal open and are returned as
  today, so **`tx.expanded` and its four consumers stay unchanged until stage 4**:
  `after_transaction`'s structural check, its invalidation, the geometry notes, and
  `pending.note`. The ids are `expand`'s path ids.
- **Owner state rolls back with the document.** Owners, the template cache and
  per-owner problems change in step 2, and again in `after_transaction`. On any
  refusal (by `run`'s checks or by `post_apply`) they are put back by an undo log kept
  beside the journal. The spec's §11.2 step 3 says to discard templates compiled in
  the step.
- **Disposal deletes explicitly, across nested owners** (the spec's §7.3). Disposing
  an owner, an item scope included, deletes every live derived node under it
  explicitly, deepest first, including the copies of the owners nested inside it.
  Otherwise the cascade tombstones them and `Changes::removed` never hears of them,
  since `Changes` learns of a derived removal only from an explicit `Delete`. All
  stale deletes of one step are sorted deepest first before any create, as
  `expand`'s diff does. A node whose type changed is deleted and recreated in
  place.
- **Which owners an op affects** is the spec's §7.2 table, plus what `wanted` covers
  today that the table misses:
  - instances and uses inside a node that gains a `repeat` or `when` facet, or is
    reparented into one (they stop expanding, so their owners are disposed), and the
    reverse;
  - an entity's `instance` set, which changes every picture that figures it, not
    only its body;
  - `size` added to or removed from a `use` (the key includes "sized");
  - a definition created, or its deletion undone, for entities that name it;
  - a node moved into or out of the library.
- **Kept whole-document in 2b:** `port::derived_signals` and `anchor::derived`,
  recomputed on structural ops, as now. A tick-count change is a structural op, so
  they are on the plot's path. They cost O(connections + anchors), and stage 3a
  measures them. `@name` resolution is re-done from an index of the cells that read
  each name, not by enumerating `bound_slots` (which walks every facet) and walking
  up from each reader.
- **The node budget** (50,000) needs a rule for which owner is refused and when one
  is let back in. Today it cuts in `wanted`'s global order. Write the rule in the
  spec's §7.5. If the old cut cannot be reproduced exactly, the budget becomes a
  §16.1 entry that needs the user's approval: the cut decides which instances a user
  sees past 50,000 nodes, which is visible and may be relied on. Until then, measure the 1,000-meter
  numbers at 700 meters, where nothing is cut.
- **Under `differential`, after each op's owner updates, and after each settle round
  that changed counts** (`wanted` is whole-document, so once per op or round, not
  once per owner), the old `wanted` runs on the subject's document and must equal
  the owners' copies. This names the op that was
  missed, not a later snapshot.
- Exit:
  - the differential suite is green with the subject on `New` (owners and the old
    graph);
  - a structural edit measured at 700 meters.

**Stage 3a. Cells replace `graph.rs`.**
- **`engine/cell.rs` and `engine/eval.rs`, from the kernel's R1–R8** (read
  `src/iron_engine_kernel` first):
  - dense cell arrays;
  - heights set on insertion and raised locally (§6.3);
  - every cell necessary, through one global observer;
  - one pass lowest height first, with ties in `Leaf` order;
  - tracked reads per R5, cycles per R6, the equality cutoff;
  - `Failed` messages identical to today's;
  - boundary type checks and `as_leaf_kind`.
- **The read order is §5.2's:** overlay, binding, signal, constant.
- **Producers** write Input cells, with `publish`'s claim, release and
  `signal_errors` rules unchanged.
- **A derived cell evaluates its stored copy's expression,** borrowed.
- **No rebuild on the success path.** `after_transaction` tells the engine each op
  and each expanded op instead of calling `rebuild` or `invalidate`, and loading is
  the same pass with everything new. After a refusal, `New` restores as stage 1 says.
- **`settle_repeats`' rounds still drive structure** through the owner layer. The
  sequence of publishes is unchanged, so published items and the 129-round cut
  match.
  - Today every structural round, and `after_transaction` itself, clears
    `repeat_inputs`. Each round then calls `drawn_repeats`, which scans every group
    (item scopes included).
  - Under `New` the repeat list comes from the owner layer's Collection index,
    sorted by `NodeId` as `nodes_of_type` sorts. The 129-round problem names
    `repeats[0]`, so the order must match.
- Exit:
  - the differential suite is green;
  - `plot_perf.rs` at 1 and 10 plots (`IRON_ENGINE=new`), and `plot-perf.txt` in a
    wasm-bench build with `engine-new`, measured and reported, with the share of
    `derived_signals` and `anchor::derived` split out.

**Stage 3b. Collections as Structure cells** (§7.1), replacing `settle_repeats`.
- Keys, `max` with its `truncated` problem, key fallback with its `keys` problem,
  `when`, and presence for `enter_ms` and `exit_ms`.
- `item.value`, `item.index`, `presence` and `leaving` become Input cells the
  Collection writes. They still appear in `signals()` under `ProducerId::ITEMS` and
  `PRESENCE`.
- The prototype still reads item 0 through `repeat.item`.
- Two Collections whose inputs read each other's items are a cycle with a problem:
  stage 1's divergence entry.
- Exit: the differential suite is green; the plot probes measured again.

**Stage 4. The transaction steps in full** (§11.2 steps 4 to 7).
- **The new engine:**
  - tell it each op, set the transaction's drivers, and stabilise;
  - owners it disposes are **held** with their state;
  - the solver stays `post_apply`, reading through the engine and stabilising after
    each cluster's writes;
  - on failure: roll back the journal and the owner log, tell the engine the reverse
    changes, re-attach held owners, stabilise, and discard the pending `Changes`.
    This replaces `New`'s restore-by-copy from stage 1;
  - commit: close the journal, drop held owners, release buffered effects (a query's
    request, `on.emit`), publish `Changes`, push history.
- `Transaction::expanded` goes away, in both engines. Its consumers read the owners'
  notes instead, and stage 1's derived-ops entry starts to apply. Undo, redo and
  replay take steps 1 to 4 and 7; `apply_session` takes 2, 4 and 7.
- Exit:
  - the differential suite is green with `IRON_ATOMICITY=1` reporting nothing;
  - `tests/rollback.rs` is green on `New`.

**Stage 5. Swap, delete, measure.**
- **Exit first,** the spec's §17 row for 2b, run before anything is deleted:
  - the differential suite and the 10,000 random scripts are green, with only §16.1
    entries and their counts reported;
  - the host reference, `bun test`, `smoke.txt` and `regression.txt` pass, and so do
    the scripts §16.2 names: `plot-view.txt`, `actions.txt`, `query.txt`,
    `fiber.txt`, `pulses.txt`;
  - **§15's plot targets are met:**
    - a step that changes the tick count costs within 2× a values-only step, at 1
      and 10 plots (about 0.3 ms);
    - a values-only step is no slower;
    - `plot-perf.txt`'s `action` p95 is under 1.5 ms, for slow pan and wheel zoom
      (wasm-bench);
  - also recorded:
    - a changed frame's time, which includes the host's paint order and spatial
      rebuild;
    - a structural edit and a placement at 700 meters, and at 1,000 once the budget
      rule is approved;
    - the demo's load time (no slower);
    - memory per meter (not expected to change until 2c).
- Then `Store` builds `New` by default, and `InPlace` is deleted. The old engine goes
  too: `graph.rs`, `expand::expand`, `wanted` and its helpers, `settle_repeats`, and
  `Engine::Old` with its clone. The spec's §16.1 makes 2b's engine the reference for
  2c, so the facade pairs two `New` stores from here.
- The roadmap row "Plot pan and zoom performance" is closed.

**Stop and report** if:
- A5 fails;
- a difference appears where it is not clear which behaviour is right, or that
  alters something a user can see and may rely on. Any other difference is a shared
  fix or a recorded entry;
- a stage's exit cannot be met;
- the plot targets are missed. Report the shortfall split between the engine and the
  whole-document passes:
  - phase 5's: `rebuild_clusters`, `resolve_layout`, `solve_network`, palette,
    contexts;
  - those kept whole-document in 2b: `port::derived_signals` and
    `anchor::derived`.

### 4.6 Phase 2c: copies leave the document

The point: the authored document holds authored nodes only, and the Tree answers for
derived ones from the owners (spec §5.4, §12.1). What it buys: no derived writes in
the authored document's journal, save, clone or `edits`; the ground phase 4's
Unloaded owners need; and later, memory per instance (spec §15). Exit (spec §17): the
Tree equals 2b's stored copies on every script; memory per instance measured against
phase 0.

Written 2026-11-17 from the spec and the code at `e0d520d`, after a survey of every
reader of derived nodes, then revised after a council review (Claude Opus 5.5 5/10,
GPT-6.2 Sol 5/10, both on stage 1's buildability). Their findings are folded in below;
the ones that change the design are marked *(review)*.

**What the code looks like now.**
- Every reader outside the crate reads nodes through `Store::tree()`, except the
  `expose_control` command (`engine_api.rs:731`), which hands `authored()` to
  `attachment::authored`, which asks whether a **derived** id is live
  (`author.rs:426`, `attachment.rs:13`).
- *(review)* Inside the crate the problem is not a list of direct readers but the
  constructor: `Tree::over(&Document)` is built at 179 non-test sites (about 100 in
  `lib.rs`, 20 in `op.rs`, and `expand.rs`, `owners.rs`, `author.rs`, `standard.rs`,
  `compose.rs`, `parts.rs`, `DocTree::over` in `layout.rs`), each over one document.
  Beside them, 39 direct `self.doc` reads in about 25 functions, among them
  `selection()` (drops a selected copy), `problem_notes` (drops notes on copies),
  `extent`, `set_state` and `is_session_state`, `drag_to`, `make_component`, the
  gesture and handle paths, `handlers` and `hover_listeners` (parent walks),
  `settle_repeats`, `publish_presence`, `publish_state`, `publish_selection`,
  `action_writes`, `context_reads`, `resolve_contexts`, `can_refuse`, `post_apply`,
  `refresh_text`; and `layout.rs` throughout.
- The writer of copies is `expand::apply_wanted` through `op::apply_op`. Authored ops
  read copies too: `set`'s `check_binding` on a binding reading a derived node
  (`op.rs:611`), `link` accepting a derived end (`op.rs:755`), `check_created`'s
  connection checks. Writes to copies read authored nodes (`check_parent`,
  `check_entity`). An authored delete tombstones its derived descendants
  (`op.rs:486`). `post_apply` can write a solved derived transform as a solver op
  (`solve.rs:175`, `lib.rs:2988`; traced, not run).
- Derived relations are made in three places (`RelationId::expanded`,
  `expand.rs:645, 691, 784`). Authored relations *to* a copy stay authored.
  `serial::save` already filters derived records.
- Validation in `run` (`check_created`, `check_scope`, `check_context`,
  `attachment::validate`) reads structure and slots, not values: A2 holds for it.
  `translate` (before the ops) and `post_apply`'s `constraint::validate_all` (after)
  read resolved values.

**The reference.** For this phase a switch chosen at construction comes back:
`Copies::InDocument` (2b as built) and `Copies::Apart`. The facade's subject is
`Apart`, its reference `InDocument`. *(review)* Both run the same code: under
`InDocument` the routing below sends every write to the authored document and the
copies document stays empty. The snapshot compares the joined Tree (live nodes, in
traversal order) and `authored()` filtered to authored and session records with the
counters. Derived tombstones are not compared (stage 1 stops keeping them, below).
Stage 4 deletes `InDocument`.

**Stage 1. Two documents behind one Tree.** Eager: every copy is materialised, in a
second `Document` (`copies`). This is the spec's A4 fallback, taken first so that the
join is exact before anything is lazy.
- **`StoreCore` holds `docs: Docs { authored, copies }`**, one field, so a Tree over
  it borrows apart from `reactive`, `machines` and the rest (the split borrows at
  `lib.rs:2275, 2294, 2341, 2352` need that).
- **`Tree::over` is deleted** *(review)*, as phase 1 renamed `document()`: every one
  of the 179 sites becomes a compile error and is decided one by one, to
  `Tree::of(&docs)` or to `Tree::authored_only(&doc)` where only authored nodes can
  matter (scratch documents cloned for planning: `compose.rs:213`, `parts.rs:763`,
  `standard.rs:215`, `author.rs:63`, audited for reads of an instance's copies).
  Direct `self.doc` reads go the same way: `docs.authored` is named, so each is a
  decision.
- **Writes route by id** *(review)*. `apply_op` takes `&mut Docs`, sends each write
  to the document its id belongs to (`id.is_expanded()`; a relation by `rel.id`), and
  builds a fresh `Tree::of(&*docs)` for each check, so a check sees the other
  document and the write it follows (`reparent` writes then checks `check_embeds`;
  `apply_wanted` retries failed ops). This routes expansion, authored ops, the
  solver's writes and undo and replay with no special case. An authored delete no
  longer cascades into copies; the owners' expansion deletes them, as it already
  does for the ops `owners.affected` sees. A fixture: a solved derived transform,
  its history and undo.
- **Walks that cross documents are the Tree's** *(review)*: `depth`, `is_ancestor`,
  `order_path`, `descendants`, `descendants_within`, `binding_cycles`,
  `reads_interaction` follow parents from a copy into its authored owner.
  `apply_wanted` orders stale deletes by joined depth, so `expanded`'s order, and
  with it `Changes` and `regraph`, match the reference.
- **`children` returns `Cow<'_, [NodeId]>`** *(review)*: borrowed when one document
  has the parent's children, merged by `(order, id)` (the document's own tie-break,
  `document.rs:934`) when both do. A derived node's authored parent is always its
  owner (an entity's body, a `use`'s picture, a repeat's items), so a merge happens
  only at an owner; assert it. Repeats interleave too (a new authored child can share
  the first item's key). Fallback if the merge's allocations show in the plot's frame:
  a merging iterator over the two slices with a slice fast path (Sol's preference).
- **Enumerations chain** because every derived id sorts after every authored and
  session id (`EXPANDED = 1 << 63`, `id.rs:44`): `nodes`, `live_nodes`,
  `nodes_of_type`, `nodes_with`, `relations`. **`bound_slots` merges per facet kind**
  *(review)*: it is kind-major, and its order reaches `graph.rs:286`,
  `derived_signals` and an error message at `op.rs:576`.
- **No derived tombstones** *(review)*: the copies document removes a deleted copy
  outright (a new mutator, journaled). They grow with item churn and the end state
  has none.
- **`Changes`** *(review)*: a deleted owner's copies are reported from expansion's
  own deletes, not from a cascade in `full_inverse`. `Changes::note` reads a removed
  node's former parent and an unlinked relation's ends, so those are captured at
  mutation time and passed with the ops, and fed alike to `regraph`, the geometry
  notes and the host. Their order is the reference's.
- **Journal and restore** *(review)*: `begin`, `commit` and `rollback` act on both
  documents together (`lib.rs:575-584, 3038, 3101`), and the shadow copy under
  `differential` covers both. `settle_repeats` writes under the transaction's
  journal from `after_transaction` and under none from `lib.rs:2129` and load: keep
  that, for both. `Saved` is unchanged (it does not hold the document). The network
  cache's key advances on rollback too (a generation, not `edits`, which rollback
  would repeat). `apply_session` touches session nodes only; its reparent guard
  checks only the child's id, so a test that a session child cannot be moved under
  a copy.
- **Scratch stores and construction** *(review)*: `StoreCore::scratch` clones both
  documents instead of re-expanding every owner on each edit plan (`edit.rs:162`,
  `compose.rs:81`, `parts.rs:1141, 1198`). `with_document` and `load` take authored
  documents only and refuse one holding derived ids. `into_document` gives the
  authored document.
- **Invariants** *(review)*: a joint `Tree::check_invariants`, with one new rule (no
  live copy under a dead authored ancestor), run after every call under
  `differential`. `Document::check_invariants` on `copies` alone is not meaningful
  (its parents are elsewhere).
- **The owner check and the budget:** `check_owners` builds a joined scratch and runs
  the whole expansion over it. The 50,000-node cut is kept exactly; a test over the
  cap against `InDocument`.
- **A12** *(review)*: extend `tests/id_order.rs` to sibling ties between authored and
  derived children.
- Exit:
  - the differential suite, with `IRON_ATOMICITY=1` and `IRON_OWNER_CHECK=1`, and the
    10,000 scripts, green with the subject `Apart`; fixtures for a derived removal,
    reappearance and refusal;
  - the host reference on `Apart`, with removal, reorder and undo fixtures; the smoke
    and the §16.2 scripts;
  - measured before and after: values-only and tick-change plot steps, picking and
    drawing in the app (`plot-perf.txt`), a constant and a structural edit at 700
    meters, the demo's load, a document clone and save.

**Stage 2. Templates, materialising copies** *(review: reordered)*. Compile a
`Template` per (definition, notation, sized, main) and per Collection (a Collection
in a template is a sub-template, today's `template_of_repeat`), with
`Ref::{Inside, Body, Item, Arg, Outside, Context}`, and a `materialise(owner, local)`
that produces an eager copy from it in place of `want_entity`, `want_use`,
`rewritten`, `apply_overrides` and `template_of_repeat`.
- Checked by `IRON_OWNER_CHECK`'s whole-expansion oracle, and by parity tests on
  effective slots, relocated literal refs, dependency edges and errors: override
  precedence, overridden nested templates, derived repeat prototypes carrying a
  picture's overrides.
- Exit: green; the meters' structural edit and placement measured.

**Stage 3. Copies materialised on read** (spec §5.4, A4). *Moved to phase 4 with
validation from templates (D15, 2026-11-18); 2c's stage 3 is the narrower list at the
end of this section.* Per loaded owner, one
write-once slot per template node, filled on first read, replacing the copies
document. Needed first *(review)*:
- an id → (owner, local) index that does not take the path registry's lock
  (`id.path()` takes an `RwLock` and clones a `Vec`, `id.rs:83`), so a read of a
  derived id costs no lock or allocation;
- per-template indexes for the enumerations (`nodes_of_type`, `nodes_with`,
  `relations_to`, `bound_slots`), or they materialise everything;
- `Changes` for copies created and removed, now that no derived ops exist;
- a graph read path that evaluates without filling slots;
- A4's threading: `OnceLock::get_or_init` can wait on another initialiser. Verify
  that no reader can, or keep stage 1's eager copies per owner.
- **Memory may not fall in 2c** *(review)*: until phase 4 every owner is loaded, and
  drawing, the graph and the snapshot read every copy, so every slot fills. The exit
  is memory per meter **measured** against phase 0 (92.4 KiB; 110.5 KiB now), cold
  and traversed owners apart, not a promised gain. A10 is decided before the slot
  and id indexes are chosen.

**Stage 4. Cells from templates, if measurement justifies it** *(review)*. `graph.rs`
is keyed by concrete `Leaf`; evaluating one template expression per owner needs an
evaluator hook that resolves `Ref`s, or the dense engine stage 3a deferred. Build it
only if stage 3's measurements show the per-copy expressions cost memory or time that
matters. Then delete `InDocument`.

**Validation from templates moves to phase 4** (the user, 2026-11-18; spec D14). While every owner is loaded the Tree answers
all derived structure, so validating from templates buys only §16.1's more permissive
item-path rule. It matters once owners can be Unloaded. If it stays in 2c, A3's
fallback is written in: validate against materialised structure for the owners the
ops touch and their dependency closure. Incremental paint order per changed parent
moves with it (paint order already reads the Tree; a subtree inserted shifts every
later preorder index, so per-parent invalidation needs its own design and keeps the
full traversal as its oracle).

**Settled by the user (2026-11-18):** the node budget cuts per owner in id order
(spec D12); A10's paths are reclaimed in 2c by a sweep (D13); validation from
templates moves to phase 4 (D14). Lazy slots (stage 3) are a goal the user wants, not
an optional step; they move to phase 4 with validation from templates (D15, below).

**Progress:**
- **Step 1a** (`e2bc90b`): `Docs` and the two-document Tree, `Tree::over` deleted,
  `apart = false` (no behaviour change).
- **Step 1b, copies apart** (`7730fbb`): the subject keeps copies in `copies`, and the
  differential reference keeps them in the document (`with_copies_in_document`). A
  joint `Tree::check_invariants` runs on both stores after every call and on rollback.
- **Step 1b, no derived tombstones:** `Docs::delete_node` removes a copy outright
  (`Document::remove_node`, journaled). It differs from this section's plan in three
  ways:
  - **The cascade stays:** an authored delete still reaches its copies through
    `Tree::descendants`, rather than through the owners' expansion.
  - **Removed nodes are recorded in `Docs::gone`:** each deletion stores the removed
    node's parent, depth, type and port/anchor flag (`changes::Went`). This is the
    capture at mutation time described above. `Changes::note` and `regraph`'s
    `made_of` read it, and it is cleared in `take_changes`.
  - **Dead nodes are treated alike, whether tombstoned or removed:**
    - `read_with` returns no value for them;
    - `resume_gesture` drops their previews and `commit_gesture` writes none;
    - for a gesture's conflict check, a dead watched node has no ancestors. Before
      this, the reference cancelled a gesture when a tombstone's former parent was
      deleted (random seeds 144, 219 and 7593).
- **Test layout:** the store's integration tests are one binary, `tests/suite.rs`
  (filter by module, `--test suite differential_random::`). The font tests are in
  `fonts` and `instance_cost` is separate. A change relinks in ~5 s rather than ~50 s.

- **Step 1c, the rest of stage 1's store items:**
  - **`StoreCore::scratch` copies the store** rather than building one: both
    documents, the owners, the graph without its previews, repeats, clusters and
    presentation state. It has no history, gesture or pending changes, and it now
    plans at the store's playhead rather than at 0. An edit plan at 700 meters went
    from 115 to 23.6 ms (`instance_cost`'s new `edit plan` column); at 1,000 meters,
    201 → 109 ms.
  - **Derived content is refused:** `load` and `with_document` refuse a document
    holding it (`Document::derived_content`: a derived node or relation, or a node or
    relation hanging from a copy). `load` returns an error; `with_document` panics.
  - **Nothing but its owner places children in a copy:** `check_parent` refuses a
    non-derived child under a copy. Authored ops were already refused earlier, as
    edits of the instance (`BadId`); this rule is what stops session ops.
  - **A12:** `id_order.rs` adds an authored child of an instance that ties its first
    copy's order key. A repeat cannot tie: it keys its items after its last authored
    child.
  - **`tests/copies_apart.rs`:**
    - removal with no tombstone, and reappearance with the same ids;
    - ops naming a copy refused, whether it is there or removed;
    - a refusal restoring both documents;
    - no children placed in a copy;
    - a solver-written copy transform with history, undo and redo;
    - `load`'s refusal;
    - the node budget against the reference (D12), `#[ignore]`d because it needs
      release.
  - **The budget path is slow:** 60,000 wanted nodes take 11 s in release for three
    transactions, and a structural edit at 1,000 meters takes 93 ms. Look at this
    before phase 4 counts loaded owners.

- **Stage 1's exit (2026-11-18).**
  - **The host reference** (`67433dc`) adds rows reordered by key, which move their
    copies and make or remove none, and an instance raised. Both are undone.
  - **In the app** (dev-opt build): `bun test`, `smoke.txt` and the §16.2 scripts
    (`plot-view`, `actions`, `query`, `fiber`, `pulses`) pass.
  - **`regression.txt`** passes again (`6d2199d`). The slider-styling commit
    `00151fd` had broken two of its steps without failing it: link-figures
    named an id the demo no longer had, and a Cmd-held scrub moved the slider.
  - **Measured before and after,** native release except the app row:

    | | before stage 1 (`518adf0`) | after |
    |---|---|---|
    | plot step, values only, pan / zoom, 1 plot | 0.065 / 0.122 ms (fix 1) | 0.039 / 0.111 ms |
    | plot step, values only, pan / zoom, 10 plots | 0.065 / 0.152 ms | 0.068 / 0.156 ms |
    | plot step, tick change, pan / zoom, 1 plot | 0.166 / 0.190 ms | 0.093 / 0.167 ms |
    | plot step, tick change, pan / zoom, 10 plots | 0.257 / 0.322 ms | 0.246 / 0.324 ms |
    | app `action` p95, wasm-bench: slow pan, fast pan, zoom | 1.32, 1.05, 1.45 ms | 1.30, 1.06, 1.27 ms |
    | structural edit, 700 meters | 5.2–5.7 ms | 5.1–5.3 ms |
    | constant edit, 700 meters | 1.09–1.15 ms | 1.14–1.33 ms |
    | document clone, 700 meters | 7.6–10.8 ms | 0.26–0.55 ms |
    | save, 700 meters | 2.4–2.8 ms | 1.9–2.0 ms |
    | load, 700 meters | 107–115 ms | 108–110 ms |
    | demo: load, save, clone | 1.4–1.8, 0.4, 0.06 ms | 1.5–2.0, 0.4, 0.05 ms |

    The "before" plot rows are fix 1's numbers. The other "before" rows come from
    `518adf0`, built out of tree and run beside the current tree: `instance_cost`,
    and a load/save/clone timer over the demo as the app saves it.
  - **Where an edit plan's time goes,** 23 ms at 700 meters: about 13 ms copying
    and dropping the store (`scratch`), 7 ms applying the edit to the copy. Planning
    inside a transaction on the live store, rolled back, would cost about the edit
    alone. Not done: it is outside stage 1.
- **Council (Opus only, 2026-11-18): 7/10.** Fixed:
  - `a_refused_transaction_restores_the_copies` refused before expansion, so it
    tested nothing. It now refuses on a context's markup, which is checked after
    expansion.
  - `scratch` carried an eased or previewed layout. It now publishes the targets
    when motion or a layout preview is on.
  - `expand::refuse` turns away an authored `Link` with a derived relation id
    (`an_authored_link_with_a_derived_id_is_refused`).
  - `Docs::gone` is journaled, so a rollback takes back what a refused transaction
    added. `regraph`'s `Create` arm reads the live node.
  - The A12 test's comment no longer overclaims: the `EXPANDED` bit decides an
    authored-to-derived tie.
  - The host reference asserts that a reorder moves copies and makes or removes
    none.

  Left: the copies document keeps an empty child list for each deleted owner that
  had copies. It is a small leak that grows with owner churn, and stage 3 replaces
  the copies document.
- **Council round 2 (2026-11-18): Opus 7/10, Sol 6/10** (Sol's first look at the
  stage). Each finding was reproduced as a failing test before it was fixed. Only
  the first was new with stage 1; the reference behaves the same way in the
  others.
  - **Deleting an instance that a constraint holds by one of its copies was
    refused** (`constraint::read`: the operand had no `transform2d`). A copy that
    went now leaves the operand dead and the constraint inactive, as a tombstone
    did (`deleting_an_instance_whose_copy_is_constrained`).
  - **Undo and redo turned the solver's writes to a copy into overrides on the
    instance** (`expand::translate`). A recorded transaction's sets on copies now
    skip translation. They apply as `post_apply` writes them, after expansion, the
    owners' check and `after_transaction` (`StoreCore::apply_recorded`)
    (`undoing_a_solved_copy_adds_no_override`).
  - **`IRON_OWNER_CHECK` failed on any transaction after the solver had moved a
    copy.** The whole expansion cannot know a solved value, so the check now skips
    leaves that are cluster variables.
  - **A link to a copy nested in a repeat's item made the file unloadable.**
    `load` now checks an expanded endpoint's authored root, not its immediate
    owner (`a_link_to_a_nested_copy_saves_and_loads`).
  - **A reorder recorded its moves with the roots as the parent each had.**
    Expansion now emits `Reorder` when a copy keeps its parent
    (`a_reorder_moves_copies_and_makes_none`).
  - **A post-apply refusal left `item_memo` ahead of the restored store.** It is in
    `Saved` now.
  - **`<place entity>` read a nested entity from the authored-only clone.** It now
    reads the store's tree.
  - **`expand::refuse`** now refuses an authored `Unlink` of a derived relation,
    and an expanded id whether or not a path is registered for it.
  - **The refusal fixture refused before any copy changed** (items settle after
    `run`). It now changes the repeat's template in the refused transaction.
  - **`scratch` laid out the whole document on every plan in the app,** where
    motion is always on. It now does so only while a transition runs or a layout
    preview is set.

  Not taken: Sol's point that a plan translates against settled layout while live
  apply may meet a preview. Round 1 asked for the settled layout, and a plan is
  applied later, mostly once the preview has ended.
- **Council round 3 (2026-11-18): Sol 7/10, Opus 7/10.** Fixed:
  - the solver's writes, normal and recorded, are noted for geometry
    (`geometry.note_ops`);
  - a new authored link to a copy that is not there, under a live instance, is
    refused once expansion has run. A recorded one may meet a copy that has since
    gone (`a_new_link_to_a_missing_copy_is_refused`);
  - `scratch` also settles layout while a gesture is active, since it drops the
    gesture's previews that layout may have read.

  Left, all about the solver's writes to copies, which predate stage 1 (the
  reference behaves the same):
  - undo of deleting an instance whose copy the solver had moved brings the copy
    back at its template position, until the next solve;
  - redo or replay drops a recorded write whose copy is not there, after the
    journal has committed, instead of refusing;
  - `IRON_OWNER_CHECK`'s exemption follows current cluster membership, not the
    solver's own writes: disabling the constraint that moved a copy makes it
    report a false difference.
  - **A copy keeps its solved value after its constraint goes** (Opus): deleting
    or unlinking the constraint does not affect the copy's owner, so the value
    stays until something re-expands the owner, and then jumps.
    - Opus's fix: `owners.affected` counts an Unlink of a `Constrains` relation
      to a copy, or the Delete of such a constraint, as affecting the copy's
      owner.
  - **Undo of a transaction that re-expanded a solved copy's instance restores
    the template's value of that moment, not the solved one** (Opus).
    - Example: a template edit `x` 0 → 7 while the copy is solved to −260;
      undo leaves the copy at 7.
    - Before `9077b32` the same undo made that wrong value an override.
    - Opus's fix: when a live apply's expansion overwrites a copy leaf that is
      a cluster variable, or deletes such a copy, push a Set back to the old
      value into `tx.inverse`.

  These are for when the solver's writes to copies get a home of their own (they
  are neither authored nor derivable from the template). That home is stage 3's
  slots, or the solver's own published values.

- **Stage 2, templates (2026-11-18).** `src/engine/template.rs`: a `Template` is
  compiled once per expansion pass per definition body, per (definition, notation)
  picture and per collection, and shared by every owner the pass expands.
  `Template::materialise(node, env)` makes one owner's copy of one template node:
  the facets cloned, each reference resolved by a class fixed at compile time
  (`Inside`, `Body`, `Root`; anything else is outside and read as it is), an `in`
  port given its argument, the owner's overrides applied. `want_entity`,
  `want_use` and `want_items` are now loops over a template, and `rewritten` and
  `apply_overrides` are gone from the production path. The overrides reaching an
  owner are worked out once per owner (with each node's own overrides read once
  per pass), where the old code worked them out again for every copied node.
  - **The old code is the oracle.** The functions that made each owner's
    copies (`want_scopes`, `want_items`, `want_entity`, `want_use`, `rewritten`,
    `apply_overrides`) moved unchanged to `expand_oracle.rs`, built only with
    `differential`. Its driver takes the owners as they expand, one at a time
    in id order (D12), so the budget's cut falls in the same place.
    `IRON_OWNER_CHECK` runs two checks after every owner update:
    - `expand::parity`: what every owner wants from templates equals what the
      old functions want: the same nodes in the same order with the same facets,
      the same relations and the same problems;
    - as before, applying the old functions' wants to the document finds
      nothing to change.

    It goes when 2c deletes `InDocument`.
  - **Departures from this section and the spec's §5.4:**
    - The key is (definition, notation), not (definition, notation, sized,
      main). Sized and main change only how `Root` and `Body` references resolve,
      so they are the owner's environment (`Env`), and one template serves both.
    - Templates live for one pass. Nothing is edited while a pass wants its
      copies, so nothing needs invalidating or rolling back. Stage 3's slots need
      templates kept between passes, with a version.
    - A collection's template was still compiled from its prototype (the
      authored children; a derived repeat's copies as this pass wants them, or
      as its owner's last expansion wrote them), not as a sub-template of the
      enclosing one. Stage 3's first prerequisite replaced this (below).
  - `apply_wanted` compares a copy's facets where they are instead of cloning
    them into a map first.
  - Parity tests, `tests/templates.rs`: nested overrides apply innermost first
    and the outermost wins (and may add a facet); an override of an inner
    picture's notation changes which template its copy is made from; a repeat in
    a template carries its picture's overrides into its items; references
    follow to each owner's copies, in bindings, in `list(…)` and from a picture
    to its entity's port. Reversing the override order or dropping the `Body`
    resolution fails two of them.
  - Measured, native release (`instance_cost`, which gains a **definition edit**
    column: one tick's width in the meter's template, so every meter's body and
    picture expand again). Load about 2:

    | | before | after |
    |---|---|---|
    | definition edit, 100 meters | 11.3 ms | 4.1–4.4 ms |
    | definition edit, 700 meters | 82–84 ms | 32–35 ms |
    | placement, 700 meters | 3.4–4.1 ms | 3.4–3.5 ms |
    | structural edit, 700 meters (expands nothing) | 6.0–7.4 ms | 6.0–6.5 ms |
    | definition edit, 1,000 meters (past the budget) | 171–174 ms | 70–74 ms |
    | placement, 1,000 meters | 27–28 ms | 14.6–15.0 ms |
    | structural edit, 1,000 meters | 96–97 ms | 44–47 ms |

    Placement and a structural edit at 700 meters expand two owners and none,
    so templates do not change them. Past the budget every update expands every
    owner, so there the whole pass's saving shows.
  - **Where a definition edit at 700 meters goes now** (1,401 owners, 46,900
    copies): wanting the copies 11 ms (was 58), collecting what the owners hold
    2.6 ms, the diff 17 ms (19–20 before), of which applying the 700 changed
    `Set`s is 0.8 ms. The rest is materialising and comparing every copy whose
    template did not change: about 0.5 µs a copy, mostly the boxed facets'
    allocation and comparison. The spec's fix is a definition edit that
    recompiles its templates and applies only the difference to each owner,
    which needs templates kept between passes (stage 3).
  - Tried and not kept: a per-node index of facet kinds in `Document`, so
    `facets_of` reads only the tables a node is in (the diff 15.5 → 13.4 ms, for
    an entry per node); reusing a parent copy's id instead of interning it again
    (no change).

- **Council on stage 2 (2026-11-18): Claude Opus 5.5 8.5/10, GPT Sol 6.2 8/10.**
  Fixed:
  - **A session node in a picture template** (Sol): the old code gives it no
    copy and skips its subtree; the compiled template kept the subtree, and its
    copies were wanted under a parent with none, as an `expansion` problem. The
    compiler now leaves out a session node and everything under it, in a
    picture and in a body's machines
    (`nothing_under_a_session_node_in_a_template_is_copied`).
  - **The owner check could not see problems, and cut the budget in another
    order** (Sol): hence `expand::parity` and the D12 driver above. With the
    session fix undone, `parity` names the first node that differs.
  - **The override matching was written twice** (Opus): `expand::overrides_by`
    is the one copy, read with a memo by `Templates` and without by
    `overrides_of`.
  - **Tests for what the oracle will stop covering** (Opus): a sized and an
    unsized `use` of one template in the same pass; a main's read of the body
    staying on the definition; a definition edit removing a facet and changing
    a field in every copy, with nothing made or removed.

  **Before stage 3**, from both reviews:
  - **A repeat in a picture must first become a sub-template of the picture's
    template,** with the picture's overrides layered in through `Env`. Today
    it is compiled from that picture's copies (wanted this pass, or as last
    written), which do not exist with lazy slots, and once per picture, not
    shared. The plot's tick labels are this case. Deriving the effective
    prototype must not fill the picture's slots during an item settle.
  - **A template kept between passes needs a version covering everything its
    compiler reads.** A picture's depends on the definition's body (which
    references are `Body`; each inner `use`'s `inner_def`, through the inner
    entity's `instance.def`), not only the template's subtree. A body's
    depends on `inputs_of`.
  - **Clearing a slot** (the spec's §5.4) must also follow a `use` gaining or
    losing `size`, since one template serves both and `Root` reads resolve by
    `Env`.

- **Stage 3, first prerequisite: a derived repeat's items from a sub-template
  (2026-11-18).** A repeat (or conditional) a picture copies, or another
  repeat's items copy, gets its items' template from the template that copies
  it: `Template::sub(outer, l)` is `outer`'s nodes under `l`, the definition's
  repeat node, with the relations `outer` derives from them. It is keyed by
  `outer`'s source and `l`, so every picture of a definition shares one; the
  copies are never read for it. An item's `Env`
  carries an `Item { repeat, key, outer }`, where `outer` is the copying
  template and its owner's environment: a reference the sub-template does not
  claim (its own nodes, the repeat's `item`, `index`, `presence`, `leaving`)
  resolves through `outer`, and `outer`'s overrides apply before the item's.
  That is the two passes the old code made (the picture's copy, then the item's
  copy of that), in one.
  - Where the outer is found: during a pass, `Wanted::origins`, recorded as
    `want_use` and `want_items` push a node that owns items; at an item settle,
    when the pass is over, `expand::maker_of` from the ids, reading only the
    copying owner's own node (a `use`'s facets and `figures`), or for an item
    of a derived repeat, recursively its maker.
  - `template_of_repeat` (prototypes from copies) moved to the oracle;
    `Template::repeat` compiles an authored repeat from the document.
  - Parity holds on the differential suite unchanged; the new test
    `items_of_a_repeat_in_a_repeat_in_a_picture_come_from_the_definition`
    settles a derived repeat inside another's items (both `maker_of` branches)
    and fails with the outer layer's overrides skipped.
  - **Cost, left for the version prerequisite:** templates still live for one
    pass, so an item settle compiles the whole picture template to take the
    repeat's sub-template from it, where it used to read the repeat's few
    copies. `plot_perf` (load about 20, both sides the same hour): a step that
    changes the tick count, pan / zoom, 1 plot 0.19 / 0.20 → 0.25 / 0.25 ms,
    10 plots 0.26 / 0.34 → 0.33 / 0.40 ms; values-only steps unchanged
    (0.07 / 0.13 ms). Templates kept between passes take the compile away.

- **Stage 3, second prerequisite: templates kept between passes (2026-11-18).**
  `TemplateCache` lives in `Owners`; a pass reads it through `Templates`, which
  also memoises each node's own overrides for the pass. Each template records
  its **scope** and that scope's stamp when compiled: a body's and a picture's
  scope is the definition, an authored repeat's the repeat, a sub-template's its
  outer template's. It is used while `Document::scope_stamp(scope)` is unchanged.
  - **The version:** every write at the authored document's choke points
    (`insert_node`, `restore_node`, `tombstone`, `remove_node`, `set_parent`
    before and after, `set_order`, `insert_facet`, `remove_facet`, `facet_mut`,
    a relation linked or unlinked, by its `from`) gives a new stamp to every
    definition, repeat and conditional enclosing the written node. Stamps come
    from one process-wide counter, so none is ever given twice (a rollback, a
    clone going its own way, another document); a scope never written is at the
    document's own base stamp. A rollback stamps the scopes of everything it puts
    back, before and after, so what a refused transaction compiled never matches
    again: the cache needs no transaction bookkeeping. Derived ids are skipped
    (session ones too, until council round 2 below).
  - **What it covers:** everything the compilers read is inside the scope: a
    picture's `Body` classes (`is_body`, the definition's children), each inner
    `use`'s `inner_def` (the inner entity's `instance`, in the body), the
    `figures` relations from template nodes; a body's `inputs_of`.
  - `expand::parity` runs on a clone of the kept cache, so the owner check now
    also checks that a stale template is never used.
  - Tests: `a_refused_definition_edit_leaves_no_template_behind` (fails without
    the rollback's stamps) and `a_definition_edited_without_instances_is_placed_as_edited`
    (fails, with one other, without `facet_mut`'s).
  - Measured (native release, load 2–10, the three builds the same quarter hour),
    a tick-count step, pan / zoom: 1 plot 0.18 / 0.20 ms before the sub-template,
    0.19 / 0.24 with it compiled per pass, **0.13 / 0.19** kept; 10 plots 0.25 /
    0.33, 0.31 / 0.40, **0.26 / 0.33**. Meters unchanged: definition edit at 700
    32.7 ms, placement 3.5 ms, constant edit 1.22 ms.
  - Not done: entries for a definition that is gone stay until the cache is
    dropped (bounded by the definitions ever compiled).
  - **The third prerequisite has nothing to build before slots exist:** today a
    `use` gaining or losing `size` re-expands its owner (`affected`, `AddFacet`/
    `RemoveFacet`), and `Env.sized` is read again. Stage 3's slot clearing must
    keep that: a picture's slots that read `Root` are cleared when its `use`'s
    `size` facet comes or goes.

- **Council on the two prerequisites (2026-11-18): Claude Opus 5.5 7.5/10, GPT
  Sol 6.2 5/10.** Fixed, each with a test that fails with the fix undone:
  - **A sub-template and an authored repeat shared a key** (Opus): a definition
    outside a library may draw its own repeat `l` in place, and is also copied
    by its uses. Sub-templates now have their own map, keyed by
    `(outer source, l)` (`a_repeat_drawn_in_place_and_copied_into_a_picture_keeps_two_templates`).
  - **A session node in an authored repeat** (Opus): `Template::repeat` kept
    it, so its order counted in `after`, and `touch` skips session ids, so the
    kept template went stale. It is left out, as in pictures and bodies
    (`a_session_node_in_a_repeat_does_not_order_its_items`).
  - **An own inner picture read its definition from the copy** (Sol):
    `want_use` took the inner `use`'s definition from the template's
    `inner_def`, fixed at compile, where the old code reads the local entity's
    `instance` at expansion. It reads it at expansion now, and `inner_def` is
    gone (`an_inner_pictures_definition_is_read_when_it_expands`,
    `an_override_of_an_inner_entitys_definition_settles_as_it_expands`).
  - **An item settle under a derived `use`** (Sol): `maker_of` cannot tell
    from the ids which picture a derived `use` draws, so it returns `None`
    and the owner is expanded whole (`a_repeat_in_an_inner_picture_settles_as_its_owner_expands_it`).
  - **An outer override naming a prototype node** (Sol): a reference-valued
    override from the outer layer named the picture's copy of a node the
    item copies; it now names the item's copy (`Template::copy_of`;
    `an_outer_override_naming_a_prototype_node_names_the_items_copy`).
  - **`restore_node` stamped only where the node comes back** (Sol): it now
    stamps where the tombstone was too (`a_body_node_restored_elsewhere_is_no_longer_the_bodys`).
  - The oracle's `template_of_repeat` copied too much, as the probes showed:
    it descended into a node's own derivation and took a node its parent
    derives, so an own inner picture in a repeat was derived twice per item,
    and it kept a session node in the authored branch. Fixed in
    `expand_oracle.rs` (`an_own_inner_picture_in_a_repeat_is_derived_once_per_item`).
  - **Recorded, not fixed (older, Opus):** editing a definition placed outside
    a library does not re-expand its uses: the owner check reports "owners
    missed … Set size.w". It fails the same at `554185f`.

- **Council round 2 (2026-11-18): Claude Opus 5.5 7.5/10, GPT Sol 6.2 6/10.**
  Both found round 1's fixes correct. Fixed, each with a test that fails with
  the fix undone:
  - **A session write stamps its scopes** (both): a session node is copied by
    no template, but the compilers see it: `definition::template_of` can pick
    a session `template`, a picture's classes count what is under a session
    group, and `inputs_of` reads a session `port_spec`. A session op moving
    one in or out left the kept template stale. `touch` now skips derived ids
    only (`a_session_template_moved_out_of_a_definition_is_no_longer_drawn`,
    `a_session_group_moved_out_of_a_template_takes_its_nodes_out`).
  - **What is under an item's node is the item's** (Sol): a reference in a
    repeat to a part of a picture the repeat holds (`#iu/box`), or to the
    outer picture's copy of a node the item copies (`#u/part`), named the
    prototype, where it names the item's copy. A collection's template now
    classifies by the id's first step, as a picture's does, and relocates into
    the item scope; what resolves through the outer owner to under its copy of
    one of the template's nodes moves into the item (`Template::into_item`,
    which also does round 1's override remap). The oracle had the same gap
    and now matches by the longest prefix in the template
    (`a_repeat_node_naming_the_pictures_copy_of_another_names_the_items`,
    `a_binding_into_an_inner_picture_in_a_repeat_reads_the_items_copy`,
    `a_reference_into_a_picture_in_an_authored_repeat_names_the_items_copy`).
  - **The sub-template key collided one level down** (Opus, speed only): a
    nested repeat's sub-template in place and in a picture had the same
    `(source, l)`; the key now has the outer template's scope.
  - **Recorded, not fixed (older, outside the template cache):**
    - Recreating a deleted id elsewhere (`Create` of a tombstone) schedules
      only its new location's owners, so a picture that read it where it was
      is not expanded again (Sol; fails the same at `554185f`). After the
      write, nothing says where it was.
    - A session op schedules no expansion: a session `port_spec` moved into a
      definition changes its inputs, but nothing expands its instances until
      an authored write does (Opus). And such a duplicate name makes
      `validate_entity` refuse authored writes to the instances.

- **Council round 3 (2026-11-18): Claude Opus 5.5 8.5/10, GPT Sol 6.2 7/10.**
  Opus found nothing (probe batteries of references and overrides across
  nested repeats, in place and pictured, all at parity). Sol found two, fixed:
  - **A binding naming the picture's copy of the repeat** (`#u/r.repeat.index`)
    read the prototype's index: `follow` matched only the definition's repeat.
    It matches the item's own repeat id too. Reachable from a main drawing its
    own definition; it held at `554185f`, so the first prerequisite broke it
    (`a_binding_to_a_pictures_copy_of_its_repeat_reads_the_items_index`).
  - **`into_item` scanned the template's nodes** for each reference resolving
    to under the outer owner (a `let` beside the repeat that every node
    reads): quadratic in the template. A template keeps a set of its locals.

**Stage 3 design (draft, 2026-11-18, `8767136`; superseded).** Copies as per-owner
pieces `(template, env)`, structure installed eagerly, facets filled in slots on
first read, derived ops checked without being written. Its review follows.

**Council on the stage 3 design (2026-11-18): Claude Opus 5.5 5/10, GPT Sol 6.2
5/10.** Both agree the storage boundary (structure eager, facets in slots) and
deferring the graph's template read path are right, and that the draft is not
buildable as written:
- **Derived ops cannot be checked against the final pieces.** Today op *k* is
  checked against the old state plus the ops before it that succeeded; `Create`,
  `Link` and `Reparent` also check the state before the op, and the retry loop
  depends on the state advancing. Checking must keep that order: a tentative view
  that advances op by op, which is the interpreter as it is.
- **Exceptions must be the effective state,** not a list of failures: a failed
  `AddFacet` leaves a whole facet absent, a failed `Reparent` the old parent and
  order, a relation unlinked and not relinked. Opus's probe: editing an inner
  entity's `args` left the new value *and* a problem (fixed below).
- **The solver's writes to copies must move in the same step as the Tree,** or the
  first constrained-copy script breaks; `post_apply` and `apply_recorded` write
  through `apply_op` and must keep writing.
- **A4 does not hold as drafted:** materialising relocates references with
  `path()` and `from_path`, which take the registry's lock (the write lock the
  first time a path is seen). Resolve a piece's relocations when it is installed.
- **Nothing would be lazy in 2c.** Wanting a copy materialises it (`want_*`), and
  so does validating it, which D14 keeps on concrete copies; every new copy's
  `Create` carries its facets, which `regraph` reads. So in 2c a slot fills when the
  copies document would be written. Laziness pays only once validation reads
  templates (phase 4).
- Also: `derived_ops` holds only the solver's writes, not expansion's (`Run::expanded`);
  children must keep merging on read; `facets_of` must keep kind order; the network
  cache keys on `Docs::edits`; the budget's cut falls inside a piece; item scope
  groups are not template nodes; D13's sweep belongs in the same work; the owner
  check must compare wanted pieces with the old generator *and* the effective
  copies with the old interpreter.

Fixed now: **a dynamic facet is replaced whole** (`apply_wanted` emitted
`RemoveFacet` then `AddFacet`; the removal fails where the facet is required, so an
inner entity's argument edit left an `expansion` problem, which re-expands the owner
on every update and stops item settling document-wide)
(`an_inner_entitys_argument_edit_reaches_its_copies_without_a_problem`).

**Decided by the user (2026-11-18, spec D15): lazy slots move to phase 4,** with
validation from templates, where a slot can stay unfilled. 2c keeps the copies
document and builds what pays now, each step with its own design note and council:
1. **A home for the solver's writes to copies,** fixing the five deferred bugs (stage
   1's council rounds 2 and 3), each with a failing test first, and the owner check's
   exemption by cluster membership with them. A behaviour that differs from the
   reference is fixed in both stores or recorded in `intended.rs`.
2. **A definition edit applies only what changed:** the template compiled again is
   compared node by node with the one it replaces, and a copy whose template node,
   reference classes and environment are unchanged is neither wanted nor diffed
   (about 28 ms of the 32.7 ms at 700 meters is wanting and diffing unchanged
   copies). Pieces with problems are never skipped.
3. **D13's sweep** of interned paths. *Moved to phase 4 (2026-11-19, the builder's
   decision while the user was away):* the path registry is process-wide and shared
   by every store in the process (scratch stores, the differential pair, parallel
   tests), so one store's sweep cannot know which paths the others hold, and freeing
   one they use breaks their live ids. A safe sweep needs each store to know its own
   slots, which is the per-store id index phase 4's slots build (D15). A10's measured
   growth (about 530 bytes per value-keyed item made) stands until then.

- **2c stage 3, step 1: the solver's writes to copies (2026-11-18).** No separate
  home was needed: the copies document keeps holding a solved value, and the two
  places that lost it now keep it.
  - **Expansion rewriting or removing a solved leaf** (bugs 1 and 5): `run`
    snapshots every copy leaf a cluster holds before the ops apply. One the
    transaction's expansion changes, or whose copy goes, gets a `Set` back to the
    solved value in the inverse. Undo applies it with the recorded writes, after the
    expansion it undoes, and after undoing any solver write of the same transaction
    (`undoing_a_definition_edit_puts_a_solved_copy_back`,
    `undoing_the_delete_of_a_solved_copys_instance_puts_it_back_solved`).
  - **A leaf no cluster holds any more** (bugs 3 and 4): `post_apply` writes it back
    to what its owner's expansion wants (`expand::wanted_slots`), as the solver
    writes, so it is in `derived_ops` and undo restores the solved value. It covers
    deleting and disabling the constraint alike, and with it the owners' check's
    exemption by current cluster membership is exact: a copy leaf outside every
    cluster is always its template's value
    (`a_solved_copy_returns_to_its_template_when_its_constraint_goes`,
    `disabling_a_solved_copys_constraint_lets_it_go`).
  - **Not fixed, bug 2:** undo, redo and replay apply recorded copy writes after
    committing, and drop one whose copy is missing. Undo and replay can meet one:
    repeat settling makes and removes copies outside history (a publish, a
    preview, the clock), so a copy a recorded write names may not be there. Then
    dropping the write is right, and refusing would block undo (council, below).
- **Council on step 1 (2026-11-18): Claude Opus 5.5 6.5/10, GPT Sol 6.2 5/10.**
  Fixed, each with a test that fails with the fix undone:
  - **Held means in a cluster, not a variable** (both): a copy under `fix`, or one
    a producer drives, leaves `var_cluster` but stays in its cluster as an input.
    The release took that for letting go, so fixing a pinned copy was refused or
    moved it to its template. `post_apply` lets go of a copy only when no cluster
    contains it, and the owners' check exempts a clustered copy's transform the
    same way (`fixing_a_solved_copy_keeps_its_pose`,
    `a_driven_solved_copy_is_not_let_go`).
  - **The snapshot read removed copies** (Opus): a copy a publish removed outside
    history is gone in the subject and a readable tombstone in the reference, so
    only the reference kept a value. The snapshot takes live copies only
    (`a_solved_copy_a_publish_removed_is_not_kept`).
  - **Repeat settling removed solved copies after the inverse was built** (Sol):
    the restores are now made in `apply` after `after_transaction`
    (`StoreCore::keep_solved`; `undoing_a_repeat_shrink_puts_a_solved_item_back`).
  - **Recorded, not fixed, both older than step 1:**
    - a solver write over a copy's bound slot is undone as a constant: undoing a
      template edit from `{= -40}` to `0` on a pinned copy leaves `Const(0)` where
      expansion had restored the expression (Sol; fails the same at the parent
      commit);
    - a release, like any solver write, changes values after repeat settling ran,
      so a conditional that reads the released copy keeps its item until the next
      settle (Sol).

- **Council round 2 on step 1 (2026-11-19): Claude Opus 5.5 6/10, GPT Sol 6.2
  6/10.** Both found round 1's "held means in a cluster" applied in one place of
  three, and Sol a case that changed the rule. **The rule now:** a copy some cluster
  holds (`Clusters::copies`, kept in `Owners` as the clusters change) has the
  leaves the solver writes (`solve::writes`: `x`, `y`, `rot`) as the solver left
  them. Expansion does not rewrite them (`apply_wanted`), the owners' check exempts
  exactly those, `run` snapshots them, and `post_apply` lets go of a copy when it
  leaves every cluster. Fixed, each with a test that fails with its fix undone:
  - **An edit of a fixed copy's template was refused** (Sol): re-expansion reset its
    `x` under the fix. Expansion now leaves a held copy's solver leaves
    (`a_template_edit_keeps_a_fixed_copys_solved_position`). With it, undoing a
    definition edit has nothing to restore; the restores are for copies that go
    (an instance deleted, a repeat shrunk).
  - **A fixed copy was never let go** (both): the snapshot read only the solver's
    variables (`a_fixed_solved_copy_is_let_go_with_its_last_constraint`).
  - **Redo and replay dropped the restores,** so an undo after a redo lost the
    solved value (both): they call `keep_solved` too
    (`undo_after_redo_puts_a_solved_copy_back`).
  - **The owners' check exempted every transform leaf of a clustered copy** (Sol;
    it hid a stale `sx` from the older outside-a-library bug): it exempts the
    solver's leaves only.

- **Council round 3 on step 1 (2026-11-19): Claude Opus 5.5 5/10, GPT Sol 6.2
  5/10.** Round 2's rule ("expansion never rewrites a held copy's solver leaves")
  dropped real edits: moving a held copy (the override reached nothing), a template
  edit of its `rot` (which a distance never moves), a new binding of a bound leaf,
  a fixed copy's binding replaced by a constant. **The rule now:** a held copy
  keeps the solver's value of a leaf unless *what is wanted for that leaf changed*
  since the copy was last made. Step 2's piece records give the last wanted facets
  (`Wanted::was`, from the recorded template and environment), and `apply_wanted`
  skips a solver leaf only when the wanted slot equals that. An edit of the leaf,
  by template or override, reaches the copy and the solver works from it; an edit
  of anything else leaves the solved pose. Also fixed:
  - **Undo, redo and replay let go** of a copy they leave in no cluster
    (`StoreCore::release`, shared with `post_apply`): undoing a transaction that
    constrained a copy and edited its template left the copy at the edit's value,
    and the owners' check failed on the next transaction (Opus; older than step 1).
  - **A release or a recorded write that puts a binding back regraphs,** so the
    graph reads it.
  - The release finds held copies in a set, not by scanning clusters per copy.
  - Tests: `moving_a_held_copy_moves_it`,
    `a_template_edit_of_a_held_leaf_reaches_the_copy`,
    `a_released_copy_reads_its_templates_binding`,
    `undoing_constraining_a_copy_and_editing_its_template`,
    `undoing_a_template_edit_of_a_pinned_copy`,
    `a_fixed_copys_binding_replaced_by_a_constant`.
  - **Recorded, not fixed (older):** repeat settling at run time (a preview, a
    publish) does not rebuild cluster membership, so a removed pinned row stays a
    cluster member until the next transaction, and one brought back by an
    unpublish comes back at its template (Sol). And a solved copy's pose is not
    saved: load derives copies from templates and does not solve, so a pinned copy
    loads at its template until the next transaction solves (Sol). Saving solved
    copy poses belongs with phase 5's "the pose is state".

- **Council round 4 on step 1 (2026-11-19): Claude Opus 5.5 7/10, GPT Sol 6.2
  6/10.** Round 3 held; three narrower cases fixed, each with a test that fails with
  its fix undone:
  - **A transaction's own edit of a held copy's solver leaf is always written**
    (Opus): moving a copy to its template's value, or to the override it already
    had, left the wanted value as before, so the edit was dropped. `run` gives
    `Owners::edit` the leaves its ops set, cleared once its expansion is done
    (`moving_a_held_copy_to_the_value_already_wanted_moves_it`).
  - **An edit landing on the solved pose is undone to it** (Sol): when what is
    wanted for a held leaf changes, or the transaction edits it, expansion writes it
    even unchanged, so `keep_solved` records the solved value for undo
    (`undoing_an_edit_onto_a_fixed_copys_pose`).
  - **An owner left with problems records no pieces** (Sol): a binding a failed
    expansion could not install was recorded as wanted, and the retry kept the old
    constant (`a_failed_expansion_does_not_record_what_it_wanted`).
  - **Recorded, not fixed:** past the node budget, the full expansion's retry
    recreates a held copy the incremental pass cut, at its template's value (Sol).
    It needs the budget to cut a held copy; left with the budget path's other
    costs (stage 1's exit notes).
  - Step 1's council stops here: four rounds, the findings narrowing to these.

- **2c stage 3, step 2: a definition edit keeps the copies it does not change
  (2026-11-19).** Each *piece* (an entity's body, a picture, an inner picture, an
  item) records the template and environment its last accepted expansion copied
  (`Owners::made`, through `Delta`, so a refused transaction records nothing; `Env`
  moved from `Rc` to `Arc` for it). When a piece is wanted again with an equal
  environment, its template is compared with the recorded one once a pass
  (`Template::same_nodes`: same source and reference classes, and each node equal
  by local), and a copy whose node is unchanged and which is there is wanted as
  *kept*: not materialised, and not compared by `apply_wanted`. `want_scopes` reads
  a kept copy's facets from the document only where it derives something (an
  instance, a picture, a repeat). An owner with problems keeps nothing; `parity`,
  `wanted_slots` and the oracle keep nothing, so the owners' checks compare every
  copy. Saying every node is unchanged fails 22 tests under the owners' check, 10
  without it.
  - Measured (`instance_cost`, native release, load about 6, before → after):
    definition edit 100 meters 4.57 → 2.46 ms, **700 meters 33.5 → 19.8 ms**,
    1,000 meters 70.8 → 43.0 ms; placement, structural and constant edits and the
    plot's steps unchanged within noise.
  - **Where the 700-meter edit goes now:** wanting 12 ms (interning 46,900 copy ids,
    deciding to keep each, pushing the wants), collecting what the owners hold
    2.5 ms, `apply_wanted` 3.6 ms (was 17). Reusing a parent's id by index did not
    measure. The rest is the per-copy id work that phase 4's slots and their
    lock-free id index replace (D15); not pursued in 2c.
  - An entry outlives its owner (bounded by the pieces ever expanded); a copy is
    kept only when it is there.

**Stop and report** as §5 says, and also if the Tree's join (stage 1) cannot be made
exact, or a template cannot reproduce a copy (stage 2).

### 4.5 Before phases 3, 4 and 5

Phases 0 to 2 can be built from the spec as it is. The later phases cannot yet: the
spec's §14 ends with a list, "Open before a phase starts", of points where the last
review found the design incomplete (when a solved pose is adopted, the full set of
things that wake a parked owner, recording followed by another pass). The spec states
a rule for each, but those rules have not been reviewed or tested. Before starting
phase 3, 4 or 5, write that phase's tasks here, check each open point for that phase
against the code as it then is, and report to the user what you would build; do not
start the phase on the rule as written without that.

### 4.7 Phase 3: time (plan, 2026-11-19)

Written by the builder while the user was away, on the user's instruction to go on
through the plan and decide (§4.5 asks for a report before a later phase starts; this
section is that report, reviewed by the council, and the build follows it).

**Where phase 2c ends.** 2c's exit holds as far as 2c now reaches: the Tree equals
the copies-in-document reference on every script (the differential suite and the
10,000 random scripts, green throughout), and memory per meter is **110.9 KiB**
against phase 0's 92.4 (2c stage 1: 110.5; the difference since phase 0 is 2b's
graph and owners, not the copies). Lazy slots, validation from templates and D13's
sweep moved to phase 4 (D14, D15, D13); stage 4 (cells from templates) is not
justified by any measurement, so it is not built. **`InDocument` stays** as the
differential reference until phase 4 changes how copies are held again.

**What phase 3 replaces.** Six host systems, each with its own liveness bookkeeping:
`motion::advance` (layout glides, presence's presentation), `advance_machines`,
`advance_steps` (traces, accumulators), `advance_queries`, `advance_presence`, and
`clock::advance` (the playhead player: rate, loop, stop at the last clip). Each calls
a store method that steps one kind of thing and publishes under its producer, then
maps what is active to compositions and marks them live.

**Steps** (each green on the differential suite and the smoke before the next):
1. **One entry point.** `Store::advance(now, paused) -> Advance { wake: Wake,
   queries }` runs today's six in today's order (the player included, moved into the
   store) and returns `Wake { next, live }`: `live` the compositions to redraw
   (today's six sets, joined), `next` `Some(now)` while anything continuous is
   active, else the earliest timed entry. The host's six systems become one. No
   behaviour change; the host reference and the smoke are the check.
2. **The scheduler.** Each stepper reports when it next changes (§10.3's table):
   a trace at its next arrival (continuous only while a dash or age is drawn), a
   query at its debounce or timeout, a machine continuously while a transition runs,
   presence and glides while they move, an accumulator while `next` differs from its
   value, an animated leaf while the playhead plays over its keys. `Wake.next` is the
   earliest; nothing due is `None`. A clock that jumps (`set_time`, a loop, a rate
   change) asks again. Exit test: an idle document with a trace and an accumulator at
   rest schedules no wake.
3. **Record, then stabilise again** (the open point). Today a machine driven by
   another's channel is brought level by a fixed 4 rounds (`advance_machines`), and a
   trace with no delay passes its change through in the same frame because the
   stepper polls after publishing. The rule as written: alternate until nothing is
   recorded, at most 8 rounds, then a problem on the logger still recording. Built as
   a loop around the steppers in `advance`; the 4-round loop goes.
4. **`now()` and `clock()`** in formulas, and the `clock` facet as a clock provider
   (offset, rate, paused, loop) resolved along the model's tree for a body and the
   picture's for a picture (§10.1).
5. **Exit:** `fiber.txt` and `pulses.txt` pass; the idle test; a machine's
   transition, interrupted transitions included, matches the old player's.

**Decisions (the builder's):**
- **Steppers stay producers** publishing Input cells, with the scheduler beside
  them, rather than becoming cells of the graph. What the cell form buys is
  necessity (an unobserved stepper skipped), which is phase 4's observation; the
  values and their timing are the same either way. Revisit with phase 4.
- **`paused` stays the host's** (an accumulator in a composition no view shows holds
  still) until phase 4's observers can answer it in the store.
- **The playhead player moves into the store** so `Wake` can say when the playhead
  next matters; the host keeps only the frame loop.

**Phase 3, step 1 done (2026-11-19): one entry point.** `Store::advance(now,
paused) -> Advanced { wake: Wake { next, live }, queries, changed }` runs the layout
glides, machines, traces and accumulators, queries, presence and the playhead player
in the host's old order; the player moved from `document_bridge/clock.rs` into the
store (`advance_playhead`: host time since the last advance times `rate`, the loop,
the stop at the last clip). It joins what each reports into the compositions to
redraw, and `changed` includes the frame something comes to rest. The host's six
systems are one (`motion::advance`). The per-kind methods stay public: about 120
tests step one kind at a time. `next` is `Some(now)` while anything is live, else
`None`: the scheduler (step 2) makes it exact. Checked: the suites, the host
reference, `smoke.txt` and `regression.txt` unchanged, `fiber.txt` (five
assertions), `pulses.txt` (four) and `idle.txt` (`changed 0`).

**Phase 3, the rest (2026-11-19), and its exit.**
- **Step 2, the scheduler: not built further, by decision.** Every time-dependent
  thing in the store today reports itself while it moves (a trace while a change is
  in flight, an accumulator while restless, a query while waiting, a machine while
  running, presence and glides while they move, the playhead while it plays), so
  `Wake { next: Some(now) }` while anything is live and `None` at rest is already
  exact for the host, whose frame loop runs every frame. Times further ahead (a
  trace's arrival with nothing drawn meanwhile, a query's debounce) would only let a
  host sleep between frames; they wait for a host that does, and for phase 4, where
  what is drawn decides what must run. The exit test passes:
  `a_trace_and_an_accumulator_at_rest_schedule_no_wake`.
- **Step 3, record then stabilise:** the machines' fixed 4 rounds are now up to 8
  rounds at one time; a chain still moving after 8 is a `machine` problem on a node
  still changing (`StoreCore::machines_unsettled`, in `problems()`). A trace with no
  delay already passes its change through in the step that sees it (`step.rs`).
- **Step 4, `now()`, `clock()` and clock providers: deferred.** They add a time
  input that the graph tracks as a dependency and a clock-provider facet, which is
  formula and authoring work (timeline.md §11's gap) that phase 3's exit does not
  need. Left for its own change.
- **Exit (§17):** `fiber.txt` (five assertions) and `pulses.txt` (four) pass; the
  idle test passes, and `idle.txt` draws nothing at rest; machine transitions,
  interrupted ones included, are the old player's (the machine suites unchanged:
  the player itself did not change, only who calls it).
- **Phase 3 is done** as far as its exit; what it leaves is above. **Next: phase 5's
  first step** (a cluster solved only when something of it changed, in the old
  engine first), ahead of phase 4: phase 5 does not depend on 3 or 4 (§17), and phase
  4 now also carries lazy slots, validation from templates and D13.

### 4.8 Phase 5, first step: a cluster is solved only when something of it changed (2026-11-19)

Built in the old engine, as §17 asks, before any of phase 5's cells. Each cluster's
last converged solve records what a solve of it could read (`solve::Posed`: the
cluster as grouped, with its constraints' values, operands, anchors and roles; every
leaf of every operand node as it reads; its other inputs), keyed by its lowest
constraint (`Cluster::key`; today's `Cluster::id`, the parent, can be shared). A
cluster with no driver and no finger on it whose `Posed` is unchanged is not solved
again, in `post_apply` and in `resolve_clusters_live` alike (`StoreCore::unmoved`).
It rests on A6: from a converged pose, with unchanged inputs and nothing pulling, a
solve returns that pose. `Posed` is a superset of what `build_problem` reads, since
the problem is built from the document as well as from `read`.
- **Checked:** under `IRON_OWNER_CHECK` a cluster left unsolved is solved anyway and
  must come out within 1e-6 of where it is (`check_unmoved`); the differential suite
  passes with it. A6 holds there. Not covered: the random scripts build no
  constraints.
- **Test:** `a_drag_in_one_cluster_solves_no_other` (two clusters; a drag in the
  pendulum's solves one). It fails with the skip off.
- `posed` is in `Saved` and in `scratch`, and entries of clusters that no longer
  exist are dropped each `post_apply`.
- **`gestures.txt` (2026-11-19):** run against this step, it exits 1, but on stale ids
  older than this work: it still names the bar `#12`, the laser body `#1f` and the
  dial `#1u`, which `00151fd`'s palette tokens moved (`6d2199d` fixed the same in
  `regression.txt` by reading ids by name). The steps naming them fail (`#12 is not
  live`, three `TypeError`s in the gesture-ownership step); the rest runs. **Fixed:**
  the script reads the bar, and the Optics composition's body and dial, by name; it
  exits 0, and gesture ownership behaves as its comments say (the edit mid-drag
  cancels it at x 80, the other drag commits at 180, 190, three undos give 60, 140).
- **Not done in phase 5:** the rest (layout cells with containment, the network
  solve per island, clips as cells, the solver as Clusters and Solve cells with a
  solved layer and a commit step). The step above is the part that changes what is
  solved; the cells change how, and need phase 2b's graph to grow Structure cells
  for clusters.

**Council on phase 3 and phase 5's first step (2026-11-19): Claude Opus 5.5 7/10,
GPT Sol 6.2 5/10.** Opus confirmed `advance` matches the six systems (order, the
playhead's step, loads, scratch) and that `Posed` covers what a solve reads. Fixed,
each with a test that fails with its fix undone where one could be written:
- **A reparent could keep the same ancestor leaves** (Sol): swapping two operands
  between parents left the union of their ancestors' leaves equal, so the cluster
  was skipped and a broken pair of fixed operands accepted. `Posed` records each
  operand's ancestors (`swapping_constrained_operands_between_parents_is_solved_again`).
- **A live solve recorded its preview as solved** (Sol): `resolve_clusters_live`
  recorded `Posed` from the overlay, so the next commit skipped the cluster and its
  answer never reached the document. Only `post_apply` records now; a live solve
  compares with the last commit (`a_live_solve_is_written_by_the_next_transaction`).
- **A channel publishing refuses was "still moving"** (Opus, Sol): a bound channel,
  or one another producer holds, is a signal error and never shown, so every frame
  ran all rounds and reported a false `machine` problem. Refused leaves are left out;
  the loop also checks once after the last round, so 8 links settle
  (`a_refused_channel_is_not_an_unsettled_machine`).
- **`Wake.next` while time moves undrawn** (Opus, Sol): the playhead playing with
  nothing animated, or a trace at the root driving a shape, gave `None`. `next` is
  `now` whenever anything is active (`a_playing_clock_asks_for_the_next_frame`).
- **The host's live set** (Sol): the gas system removes its compositions from the
  shared `LiveSources`; `motion::advance` returned early when its own set had not
  changed, so a composition both kept live was dropped. It re-adds every frame.
- `Posed` reads only the operands' transform, size and radius facets, and `posed`
  is pruned with a key set, also when the last cluster goes.
- **Recorded:** playback started before the first `advance` after a load steps 0 on
  that frame (Sol; `advanced_at` starts empty, the old player used Bevy's delta).
  `Advanced::changed` has no reader in the host today (Opus).

**Phase 5, step 2 (2026-11-19): the per-transaction scans, measured and the dominant
ones removed** (§17: "the remaining per-transaction scans measured and, where they
dominate, indexed"). Sampled with macOS `sample` on a native release build with line
tables (`CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`), a loop of each edit at 700
meters (below the node budget). Found and fixed, each a whole-document pass:
- **A structural edit copied the derived signals and anchors whole** (80 % of it):
  `Reactive::restructure` cloned `derived` when nothing could change it, compared
  the copy entry by entry and dropped the old one. Without `rederive` it is left as
  it is.
- **An edit that rederives made every anchor's position again** (most of a
  placement): an anchor's `wx`, `wy`, `wrot` are a function of its chain of
  transformed ancestors alone, so `Reactive::chains` keeps the chain each was made
  from, and only an anchor whose chain changed is made again (`anchor::chain`,
  `positions`, `followers`; `anchor::derived` is gone). Signals and followers, which
  follow connections, are still made whole.
- **`plug::implied_mates` worked out every port anchor** (strings, compositions,
  reads) on every transaction, twice (`rebuild_clusters` and `post_apply`), to keep
  the connected ones: it finds the connection first.
- **`Changes::remove` scanned what it had already noted** for each node a delete
  took: quadratic in a subtree's size (`removed_ids`).

Measured (`instance_cost`, native release, before → after):

| | 100 meters | 700 meters | 1,000 meters (past the budget) |
|---|---|---|---|
| place one more | 0.54 → 0.38 ms | 3.63 → **0.45** ms | 11.45 → 5.98 ms |
| structural edit | 0.65 → 0.05 ms | 6.86 → **0.32** ms | 30.9 → 22.4 ms |
| constant edit | 0.10 → 0.05 ms | 1.45 → 0.29 ms | 1.50 → 0.48 ms |
| edit plan | 3.74 → 3.09 ms | 28.3 → 19.7 ms | 48.0 → 40.4 ms |

Below the budget §15's meter targets are met at 700 meters (structural under 2 ms,
placing under 1 ms). At 1,000 meters the document is past `expand::NODE_BUDGET`,
where the rule is to expand everything on each edit so the cut never depends on
history (`Owners::expand`): that is the budget's cost, not a scan, and lazy slots
(D15) are what remove the budget. **The user's decision (2026-11-19): the fast path
only below the budget is acceptable;** an incremental cut (owners in id order up to
the budget, from the counts `Owners` keeps) is the fix if it becomes a problem, and
is not built until then. What is left in a constant edit is two linear
walks over ports and anchors (`port::network`, the mates' anchor walk), about 0.1 ms
each at 700 meters. A definition edit and an edit plan are the per-copy id work 2c
stage 3 step 2 recorded.
Verified: differential suite (478) with IRON_ATOMICITY and IRON_OWNER_CHECK, plain,
app lib tests, build, 10,000 random scripts; smoke, regression (identical), fiber,
pulses and idle scripts.

### 4.9 Phase 4: observation (plan, 2026-11-19)

Written by the builder on the user's instruction to go on and decide (§4.5's report).
Phase 4 now carries, besides §17's row, what 2c moved here: lazy slots (D15),
validation from templates (D14) and the path sweep (D13).

**Where the code is.** The graph (`graph.rs`) evaluates every bound leaf eagerly
(height order, equality cutoff); nothing knows what is observed. The host decides
liveness from its views (`document_bridge/liveness.rs`: `Liveness`, `reached`) and
passes the store one thing, `paused` (accumulators of compositions no view shows).
Owners exist (stage 2) but have no states; every copy is stored.

**Steps**, each with the differential suite green under `observe_all` (today's
behaviour, the reference mode, §8.2):
1. **Observers in the store, no behaviour change.** `observe(Interest) ->
   ObserverId`, `reobserve`, `unobserve`; `Interest::{Region, Nodes, Look, Pin}`
   (Volume with 3D, later). The host registers its panels' and embeds' views
   instead of computing `paused` itself; `observe_all` is the default and what the
   differential test runs. `paused` comes from the store.
2. **Necessity in the graph.** A cell is necessary when an observed leaf needs it,
   or by §8.3's list (state: a session `let`, a trace, an accumulator, a machine, a
   query; what a solve reads; what a repeat's count reads). Only necessary cells are
   evaluated each pass; the rest are marked stale and evaluated when read
   (`Reactive::read` of a stale cell evaluates it and what it reads). Exit check:
   under `observe_all` nothing changes; with one composition unobserved its pure
   pictures cost 0 evaluations (§17's first exit).
3. **Dormant pictures.** A picture owner none of whose bound meets an observed region
   keeps its cells but is not evaluated; drawing it uses its last values (§8.5's
   extent cell keeps its bound right). Needs the spatial index's owner entries
   (§12.3).
4. **Unloaded owners, with the lazy slots (D15) and validation from templates
   (D14).** An Unloaded owner has no cells and no copies: its copies are answered
   from slots filled on read (the 2c stage 3 design and its council findings,
   §4.6, are the starting point: pieces, an id index without the registry's lock,
   exceptions as effective state, solver values kept), and validation of ops
   touching it reads templates. D13's sweep comes with the per-store id index.
5. **Waking:** the wake budget, the idle sweep, reads that never load (§8.9), a
   parked stateful owner woken by what it waits on (§8.3, A13).

**The open points (spec §14) for phase 4, against the code as it is:**
- *When an owner is at rest, and what wakes a parked one (§8.3, A13).* Today nothing
  parks: every owner is loaded. Steps 1–3 do not park anything (Dormant keeps cells,
  and stateful owners stay Live), so A13 is first needed at step 4. **Decision:**
  at step 4 only pure pictures are Unloaded; stateful owners never are, until A13's
  property test (§8.8) exists.
- *A parked Body and its port readers (§8.4).* Bodies stay Live in steps 1–4
  (only pictures go Dormant or Unloaded): a port reader never meets a parked body.
- *A simulation range:* not designed; not built, as §8.3 says.
- *A 3D owner's declared extent (§8.10):* 3D views stay `observe_all`-like (every
  owner they show Live) until a `Volume` interest is designed.
- *The limits (D9's owner count, the wake budget, the height to recompute):* set
  from measurement when step 5 is built.

**Decisions (the builder's):** the steps above, in that order; necessity before any
owner state, because it gives §17's first exit on its own; pictures only, never
bodies or stateful owners, change state in this phase.

**Phase 4, step 1 done (2026-11-19): observers in the store.** `observe(Interest)
-> ObserverId`, `reobserve`, `unobserve`, with `Interest::{Region { composition,
rect, zoom }, Nodes}` (Look, Pin and Volume when something needs them).
`observe_all` is a store's default (today's behaviour, and the differential
reference); a host that registers what it shows turns it off. What it changes now:
`advance` no longer takes `paused` from the host; the store works it out from the
observers (accumulators in a composition no observer reaches on an open path hold
still, as the host's `liveness::reached` decided, now deleted). The host's
`motion::observe` keeps one region per composition each panel and open embed shows,
registering again after a load; the region is the whole composition until views
report their visible rectangles (step 3). Test:
`an_accumulator_steps_only_where_observed`.

**Phase 4, step 2 done (2026-11-19): necessity in the graph.** The store decides
which cells nothing observed needs (`Store::necessity`, `Reactive::necessity`): the
cells of a composition no observer shows (with what its embeds show, `composition::
shown`, at any run mode), except on nodes the store reads besides drawing them
(`ROOTED`: state, structure, ports and equations, constraints and a cluster's nodes,
anchors, the timeline), and except what any needed cell reads. Those cells are not
evaluated when their inputs move; they are marked stale (`Reactive::is_stale`) and
keep their last value (§8.9). Exactness, the builder's decision: a transaction, an
action (`fire_at`), a session write and a scratch plan first bring every stale cell
up to date (`exact`), as does any structure change inside the graph (`relax`), and
the store decides necessity again at the next `advance` or observer change; so the
saving is in frames (publishes, steppers, the playhead), which is where §17's exit
is measured, and a transaction reads and writes what it did before. A tracked read
(`deref`) that reaches an unneeded cell makes it and what it reads needed, and
evaluates the stale ones before reading again. Tests:
`an_unobserved_compositions_pictures_cost_no_evaluations` (§17's first exit: a
publish costs the shown picture 1 evaluation where observing everything costs 6;
showing the other composition brings it up to date at once, equal to the
reference), `a_tracked_read_into_an_unobserved_composition_reads_it_current`,
`a_pulse_from_an_unobserved_laser_reaches_an_observed_meter_on_time` (§17's third
exit: frame by frame equal to the reference, the hidden fiber's light resting), and
§8.8's property over the random scripts
(`observing_part_of_the_time_changes_nothing_once_all_is_observed`, 40 seeds; 1,000
ignored): a store observing the composition at random accepts the same transactions
and, once it observes everything, reads every leaf and problem as the reference.
The property found an existing bug, fixed: ending a gesture dropped its previews
without deriving again (a cancelled plot pan left the tick repeat's first item at
the previewed value) unless the playhead had moved. Open: problems of an
unobserved composition's pictures are as of their last evaluation (§8.7's idle sweep
is step 5); the host reads `resolved` of hidden nodes (the document panel, the
agent's reads) as last values, which §8.9 allows; Look interests are not built.
Verified: differential suite (461) with IRON_ATOMICITY and IRON_OWNER_CHECK, plain,
app lib tests, build, 10,000 random scripts; smoke, regression (the pendulum's last
digits only), fiber, pulses and idle scripts.

**Phase 4, step 3 (RE-4.3) deferred (2026-11-19): Dormant by view rectangle.** Moved to
[`handoff-render.md`](../handoff-render.md) §10 when this plan closed (2026-11-21), with
the reasons.

**Phase 4, step 4 deferred (2026-11-19, the builder's decision): Unloaded owners and
lazy slots.** This plan's own rule (above) unloads only pure pictures, never a
stateful owner until A13's property test exists. Among the standard components only
the readout's and the slider's pictures are pure: the laser's, fiber's and meter's
hold port anchors that connections follow (needed whatever is observed), and the
plot's holds lets, nine repeats and, per §8.3, its `vis` machine. So step 4 as
planned would free the copies of readouts and sliders in hidden tabs, for the
largest change left in the engine (per-owner pieces in place of the copies document,
validation from templates, an id index without the registry's lock: the design the
2c stage 3 council scored 5/10). What would make it pay is parking stateful owners
(§8.3, A13), so that a plot in a hidden tab keeps its state and frees its 281 copies.
That is the next design to write, with D13's sweep and validation from templates in
it, and memory measured on a document of several tabs first. D15 (lazy slots, the
user's goal) stands; this records why it is not built yet.

**Phase 4, step 5 in part (2026-11-19): the idle sweep** (§8.7). An `advance` with
nothing active brings up to 256 stale cells up to date (`Reactive::sweep`, lowest
first, following what turns stale behind them), runs the pipeline after them, and
asks to be called again while any are left; so an unobserved composition's values and
problems are current soon after things come to rest. Test:
`at_rest_stale_pictures_are_swept_up_to_date`. The wake budget and reads that never
load concern Dormant and Unloaded owners, which are not built (steps 3 and 4).
Verified: differential suite (463) with IRON_ATOMICITY and IRON_OWNER_CHECK, plain,
app lib tests, build, 10,000 random scripts; smoke, regression (unchanged), fiber,
pulses and idle scripts.

**Phase 4, steps 1–2: council fixes (2026-11-19).** Sol scored the two commits 4/10
(Opus's review was cut short, no score): the store reads cells outside the graph's
edges, so necessity, which follows only the edges, let a handle commit a value held
to a stale maximum, layout and the network's solver publish from stale sizes and
branches into a shown composition, and the machine player start a transition with a
stale duration. Fixed:
- **Reads outside the graph are recorded** while the store's own code runs
  (`Store::recorded`: `derive`, the time phases of `advance`, and `current` for a
  handle's target and an action's writes). A read of an unneeded cell makes it and
  what it reads needed (`Reactive::need`), and it stays needed (`pinned`, until a
  rebuild); if that brought a stale cell up to date, the computation runs again
  (`pipeline` up to four times, then from an exact graph). The host's reads are
  not recorded: what it draws of a hidden composition may be stale (§8.9). Checked
  in the app: through the regression script nothing is pinned and 55–79 cells stay
  unneeded.
- **What the machine player reads is rooted:** `Args`, `Transition`, `Tween`. It
  keeps state, so a second run is no answer.
- **A tracked read's wake comes before its cycle check:** a stale cell's edges are
  what it read when last evaluated, so the reader runs again after it (false cycle
  otherwise). The wake adds four evaluations per cell it takes to the budget (a
  chain of 1,100 reported "did not settle").
- **An accumulator a shown picture reads keeps stepping** (`Reactive::feeds`).
- **A session write of constants does not relax** (the playhead each frame did).
- **Necessity is decided again** when a tracked read leaves a cell a wake made needed.
- **The host observes after resolving the views**, in the same frame a tab switch
  shows (`motion::observe` last in PreUpdate).
- `is_stale` documented as where evaluation stopped, not every old value.

Not taken: a rejected session write's wake (it only brings cells to the values the
reference holds). Tests: `a_hidden_stack_places_from_current_sizes`,
`a_tracked_read_into_a_stale_cell_finds_no_false_cycle`,
`a_tracked_read_wakes_a_long_hidden_chain`,
`a_handle_on_a_hidden_slider_reads_its_current_range`,
`a_hidden_accumulator_a_shown_picture_reads_keeps_stepping` (each fails before).
Verified: differential suite (468) with IRON_ATOMICITY and IRON_OWNER_CHECK, plain,
app lib tests, build, 10,000 random scripts, the 1,000-seed observation property;
smoke, regression (the pendulum's last digits only), fiber, pulses and idle scripts.

**Phase 4: council round 2 fixes (2026-11-19).** Opus 5/10, Sol 5/10 on the four
commits. Fixed:
- **Every derivation outside a transaction is recorded.** `preview`, `unpreview`,
  `set_time`, `set_state`, `resume_gesture`, the session write's clock sync, the
  selection follow, presence's resettle, the rebuild's live pass went to
  `resolve_physics` directly; they go through `pipeline_from` now, and a theme, a
  font or a layout preview through `relayout`.
- **Any recorded read reruns the derivation,** not only one that was stale when
  settled: a tracked read inside the graph can wake the same cell first (the
  network's `Q = P > 0.5 V ? deref(w).x × 1 V : 0 V` published 0 V).
- **A cycle check waits for a reader still queued in the pass** (a nested wake beneath
  it), and a woken reader requeues after everything waiting.
- **A trace carries necessity** for `Store::paused` (`feeds` follows a trace's or an
  accumulator's input to its outputs).
- **The idle sweep:** runs only when nothing was evaluated since the last `advance` (a
  host producer publishing every frame made it evaluate each frame what necessity
  saved); a round returns all it took out of `unneeded` (a swept cell's reads and a
  tracked wake in it left cells needed until the next decision) and wakes only what a
  needed cell reads; `budget` counts evaluations, checked between rounds (one
  tracked wake in a round may overrun it once).
- **Necessity re-decided when a tracked read leaves a candidate cell** (`candidates`,
  the hidden unrooted cells as last decided, in place of `woken`, which a refresh
  cleared).
- **`Store::exact` is public:** the inspector's unbind and empty bind (they keep the
  value shown) and the agent's `view` (it reads to write back) call it first.
- **Spec §8.8:** an accumulator in a composition no observer reaches holds still
  (demo.md's rule, older than this spec) unless what it feeds is read outside the
  hidden compositions; listed outside the property beside `pause`.

Not taken: picking in the frame of a panel switch uses the previous spatial scene
(the picker runs before the scene is rebuilt, observed or not: older than phase 4).

Known, next: layout lays out every group, so a hidden layout's children's sizes
bound to something moving are recorded and pinned at the first pipeline (none in the
demo: nothing pinned through the regression script). Skipping a hidden composition's
layouts unless a needed cell reads their placements is the fix when a document needs
it.

Tests: `a_preview_lays_out_a_hidden_stack_from_current_sizes`,
`a_nested_tracked_wake_finds_no_false_cycle`,
`a_hidden_accumulator_read_through_a_trace_keeps_stepping`,
`a_hidden_cell_rests_once_the_tracked_read_that_woke_it_leaves`,
`the_idle_sweep_keeps_what_a_woken_cell_reads_needed`,
`the_network_reads_a_hidden_rect_through_the_branch_its_solution_opens`,
`a_publish_every_frame_leaves_hidden_pictures_resting` (each fails before).

**The observation property test strengthened (2026-11-19).**
`differential_random::observed_script` adds a second composition whose pictures read
the sheet's through a reference a producer moves or a preview moves, one of them laid
out in a stack, and a sheet picture that reads them back; the observer moves between
none, either and both. Each step compares the committed transactions whole and every
leaf of the shown compositions before and after `advance` (a tool previews after
`advance` and draws in that frame: compared only after, the idle sweep had repaired
round 1's stale layout first, and 1,000 seeds caught nothing). A failed binding
counts by its reason, not the value it holds (its last computed, which depends on
when it ran). Against `a2e530d` it fails at seed 14; on this code 20 seeds (the
default, ~8 min in the gates' debug build with both checks) and 1,000 pass. Spec
§8.8 now lists a value on a loop through layout or the solver as outside the property
(each run moves it a step, observed or not): the test's reference never points the
viewer at its own pictures.

**Phase 4: council round 3 fixes (2026-11-19).** Opus 7/10, Sol 6/10. Fixed:
- **A derivation reruns only after reading an old value:** a recorded read notes
  whether a stale cell was it or upstream of it (`Reactive::behind`); one that was
  current costs no second run (an extra run moves what reads a later stage, a repeat
  counting a placement: Sol's case, not reproduced here).
- **A cycle check waits once for everything still queued in the pass,** not only what
  it reads directly (a hidden `K` two hops down still to switch its tracked read).
- **Pausing is decided per composition:** a hidden composition read from one that
  runs, or from outside every composition, runs too, to a fixpoint (`read_by` in place
  of `feeds`): a trace, a stack's placements or another hidden tab in between.
- **The idle sweep waits for a still frame by propagations, not evaluations**
  (`Reactive::propagations`): a publish only hidden pictures read evaluates nothing
  but kept the sweep running every frame.
- **Host commands that author from values make the store exact first:** Make
  Component, Author, Link, Detach, Component, the table and text-style commands, run
  mode, the timeline's axis, the inspector's set, convert to path when forced, and SVG
  export (a file handed to the user). The select tool's convert commits only when
  nothing reads the shape (no value baked), so it needs none.

Tests: `a_tracked_wake_two_hops_down_finds_no_false_cycle`,
`a_hidden_accumulator_read_through_layout_keeps_stepping`,
`a_publish_only_hidden_pictures_read_leaves_them_resting` (each fails before).

Verified: differential suite (478) with IRON_ATOMICITY and IRON_OWNER_CHECK, plain,
app lib tests, build, 10,000 random scripts, the 1,000-seed observation property;
smoke, regression (the pendulum's last digits only), fiber, pulses and idle scripts.

### 4.10 Phase 5's cells: the general cell (plan, 2026-11-20)

What is left of §17's phase 5 is the network, layout and the solver as cells. RE-5.1
(a cluster solved only when something of it changed) and RE-5.2 (the scans) are done
(§4.8).

**The user's decision (2026-11-20):** build one general cell in the graph, with the
binding as one rule among others, not a second special kind beside it. Spec §5.1
already describes a `Cell` with a kind; 2b built only the binding
(`BTreeMap<Leaf, BoundNode>`). Everything else that computes is a producer outside
the graph.

#### Why: what is wrong today

The pipeline runs outside the graph's height order. `resolve_physics` runs repeats,
state, the network, the theme, layout and contexts in a fixed order. Each is a Rust
function that reads through plain lookups, so the graph has no edges into it and
sees only what it publishes.
- **A stage that reads a later stage's output is one step behind, indefinitely.** A
  probe (2026-11-20): a row with a `fill` child and a repeat with
  `count = floor(child.size.w / 50)`.

  | Step | Width | Items | Should be |
  |---|---|---|---|
  | Load | 300 | 0 | 6 |
  | After an `advance` | 300 | 0 | 6 |
  | A transaction setting width 500 | 500 | 6 | 10 |
  | Drag frames at 200, 400, 250 | | 10, 4, 8 | 4, 8, 5 |

  `pipeline_from`'s rounds repeat only when a stage read a hidden cell's stale value
  (`settle_reads`), so nothing repeats here. This is §17's exit test, and it fails.
- **Cost.** The one §15 target this phase still misses is the solver (`solve_cost`,
  native release): an unrelated edit costs 0.19 ms with one pendulum and **2.82 ms
  with 100 at rest**. Every transaction regroups every cluster, and `unmoved` builds
  every cluster's `Posed`.

#### The design (revised after the council, 2026-11-20)

The first draft scored 6/10 from both members (Opus 5.5, Sol 6.2); their findings
are below. Three changes come from them: outputs are claims in the runtime layer, not
cells keyed by leaf; output sets and indexes are fixed by structure; and every read a
native makes is tracked, its non-value inputs included. `Env`, native state and index
natives that make cells inside the pass are dropped or deferred.

**A cell** is keyed by a `CellId`:
- `Leaf(Leaf)`: a Formula, today's `BoundNode`. Leaf-keyed cells stay Formula-only, so
  `restructure`, `rebuild`, `is_bound`, `expr_of`, `contexts` and `transitive_deps`
  keep their meaning (Opus 1).
- `Native(NativeKey)`: an engine computation keyed by what it computes for. It has a
  `ProducerId`, a fixed output set, and the leaves it read last time.
- `Input(InputKey)`: a source with a revision the store bumps, for what a native
  consults that is not a leaf. In RE-5.3 there is none; RE-5.4 adds the font generation
  and the layout preview per container, and RE-5.5 the fingers and drivers. So no
  call site has to "prime every layout" by hand (Opus 3, Sol 4).

```rust
trait Native: Send + Sync {
    fn outputs(&self) -> &BTreeSet<Leaf>;                    // fixed at structure time
    fn compute(&self, cx: &mut Cx) -> Result<BTreeMap<Leaf, Value>, Failure>;
}
enum Failure { Error(String), KeepLast(String) }            // KeepLast: did not converge
```

**Outputs are claims in the runtime layer** (Opus 1, Sol 6). A native's result is
written inside the pass with `publish`'s rules, under its `ProducerId`:
- first claim wins and a second is reported in `signal_errors`;
- a leaf a Formula binds is refused;
- the overlay still reads first;
- an output left out of the result is released and reads through to what is beneath.

`signal`, `driven_by`, `published_by`, `signals()` and `handle.rs`'s messages are
unchanged. The cutoff is `publish`'s per-leaf diff, released leaves included, so the
readers of a leaf that did not move do not run. An index `owned: BTreeMap<Leaf,
NativeKey>` takes a read of an output to its native, for heights (a reader of an output
sits above the native), priming, `behind`, wakes and necessity.

**The output set is fixed by structure, as a superset** (Opus 2): every leaf the native
could write. Layout chooses what to write from sizing words and fills, but a value
change never adds a claim, so a reader is never found below a native mid-pass. The
network's unknowns are structural already.

**Reads go through `Cx`, and every one is tracked** (Opus 3, Sol 1, Sol 4).
- `cx.read(leaf)` reads through the layers and records the leaf, constants included.
  So a transaction that only sets a constant re-runs the native through `invalidate`.
  `port::network`'s inlined literals, `solve::is_free`'s `constant()` and
  `layout::constrained`'s template reads change to read through `cx`.
- **Beneath.** `cx.read` of a leaf that the native itself owns records a separate
  identity, `Base(leaf)`: the authored constant or the facet's default, as
  `layout::base` reads now. A change of that constant primes the `Base` readers. So
  a layout reading a width it also writes is no cycle, and it still sees the
  author's change.
- Structure reads (children in order, parent and liveness, membership by type,
  relations) are recorded as such. `regraph` reports both the old and the new parent
  of a create, delete, reorder and reparent, and the ends of a link and an unlink, so
  `restructure` primes the natives that read them. Layout needs this, so it is built
  in RE-5.4, not before.
- A native gets no raw `Tree` and no `&StoreCore`. The network reads through
  `port::network(Tree)` plus `cx` now. Layout's ~50 helpers that take `&StoreCore`
  move onto a `Reads` trait that `StoreCore` and `Cx` both implement, as RE-5.4's
  first, mechanical step (Opus 9).

**Indexes are computed in `restructure`, not by cells** (Opus 5, Sol 5). Islands (and,
in RE-5.5, clusters) change only when the document is written. So `restructure` works
them out next to `derived_signals`, under the `rederive` flag, diffs them by their
full description (not only by key), and makes and drops the natives as one batch
before the pass. Making cells inside a pass is needed only by repeats (RE-5.4).

**No native state** (Opus 6, Sol 3). The network's warm start is the current value of
its own outputs, by leaf, as `x0` is today (`signal`). It survives islands merging
and splitting, and it is read outside the cycle rule (`cx.previous(leaf)`). The
solver's pose (RE-5.5) is designed there: an immutable `Arc` staged and adopted at the
public call's boundary, so `Saved` and `scratch` clones stay cheap and rollback
restores it with the rest.

**The pass** (Sol 2, Opus 8) takes two of the kernel's rules (`iron_engine_kernel`,
`engine.rs`):
- **R5:** a cell whose new reads raise it is queued again if anything queued sits
  below its new height. Today only a directly primed new read requeues it, and a
  native reads too much for that.
- **R6:** a cell blocked on a cycle is run again in any pass that removes an edge.
  Today only `restructure` retries it.

Each rule comes with a test that fails before it. A raise uses `raise(&[cell])`, not
the global `compute_heights`.

**Necessity** (Opus 7, Sol 7). Natives are found by node through an index, since
`CellId::Native` sorts outside the per-node leaf range. A native is a candidate only
when every node it writes is hidden. Islands are rooted (D1: the network integrates
from its warm start), as the network always runs today. Native failures live in their
own map: an Error native's outputs carry its error in `errors[leaf]`, so readers
fail as from a binding, and a KeepLast keeps its claims and reports a problem.
`errors` stays as it is.

**Verification** (Opus 4, 10, 11; Sol 8).
- Each move onto cells is preceded by the same change in the old pipeline, on both
  sides of the facade, exact. RE-5.1 was done that way. Then the move itself is exact
  against a reference that keeps the old pipeline behind a mode flag on `StoreCore`.
  A snapshot `INTENDED` entry stops comparing a script at its first difference, so
  none is used for these steps.
- `evaluations` is compared only by unit tests (the snapshot leaves it out), as the
  step says.

#### Steps

**RE-5.3: the cell, the pass's rules, the network.**
1. **Rekey.** The queue, heights, primed, `unneeded`, `stale`, `escaped`, `pinned`,
   `candidates` and `blocked` are keyed by `CellId`, with `Leaf` only, and
   `dependents` maps a `Leaf` to `CellId`s. No behaviour change: the suites, the random
   scripts, §8.8's property and the `evaluations` assertions in the unit tests are
   unchanged.
2. **Natives in the graph.** `Native`, `Cx` (tracked reads, `Base`, `previous`), outputs
   as claims, `owned`, the native index for necessity, failures, and R5 and R6. Unit
   tests in `graph.rs` with test natives:
   - a reader of an output runs once per pass;
   - a diamond through a native;
   - per-leaf cutoff and release;
   - `Base` is no cycle and still sees a constant change;
   - a constant-only transaction re-runs a native;
   - a cycle through a native is an error, and recovers when its edge goes (R6);
   - R5's requeue;
   - a hidden native is not run, and is run when read;
   - host-first and native-first claims.
3. **Islands in the old pipeline.** `solve_network` solves each island separately:
   unknowns joined by any shared equation, with any equation holding a `deref` joining
   all of its possible unknowns into one conservative island. Its warm start is by
   leaf and `physics_error` is per island. On both sides, exact.
   - Test: an edit in one circuit solves no other.
4. **The network on natives.** Islands are computed in `restructure` (above), with
   one native per island, rooted, KeepLast. `solve_network` stays only in the
   reference's mode, exact against it. The `ports`, `schematic` and `signals` suites
   stay green.

**RE-5.3, as done (2026-11-20).**
1. **Rekey** (`808c698`). Exact: the suites pass unchanged.
2. **Natives** (`f4fc24f`): `Native`, `Cx`, claims, `owned`, `base_readers`, failures,
   necessity, R5 and R6.
   - Nine unit tests in `graph.rs`. The R5 and R6 tests fail with their rule taken
     out.
   - R6 retries blocked bindings too. The suites are unchanged by it.
3. **Islands in the old pipeline** (`a34b77e`), on both sides.
   - Test: `a_circuit_that_cannot_settle_does_not_hold_back_another`, which failed
     before.
4. **The network on natives.**
   - `port::IslandNative` is one native per island, rooted and KeepLast.
   - `solve_island` is shared with the old pipeline, so the two agree to the bit.
   - `StoreCore::old_network` keeps the old pipeline in the differential reference,
     the store with copies in its document. The suites are exact against it.
   - `physics_error` is the unsettled island with the lowest unknown, on both sides.
   - Found while comparing: when islands merge or split, the new native starts with
     no claims. Its first solve can fail to converge (a connection mid-way), and the
     island's values then vanished where the old pipeline kept them by leaf.
     `set_natives` now passes a dropped native's claims on a leaf to the new native
     that writes it, in the same batch.
   - Test: `an_edit_in_one_circuit_solves_no_other` (`Reactive::native_runs`).
   - The network's structure is still found again on any document write
     (`docs.edits()`), as before, and only islands whose unknowns or equations
     changed are made again.
   - Not done: a `deref` in an equation that reads one of its own island's unknowns
     reads it beneath the claim (its constant), where the old pipeline read the
     last solution. No document does that today. (An equation's constant side was
     inlined as a literal, so editing it remade its island; the council round below
     fixed it.)

   Verified:
   - the differential suite (488) with `IRON_ATOMICITY` and `IRON_OWNER_CHECK`, and the
     plain suite;
   - the app's lib tests and the build;
   - the smoke, pulses and fiber scripts, and regression, the same as expected once
     line numbers and timings are stripped.

**Council on RE-5.3 (2026-11-20): Claude Opus 5.5 7/10, GPT Sol 6.2 6/10.** Fixed,
each with a test that fails with its fix undone (checked for three):
- **A native's cycle check came after its edges** (both). The closing read was
  installed, which left a cycle in the graph that every height repair then ran
  into. The check now runs first:
  - on the new reads only, except after cells were made or remade (`structure`),
    when it checks all of them, since a binding made over a native's output closes
    a cycle through a read the native already had;
  - a native that drops a closing read is put back above what it still reads.
  (`a_native_closing_a_cycle_fails_and_leaves_no_cycle_in_the_graph`)
- **R5 left raised cells at their old place in the queue** (Sol): a reader queued by
  its own input ran before the native. `raise` returns what it moved, and the
  queue is re-keyed (`a_reader_queued_on_its_own_runs_after_a_native_its_input_raised`).
  The scan stops at the native's height.
- **A binding made after a native took a height that ignored it** (Opus): `set_cell`
  reads heights through `cell_of` (`a_binding_made_after_a_native_sits_above_it`).
- **A structural transaction skipped `base_readers`** (both): `restructure` primes the
  natives that read a touched node's leaves beneath their claim
  (`a_structural_transaction_reaches_what_a_native_reads_beneath_its_claim`).
- **A host releasing a leaf left the native that may write it unclaimed** (Sol):
  `invalidate` primes the native of a changed leaf it does not hold
  (`a_native_claims_a_leaf_again_when_a_host_releases_it`).
- **Dropping a native cleared a binding's own error, and Error → KeepLast left the
  error** (Sol): a native clears only the errors it wrote (`errored`)
  (`a_native_clears_only_the_errors_it_wrote`).
- **Islands were made after the pass, not before** (Opus): `regraph` makes them first
  and the pass runs them (`set_natives` without `run` leaves them `pending` for it).
  `solve_network` still catches a write that reached no `regraph`.
- **An equation's constant side was a literal**: it is now read where it is
  (`Expr::Ref` to `equation.lhs`/`rhs`), so editing it re-solves the island.
- **R6 retries:** they run while a retry frees something, not a fixed four. Dropping
  a native counts as an edge going.
- **Clock gating** (`read_by`): a native counts at every node it writes, and a rooted
  one always (Sol).
- **Run counts** carry across a native remade under the same key.
- A native's readers are walked lazily.
- `ports::a_circuit_that_cannot_settle_does_not_hold_back_another` now undoes across
  the merge.

**Decided, and recorded as a behaviour change:** an equation that reads a binding
over an unknown of its own island is a cycle. The island fails with "a cycle", and so
does what reads it. The old pipeline solved it one step behind. Substituting the
binding's expression into the island is the alternative; it belongs with the solver's
fixed points (RE-5.5) if a document needs it. No document does today.

**For RE-5.4's sub-plan, not done here:**
- `Cx::read` beneath a claim ignores the overlay; without a claim it reads it. Layout
  must settle which it wants.
- `Cx::read` takes a failed binding's last good value, as the old pipeline did, not
  "depends on" (reactivity.md §4.1).
- `Cx::previous` reads only the native's own producer.
- R5 solves a new native twice when its first run raises it (every island at load and
  after a sync that remakes it).
- R6 re-runs every blocked cell when any edge goes, including the "did not settle"
  ones.
- A clock-gating fixture with an island across a shown and a hidden composition is
  not written.

**RE-5.4: repeats and layout.** Its first steps:
1. Fix the lag in the old pipeline on both sides: run the rounds again when a stage
   read a published leaf that a later stage then moved, bounded as now. The probe
   test passes from here on, in both. It is run at load and after each resize call,
   with a true feedback case that keeps its last items and reports a cycle.
2. The `Reads` trait, mechanical and exact.
3. Structure reads, and `regraph`'s parents.

**RE-5.4 step 1, as done (2026-11-21).**
- `resolve_physics` settles the repeats again after layout and lays out again
  what they made, until they rest, up to `LAYOUT_ROUNDS` (4). This is the same on
  both sides of the facade.
- A repeat still moving after the last round is a `feedback` problem on it
  (decision D6). It does not freeze: each later call goes round again until its
  items rest (a `repeat.max` stops it).
- Tests (in the `fonts` binary, since layout measures text):
  - `a_repeat_counting_a_laid_out_width_follows_it_at_once` (the probe: at load, after
    a frame, after a transaction, and on drag frames);
  - `a_count_that_reads_its_own_items_layout_is_a_problem`.

  Both fail before the change.
- Cost: `plot_pan_and_zoom_cost` (release), values-only pan p50 0.050 ms, against 0.077
  ms without the change in the same run: no slower.
- `regression.txt`: the pendulum ball's last digits came out as 323.714909407 twice,
  so SKILL.md was updated. That was chance: in step 2 runs they were 323.714908166
  and then 323.714905573 (the old value). They vary from run to run with frame
  timing, as SKILL.md says: a finger's solves depend on how many calls a drag makes
  (§9.4).

#### RE-5.4's sub-plan: layout and repeats as cells (plan, 2026-11-21)

The user chose to build all of RE-5.4 before RE-5.5 (2026-11-21), though step 1
already meets §17's exit for it and there is no measured cost target here. What it
buys:
- each container is laid out only when something it reads changed (today every
  container, on every pipeline call);
- a repeat makes its items in the pass that changed its inputs, not in a later
  round, so `LAYOUT_ROUNDS` goes;
- a box can be contained (§9.2), so structure may read its size.

**Council on the sub-plan (2026-11-21): Opus 5.5 5/10, Sol 6.2 6/10.** Its first draft
paused the pass so the store could make a repeat's items inside it. Both members
showed that this breaks how expansion works:
- owners' fallbacks reach beyond the repeat;
- expansion is batched, under a budget in id order;
- presence changes during settling;
- `restructure` runs passes of its own;
- 45 call sites would have to handle the pause.

They also found that the first draft's glides were not exact, nor were its
implicit containment and its write-time rule. Revised as follows; the findings are
cited by member and number.

**Decisions:**

1. **The old pipeline's lags are fixed first, on both sides** (Opus 4). Each is
   confirmed by a probe test, as step 1 did for repeats:
   - a container nested in a plain group, read through `content_bounds`, sees the
     last call's placement;
   - a layout → binding → layout chain is one call behind, because the rounds
     repeat only when repeats made ops.

   `resolve_physics` lays out again while a LAYOUT leaf that something reads moved,
   bounded as the repeats are. That fixed pipeline is also the honest baseline for
   the demonstrations.
2. **One `lay_out_root(read, tree, root)`**, shared by the old pipeline and the
   natives as `solve_island` is shared (Opus 10, 7).
   - Values are read through a closure, after `table::place_with`. There is no trait
     over ~50 helpers.
   - The helpers layout calls take the closure:
     - `text`'s style, content, runs, measure and cell padding;
     - `geom`'s local bounds and paint padding;
     - `shape::outline`;
     - `contour`;
     - `composition::embed_3d_extent`;
     - `table`'s place and column count.
   - Constants are read through the closure too (`constant(id, path)`, tracked), not
     from a raw slot.
   - `beneath` means beneath any LAYOUT claim, as `base` does today, not only the
     native's own (Opus 7).
3. **Layout natives are keyed by structure, not values** (Opus 3).
   - There is one per *structural root*: a group with a `layout` facet, or a table,
     whose parent has neither.
   - A bound `layout.mode` or `layout_item.absolute` is handled inside its compute,
     including absolute nested containers, each its own Taffy tree.
4. **Scope covers structure only** (Opus 6, Sol 6). A structural op on a node in a
   native's subtree, or on the old or new parent of one, primes it:
   - create, delete, reparent, a facet added or removed;
   - a pure reorder, which `regraph` is given.

   A `Set` does not; values are tracked reads. Reads outside the subtree are tracked
   through the closure:
   - pins (`constrained`: the definition through its link, template constants,
     `Constraints` facets);
   - another composition's extent for an embed;
   - the parent's kind.
5. **Inputs.** The font generation is one `Input`. The layout preview is an `Input`
   per container, so a drag frame runs only its own container (Opus 7).
6. **Glides are applied inside the claim, before readers run** (Opus 5, Sol 4).
   - The glide state lives in `Reactive`: the targets, the transitions in flight and
     the clock.
   - When a LAYOUT native claims, the claim starts a transition for each node whose
     target moved and that has a glide declared, then claims the glided values. The
     targets stay private.
   - What a node may glide by (the context rule's `ms` and `ease`) is set by the
     store, in a map it keeps current whenever contexts or structure change. The
     claim does not resolve contexts.
   - `advance_motion` moves the clock and re-claims only the nodes in flight, with no
     layout run.
   - So readers see what `motion.apply`-before-publish gives them, at the same
     moments.
   - `scratch()` clears it.
7. **Repeats in batches, not inside a pass** (Opus 2, Sol 1, 2, 3).
   - A repeat is a Structure native. Its compute reads its inputs (`each`, `count`,
     `key`, `when`, `max`, the timings) and outputs the wanted items as one value on
     a membership leaf of the repeat. It makes nothing.
   - When the pass empties its queue, `stabilise` takes every repeat whose wanted
     items changed. It settles them with today's code, in one batch: owners batched,
     the budget in id order, the memo, presence, the fallbacks, `store_found`,
     `check_owners`, `regraph`. Then it runs the next pass. It is bounded by
     `DEPTH_MAX + 1` and `LAYOUT_ROUNDS`, as now.
   - This is exact against `settle_repeats` by construction. The saving is that
     only repeats whose inputs changed are looked at; today every drawn repeat's
     inputs are read on every call.
   - A layout native that holds a repeat's items reads its membership leaf, so it
     sits above the repeat (Sol 1). Feedback stays the bounded `feedback` problem.
   - `LAYOUT_ROUNDS` stays. One pass per round, not one pass per change, is the
     honest claim. The test "readers run once per change" becomes "once per round".
8. **Containment is opt-in, and has no write-time rule** (Opus 8, Sol 5).
   - `layout.contain` (`none`, `both`) comes with a schema bump.
   - A contained box is a leaf of its own size in the outer run, with `flex_shrink`
     0 so it is not shrunk there, and an interior run at that size (the border box
     as `Px`).
   - A contained group that fills or hugs is sized as if empty (CSS size
     containment).
   - No box is contained implicitly: a fixed group laid out at its authored size
     would differ from today where an overflowing stack shrinks it.
   - A structural read of an uncontained box's size is the runtime cycle (D6), not a
     write-time refusal: a static check misses `deref`, transitive bindings and an
     edit that turns containment off.

**Steps.**
1. Done: repeats settle again after layout.
2. **The old pipeline's two lags fixed, then `lay_out_root` shared.** Probes first;
   exact on both sides.
3. **Inputs, and `regraph` for reorders and old parents.**
4. **Layout natives, with glides in the claim.** The reference keeps
   `resolve_layout` (`old_pipeline`, which widens `old_network`).
   - Tests:
     - an edit in one container lays out no other;
     - a drag preview in one container runs only it;
     - a glide's start, interruption and release are the same as before.
5. **Repeats as Structure natives in batches.** `settle_repeats`' scan of every drawn
   repeat goes from the subject.
6. **Containment.** Tests:
   - a repeat in a contained box, whose count reads the box's width, follows a resize
     with no `feedback` problem;
   - the same in a hugging group is the `feedback` problem.
7. **The demonstrations** (below). The reference's old layout and repeats go. A
   council, then the roadmap.

**RE-5.4 step 2a, as done (2026-11-21): the old pipeline's two lags, and `Reads`.**
- **`Reads`** (`reads.rs`): `tree`, `resolved`, `layout_base` (beneath any LAYOUT
  claim, which is `layout::base`) and `layout_preview`.
  - `StoreCore` and the differential facade implement it.
  - `layout.rs` and what it calls take `&dyn Reads`: `geom`'s local bounds and paint
    padding, `text`, `contour`, `shape`, `composition::embed_3d_extent`, `table`.
  - Callers passing `&StoreCore` are unchanged.
- **Nested roots:** `layout::compute` lays out the deepest roots first, and later
  roots read what the run has placed (`Fresh`, over the store).
  - Test: `an_outer_layout_reads_a_nested_one_through_a_plain_group_at_once`. It was
    wrong even at load: 20 where 30 was right.
- **Layout → binding → layout:** `resolve_layout` says whether a placement that a cell
  reads moved, and `resolve_physics` (and `relayout`) lay out again while one did, up
  to `LAYOUT_ROUNDS`.
  - Test: `a_layout_reading_a_binding_over_another_layout_follows_it_at_once`.
- Both tests fail before; both run on each side of the facade.
- `resolve_layout` is private now; the facade forwarded it and nothing called it.

**RE-5.4 steps 2b to 4, as done (2026-11-21): layout natives.**
- `layout::lay_out(reads, root)` lays out one structural root. `compute` (the old
  pipeline) and the natives share it.
- `LayoutNative` (one per structural root) and `PinsNative` (`constrained`, one for
  the document) are natives with producer LAYOUT.
  - Their outputs are fixed by structure: the placing nodes and their children, a
    table's rows and cells, and the pinned parts for the pins.
  - Values and constants are read through `Cx`, which implements `Reads`; template
    constants go through the tracked `Reads::constant`.
  - Table grids are the native's extra (`table_grid` reads them), and a grid that
    moved marks `table.columns`.
- **Inputs:** `InputKey::Fonts` (`refresh_text`) and `InputKey::LayoutPreview`
  (`set_layout_preview`), with `Reactive::set_input`.
- **Structure:** `regraph` collects the nodes a structural op touched, and their old
  and new parents, and primes the layouts whose subtree holds one (`prime_layout`).
  A plain reorder, which is not structural to the graph, primes them too
  (`prime_reorders`). `sync_layout` makes and drops natives when the document's
  structure changed (`layout_at`).
- **Glides** live in the graph (`Glide`). A LAYOUT claim starts a transition for each
  node whose target moved and claims the eased place. `advance_motion` moves them
  (`glide_to`). The store keeps each node's declared glide (`glide_specs`). The
  `contexts` glide test is exact against the reference.
- **Held until `resolve_layout`.** Layout natives wait in `pending` and run only
  where the old pipeline laid out: `resolve_layout`, `relayout`, and `scratch`.
  - Running them in every pass made a repeat reading its own items' layout grow
    faster than in the reference, so they are held to stay exact.
  - They are also rooted, so a hidden composition stays laid out, as the reference
    does it.
  - Both go when the reference's old layout does (step 7): then a pass lays out in
    height order, and a hidden composition is not laid out.
- What is incremental now: a native runs only when what it read, its structure, or
  an input changed.
  - Test: `an_edit_in_one_container_lays_out_no_other`.
- Not yet: the layout preview is one input, so a reorder drag runs every layout. Per
  container is planned.
- Verified:
  - the differential (484) and `fonts` (27) suites, exact against `old_pipeline`
    (renamed from `old_network`), with `IRON_ATOMICITY` and `IRON_OWNER_CHECK`;
  - the plain suite and the app's lib tests;
  - `plot_pan_and_zoom_cost`: pan p50 0.060 ms;
  - the build;
  - the smoke, pulses, fiber, idle and plot-view scripts, and regression (as
    expected, pendulum digits aside);
  - `gestures.txt` was not run: it takes about 2.5 min and needs a check-in, and the
    user was away.

**RE-5.4 step 5, as done (2026-11-21): repeats as natives** (`45099d3`).
- A `RepeatNative` per drawn repeat or conditional reads what `settle_repeats`
  compares (`each`, `count`, `key`, `when`) and writes nothing.
- `settle_repeats` returns at once when none ran and `repeat_inputs` stands. It is
  exact by construction: presence clears `repeat_inputs`, and that still forces a
  full settle.
- Native runs no longer count in `evaluations`, which counts expressions.
- Clock gating counts a native at the nodes it writes, or at its own node when it
  writes none. Only an island counts everywhere.

**RE-5.4 step 6, as done (2026-11-21): containment.**
- `layout.contain` (`none`, `both`) is on the `layout` facet. Schema 15.
- A contained box is a leaf of its own size in its parent's run: fixed, filled, or
  as if empty (its padding) where it hugs, kept from shrinking (`iron_layout`'s new
  `Item::keep`).
- Its interior is a run of its own at that size, after the run that sized it, and
  shared by the old pipeline and the natives.
- No box is contained implicitly, and there is no write-time rule: a structural read
  of an uncontained size is the runtime `feedback` problem.
- The model's manifest says what `contain` does.
- Tests:
  - `a_repeat_inside_a_contained_box_counts_by_its_width`: a resize of the row moves
    the box and the item count with it, with no feedback;
  - `a_contained_hugging_group_is_sized_as_if_empty`.
- `scopes` tests (Rust and TS) are updated to schema 15.

**Decided: the reference keeps the old pipeline through RE-5.5.**
- The solver reads layout and the network, so the differential stays exact against
  the old pipeline while the solver moves.
- When RE-5.5 ends (RE-5.6), `old_pipeline` goes from the reference, and with it the
  layout natives' hold and rooting.
- Then a pass lays out in height order, and a hidden composition is not laid out.
  Both are behaviour changes, and get their own tests then.

**Council on RE-5.4 (2026-11-21): Opus 5.5 6.5/10, Sol 6.2 5/10.** Fixed, each with a
test that fails before (five checked):
- **Reads layout made from the tree, not through `Cx`** (both).
  - An embed's source, its camera and the source's `dims` are now tracked constant
    reads (`composition::embed_3d_extent`).
  - So are a pinned part's `constraints` modes and whether its leaf is bound
    (`constrained`'s `free`).
  - Test: `frame_constraints::parts_follow_a_resized_instance_by_their_constraints`
    now edits a part's constraint.
- **Pins** (both). A layout native now holds the pins of the parts in its subtree, and
  applies them over what it lays out, as `constrained` overrode containers. Where a
  pin writes nothing, the layout's value stands. A `PinsNative` holds only the parts
  no layout holds.
- **A leaf a native may write but does not claim is no edge to it** (Sol): `cell_of`
  maps a leaf to its native only while the native claims it. A leaf claimed anew
  raises its readers. Test: `a_fixed_stacks_gap_may_read_its_own_width`, which
  failed as a false cycle.
- **A settle that only moved item values left a layout waiting** (Sol): the round
  loop goes on while a layout native is pending. Test:
  `a_layout_reading_a_repeats_value_over_another_layout_follows_it_at_once`.
- **A dropped layout native's claims were released at once** (Sol): while layout is
  held, they are released where it is let go (`release_later`).
- **A contained box that fills laid out its interior at its authored size** (both):
  an item of a laying-out parent takes its allocation; otherwise it is fixed or as if
  empty. Test: `a_contained_box_that_fills_lays_out_its_interior_at_its_allocation`.
- **Glides:**
  - another layout's placement is read at its target, not as drawn mid-glide (Opus 3);
  - turning motion off primes the layouts in flight, so they land at their next run,
    as the old pipeline published targets at its next layout. Test:
    `turning_motion_off_mid_glide_lands_the_items`;
  - turning motion on again keeps what is in flight;
  - deleting the last context renews the glide specs.
- **Cost:**
  - `sync_layout` and `sync_repeats` are keyed on a count of structural changes
    (`structure_gen`, moved by `regraph`), not on every edit (Opus 5);
  - held natives no longer dirty their readers on every pass (Opus 7).
- **Undeclared outputs:** a layout's values outside its outputs are a `debug_assert`.
- **Found by the 1,000-seed property (seed 682):** a layout claim at an unchanged
  value cleared a host's claim error on its leaf, where the old pipeline, publishing
  nothing, kept it. Layout's claims now clear errors all at once, when a placement
  moved (`layout_published`, `clear_layout_errors`), as one publish of every
  placement did.

**Not fixed, recorded:**
- `pinned_parts` depends on a `use`'s notation, a value, but is kept by structure: a
  notation change keeps the old pins until the next structural change.
- The layout preview is one input.
- A probe for an embed's camera change is not written: its reads are tracked, as
  above.

**The demonstrations, as measured (2026-11-21).** `tests/cells_demo.rs`
(`cells_demo`, ignored, release), against `StoreCore::with_old_pipeline()`. The
counters are structural roots laid out (`layout::lay_out_count`) and settles that read
every repeat's inputs (`settle_scan_count`). Times are p50 / p95 per change.

| Case | Old pipeline | Cells |
|---|---|---|
| 10 containers, edit one | 0.061 / 0.078 ms, 10 lay-outs, 3 scans | 0.024 / 0.029 ms, 1 lay-out, 1 scan |
| 100 containers, edit one | 0.823 / 0.889 ms, 100 lay-outs | **0.133** / 0.155 ms, 1 lay-out |
| 1,000 containers, edit one | 9.240 / 9.476 ms, 1,000 lay-outs | **1.371** / 1.577 ms, 1 lay-out |
| 100 containers, preview a width in one | 0.810 / 0.872 ms, 100 lay-outs, 2 scans | **0.012** / 0.015 ms, 1 lay-out, 0 scans |
| Chain of 4 (layout → binding → layout) | 0.076 / 0.090 ms, 16 lay-outs, 6 scans | 0.044 / 0.059 ms, 4 lay-outs, 1 scan |
| Chain of 8 | 0.194 / 0.204 ms, 40 lay-outs | 0.091 / 0.107 ms, 8 lay-outs |
| 200 repeats, an edit none reads | 0.371 / 0.428 ms, 3 scans | 0.361 / 0.426 ms, 1 scan: no faster, since scans are cheap |
| One container of 1,000 items, edit one | 1.752 / 1.781 ms | **3.632** / 3.680 ms: slower |
| Building 100 containers (best of 5) | 3.868 ms | **7.079** ms: slower |

- **Where cells win:** an edit or preview in one container among many is laid out
  once, not once per container. The cost becomes the one container's, which is
  6.7× faster at 100 containers and 67× faster for a preview.
- **The chain:** the old pipeline lays everything out in each round. The cells lay
  out each link once, in height order.
- **Where cells lose:**
  - one big container: every value is read through `Cx`, which records it, and the
    claims are diffed;
  - building: each native is made, then run twice by R5.

  Both are candidates for later work (deduplicating reads, a native's first height
  taken from its structure). Neither is built.
- The chains of 4 and 8 end in the same places in both.

**The demonstrations** (the user's request, 2026-11-21: show the design's efficiency
with simple cases), in release, against the old pipeline as step 2 leaves it. Each
reports:
- p50 and p95;
- layout runs (Taffy trees);
- native runs;
- evaluations;
- repeats settled.

The cases:
- **N containers, edit one** (N = 10, 100, 1,000). The old pipeline lays out N, the
  new one 1.
- **A chain**: K containers, each one's gap bound to the last's laid-out width,
  editing the first. The old pipeline lays out every container in each of K rounds;
  the new one runs K natives once each.
- **A drag preview** in one container among many.
- **What does not improve, also reported:**
  - load (R5 runs a new native twice);
  - one container of 1,000 items;
  - a structural edit inside a big container;
  - text-heavy containers;
  - the 700-meter document.

(Superseded by the sub-plan above.) Layout natives per container, cut at contained boxes (§9.2). Repeats as natives
whose output is structure: the kernel's R4 is the worked example
of that. Glides are presentation over the target. `layout.contain` comes with a schema
bump. **This step's structure part gets a sub-plan and a council before it is built.**

**RE-5.5: the solver** (§9.4). Clusters are computed in `restructure`, with one Solve
native per cluster: rooted, KeepLast, its pose staged and adopted as above, and its
outputs in a solved layer. `resolve_clusters_live`, `rebuild_clusters`, `Posed` and
`unmoved` go.
- Target (`solve_cost`): with 100 pendulums at rest, a drag step and an unrelated edit
  each within 2× of one pendulum's.
- The suites listed in §17. The random scripts gain constraints.

**RE-5.5, as done (2026-11-21): the solver's work only where something changed.**
- **Measured first.** `sample` of an unrelated edit with 100 pendulums at rest (2.96 ms)
  showed where the time went:
  - grouping every constraint, twice per transaction (`rebuild_clusters`, then
    `post_apply`): about 50%;
  - building and comparing every cluster's `Posed` (`unmoved`): 22%;
  - `validate_all`: 8%;
  - cloning and dropping `Saved`: about 7%.
- **Built, on natives that watch:**
  - **`ClustersNative`** (one, rooted) groups through `Cx` (`solve::clusters_with`).
    It runs again only when:
    - a value it read changed (constraint values, and a constraint's `kind` and
      `enabled`, now read through `read`);
    - a claim on an operand came or went (`Cx::driven`, a new tracked read of
      whether a producer drives a leaf, so a variable's value moving does not
      re-group);
    - the contexts library changed (`InputKey::Contexts`);
    - structure changed (`regraph` primes it).
  - **`ValidateNative`** (one, rooted) runs `validate_all` the same way.
  - **`ClusterNative`** (one per cluster, rooted) reads what the cluster's solve reads
    (`Cluster::posed`'s leaves). That it ran since the cluster was last solved and
    committed is what `unmoved` asks in native mode, instead of building and
    comparing `Posed`.
  - `committed` marks the clusters solved and committed.
  - `IRON_OWNER_CHECK` still solves a cluster left alone and checks it does not
    move.
- **Measured after** (`solve_cost`, native release; the 2× target in §15):

  | Pendulums at rest | Drag step | Unrelated edit |
  |---|---|---|
  | 100, before | 0.59 ms | 2.96 ms |
  | 100, after | **0.146** ms | **0.78** ms |
  | 1, after | 0.27 ms | 0.37 ms |

  The probe times only 9 edits, each just after a drag ends, so its small-count
  rows carry the drag's aftermath. A steady loop of the same edit with 100
  pendulums ran at about 0.08 ms. What is left at 100 is mostly cloning and dropping
  `Saved` (the graph, which holds more now). The target (an unrelated edit within 2×
  of one pendulum's) is met in steady state; the probe's own number is 2.1×.
- **Not built, and why:**
  - the spec's Solve cells that solve inside the pass;
  - a solved layer;
  - pose adoption at the call boundary;
  - removing `resolve_clusters_live`.

  Solving stays in `post_apply` and `resolve_clusters_live`, as before: they write
  `derived_ops` and previews, and their pose semantics (drags, undo, replay) are
  unchanged. With grouping, validation and the change test as natives, the measured
  cost was the scans, not where the solve runs. Moving the solve into the pass would
  change gestures and pose. §17's exit "`resolve_clusters_live` has no callers" is not
  met, and is recorded so.
- Verified:
  - the differential suite (484), exact against the old solver, with
    `IRON_ATOMICITY` and `IRON_OWNER_CHECK`;
  - the plain suite and the app's lib tests;
  - the build;
  - the smoke, pulses, fiber, idle and plot-view scripts, and regression (pendulum
    digits aside).

  Found and fixed: grouping missed a constraint's `enabled` and `kind`, read from the
  slot (`copies_apart::disabling_a_solved_copys_constraint_lets_it_go`).

**Council on RE-5.5 (2026-11-21): Opus 5.5 6.5/10, Sol 6.2 5/10.** Fixed, with tests
that fail before (three checked):
- **Context edits that decide a mate** (both). The contexts resolver reads notation,
  inline styles and context names from the document. A constant set on a
  `composition`, `template`, `use`, `context`, `affords` or `context_def` leaf now primes
  grouping and validation (`prime_grouping`), and so does a session link or unlink
  (Sol). Test: `anchors::plugging_two_rigid_ports_mates_their_parts` now parts a mate
  with a style and makes it again.
- **A `deref` retargeted at the same value** (both): a binding whose tracked reads
  change primes grouping, which uses them for the solve order and its cycles. Test:
  `a_deref_retargeted_onto_its_own_clusters_variable_is_a_cycle`.
- **Wrappers swapped between parents** (Sol): any reparent primes every cluster's
  watcher. Test: `swapping_wrapped_operands_between_parents_is_solved_again`.
- **Grouping on every frame of a mated drag** (Opus): a mate's frame in its body's
  space is the local transforms between them, not the body's world chain.
- **Cost:** `rebuild_clusters` takes the grouping again only when its native ran
  (`grouped_at`), and `post_apply` syncs the cluster watchers only when the grouping
  changed.
- A cluster's watcher reads `size` and `radius` whether its facet is there or not.
- Test: `an_unrelated_edit_runs_no_constraint_native` (run counts, `Reactive::runs_of`).

**Not fixed, recorded:**
- A live solve uses the grouping as of the last transaction, as the old pipeline
  did (Sol 5).
- A context's `for` selector reading an arbitrary leaf is not tracked.
- `solve_cost` after: 100 pendulums, drag step 0.155 ms, unrelated edit 0.84 ms.

**RE-5.6 so far (2026-11-21).**
- **§15 measured** (native release):

  | Measure | Result |
  |---|---|
  | 700 meters | place 0.62 ms, structural edit 0.35 ms, constant edit 0.17 ms |
  | 1,000 meters | structural edit 21.7 ms: the node budget, the user's accepted limit (§4.8) |
  | Plot pan, values only | p50 0.060 ms |
  | Solver, 100 pendulums at rest | drag 0.155 ms, unrelated edit 0.84 ms |
  | Memory per meter | **114.7 KiB**, against 113.5 before phase 3's commits on 2026-11-20 (`b04c2e5`) |

  The 217 KiB seen once was both `instance_cost` tests running in one process,
  sharing the allocator's count. `memory_per_meter` must run alone.
- **The old pipeline is gone from the differential reference.**
  - `old_pipeline`, `with_old_pipeline`, the old `solve_network` body, `resolve_layout`'s
    `compute` path, `layout::compute`, the old glides (`Motion`'s transitions,
    `start_motion`), the `Posed` comparison in `unmoved` and the `posed`,
    `physics_error` and `tables` fields are all deleted.
  - The reference is now natives too, with copies kept in its document.
  - `cells_demo` reports the cells alone; the old pipeline's numbers stay in the table
    above. On today's code an edit in one of 1,000 containers is 0.063 ms, where
    RE-5.4 measured 1.37 ms before `sync_layout` was keyed on structure.
  - All suites pass; the build and the scripts pass.
- **The hold, dropped (step 1).**
  - `hold_layout`, `holding`, `release_later`, `layout_pending` and the store's
    `resolve_layout` are deleted. Layout natives run in any pass, in height order,
    with what they read. `relayout` flushes what waits and settles escaped reads;
    `scratch` flushes only when glides were in flight.
  - A layout claim that moves clears layout's claim errors, in `claim`, as the
    release of the hold did. It walks the errors (few), not the placements.
  - **The feedback bound.** With layout inside the settle's own passes, a count that
    reads its own items' layout grew to `repeat.max` in one settle (1,002 items,
    67 s in debug). Now, when a repeat's items change `REPEAT_CHANGES` (5) times in
    one `resolve_physics`, the store asks the graph whether the repeat reads its
    own items (`Reactive::reaches_into`, a walk up what it reads). One that does is
    `restless`: its items are held as they are, and it is the `feedback` problem,
    until the next transaction lets it try again. Every other repeat goes on
    settling. One that reads a chain of others changes once per link and settles
    (`a_repeat_reading_a_chain_of_eight_settles`).
  - **Gestures set aside around a transaction** (`apply`) no longer restart glides:
    `Reactive::glide_keep` restores a transition whose target came back to where it
    was (`a_refused_edit_mid_drag_does_not_restart_a_glide`).
- **The rooting stays (step 1, not met).** Layout natives were made unrooted, so that
  a composition nothing shows is not laid out. The council found two failures, so
  they are rooted again:
  - a native that has not run claims nothing, so a host's publish could take a
    placement in a hidden stack first, and keep it once shown (it was refused
    before);
  - a capture (`document_bridge/capture.rs`) draws a composition no observer shows,
    so it could read a hidden table's stale grid.

  Unrooting waits on natives reserving their outputs before they first run, and on
  captures observing what they draw.
- **What still runs in `pipeline_from`'s rounds (step 2).** The rounds re-derive when
  the store's own code, between the natives, reads an unneeded cell that was stale.
  The code that reads cells directly is the repeat scan in `settle_repeats` and the
  live cluster solve (`resolve_clusters_live`, §17's unmet item); `network_now`,
  `publish_state` and the glide specs read structure, session state and contexts.
  Natives read through `Cx`, so their reads are edges and never escape. Counted over
  the whole differential suite, with each escape tagged by stage (temporary
  instrumentation, not committed): **one** stale escaped read, from `current` (an
  action's or a handle's computation, which settles its own reads), and none from
  any stage of a derivation. Only four test files turn `observe_all` off, so the
  count covers little. The rounds stay as the guard; removing them waits on the
  cluster solve moving in.
- **`cells_demo` after step 1** (release, p50): edit one of 1,000 containers
  0.030 ms; a preview 0.012 ms; a chain of 8 0.084 ms; one container of 1,000 items
  3.16 ms; 1,000 containers all re-laid out by one edit 49.4 ms (a new case, the
  council's: 142.7 ms while each claim walked every placement); building 100
  containers 7.5 ms.
- **The differential's reference** has been natives since the old pipeline went
  (2862265): it compares copies kept apart against copies in the document, so it no
  longer checks the natives' semantics against an independent oracle. The tests
  above are the check for this step's changes.
- **Measured on the way:** `observing_part_of_the_time_changes_nothing_once_all_is_observed`
  takes about 3.5 min in debug, and did before this step (seed 2 is 22 s either
  way). The run-iron durations row and the iteration skip list now name it.
- **Council on RE-5.6** (2862265 and 24367eb): Opus 6.5/10, Sol 5/10. Found and
  fixed: the feedback bound stopping a valid chain of repeats, and stopping every
  repeat; the claim-error walk (quadratic in re-laid-out roots); glides restarted
  by a refused edit mid-drag; stale comments naming deleted code. The unrooting's
  two failures are above. Ruled out by Opus: claims released at once when a native
  is dropped (passed on through `incoming`), unrooted stale reads by transactions,
  plans and the agent API (`exact`, `scratch`).

**RE-5.6: phase end (2026-11-21).** Spec §17's exits for phase 5:

| Exit | State |
|---|---|
| A repeat whose count reads a contained box's width adds items during a resize drag | **Met** (RE-5.4: `layout.contain`, schema 15; the probe test and the contained-box tests) |
| Solving only dirty clusters agrees with solving all on every script; a drag in one cluster solves no other | **Met** (RE-5.1) |
| `resolve_clusters_live` has no callers | **Not met** (RE-5.5): solving stays in `post_apply` and the live solve; grouping, validation and each cluster's change test are natives |
| `layout`, `table`, `ports`, `schematic`, `constraints`, `gestures` and `handles` suites green | **Met** |
| The meter targets of §15 | Measured in "RE-5.6 so far" above (700 meters: placing 0.62 ms, a structural edit 0.35 ms, a constant edit 0.17 ms) |

Not built, with the reasons above: `fits`; one-axis containment; the solver's Solve
cells, solved layer and commit step (RE-5.5); layout natives unrooted (RE-5.6, the
council's two failures); deduplicated reads for one big container and a native's
first height from its structure (`cells_demo`'s two slower cases). Clips as cells
are PT-9.

**Not in this plan:**
- clips and steppers (PT-9, PT-10);
- `fits`;
- one-axis containment;
- dense arrays;
- the live simulation;
- native rules written by authors;
- native state beyond a value (PT-10 adds it if it needs to).

**Councils:** the design (done, 6/10 from both; revised as above); RE-5.4's structure
sub-plan; the end of each step.

## 5. Rules, and when to stop

**Rules.**

1. Every document mutation is an op in a transaction through `iron_document`
   (AGENTS.md). Derived structure is never an op.
2. No backwards compatibility: change every caller, delete the old path (AGENTS.md).
3. Never weaken the snapshot (§4.1, task 3), an exit, or a test to make something
   pass. Never add an intended difference that is not in the spec's §16.1.
4. Whether a transaction is accepted never depends on what is observed (spec §11).
5. `stabilise()` records nothing and has no effects; recording and effects happen at
   commit and at `advance` (spec §10.4, §11.1).
6. A read through `&Store` never loads or evaluates (spec §8.9).
7. Use the spec's words (§3): owner, template, cell, observer, necessary. "Scope"
   means storage (scopes.md), not this.
8. Commit and push each task (AGENTS.md); measure with `wasm-bench` or native release
   only (§6).

**Stop and report to the user, with what you found, instead of working around it,
when:**

- an assumption in the spec's §14 turns out false;
- the old engine does something the spec does not describe and the new design cannot
  reproduce (say what, with the test that shows it);
- a difference from the old engine where it is unclear which behaviour is right, or
  that alters something a user can see and may rely on. Any other difference is fixed
  in both engines or recorded in `intended.rs` with a test (the spec's §16.1, the
  user's rule of 2026-11-16);
- a settled decision (spec §19) blocks the work;
- a phase's exit cannot be met, or a target in the spec's §15 is missed by more than
  a factor of two after a real attempt;
- the facade cannot mirror a public method (§4.1, task 2);
- the same test has failed for the same reason three times.

A report says: the phase and task, what was expected, what happened, the smallest
test that shows it, and what you would do next. It is always better than a quiet
change of design: the design's weak points are known (the spec's §14), and an
unreported workaround hides which one gave way.

## 6. Measuring: recipe and traps

```sh
# Native store probe (fast, no browser)
cargo test --release -p iron_document --target aarch64-apple-darwin --test suite plot_perf:: -- --ignored --nocapture
PLOTS=10 cargo test --release -p iron_document --target aarch64-apple-darwin --test suite plot_perf:: -- --ignored --nocapture

# In app, optimised build (webgl2 is what the headless driver uses)
PROFILE=wasm-bench BACKENDS=webgl2 .claude/skills/run-iron/build.sh
bunx vite --port 5185 --strictPort --force &
cd .claude/skills/run-iron
GPU=1 SIZE=1680x1000@1 URL="http://localhost:5185/Ironfell/?gfx=webgl2&showcase=0&demo&bevy=debuglog" \
  bun run drive.ts < plot-perf.txt
```

- Serve on 5185, not 5173: `serve.sh` kills whatever holds 5173, which may be the
  user's own Vite. Restart Vite with `--force` after a wasm rebuild or it can serve the
  old binary.
- Compare only `wasm-bench` builds; `dev-opt` is opt-level 1 and overstates CPU time.
  Rebuild `dev-opt` (`build.sh`, both backends) before the smoke and when done, so the
  served build is the normal one.
- Read `action` beside `changed_frame`; `frame` includes idle frames.
- `plot-perf.txt`'s zoom dispatches one wheel event per animation frame from the page;
  its pans are driver-paced `dmove`s 16 ms apart.
