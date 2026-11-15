# Handoff: structure that costs what changed (reactive engine)

**Date:** 2026-11-15 · **Branch:** `master` (`639f07b` and later) · **Status:** brief for a
new agent. The options and the recommended order are below; the open questions in §6 are
for that agent to answer in a design note (§7) **before** building, and for the user to
approve.

**Read first:** this file; then [`specs/reactivity.md`](specs/reactivity.md) §3 (scopes,
keys, settling) and §13 (what is built, including the 2026-11-14 entry this brief
follows from); then [`specs/instance-cache.md`](specs/instance-cache.md)'s "Open" (the
same problem, measured for component instances). Build and measure with the run-iron
skill ([`.claude/skills/run-iron/SKILL.md`](../.claude/skills/run-iron/SKILL.md)).

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

## 2. How the pieces fit now (for orientation)

- **Scopes.** Instances, repeat items and conditionals are one mechanism (reactivity.md
  §3.1). A derived node's id is its path: `#22/1f/~5/1g` is instance `#22`, its part
  `1f` (a repeat), item key `~5`, template-local node `1g`. Ids are **computed, not
  allocated**, so the same document and items always give the same ids.
- **Keys** (reactivity.md §3.2, `item_keys` in lib.rs): `repeat.key`, else node refs,
  record `id`s, distinct words, else the index. `std:plot`'s tick labels are index
  keyed, so a pan moves values between the same keys and only the *last* key is added
  or removed when the count changes.
- **Derived nodes are real document nodes, never saved** (`serial.rs` ~68: "derived on
  load"). They are selected, picked, inspected and overridden; the renderer, spatial
  index and render analysis treat them like any node; relations to them (connections
  to an instance's port) **are** saved and re-attached on load.
- **Overrides** on a copy are stored on the owner (instance or repeat) and keyed by the
  item key (`expand::overrides`, `apply_overrides`); they survive the item leaving and
  returning.
- **Settling** (`settle_repeats`): after values resolve, read each drawn repeat's
  inputs; if items changed, re-expand everything, publish item values
  (`ProducerId::ITEMS`), presence, rebuild the graph, and loop (one round per nesting
  level) until nothing changes. The derived ops queue for the host
  (`take_derived_ops`), which applies them in `document_bridge/mod.rs` (~526).
- **The graph** (`graph.rs`): one node per bound leaf; dependencies are **static**,
  parsed from the formula (`expr.deps()`), plus context reads resolved at build time
  (`palette::provided`) and `deref` tracked reads; derived signals from connections and
  anchors (`port::derived_signals`, `anchor::derived`) are built globally at rebuild.
  `invalidate` re-evaluates dirty readers in height order and stops where a value is
  unchanged.

## 3. Constraints (fixed; do not revisit)

1. **Every document mutation is an op in a transaction through `iron_document`**
   (AGENTS.md). Derived structure reaches the host only as ops, as now.
2. **Derived nodes stay real, addressable document nodes** with deterministic path ids.
   Selection, overrides, picking, saved connections, render analysis and the agent's
   view depend on it. An incremental path must produce **exactly** the ids, ops and
   values a full expansion would.
3. **Overrides stay keyed by item key** and survive an item's disposal.
4. **Dependencies stay static and analysable.** Do not adopt Solid-style dynamic
   dependency tracking: wires drawn on demand (wires-and-formulas.md), cycle refusal at
   write time, the look-only rule for interaction state (reactivity.md §4.2) and render
   analysis (static/keyed/live parts) all read the static graph. `deref` stays the one
   tracked read.
5. **No backwards compatibility** (AGENTS.md): change callers, delete old paths, bump
   `serial::SCHEMA` if a file format changes (it should not need to).
6. **The full rebuild survives** as the load path, a fallback for changes too large to
   patch, and the reference implementation for tests (§5).

## 4. Options

### A. Scoped settling: re-expand and patch only the scope whose items changed *(recommended first)*

When repeat or conditional `R`'s keys change:

1. Diff keys, not the document: added, removed, kept.
2. Removed key: delete the subtree `R/~k` (deepest first) and remove its bindings from
   the graph; keep its overrides; respect `exit_ms` (a leaving item stays until its
   presence reaches 0).
3. Added key: expand only `R`'s template under `R/~k` (with the repeat's overrides for
   that key), recursing into scopes inside the new copy; insert its bindings into the
   graph with heights from their dependencies; evaluate only those.
4. Kept keys: untouched structurally; their item values are republished, and
   `invalidate` re-runs only readers of values that changed.

Needs two new capabilities: **expand one scope** (an entry point into `want_scopes` /
`want_items` for one owner, producing the same `Wanted` entries a full expansion would
for that subtree) and **an editable graph** (insert and remove nodes and edges, adjust
heights locally, as Jane Street's Incremental does on new edges, instead of the global
`compute_heights` fixpoint).

Acceptance: a structural pan or zoom step costs about the same with 1 plot as with 10
(the native probe); `action` p95 in `plot-perf.txt` near the values-only cost; the
full-rebuild oracle (§5) agrees after every step.

### B. The same path for transactions *(recommended second)*

Make every structural transaction cost what it affects, not the document:

| Edit | Affected set |
|---|---|
| Create, delete or reparent an authored node outside definitions | that node's subtree and the formulas that read it |
| Write a formula on a leaf | that leaf's graph node, its dependencies and readers |
| Edit inside a definition (its template, ports, lets) | every instance (and repeat) of that definition: re-expand each, not the document |
| Change a context provider (`let.provide`, its name, its position) | readers below it whose `@name` resolves differently |
| Link or unlink (connections, anchors) | the derived signals at its ends |

This is what instance-cache.md's "Open" asks for (161.7 ms a structural edit at 1,000
meters) and what `handoff-render.md` §4 names as the render track's next item
("incremental instance expansion"). Most of A's machinery carries over; the new parts
are the indexes that find affected sets (definition → instances, provider → readers,
node → derived signals).

