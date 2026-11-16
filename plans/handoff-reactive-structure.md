# Handoff: building the new reactive engine

**Date:** 2026-11-15 · **Branch:** `master` · **Status:** the design
([`specs/reactive-engine.md`](specs/reactive-engine.md), "the spec" below) is approved
and its decisions settled. **Nothing of the engine is built.** The current task is
**phase 0** (§4.1). This file is for whoever builds it: what to read, the order of
work, the first two phases as tasks, and the rules.

## 0. Read first, in this order

1. This file.
2. The spec, whole. Its §3 is the vocabulary; §14 lists what is assumed and not yet
   verified; §17 is the phase table; §19 the settled decisions.
3. `src/iron_engine_kernel` (`src/lib.rs`, then `src/engine.rs`, then
   `tests/kernel.rs`): the core pass of the spec's §6 and §7 as about 700 lines of
   running code, tested against recomputation from scratch. It is a model to learn the
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
history (two council rounds, what each found). It is not needed to build.

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
| 0 | Baselines and the differential harness, run old against old | **Current.** Tasks in §4.1 |
| 1 | The read contract (`Tree`, `take_changes()`) on the old engine; every caller moved | Tasks in §4.2 |
| 2a | Transactions in place on the old engine: the document journal | Tasks written when phase 1 ends |
| 2b | The new cell graph and owners, copies still stored. **This is the plot fix**; it closes the roadmap's "Plot pan and zoom performance" row | |
| 2c | Copies leave the document: instances as references | |
| 3 | Time: clocks, the scheduler, steppers as cells, recorded events | |
| 4 | Observation: Dormant, Unloaded, parking, extents | |
| 5 | The pipeline as cells: layout, the network solve, clips, the constraint solver. Does not depend on 3 or 4; closes instance-cache.md's "Open" at 1,000 meters | |
| 6 | Value slices | |

At the end of each phase: its exit in the spec's §17 is met and shown by a command
whose output is pasted into the commit or the roadmap row; this table and the
roadmap's Status table are updated; the next phase's task list is written here in the
form of §4, from the spec, before starting it.

## 3. Verification, in one place

- **The old engine is the reference until the swap** (spec §16.1). From phase 0 on,
  every test in `src/iron_document/tests` is also a differential script (§4.1, task 2).
- **Differences from the old engine are allowed only if listed** in
  `src/iron_document/tests/differential/intended.rs`. The spec's §16.1 gives the list
  that is already agreed.
- **Behaviour the old engine does not have** (dormancy, parking, the scheduler) is
  tested against the engine itself by the consistency property (spec §8.8, §16.3). The
  kernel's `random_observation_is_consistent` is that test in miniature.
- **Before committing a behaviour change:** `cargo test -p iron_document --target
  aarch64-apple-darwin`, `bun test`, and the fast smoke (run-iron skill). Rust builds
  and tests need no check-in; other steps over a minute do (AGENTS.md).

## 4. The first two phases as tasks

### 4.1 Phase 0: baselines and the harness

Nothing in this phase changes behaviour. Its purpose is that every later phase has a
number to beat and a test that cannot be passed by a wrong engine.

**Task 1. Baselines.** Each is a native, ignored, release test beside
`tests/plot_perf.rs` and `tests/instance_cost.rs`, printing one line per measure.
Record the numbers in §1 of this file and in the "Today" column of the spec's §15.

| Measure | How |
|---|---|
| Memory per placed meter beyond the first | In `instance_cost.rs`, a counting `#[global_allocator]` for that test binary; live bytes at 100 and at 1,000 meters, divided |
| Where a **constant** edit at 1,000 meters spends its 14.1 ms | Temporary timers around each step of `Store::run` and `after_transaction` for that edit: the document clone, `expand::affected_by`, `attachment::validate`, `rebuild_clusters`, `resolve_physics`'s parts (layout, network, palette, contexts). Report the split; remove the timers |
| Solver cost with clusters at rest | A new `tests/solve_cost.rs`: 1, 10 and 100 pendulums (the document `tests/constraints.rs` builds), then the time of one gesture step dragging one ball, and of a transaction setting an unrelated constant |
| Growth of interned id paths | A script that pans a plot through 10,000 distinct tick values; the registry's length before and after (`id.rs`; add a test-only accessor) |
| Loading the demo document | The demo is built by `document_bridge::demo::demo_scene` in the app crate. Apply it, save, and time `Store::load` of the saved text; do it wherever both are reachable (a test in the app crate, or the app with `__iron.timings`) and say which |

**Task 2. The differential facade.** Make every existing test a differential script
without rewriting any test.

