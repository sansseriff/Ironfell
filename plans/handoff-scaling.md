# Handoff: costs that scale with the document

**Date:** 2026-11-21 · **Branch:** `master` · **Keys:** `DS-n` (plans/README.md, "Work
keys") · **Status:** plan, revised after council rounds 1 (Opus 5/10, Sol 5/10) and 2 (Opus
6/10, Sol 6/10; both: DS-1 can start). **DS-1, DS-2 and DS-3a done 2026-11-21; DS-3b
stopped at part 2 (`08e0539`, 2026-11-22).** DS-9 done 2026-11-22; its code review fixed in `fd884ac`; the spec rewritten 2026-11-22 after a review of its direction (§9). Next: the rewritten spec to the council, then DS-10 (the oracle), then DS-11 (the layout family), from
[`specs/structure-in-the-graph.md`](specs/structure-in-the-graph.md), whose phases DS-10 to DS-15 (renumbered 2026-11-22) and DS-7, brought forward, replace the rest of DS-3b. DS-4a, DS-4b and
DS-5 stay as planned here and may land any time after DS-9. Why (the user, 2026-11-22): DS-3b's
fixes were rules for when a whole-document pass may be skipped; the cause is that cells do
not record what they read of structure. The council reviews each phase's design and
code. The user asked for it (2026-11-21): "I want all costs that scale with
document size to be gone", councils on this plan and through the code.

## 0. Read first

1. This file.
2. [`specs/reactive-engine.md`](specs/reactive-engine.md): §1.2's goals G1 ("structure
   costs what changed") and G4 ("work that nothing observes is not done"); §6.1
   (necessity as counts, maintained per observer and per edge); §8.3 (what stays
   necessary without an observer: `ROOTED`); §8.9 (reading); §11 (transactions).
3. [`archive/handoff-reactive-structure.md`](archive/handoff-reactive-structure.md) §4.9
   (phase 4: necessity as built, and its "builder's decision" to relax on every structure
   change and transaction) and §5 (rules).
4. [`handoff-patches.md`](handoff-patches.md) PT-1.2 (the visibility gate, its costs and
   the fixes of 2026-11-21).
5. `src/document_bridge/demo.rs` tests `document_scaling_time`, `tick_change_time`,
   `drag_in_a_new_composition_time`, `lorenz_frames_time` (the probes below).

## 1. The problem, measured

The engine meets its targets in small documents, and every probe it was measured with
had everything observed or almost nothing hidden. In a large document with one small
composition shown, every operation still costs in proportion to the whole document.

`document_scaling_time` (native, release, after the fixes of 2026-11-21): the demo with
`k` extra copies of the gallery's nine pages, all hidden; one small shown composition
`perf` holding a rect, a `let` and a three-item repeat; p50 per operation:

