# Handoff: costs that scale with the document

**Date:** 2026-11-21 · **Branch:** `master` · **Keys:** `DS-n` (plans/README.md, "Work
keys") · **Status:** plan, revised after council rounds 1 (Opus 5/10, Sol 5/10) and 2 (Opus
6/10, Sol 6/10; both: DS-1 can start). DS-1 next; the council reviews the code by
groups (§6). The user asked for it (2026-11-21): "I want all costs that scale with
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