### C. Structure inside the graph (Solid-style ownership) *(optional; decide from measurements after A and B)*

Make a repeat (and a conditional, and an instance) a node of the reactive graph whose
evaluation diffs its keys, emits the create/delete ops for its own items and inserts or
removes their bindings, which sit above it and run later in the same pass. This is
SolidJS's `mapArray` / owner tree (Index for index keys, For for value keys) and
Incremental's `bind`, adapted to Iron's materialized, addressable derived nodes.

Gains: no separate `settle_repeats` loop and no rounds; nested scopes settle in one
pass; a reader of a repeat's count never sees a stale structure. Costs: the graph must
accept new nodes during propagation (heights above the current frontier), which is the
hardest part. Most of the performance comes from A; C is about coherence and removing
rounds. Do it only if A and B's measurements or the edit path show the rounds matter.

### Not in this brief (separate follow-ups, noted so they are not mistaken for this work)

- **Evaluate only what is observed.** Every binding is evaluated today, including
  those in compositions in hidden tabs. A demand-driven graph (queries already ask only
  while read) is a separate design with its own trade-off (picking, bounds and the
  agent's view sometimes need unseen values).
- **Geometry-valued formulas.** The plot's formulas build SVG path *text*
  (`points`, `segments`, …) that the store parses back into geometry every frame
  (`graph::as_leaf_kind`); tick labels re-layout their text. Values-only pan cost.
- **The renderer's share of a pan** (about 3–4 ms of a 4.3 ms changed frame: display
  list, encode, bounds, gesture reconcile) and `bounds` at 1.2 ms per zoom frame.
- **The constraint solver's clusters** (`rebuild_clusters`) and layout have their own
  whole-document paths.

### Rejected stopgaps

- **Fixed label slots in `std:plot`** (pad the tick list to a constant length so pan
  and zoom never change structure). It works (tick count is bounded by about 1.6 × the
  requested count + 1), but hides an engine cost in one component's markup, reverses
  plot.md's "one label per tick, no empty slots", and cannot work with value-keyed ticks
  that slide and fade (reactivity.md §13 defers those until structure is cheap).
- **Hysteresis on the tick count.** A tuned threshold for the same symptom.

## 5. Verification (required from the first commit of A)

1. **Full rebuild as the reference.** A test helper (and optionally a debug-build
   check) that, after an incremental step, runs a full expansion and graph rebuild on a
   clone and asserts the same live nodes, facets, relations, problems, graph nodes,
   edges, heights' order and resolved values. Run it after every step of the existing
   reactivity, repeat, plot, machines and instance tests.
2. **Randomised edits** against that reference: repeats nested in instances and
   instances in repeats, conditionals toggling, keys changing kind, overrides on items
   that leave and return, enter/exit in flight, context providers moved, connections to
   instance ports.
3. **Existing suites:** `cargo test -p iron_document --target aarch64-apple-darwin` (361
   tests on 2026-11-14), `bun test`, the fast smoke, `plot-view.txt`, `actions.txt`,
   `query.txt`, `fiber.txt`, `pulses.txt` (run-iron skill).
