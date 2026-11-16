# Handoff: the new reactive engine

**Date:** 2026-11-15 · **Branch:** `master` · **Status:** the design is
[`specs/reactive-engine.md`](specs/reactive-engine.md), **proposed, revised after a council review, and
waiting on the user's approval**. Do not build until it is approved. This file holds the problem as
measured, the work order and the measuring recipe.

**History:** the first version of this handoff (same day, `e0935bd`) proposed patching
the existing engine: scoped settling (A), the same for transactions (B) and, optionally,
structure inside the graph (C). In review the user widened the goals (work that nothing
observes is not done, instances as references, layout that can drive structure, time as
a dimension, readiness for partial loading) and chose a new engine written beside the
old one. The spec's §14 answers the first version's open questions.

**Read first:** this file; then [`specs/reactive-engine.md`](specs/reactive-engine.md);
then [`specs/reactivity.md`](specs/reactivity.md) §3 and §13 (the behaviour to keep) and
[`specs/instance-cache.md`](specs/instance-cache.md)'s "Open". Build and measure with
the run-iron skill ([`.claude/skills/run-iron/SKILL.md`](../.claude/skills/run-iron/SKILL.md)).

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

Phases, their contents and their exits are in the spec's §17. A council review
(2026-11-15, Claude Opus 5.5 and GPT-6.1 Sol) returned "rework"; the spec was revised
the same day and its §20 records each finding. In order:

1. **Approval.** The user approves the revised spec and settles the decisions in its
   §19 (each has a default; D1, D2, D6, D9 and D10 changed or were added after the
   council).
2. **Phase 0:** baselines (memory per instance, load time, a profile of the constant
   edit at 1,000 meters, growth of the interned id paths), the differential harness and
   the host reference, green on old against old.
3. **Phase 1:** the read contract (the `Tree`, `take_changes()`) on the old engine, with
   every caller inside and outside the crate moved to it. No behaviour change.
4. **Phase 2a:** transactions in place, on the old engine.
5. **Phase 2b:** the new cell graph and per-owner structure, with copies still stored
   in the document. **This is the plot fix**; it closes the roadmap's "Plot pan and zoom
   performance" row.
6. **Phase 2c:** copies leave the document (instances as references).
7. **Phase 3** (time), then **phase 4** (observation). Phase 5 (the pipeline as cells)
   does not depend on them; it closes instance-cache.md's "Open" at 1,000 meters. Then
   phase 6 (value slices).

Commit and push each step (AGENTS.md). Run the fast smoke before committing behaviour
changes; Rust builds and tests need no check-in, other steps over a minute do. Record
each phase in `roadmap.md`'s Status table and in the spec.

## 3. Verification

The spec's §16: the old engine is the reference until the swap (a differential harness
over the existing tests' scripts and random ones); the existing suites; and, for
behaviour the old engine does not have, the consistency property of the spec's §8.8.

## 4. Measuring: recipe and traps

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
