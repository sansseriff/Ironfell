# Handoff: building the new reactive engine

**Date:** 2026-11-16 · **Branch:** `master` · **Status:** the design
([`specs/reactive-engine.md`](specs/reactive-engine.md), "the spec" below) is approved
and its decisions settled. **Phases 0, 1 and 2a are done** (2026-11-16; §2):
baselines, the differential harness, the host reference, the read contract (`Tree`,
`take_changes()`), and transactions applied in place with a document journal, all on
the old engine. Nothing of the new engine is built. The current task is **phase 2b**
(§4.4), whose task list awaits its council review. This file is for whoever builds it: what to read, the order of
work, the phases as tasks, and the rules. Phases 0 to 2 are ready to build;
phases 3, 4 and 5 each have design points to settle first (§4.5).

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
4. [`specs/reactivity.md`](specs/reactivity.md) §3 and §13: the behaviour that must
   survive.
5. The code being replaced, enough to recognise it: `Store::settle_repeats`,
   `Store::run`, `after_transaction`, `post_apply` and `resolve_physics` in
   `src/iron_document/src/lib.rs`; `graph.rs`; `expand.rs`.
6. The run-iron skill ([`.claude/skills/run-iron/SKILL.md`](../.claude/skills/run-iron/SKILL.md))
   for building, the smoke and timings.

[`specs/reactive-engine-review.md`](specs/reactive-engine-review.md) is the design's
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

| | Phase | State |
|---|---|---|
| 0 | Baselines and the differential harness, run old against old | **Done 2026-11-16** (below) |
| 1 | The read contract (`Tree`, `take_changes()`) on the old engine; every caller moved | **Done 2026-11-16** (below) |
| 2a | Transactions in place on the old engine: the document journal | **Done 2026-11-16** (below; tasks in §4.3) |
| 2b | The new cell graph and owners, copies still stored. **This is the plot fix**; it closes the roadmap's "Plot pan and zoom performance" row | **Current.** Tasks in §4.4, in five stages |
| 2c | Copies leave the document: instances as references | |
| 3 | Time: clocks, the scheduler, steppers as cells, recorded events | |
| 4 | Observation: Dormant, Unloaded, parking, extents | |
| 5 | The pipeline as cells: layout, the network solve, clips, the constraint solver. Does not depend on 3 or 4; closes instance-cache.md's "Open" at 1,000 meters | |
| 6 | Value slices | |

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

**How the engines are chosen.**
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

### 4.5 Before phases 3, 4 and 5

Phases 0 to 2 can be built from the spec as it is. The later phases cannot yet: the
spec's §14 ends with a list, "Open before a phase starts", of points where the last
review found the design incomplete (when a solved pose is adopted, the full set of
things that wake a parked owner, recording followed by another pass). The spec states
a rule for each, but those rules have not been reviewed or tested. Before starting
phase 3, 4 or 5, write that phase's tasks here, check each open point for that phase
against the code as it then is, and report to the user what you would build; do not
start the phase on the rule as written without that.

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
cargo test --release -p iron_document --target aarch64-apple-darwin --test plot_perf -- --ignored --nocapture
PLOTS=10 cargo test --release -p iron_document --target aarch64-apple-darwin --test plot_perf -- --ignored --nocapture

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