| Operation in `perf` | +0 (3,730 nodes) | +1 (6,515) | +3 (12,085) |
|---|---|---|---|
| Idle frame (`advance`) | 0.09 ms | 0.18 | 0.36 |
| Drag frame (preview a leaf, `advance`) | 0.10 | 0.20 | 0.40 |
| Structural preview (the repeat's count 3 ↔ 4) | 1.75 | 3.30 | 6.29 |
| Constant edit (a transaction) | 2.74 | 5.35 | 11.04 |
| Structural edit (a transaction changing the count) | 4.34 | 7.91 | 15.79 |
| Playing (the playhead looping) | 0.42 | 0.84 | 1.72 |

wasm runs this code about 2.5 to 6 times slower. The user found three of these with
Chrome's profiler on 2026-11-21; the harness found none.

## 2. The rule

**No operation costs in proportion to anything it does not touch or show.** For work in a
shown composition C: the cost depends on C, on the cone of what changed (what it reads
and what reads it), and on what is shown, never on hidden compositions, hidden entities,
or the number of nodes, cells, repeats, machines, ports, clips or relations elsewhere.

Excluded, as inherently whole-document: loading and saving; export; a query that asks
about the whole document (search, the agent's view of everything); undo history's memory.

## 3. Inventory: every whole-document cost found

Found by sampling `document_scaling_time` at +3 per operation (`sample`, macOS), by
reading the code, by a read-only audit of the host, and by council round 1 (§9). Each
item names what triggers it.

### 3.1 Necessity is a sweep, not counts

- `Store::necessity` / `Reactive::necessity` list every hidden composition's cells and
  walk back from what is needed, whenever the graph is `relaxed`.
- **Every transaction, action, scratch plan and non-constant session write relaxes**
  (`exact()`): every cell needed, every stale one evaluated, and the next `advance`
  sweeps. Constant edit at +3: `necessity` is 1,155 of 1,434 samples.
- **Escaped reads pin for good:** `pinned` cells stay needed until the next rebuild, so
  once more reads pull (§4.2), the needed set would drift back to the whole document.
- **The idle sweep** clones `unneeded` each round and sorts every stale cell.
- Structure no longer relaxes (`need_what_reads_reach`, fd2db00), but a cell only a
  removed one needed stays needed until the next sweep.
- **Fallbacks that relax the whole graph:** `pipeline_from` after four rounds ("derive
  from an exact graph"), `current()` after four rounds, `scratch()`, and
  `apply_session`'s cycle check, which needs every edge as it is now.
- **Stale marking pushes into every reader of a changed leaf**, needed or not: a theme
  token, palette entry or `@name` context is read on every page, so changing one visits
  `dependents[leaf]`, document-sized.

### 3.2 The visibility gate

- `Store::paused` (PT-1.2) walks every hidden composition's subtree and readers; cached
  (`Clocks::gated`) and one pass (8148122), it still runs in full on any structural
  change or rewiring anywhere (0.9 ms at +0, growing).
- **Every frame, on a cache hit too:** `gate` clones the paused and pictured sets, and
  `Clocks::set` walks every paused and historical clock.

### 3.3 `ROOTED` and the rooted natives keep hidden pages computing

- `ROOTED` makes every cell of 29 facet kinds needed wherever it sits; and layout,
  repeat, island, `Clusters`, `Validate`, `Pins` and cluster natives are created rooted.
- RE-5.6 tried unrooting layout natives and reverted it, for two recorded reasons: an
  unrun native claims nothing, so a host publish takes a hidden stack's placement
  first; and `capture.rs` draws compositions no observer shows.
- Playing at +3: 1,669 of 2,227 samples in `evaluate`, hidden gallery pages driven by
  their clips.

### 3.4 Per frame

- `advance_presence`, `publish_presence`: every repeat's items, interning item scopes.
- `advance_machines` (`machine::Player::advance`): every machine's poses.
- `clock()` = `live_of_type("clock")`: every live node, several times a frame.
- `advance_steps`, `advance_queries`: scan all, skip most.
- **While playing:** `resample` → `time::sample` samples every clip in the document
  (also on every transaction); `advance` walks every `Animates` relation; without a loop,
  `clips_end` scans every clip.
- The gate's clones and `Clocks::set` (§3.2); the idle sweep (§3.1).

### 3.5 Per change

- **Every pipeline:** `port::network` / `network_now` scans every port;
  `publish_selection`, `resolve_contexts` and `palette::publish` scan their kinds;
  `published_by` clones a producer's map to compare, and a publish replaces it whole.
- **Every structural change:**
  - `settle_repeats` re-reads every drawn repeat's inputs (`repeat_inputs` reset;
    `expand::drawn_repeats`, `sync_repeats`);
  - `sync_layout`: `layout::structural_roots` over every group and table (twice),
    `pinned_parts` over every `use`, `LayoutNative::new` for every root;
  - `Clusters`, `Validate`, `Pins`: whole-document natives, primed;
  - `regraph`: `palette::leaves` every time; with `rederive` (reparent, link, unlink,
    port or anchor ops) `anchor::anchors` and a chain each, `port::derived_signals`,
    `followers`; with `rescope` every `token_readers` entry; inside a definition
    `nodes_of_type(entity)`;
  - `compute_heights`: a whole-graph fixpoint when `raise` fails or a tracked read
    raises a height.
- **Every transaction:** `Saved::of` (when `can_refuse()`: any constraint anywhere)
  clones the whole `Reactive` graph, clusters, geometry, repeats, item memo and presence,
  and drops them on commit (`Saved` drop 432, `Reactive::clone` 217 samples at +3); in
  `run`, `nodes_with(Attachment)` and `owners.held()`; `time::sample` (above).
- **Every `edit`, `compose` and `parts` plan:** `scratch()` clones `Docs`, `Reactive`,
  `Owners` and the players, then relaxes; some compose and parts paths clone the
  document directly.
- **Every pipeline with constraints:** `resolve_clusters_live` clones the cluster list
  and checks `unmoved` for every cluster; `post_apply` walks them all.
- **Rare:** interaction-derived `let` validation (`check_binding`) scans `bound_slots()`.

### 3.6 Host side

From a read-only audit of `src/document_bridge`, `src/iron_spatial`, `src/vector`,
`src/bevy_app` and the render path (2026-11-21). Already limited to the change or the
screen: `sync_document`'s idle exit, `reconcile`, `GeometryRevision::touch`, the render
layer's per-home filter and re-cutting, chunk retention, the draw walk and picking
(`spatial.candidates`), `snap_session`, `resolve_panel_views`.

Whole-document, by when it runs:

- **Every frame, idle included:**
  - Embed scans, several a frame: `nest::collect_embed_views` → `embeds_under`
    (nest.rs ~131, per 2D view and nesting level), `liveness::compute_liveness` →
    `composition::reach` → `embeds_in` (every embed, a `composition_of` walk and a sort,
    per composition reached), and `GeometryRevision::of_view` → `nest::shown_compositions`
    from the overlay and hover cursor (select.rs ~4464, ~3855).
  - `select::reachable_controls` (select.rs ~2115): every `empty` node in the document,
    each walking its ancestors, from `draw_select_overlay` every frame.
  - `scene3d::render_active_shapes` and `planes::sync_planes`: every 3D entity, hidden
    3D compositions included (every 3D node has an entity), and Bevy's own visibility
    work over them.
  - `materialize::decide_modes`: up to 2,000 nodes a frame while a large group is
    hovered (capped; listed, not a goal).
- **Every geometry change (a drag, an animation):** the spatial scene's `Bounds` phase
  calls `begin_changes` and `commit` on every scope (iron_spatial scene.rs ~371, ~479):
  one per group in every composition, hidden ones included.
- **Every sync that does not exit early:** `timeline_view::sync_themed` → `plan`
  (timeline_view.rs ~220) visits every clip and its `Animates` relations; the history
  log is filtered whole (mod.rs ~549: grows with history, not the document).
- **Every structural change:** `paint_order` (mod.rs ~690) walks every live node from
  every root and builds a new map, possibly twice a frame; the spatial scene's
  `rebuild_root` scans every entry and scope list (scene.rs ~613–646), and a change
  touching a template or a gone root rebuilds the whole scene (`rebuild`, ~535).
- **Rebuilding frames:** `render.rs` ~335 calls `shown_compositions` per panel and plane;
  plane views exist for every 2D embed in a 3D composition, hidden ones included.
- **A commit:** `gas::find` (every composition and `port_spec`), and `interaction.rs`
  ~65 over every `Interaction` entity when hover or press state changes.

## 4. The design

### 4.1 Cell necessity as counts

Each cell keeps a **need count**: its needed readers' edges into it, plus its holds.
Needed means a count above zero.

- **Holds:**
  - the cells of the **shown set**: shown compositions and what their embeds show (as
    §4.3 computes it), and the subtrees of the entities they picture through `Figures`
    (the host draws an entity's plot by reading the document, not through edges);
  - the cells of the **free owner** (model nodes no composition pictures, the timeline,
    loose roots), held whole, as today;
  - the roots of running owners (§4.4);
  - a gesture's previewed leaves, released when it ends, is cancelled, suspended or
    rolls back;
  - an operation's pulls (§4.2), released when the operation ends unless an owner keeps
    them (the player for its running entity, layout for its running composition).
    `pinned` goes.
- **Edges:** a binding's static deps and its tracked reads; a native's reads and its base
  reads. **A native is counted by its outputs:** a native reserves its outputs when it
  is made (RE-5.6's first prerequisite, moved here), and a needed reader of any output
  counts toward the native whether or not the native currently claims it; heights and
  cycle checks keep using claims. When a claim moves between a native, a producer and a
  binding, the needed readers of that leaf move their counts to the new source.
- **Order within a pass:** increments apply at once, decrements after the pass, on
  stable cell identities. An R6 waiter's hold on what it could not read is scoped to
  the pass and is not a counted edge, so a cycle cannot hold itself once nothing outside
  does.
- **Rising from zero** a cell raises its reads' counts and is evaluated once they are
  current: unconditionally, the cost of showing it (§2 allows it). **Falling to zero**
  lowers them.
- **Stale marking visits needed readers only.** Each leaf's dependents are split into
  needed and unneeded readers, moved when a count crosses zero; a change pushes to the
  needed ones. An unneeded cell is not touched by changes (spec §6.1) and is evaluated
  when it is needed again. There is no stale set.
- **The idle sweep** keeps only its diagnostic role, budgeted: cells whose problems may
  be out of date are queued when they become unneeded with a problem, and a frame takes
  up to its budget.
- **Replaces** `Store::necessity`, `Reactive::necessity`, `relaxed`, `pinned`, `relax`,
  the stale set and `need_what_reads_reach`.
- **The oracle** (tests only): after each operation, reachability from the holds over
  `reads_of` equals the needed set; every cell's count equals a recount; forward and
  reverse edges agree; every needed cell is current. It runs in a variant of the
  differential suite and the random scripts with `observe_all(false)`, random observe
  and unobserve steps, and pages left on Auto (the default suite observes everything,
  where the check is vacuous).

### 4.2 Exact reads without `exact()`

A transaction, an action, a session write and a plan read only what they read, current:

- **Store operations:** a read-only pull phase before `docs.begin()` under `current()`:
  `run`'s `expand::translate`, `machine::record`, `gesture::conflict`; `fire_at`'s
  `action_writes` and `handle::resolve_target`. `after_transaction`'s `settle_repeats`
  reads are recorded.
- **`apply_session`'s cycle check** pulls the upstream cone of the new binding's reads
  before checking.
- **The players** (machine, step, query) pull their inputs before they change state or
  send a request.
- **The host:** sites that read a few leaves use `Store::pull(leaves)` (inspector set,
  bind, unbind and patch, run mode, text style, table edits, latency); sites that read
  subtrees (detach, to-path, the component and authoring sites) run under
  `current(|s| …)`, recording their reads.
- **`document_view`** pulls its query's scope; only an unscoped view, and `export_svg`,
  pay the whole-document catch-up, renamed `exact_all` (§2).
- **No whole-graph fallback:** `current()` and `pipeline_from` pull the escaped set in a
  bounded loop; past four rounds they report a problem instead of relaxing.
- **Interim (DS-4a, before DS-4b):** `exact()` becomes a hold on every cell, released
  when the operation ends; escaped reads are holds scoped to the operation.
- Every outside-the-graph read is listed in DS-4b with its pull.

### 4.3 The gate: a composition graph and a traversal

Running is decided per **owner**: a composition, a pictured entity, or the free owner.

- **Each node's owner is cached:** its composition, else its nearest pictured ancestor
  entity, else free. A reparent or a `Figures` link or unlink re-keys the subtree's
  edges (the cost of the subtree).
- **Cross-owner read edges, keyed on leaves:** every graph edge, needed or not, whose
  reader runs in owner A and whose leaf (any leaf: a constant, a published value such as
  an accumulator's or a trace's output, a channel) sits in owner B ≠ A adds to a count
  on (A, B). A native runs in every owner it writes, or its own node's when it writes
  none. A rooted island links every owner it touches both ways: it integrates as a unit,
  so an island touching a running owner runs whole, as today.
- **Seeds**, from store-side indexes (embeds by home and by source, accumulators per
  composition, the set of Always compositions), not scans: the shown set; an
  accumulator reached only through an `on_demand` path holds (`paused()`'s
  `holding`/`still`: shown and open-path reachability are kept apart); `run = always`;
  the free owner. `observe_all` runs everything. The store's own whole-document natives
  (`Clusters`, `Validate`, `Pins`) are not readers for the gate: a deliberate change from
  today, where node 0 counts as free.
- **Running** is the traversal from the seeds over (A, B) edges and from a running
  composition to the entities it pictures. It runs at `advance`'s gate point, as today,
  only when a seed changes or an (A, B) count crosses zero, and visits the old and new
  running sets and their edges. Mutually reading hidden pages pause once no seed
  reaches them.
- **Outputs** are the running set's delta: `Clocks` starts and stops those owners; a
  clock not running is paused from when it stopped. Nothing document-sized is handed
  over or cloned.
- **Deleted:** `Store::paused`'s walks, `Clocks::gated`, `Reactive::reader_nodes`,
  `Docs::reshaped`, `Reactive::wired`, `op::reshapes`.

### 4.4 Roots

`ROOTED` by facet kind is replaced by this table; the owner is "running" in §4.3's sense
(free included).

| Kinds | Disposition | Their reader outside the graph |
|---|---|---|
| `accumulate`, `trace`, `machine`, `transition`, `tween`, `channel`, a machine entity's `args` | **Rooted everywhere** (with their input cones): a paused owner's steppers hold, but a trace must hear its input change and a machine its state change; their cost is their inputs' changes, which a paused page does not make | the step and machine players |
| `query` | Rooted everywhere, behaviour unchanged (presentation-time debounce) | the query player |
| `constraint`, `constraints`, cluster nodes | Rooted everywhere: a pose depends on the pose before (spec §8.3) | the solver natives |
| session `let`s, `selection`, `write` | Rooted in running owners | selection, actions |
| `repeat`, `when`, `item` | Rooted in running owners; in a paused one, settled when a pull reaches its subtree or an op addresses its copies | `settle_repeats` |
| `port`, `equation`, `connection` | Rooted in islands touching a running owner | the network |
| `embed`, `embed_camera`, `composition` | Rooted everywhere (cheap, structural; the gate and the host read them) | the gate, the host |
| `anchor`, `attachment` | Rooted everywhere (read by `regraph` and validation) | `regraph`, validation |
| `clock`, `clip`, `key`, `timing`, `track` | Rooted everywhere (the timeline is free) | `time::sample`, the timeline |
| `let` (not session), `args` (not a machine entity's) | **Unrooted**: needed only when read | — |

- **What changes:** non-session `let`s and plain `args` (what §3.3 measured) are no longer
  rooted; repeats, conditionals and items stop being rooted in paused owners.
- **Clips in a paused owner hold their value** and are sampled when the owner resumes
  (or when an exact pull reads them); the §8.8 reference holds them the same way.
- **Layout and repeats:** a layout native has a necessity-only edge to the repeat natives
  in its subtree (counted for need, excluded from heights and cycle checks), so a shown
  reader of a hidden stack's placement pulls the repeat first without closing a cycle
  (repeats may read placements, spec §9.2).
- **Captures:** `capture.rs` observes what it draws (RE-5.6's second prerequisite), and
  whether a capture seeds the gate is decided in DS-6a.

### 4.5 Indexes for the scans

- **Presence:** the repeats with items entering or leaving; only those, on running clocks.
- **Machines:** those mid-transition or whose state changed.
- **Steps and queries:** the running accumulators and traces; the queries with a request
  out or a debounce running.
- **The clock:** its id cached, kept by the ops that make or delete it.
- **Clips:** an index of clip targets; `time::sample` samples the clips of running
  owners (§4.4); `Animates` wakes from the index; the end of the last clip kept
  incrementally.
- **Ports:** islands kept by the ops that touch ports, connections and equations;
  `network_now` visits the islands whose natives ran.
- **Repeats:** inputs read for the repeats whose natives ran and those structure made or
  removed; `repeat_inputs` never reset whole.
- **Layout:** structural roots and pinned parts kept by structural ops; `sync_layout`
  visits the roots under what changed.
- **`regraph`:** `palette::leaves`, anchors, derived signals, followers and token
  readers indexed by the nodes they involve; rederived for the nodes an op touched.
- **Heights:** `compute_heights` replaced by raising along the cone (`raise`) in every
  case; the fixpoint kept as a test oracle.
- **Per-pipeline kinds:** selection, contexts, palette published for what changed.
- **Clusters:** `resolve_clusters_live` and `post_apply` take the dirty cluster keys,
  not a clone of the list with an `unmoved` check on each.
- **Published maps:** compared and replaced by the entries that changed.

### 4.6 Transactions and plans

- **No snapshot:** a refused transaction aborts the document journal (`docs.begin()`,
  `rollback`, as now) and re-derives the cone it touched from the restored document:
  `regraph` of its ops and their inverses, `invalidate` of the changed leaves, the
  pipeline. Deterministic: a refused transaction does not advance time. `Saved` goes.
  Test: a solver refusal after expansion restores every value, copy and edge.
- **Plans** (`edit`, `compose`, `parts`) run as a transaction on the store that is then
  aborted the same way, instead of on `scratch()`'s clones; the paths that clone the
  document directly go too.
- `nodes_with(Attachment)` and `owners.held()` in `run`: indexed.

### 4.7 Host side

Each pass of §3.6 starts from the shown compositions or from what changed (the store's
`Changes` name the nodes): an iteration that filters a global query or the whole tree by
"shown" still visits everything and does not count.

- **Embeds:** an index of embeds by the composition they sit in and the one they show,
  kept from `Changes`; every embed scan reads it.
- **Controls:** `reachable_controls` from an index of `empty` nodes by composition.
- **Spatial:** `begin_changes` and `commit` only on the scopes a change touched;
  `rebuild_root` without the full scans; a template change rebuilds what places it.
- **Paint order:** kept per composition; a structural change renumbers the parent's
  children, not the document.
- **3D:** shapes, planes and the per-camera content scan (`scene3d.rs` ~557) iterated
  from the shown 3D compositions' entities (an index by composition).
- **Timeline:** `sync_themed` replans the lanes whose clips, keys, machines, axis or theme
  changed, and on a scrub of pinned state; not on every sync.
- **`gas::find` and `interaction.rs`:** indexed.

Cut: `materialize::decide_modes` (capped at 2,000), the history-log filter (grows with
history, not the document), and Bevy's own visibility work over hidden 3D entities
(culling them is a separate decision).

## 5. Verification

- **Work counted at the choke points:** a debug counter (`#[doc(hidden)]`) incremented by
  `Tree::nodes_of_type`, `nodes_with`, `relations`, `descendants`, `live_nodes`,
  `bound_slots`, `roots`, and by every iteration of the store's and graph's large maps,
  wrapped in a counting newtype (not a list of fields); and a counting global allocator
  in the test binary, with a small tolerance (B-tree splits vary with size).
- **The scaling test**, two axes: many hidden pages (+0, +3, +6 copies of a fixture set)
  and one large hidden page. The fixture holds every kind §3 names: constraints, network
  islands, clips targeting hidden pages, machines, queries, embeds, a 3D composition,
  pictured entities, repeats. **No slope:** each operation's work and allocation equal
  across sizes, within a stated constant for operations whose own target grows. Idle is
  measured after the sweep's queue is empty.
- **Operations:** idle; a running accumulator on the shown page; drag; structural
  preview; constant edit; structural edit; reparent; link and unlink; a `let` made; a
  theme change; `fire_at`; a session write; an `edit` and a `compose` plan; paste; undo
  and redo; a rejected edit and a solver refusal after expansion; an agent transaction on
  a hidden page; showing, hiding, deleting and duplicating a page; selection and hover;
  a scoped agent view; observing nodes (`Interest::Nodes`); a capture; playing,
  scrubbing within one keyed transition; a query's reply on a paused page; a
  hidden-to-shown publish; a gesture cancelled. Each with constraints and without. Host
  operations are probed directly (DS-8).
- **Correctness:** the differential suite and the random scripts in their default mode
  and in the partial-observation variant with the oracle (§4.1); the heights oracle
  (edge order, not only minimal heights); the plain suite; the §8.8 property with paused
  pages, its reference stepping each page on its own clock and holding paused clips;
  its 1,000-seed run; the 10,000 random scripts; the host reference.
- **Specific tests:** two hidden pages reading each other pause once unseen; a meter on a
  shown page keeps a hidden laser running; `on_demand` behind an Auto ancestor; an
  entity pictured in two pages; a reparent and a `Figures` change re-key the gate; a
  hidden constraint does not run its page; a shown page's placed plot is current; a
  native made unneeded still claims its outputs; after a long script of edits the needed
  set returns to its baseline.
- **Timing:** `document_scaling_time` (ignored, release) at +6 within 1.2× of +0;
  `single-tab.txt` with the gallery three times over, `advance` and `frame` within 1.5×
  of the plain demo's.

## 6. Phases

Each phase is committed alone with its tests and retires its rows of the scaling test.
The council reviews the code by groups: DS-1; DS-2 and DS-3; DS-4a and DS-4b; DS-5 and
DS-6; DS-7 and DS-8.

| Key | Builds | Exit |
|---|---|---|
| DS-1 | §5's counters, allocator, fixture and scaling test over every operation, failing where today's costs are, each failure naming its passes | The test runs; its failures are §3's list |
| DS-2 | Per frame: presence, machines, steps, queries, the clock; clip sampling for running owners, the `Animates` index, clip ends; the gate's clones and `Clocks::set` by delta | Idle, drag and playing lose their per-frame counters |
| DS-3a | Per pipeline: ports, selection, contexts, palette, published maps, clusters' dirty keys | Value changes lose their counters |
| DS-3b | Per structural change: repeats, layout, `regraph`'s scans | Structural changes lose theirs but for necessity and the gate |
| DS-3c | Heights by `raise` along the cone, the fixpoint as an oracle | No `compute_heights` outside tests |
| DS-4a | Cell necessity as counts (§4.1): holds, output reservation, needed dependents, the idle queue, the interim `exact()` | The oracle in the partial-observation variant, on every operation |
| DS-4b | `exact()` replaced (§4.2), the host sites moved, no whole-graph fallback | Edits lose their necessity counters; no pin drift |
| DS-5 | The gate (§4.3) | Every gate test of §5 and PT-1.2; its whole-document code deleted |
| DS-6a | Captures observe what they draw | Capture tests |
| DS-6b | Roots (§4.4), one kind group per commit | The §8.8 property with paused pages; playing loses its counters |
| DS-7 | Refusal by journal and re-derivation, plans as aborted transactions (§4.6) | Rejected edits and plans lose their counters |
| DS-8 | The host side (§4.7), one commit per trigger: per frame, per geometry change, per structural change | Host probes flat |

**Final exit:** every operation of §5 has no slope, store and host, at both axes.
DS-2, DS-3 and DS-7 do not depend on the core; DS-4a precedes DS-4b, DS-5 and DS-6.

### DS-1 as done (2026-11-21)

- `iron_document::work` (feature `work`, tests only): per-thread counters at the
  choke points (`Document::nodes`, `live_nodes`, `roots`, `descendants`,
  `descendants_within`, `nodes_of_type`, `nodes_with`, `bound_slots`, `relations`) and
  `Counted<M, N>`, a wrapper whose iteration counts each entry, around the graph's
  `nodes`, `dependents`, `natives`, `unneeded`, `stale`, `runtime`, `held`, `owned`,
  `claims`, `base_readers`, `derived`, `token_readers`, `resolved`, `candidates`, and
  the store's `presence`, `item_memo`, `observers`, the machine player's `known`, the
  step player's `traces` and `values`, the query player's `states`, `Clocks`'s
  `since` and `behind`. The app's tests enable it (a dev-dependency) and count
  allocations per thread (`src/alloc_count.rs`, a test-only global allocator).
- `document_bridge::scaling::work_does_not_grow_with_the_hidden_document` (about 70 s,
  debug): the demo, twinned once (every composition's contents pasted into a twin:
  constraints, islands, 3D, embeds, clips, machines, pictured entities) and with one
  600-rect hidden page, against the plain demo; one shown composition `perf`; 19
  operations. **All 19 grow**, each listed in `SCALING` with the phase that should stop
  it; the test fails when an unlisted one grows or a listed one stops. What grows is §3's
  list: e.g. an idle frame visits every node (the clock lookup), `let made` visits roots
  106,208 times (rescope's providers) and 367,024 with one twin.
- **Code review (Opus 6/10, Sol 5/10), fixed the same day:**
  - expectations are per (operation, count), 347 pairs each with its phase, so a new
    pass in a listed operation fails; a count grows when it rises at both steps of three
    sizes on each axis (+0/+1/+2 twins; one page of 0/400/800 rects); allocations and
    bytes are counts like the rest, with absolute tolerances (24, 24 KiB);
  - two canaries must always grow: `save` (allocations) and a walk of every
    composition's descendants (the counters);
  - newly wrapped or counted: `repeats`, the clusters list, `chains`, `rest`, the
    owners' `counts` and `by_def`, `expanded_ids`, `signals()`, a leaf's readers
    (`Readers`), a producer's published set; `difference` charges what it yields;
    `last_root` for callers that only want the last root;
  - the fixture: a page of queries added (the demo has none), twins checked kind by kind
    (constraints, clips, machines, queries, embeds, ports, accumulators, repeats, 3D
    compositions, pictured entities, placed plots each grow at every step), a refused
    paste retried without its bad ops instead of swallowed; the hidden edit and
    show-a-page use a page made after the growth, the running accumulator a page
    observed only around that operation;
  - each operation is set up and torn down outside the measurement, measured from a
    settled store (nothing stale, a frame that changes nothing), and checks that it took
    effect; the five fixtures are built in parallel threads (about 50 s).
- **Not covered yet** (added by the phase that touches them): a `compose` plan, a
  solver refusal after expansion, deleting and duplicating a page, a scoped agent view, a
  capture, a query's reply on a paused page, a hidden-to-shown publish, a fixture without
  constraints, link and unlink, selection and hover, and the host operations (DS-8).

### DS-2 as done (2026-11-21)

Per frame, nothing visits what is at rest or paused:
- **The gate's cache hit does nothing** (no clone, no `Clocks::set`); a decision passes
  its sets by reference. Each decision now also lists the steppers on running clocks,
  the clips animating something running and the compositions they animate, watches a
  held trace's input and delay and every query's inputs, and has the machine player
  visit every instance once.
- **The graph wakes players** (`watch`, `watch_nodes`, `take_woken`,
  `take_woken_nodes`): a watched leaf, or any leaf of a watched node or of an expanded
  copy it owns, is woken when `note` or `mark` sees it change. **`publish_delta`** sets
  and withdraws single leaves of a producer's map.
- **Presence:** only repeats with an item entering or leaving (`presence_moving`, kept
  by `settle_presence`).
- **Steps:** only the running traces and accumulators, and held traces woken by their
  input; the step length is published for running accumulators only (it was published
  for every accumulator every frame, re-evaluating held ones); outputs by difference.
- **Queries:** only those whose inputs changed, whose answer arrived, or that are
  waiting; `pending()` from the waiting set; outputs by difference.
- **Machines:** only instances woken (their leaves or copies changed), moving
  (a transition, or reported running), clocked when the playhead moved, or all after a
  structural change; outputs by difference, kept per instance.
- **Clips:** the playhead and a constant edit sample only running clips; every clip
  after structure or a gate decision; `Animates` from the decision's list; the end of
  the last clip cached until the clips change.
- **The clock** is found through the type index (`live_of_type`), not a scan of
  every node; `ensure_clock` orders by the session root's children.
- **Result:** the idle frame has no slope at all; 101 (operation, count) pairs left
  `SCALING`. In the app (`single-tab.txt`, wasm): a drag's `advance` 0.49 → 0.04 ms,
  playing 3.21 → 0.67 ms. Differential (490), plain (486), app lib (20), smoke, fiber,
  pulses, idle and single-tab pass.
- **Left for later phases:** a drag's clusters list (DS-3a); playing's natives and
  ports scans (DS-3a, DS-3b); the gate deciding after an observer change (DS-5); theme
  tokens' readers (DS-4a).

### DS-3a as done (2026-11-21)

Per pipeline:
- **The port network** is cached against its own document stamp
  (`Docs::network_shaped`, `op::rewires_network`: structure, or a port's,
  connection's or equation's leaf), not against every edit, so the playhead's `t` and
  a constant edit no longer rebuild it.
- **The constraint clusters:** `resolve_clusters_live` solves only those whose watching
  native ran, under a finger, or not yet committed (indexed by key and by node,
  `Clusters::at`, `by_node`), not a clone of the list checked one by one; `committed`
  is pruned to current clusters wherever they are replaced.
- **The theme** is published again only when its tokens (`Docs::tokens_shaped`) or the
  scheme change; `palette::ensure_ops` looks at the ops before any token.
- **A diagnostic:** `scaling::trace_one_count` (`WORK_TRACE=<count>`,
  `WORK_OP=<operation>`) prints the call stacks behind a count, through
  `work::trace`.
- **Result:** a drag, playing and scrubbing have no slope at all. What a constant edit
  still grows by, traced: `expand::drawn_repeats` (every group, in `settle_repeats` after
  every transaction) and `published_by(ITEMS)` (DS-3b); the necessity sweep (DS-4).

## 7. Risks

- **Counts drift:** the oracle (§4.1) after every operation in tests.
- **A read that does not pull** reads a stale value: DS-4b lists every outside-the-graph
  read; the §8.8 property with paused pages checks it.
- **Unrooting** has failed once (RE-5.6): its prerequisites come first, and each kind
  removed names its reader.
- **Scope:** the engine's core again. Anything not in §3 is out.

## 8. Decisions (council rounds 1 and 2, adopted)

1. Cell necessity as counts (§4.1); running status by traversal over a composition
   graph (§4.3), not by counts.
2. `exact()` replaced by scoped pulls; a whole-document catch-up kept only for export
   and the agent's whole view (§4.2).
3. The gate from leaf-keyed cross-owner edges and a traversal from the seeds (§4.3).
4. Roots: §4.4's list; constraints everywhere; nothing else in a paused composition but
   scheduled work.
5. No snapshot and no transaction journal: refusal restores the document journal and
   re-derives the cone; plans run as aborted transactions (§4.6).
6. Free cells held whole; trace, accumulate and machine rooted everywhere; hidden
   physics islands touching a running owner run whole, as today.

## 9. Council

**Round 1 (2026-11-21): Opus 5/10, Sol 5/10.** Both: the direction holds; the gate cannot
come from cell counts (published values have no cell; mutual holds never collect);
`exact()` cannot simply go; unrooting failed once in RE-5.6; the inventory missed
`sync_layout`, `regraph`'s scans, whole-document natives, clip sampling, the gate's
per-frame clones, the idle sweep, heights and `scratch()`; escaped reads pin for good;
verification needs choke-point counters, an allocator, more operations and paused
pages in the §8.8 property. Opus: narrow the snapshot instead of a journal. Sol: root
consumers and scheduled work, not kinds; paused pages still have scheduled work. All
adopted above.

**Round 2 (2026-11-21): Opus 6/10, Sol 6/10; both: DS-1 can start.** Adopted: holds
include entities pictured by shown pages and the free owner; natives counted by their
outputs with reservation moved to DS-4a, and claim moves transfer counts; stale marking
only to needed readers (no stale set; unconditional evaluation on need); R6 holds
scoped to the pass; the oracle by reachability and exact recount, run with partial
observation; the gate's owners cached and re-keyed, every edge counted, natives in every
owner they write, islands whole, seeds and outputs from indexes and deltas; stepper
kinds rooted everywhere, a per-kind roots table; clips in paused owners hold; the
layout→repeat edge necessity-only; refusal by journal and re-derivation instead of a
snapshot, plans as aborted transactions; the remaining fallbacks and inventory items;
adversarial fixtures, two scaling axes, many more operations; DS-3 and DS-6 split;
council reviews by groups.

### Structure in the graph (specs/structure-in-the-graph.md)

**Round 1 (2026-11-22): Opus 5/10, Sol 5/10.** Both: the diagnosis holds, the rule is
right. Adopted: facet contents are a key; a touched-key log, not the rollback journal;
binding wiring and the graph's topology recorded; always record, hash and skip; the
oracle reruns cells rather than trusting the rules; each rule deleted with its
replacement; necessity and the gate decoupled; clips and grouping's split deferred; the
size estimate was low.

**Round 2 (2026-11-22): Opus 6/10, Sol 6/10.** Both: DS-9's slice could not delete
`layout_touched`, which is maker work; the oracle could pass with a key missing (the
differential suite is two incremental stores, not a rebuild); drains inside `run` would
evaluate on a document that can still roll back without the graph; scoped indexes belong
with makers. Adopted: DS-9 narrowed to existing, unpinned layout natives (§3.4); an
independent reference, a shadow and a canary (§3.5); drains only at §3.3's points, the
log's position journaled; `has` for presence and `Cx::read` per leaf (§3.1); `Bound` and
`Reads` folded into `Wired`; per-unit connection and anchor cells cut (DS-13); later
phases' open questions listed for their own designs (§5). Fixed in code at once: `Saved`
now restores the layout roots, pinned parts and glide nodes DS-3b part 2 added
(`tests/rollback.rs`, a layout made in a refused transaction).

**Round 3 (2026-11-22): Opus 7/10, Sol 7/10, on DS-9 alone.** Both: draining before
`sync_layout` would run old layout natives against new children (an output assertion),
so a drain only marks, where the graph is primed today; `prime_layout` is also the only
structural trigger for `Pins` and pinned layouts, which keep it; the oracle's reference
must not share the invalidation it checks; the canary must silence a choke point, not a
key kind. Opus: keys act only on unpinned layout cells in DS-9; the reference is
`layout::lay_out` on the store; an empty log at `begin` instead of a journaled position;
the shadow moves to DS-10; the key table shrinks to the four the slice uses; no hash.
Sol: refusal through implied mates with no constraint node; a session reorder still
evaluates; cost budgets named. All adopted (§3, §4).

**Code review of DS-9 with DS-2 and DS-3 (2026-11-22): Opus 6/10, Sol 5/10.**

Both: DS-9's mechanism holds (Opus traced every write path to a drain before the next
`begin`). The findings were in DS-2's and DS-3's code, untested because the tests ran
with everything observed and none added or removed a `layout` facet. Fixed, each with a
test that fails without the fix:

- **A nested layout root** was not made or dropped when its parent gained or lost a
  `layout` facet (both; "owned twice" in debug): `sync_layout` looks at a touched node's
  children too (`layout::a_layout_given_to_and_taken_from_a_parent_moves_the_root`).
- **A repeat whose layout reads its item** ran the old layout over new children (Sol;
  an output assertion): `settle_repeats` regraphs before it publishes a round
  (`layout::a_stack_whose_gap_reads_its_own_item_settles`).
- **A cluster reading another's variable** was left unsolved (Sol): `post_apply` and the
  live solve take the next cluster after each write (`next_to_solve`;
  `rollback::a_cluster_that_reads_another_s_variable_solves_after_it`).
- **A constant edit woke no player** (both): `invalidate` wakes each changed leaf
  (`reactivity::a_query_s_constant_input_set_asks_again`, one view observed).
- **`repeat.max` and the enter and exit timings** were not compared (both):
  `REPEAT_INPUTS` (`reactivity::a_repeat_s_max_set_settles_its_items`).
- **Clips:** a running clip withdraws what it drives and gave nothing for (scrubbing
  before its first key; Sol); a clip's own leaves set resample every clip (a `prop`
  retargeted; Opus); the clips' end is cleared only when clips or structure change, not
  by every playhead move (both); clocked machine instances are revisited when clips
  change (`Player::reclock`; Opus) (`time::scrubbing_before_the_first_key_withdraws_the_clip`,
  `time::a_clip_retargeted_lets_go_of_its_old_leaf`, `machines::keyed_state_scrubs_and_one_occurrence_retimes`).
- **`can_refuse`** counts anchors only where a connection is (both: any anchor made a
  circuit snapshot the graph on every edit); the fallback without `Saved` fails under
  `differential` (Opus).
- **The oracle** checks claims both ways (Opus); the grouping oracle fails on an error
  the kept grouping lacks (Sol).
- **Smaller:** glide specs found afresh when motion turns on record their nodes; players
  forget leaves the graph refused (`publish_delta` returns them); `scratch()` keeps the
  running clips; `structure_gen` deleted.

Left for DS-10: a binding retargeted onto a cluster's variable is not regrouped (Sol), the
grouping reach's gap that DS-10's recorded wiring reads close. Left for DS-12:
`sync_layout`'s pass over every pinned part, and `update_drawn`'s look at every gone
node (Opus).

**Direction review (2026-11-22): Opus 6/10, Sol 6/10; both: continue, with changes.** The
user asked whether this is the necessary path to both generality and performance. Both:
recorded reads are the right mechanism for *when* a kept result reruns, with no simpler
correct alternative. But they decide only when, not *how much*: grouping, validation
and connections stay whole-document cells unless scoped, and the plan deferred scoping.
The structure oracle can't fail for most writes (hand priming still covers them), and it
reimplements claim rules (it passes stale output sets and panics on a bound leaf). Key
lists were rewritten whole, and `Saved`'s clone grows with every keyed cell. Opus: most
of the review's bugs were in makers and hand lists, so design makers first. Sol: make
DS-10 a measured decision point. Both reproduced regressions from `fd884ac`: the
animation end went stale on a transition edit, and a running clip erased a paused
clip's held value. Fixed (tests in `machines` and `time`), with key changes applied as
a diff. The spec was rewritten around one rule: every kept result is a from-scratch
function over a bounded scope, rerun by its recorded keys. Its families are converted
one at a time, each deleting its rules (DS-10 to DS-15), makers are cells reconciled before
evaluation, refusal goes by the journal (DS-7), and one oracle replaces the per-family
ones. A spike showed a store rebuilt from the document can't be that oracle (72 of 494
tests differ by design, through history the document doesn't hold), so the oracle
compares structure from scratch and reruns every current cell.

**Review of the rewritten spec (2026-11-22): Opus 6.5/10, Sol 6/10; both: adopt with
named changes.** Both: the rule is right for *when*, but *in what order* derived work runs
was left to a loop the spec barely described, and "makers write no structure, so the
loop terminates" was false (expansion, settling and the solver's write-back write
structure; wiring changes during evaluation). Opus: grouping reads values, so it must
split into a structural partition and per-cluster cells; `Cx::graph()`, `previous()`,
`scope_stamp` and side tables read around the recorder; `Saved` restores far more history
than the spec named, so DS-7 should be a graph journal and not depend on the families;
the oracle should check history cells as a fixpoint; the §1 table missed several caches,
including the host's. Sol: separate reservations from claims and reconcile in one batch;
define the state boundary for refusal and the oracle; rebuild maker candidates from raw
records; scope by what computes together; name the whole-document costs left. Adopted:
the four-stage loop with makers reading no values by type (§3.2); the grouping split
(DS-12); `Cx` limited to recorded reads (DS-10); refusal by a graph journal, decoupled
(DS-7); state as an explicit contract (§3.4); a fixpoint oracle with an index check
(§4); both tables of §1 complete; the costs left after DS-15 listed (§6). Declined: a
coarser first cut rerunning every maker of a touched composition (Opus), which would
make structural edits grow with the active composition; solving clusters across
compositions (Sol), in favour of checking assumption A11; per-clip animation ends with a
maintained maximum now (Sol), left to PT-9; whether refusal is needed at all (Opus),
which is product behaviour.

**Review of the loop and the oracle (2026-11-22): Opus 7/10, Sol 6/10; both: adopt with
named changes.** Both found that the solver and its held copies didn't fit the loop:
expansion reads the holds, `release` writes template values back, and the cluster cells
declare no outputs, so "order by heights" had nothing to order. Sol: the loop could stop
with work pending (a repeat publishing item values with no copy written changes another
repeat's count), and the oracle skipped expansion and couldn't judge the solver (two
valid poses from two starts); the same step from the same state, with nothing cached,
can. Opus: five round loops decide order today, not four functions; `Saved`'s field list
is another hand list, which persistent maps would make unnecessary; the door stays open
for clips, the host and the gate; "no values" means authored constants only. Both: name
the costs left (blocked retries, session publish, attachment validation, the expansion
budget, network `deref`), and stop gating phases on net lines. Adopted: a loop of five
stages run to quiescence, with one budget, replacing the five loops; holds as state, and a
dropped hold marks its owner (no `release`); the solve as stage 4, ordered as
`next_to_solve` does, with ring checks per cluster; the oracle as the same call on a clone
with every derived result dropped; the atomicity check made a failure for refusals; DS-7
to choose between persistent maps and a journal by measuring; a design of copies as views
before DS-15; every remaining cost given an owner; an honest account of where the speed
comes from. Declined: clips converted here (Opus), since PT-9 replaces clips and must
follow the rule; ordering clusters by graph heights with claimed variables (Opus), since
the solve loop already orders them.

**Review of the loop to quiescence (2026-11-22): Opus 7/10, Sol 7/10; both: adopt with
named changes.** Both: recorded reads are right, but merging the writers' loops into one
under one budget would change behaviour. An unsatisfiable solve refuses, a live solve
keeps its pose, and repeat feedback is held per repeat; a shared budget lets one
composition's feedback cut another's. Opus: `pipeline_from`'s and `current`'s rounds
exist for reads outside the graph and belong to DS-4b; history cells aren't idempotent
(measured: re-solving a converged cluster from its own pose changed bits in 173 of 200
cases), so an oracle that steps them where the subject didn't fails correct code; a
cluster's write-back marks its own change test; the solve needs Kahn's order; hold
membership is derived; undo needs `release`'s recorded writes; build the oracle's
reference from a list of what to keep; decide copies as views before DS-12. Sol:
separate recomputation from state transitions; make locality an exit test with unrelated
units inside the active composition and joins across roots; put refusal's boundary before
`run`'s first effect; place `glided`'s sweep. Adopted: the order as a written, checked
table, with each writer keeping its loop and failure rule (§3.2); the cluster order as a
cell; a writer's own writes don't mark it; the oracle as a fresh store with a declared
state list, comparing derived results and checking that history stepped exactly when its
inputs changed (§4); refusal checked apart; holds derived, write-backs kept; the
copies-as-views decision after DS-11; locality tests in every exit. Declined: making
history cells idempotent by construction (Opus), since the oracle no longer needs it.