- Add a cargo feature `differential` to `iron_document`. With it on, the public
  `Store` is a facade holding two stores, `subject` and `reference`. It implements
  `Deref` to `subject` for every `&self` method, and **not** `DerefMut`: each of the 55
  public `&mut self` methods is forwarded by hand (or one macro) to both, and after
  each call the facade compares the two return values and the two snapshots (task 3)
  and panics with the method's name and the first difference.
- This works because the store's public API takes and returns plain data: timestamps
  are arguments, producers are registered by name, there are no callbacks. If you find
  a method for which that is false, stop and report (§5).
- `Store::new` and `Store::load` build both stores. A store is built with an engine
  choice (`Engine::Old` or `Engine::New`), which until phase 2b has one value. So in
  this phase `subject` and `reference` are both the old engine, and a green run proves
  the facade, the snapshot and the determinism of the old engine. From 2b, `subject`
  is the new engine.
- Both engines live in `iron_document`, chosen at run time inside the store, so the
  two stores share every type. Do not make a second copy of the crate: its types would
  not be comparable.
- Exit: `cargo test -p iron_document --features differential --target
  aarch64-apple-darwin` is green, and deliberately breaking the old engine in one of
  the two stores (for example, skipping one `settle_repeats` call when a debug flag is
  set on the reference) makes it fail. Show the failure, then remove the flag.

**Task 3. The snapshot.** `fn snapshot(&StoreCore) -> Snapshot`, with `PartialEq` and a
`diff` that names the first difference. It is the definition of "the same", so it is
written out here and in the spec's §16.1; do not weaken it to make something pass.

| Compared | Detail |
|---|---|
| Nodes | The set of live ids, authored and derived; each one's type, parent, order key and every facet, slot for slot |
| Relations | Every relation: id, kind, ends, data |
| Values | The resolved value or the error of every leaf that has a slot, read as a view reads it (overlay included) |
| Problems | As a set of (node, kind); message text is not compared |
| Counters | Version, next node and relation ids per client, session counters |
| The view | What a `ViewQuery` of the whole document prints at full fidelity |
| History | Undo and redo depth; the last transaction's `ops`, `derived_ops` and `inverse` |
| Effects since the last step | Query requests issued, `on.emit` events (`take_emitted`), session cells made and dropped, revisions reported, the selection |
| After a failed call | Everything above is identical to before the call |

Not compared: `Transaction::expanded` and any op emitted for derived structure;
evaluation counts; heights; timing.

**Task 4. The intended-differences file.**
`src/iron_document/tests/differential/intended.rs`: a table the facade consults, each
entry with a name, the predicate that narrows it (which nodes or leaves, in which
situation), and the name of the test that asserts the new behaviour. Empty in this
phase. The agreed entries, added in the phase that first needs each, are the spec's
§16.1 list. Any other entry is a stop-and-report.

**Task 5. Random scripts.** `tests/differential_random.rs`: a seeded generator of
documents and of steps over them, run through the facade. It must reach, and count
that it reached, each of the situations the spec's §16.1 lists under "Scripts"
(repeats in instances and instances in repeats, conditionals toggling, keys changing
kind, overrides on items that leave and return, enter and exit in flight, providers
moved, connections to instance ports, definition edits, failing transactions, undo and
redo, previews, publishes, clock moves). Assert the counts at the end, as the kernel's
random test does: agreement over scripts that never reach the hard paths proves
nothing. Exit: 10,000 seeds green, with the counts printed.

**Task 6. The host reference.** In the app crate: a headless Bevy `World` (no window,
no GPU) with the document bridge's systems, a function that rebuilds the host's
derived state from scratch (drawables, paint order, the spatial scene), and a test
that after each step of a script compares the incrementally kept state with the
rebuilt one. `materialize::reconcile` takes `Commands` and mesh assets
(`materialize.rs:352`), which is why this needs a `World`. Exit: green over the
fast smoke's document and a few written scripts.

**Phase 0 is done when** tasks 1 to 6 meet their exits, the numbers are recorded, and
this file's §2 and the roadmap say so.

### 4.2 Phase 1: the read contract, on the old engine

The point: after this phase no code outside the engine assumes a derived node is
stored in the `Document`, while the old engine still stores them there. So when
phase 2c takes the copies out, nothing else changes. **No behaviour changes in this
phase.** The spec's §12 is the contract.

**Task 1. `Tree`.** `Store::tree()` returns a read view with `Document`'s read methods
under the same names (spec §12.1 lists them). In this phase it forwards to the
document. It is a type of its own, not `&Document`, so later phases can change what
is behind it.

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
`take_changes()`.

**Task 5. The snapshot reads through the `Tree`.**

**Exit:** the differential and host harnesses are green; the fast smoke's output is
byte-identical to before the phase; `git grep` finds no caller of the deleted drains
and no `document()`; timings in the smoke are within noise.

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
- a difference from the old engine is needed that the spec's §16.1 does not list;
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