4. **Measurements:** `tests/plot_perf.rs` with `PLOTS=1` and `PLOTS=10`, `plot-perf.txt`
   at `PROFILE=wasm-bench`, and instance-cache.md's meter rows for B
   (`bench-instances.txt`, `instance_cost`).

## 6. Open questions (answer in the design note)

**Scope and identity**

1. **Expanding one scope exactly as a full expansion would.** `wanted` is written as
   one worklist over the whole document (`want_entity`, `want_use`, `want_scopes`,
   `in_template`). What state does expanding one owner's subtree need from outside it
   (parents' overrides along the path, `in_template` checks, sizes, notation)? Can the
   same functions run rooted at one owner, or does the worklist need restructuring?
2. **Problems.** `expansion_problems` and `truncated` are replaced wholesale per
   expansion. How are they kept per scope?
3. **The template's own bindings** are evaluated once, reading item 0 (reactivity.md
   §13). Keep that, and keep it out of the incremental path's cost?
4. **Index keys for the plot.** Keep them (pan moves values, the last key is
   added/removed), or move ticks to value keys once structure is cheap, so labels can
   slide and fade? This is a product question for the user as well.

**The graph**

5. **Edges across a scope boundary.** Readers inside a copy of things outside (every
   tick label reads the plot's `xp`) are ordinary edges. Readers outside of things inside
   a copy (`#g/~3/7`, node-reference lists, `deref`): on disposal they must see the read
   fail or change. What index finds them (a reverse index by scope prefix?), and what do
   they read in between?
6. **Heights.** Replace `compute_heights`' global fixpoint with local adjustment on
   insert (raise dependents' heights only when needed). Is a height-ordered queue still
   right, or is a topological order per pass simpler?
7. **Context reads** (`@name`) are resolved per reader at build. A new scope's readers
   resolve on insert; a provider change must find its readers. Which index?
8. **Derived signals** (`port::derived_signals`, `anchor::derived`) are computed for
   the whole document at rebuild. Make them per node, or recompute them only for the
   relations an op touched?
9. **Removing a binding** reverts its leaf to the constant and notes it (`rebuild`'s
   `old_bound` loop). Per binding when its scope goes.

**Batching and fallbacks**

10. **When to fall back to a full rebuild**: load, undo/redo of large transactions, a
    paste of thousands of nodes, a repeat jumping from 10 to 10,000 items. A threshold
    by size of the affected set?
11. **Transactions run on a working copy** (`Store::run` expands `work` before checks,
    and the derived ops go in `tx.expanded`, which undo replays). Does the scoped path
    run there too, and does undo stay a replay of recorded ops?

**State attached to items**

12. **Presence**: a leaving item stays until its exit finishes, then is removed. With
    scoped settling, is its removal one more scoped step from `advance_presence`?
13. **Session lets** on items are dropped when their node goes (reactivity.md open
    question 1). Unchanged by this work, but the disposal path must do it.
14. **Render analysis and instance sharing** (`analysis.rs`, instance-cache.md): a new
    copy needs its parts classified; equal instances share drawings. What must the
    incremental path tell the host so caches invalidate only what changed?

**Option C**

15. Is a repeat-as-graph-node worth it after A and B? What would show it: rounds
    measured as a real cost, a stale-count glitch, or simpler code?
16. If built: how does a graph node emit ops during propagation, given the host
    applies derived ops once per frame, and the transaction path works on a copy?

## 7. Work order

1. **Design note** in reactivity.md (a new section, "Incremental structure"),
   answering §6 and choosing between restructuring `wanted` and rooting it, with a
   sketch of the graph's insert and remove. **Stop for the user's approval.** The
   user may also ask for a council review (AGENTS.md, "Council review").
2. **The reference check** (§5.1) wired into the existing suites, green against the
   current (full) path.
3. **Option A**, with the probes before and after; record results in reactivity.md
   §13 and the roadmap's Status row "Plot pan and zoom performance".
4. **Option B**, measured on instance-cache.md's meter rows; update instance-cache.md's
   "Open" and `handoff-render.md` §4.
5. **Option C**, only if the user agrees after 3 and 4.

Commit and push each step (AGENTS.md). Run the fast smoke before committing behaviour
changes; Rust builds and tests need no check-in, other steps over a minute do.

## 8. Measuring: recipe and traps

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
