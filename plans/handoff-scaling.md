# Handoff: costs that scale with the document

**Date:** 2026-11-21 · **Branch:** `master` · **Keys:** `DS-n` (plans/README.md, "Work
keys") · **Status:** plan, revised after council round 1 (Opus 5/10, Sol 5/10); round 2 next,
then DS-1. The user asked for it (2026-11-21): "I want all costs that scale with
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
- **Every `edit`, `compose` and `parts` plan:** `scratch()` clones `Docs`, `Reactive` and
  `Owners`, then relaxes.

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

Each cell keeps a **need count**: its needed readers' current edges into it, plus its
holds. Needed means a count above zero.

- **Holds:** the cells of shown compositions (an observer added or dropped adds or
  removes them: that composition's cost); the cells of running compositions' roots
  (§4.4); a gesture's previewed leaves, released when the gesture ends, is cancelled or
  rolls back; an operation's pulls (§4.2), released when the operation ends unless an
  owner keeps them (the player for its running entity, layout for its running
  composition). `pinned` goes.
- **Edges:** a binding's static deps and its tracked reads; a native's reads and its base
  reads; a native is one cell for all its outputs, claim transfers included. Within a
  pass, increments apply at once and decrements after the pass (restructure clears
  tracked sets and natives re-record their reads, which would otherwise cascade cells to
  unneeded mid-pass). A count rising from zero raises its reads' counts and, if the cell
  is stale, evaluates it once they are current; one falling to zero lowers them.
- **Cycles** at cell level are binding errors or native R6 waits; an R6 cycle that loses
  its last hold from outside drops, since holds come only from outside the cycle.
- **Stale marking stays as push** along the change's cone (it touches the readers at
  the boundary of what is needed, which is the cone, not the document);
  `changed_at`/`computed_at` timestamps are not adopted now.
- **The idle sweep** becomes a queue: stale unneeded cells with a pending problem are
  queued when marked, and a frame takes up to its budget, never cloning or sorting the
  set.
- **Replaces** `Store::necessity`, `Reactive::necessity`, `relaxed`, `pinned`, `relax`
  and `need_what_reads_reach`. **The old sweep stays as a test oracle:** after every
  operation in the differential suite, the counts' needed set equals the sweep's.

### 4.2 Exact reads without `exact()`

A transaction, an action, a session write and a plan read only what they read, current:

- **Store operations:** a read-only pull phase before `docs.begin()` under `current()`:
  `run`'s `expand::translate`, `machine::record`, `gesture::conflict`; `fire_at`'s
  `action_writes` and `handle::resolve_target`; `apply_session`'s binding checks (cycle
  checks kept). `after_transaction`'s `settle_repeats` reads are recorded.
- **The machine, step and query players** pull their inputs before they change state or
  send a request.
- **The host:** `Store::pull(leaves)` (and `resolved_current`) replaces the 16
  `store.0.exact()` call sites in `engine_api.rs` that read a few leaves after it
  (inspector set, bind, unbind and patch, run mode, text style, table edits, detach,
  link, to-path, component and authoring, latency).
- **Whole-document reads stay whole:** `export_svg` and the agent's whole-document view
  (`document_view`) keep a whole-document catch-up, renamed `exact_all`, as §2 allows.
- **`scratch()`** clones what it plans against: §4.6.
- Every outside-the-graph read is listed in DS-4b, each with its pull.

### 4.3 The gate: a composition graph and a traversal

Running is decided per owner (composition or pictured entity), not from cell counts:

- **Cross-owner read edges, keyed on leaves:** for every graph edge whose reader runs in
  owner A and whose leaf (any leaf: a constant, a published value such as an
  accumulator's or a trace's output, a channel) sits in owner B ≠ A, a count on (A, B).
  Kept as edges are made and removed; nothing walks a composition.
- **Running** is a traversal from the seeds over the (A, B) edges: shown compositions
  (with what their embeds show) except an `on_demand` one holding an accumulator with
  no open path (`paused()`'s `holding`/`still`), `run = always` compositions, and free
  readers (model nodes no composition pictures, the timeline, loose roots). An entity is
  reached through its `Figures` links from a running composition and through read edges.
  The store's own whole-document natives (`Clusters`, `Validate`, `Pins`, at node 0) are
  not seeds.
- **Cost:** the traversal visits the running set and its edges, recomputed only when a
  seed changes or an (A, B) count crosses zero; nothing hidden and unread is visited.
  Mutually reading hidden pages pause once no seed reaches them.
- **Clocks change by delta:** `Clocks::set` takes what started and stopped.
- **Deleted:** `Store::paused`'s walks, `Clocks::gated`, `Reactive::reader_nodes`,
  `Docs::reshaped`, `Reactive::wired`, `op::reshapes`.

### 4.4 Roots: consumers and scheduled work

`ROOTED` by facet kind is replaced by roots that name a consumer:

- **In a running composition:** state and what moves it: `accumulate`, `trace`,
  `machine` with its entity's `args`, `transition`, `tween`, `channel`; `query`; session
  `let`s; structure (`repeat`, `when`, `item`), settled as now; ports and equations of
  islands that touch a running composition.
- **Everywhere:** constraints and cluster nodes (a pose depends on the pose before;
  spec §8.3; their cost is their changes, not the document).
- **In a paused composition:** nothing else. Work that must still happen there is
  scheduled, not rooted: a trace records an input change (on the input's change); a
  query with a request out or a debounce running (its deadline); a keyed or externally
  driven machine (its input's change). Structure in a paused composition settles when a
  pull reaches its subtree or an op addresses its copies; a layout native has an edge to
  each repeat native in its subtree, so a shown reader of a hidden stack's placement
  pulls the repeat first.
- **Prerequisites, before any native is unrooted** (RE-5.6's reasons): a native reserves
  its outputs when made, so an unrun one still claims them; a capture (`capture.rs`)
  observes what it draws.
- **Clips:** clips whose targets are in paused owners are not sampled or published;
  they are sampled when their owner resumes (DS-2, before any root changes).

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
- **Published maps:** compared and replaced by the entries that changed.

### 4.6 Transactions and plans

- **The snapshot narrowed:** `can_refuse` is decided for the transaction's cone (the
  clusters it touches), not "any constraint anywhere": a constant edit in `perf` takes
  none. Where one is still needed, it covers the clusters touched, not the store.
- **`scratch()`** clones the store's large maps; if DS-1's counts show it scaling after
  the above, persistent maps (structural sharing) for `Docs`, `Reactive` and `Owners`.
  No transaction journal.
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
- **3D:** shapes and planes iterated from the shown 3D compositions' entities (an index
  by composition), not a global query.
- **Timeline:** `sync_themed` only when clips or the clock changed.

Cut (bounded or not document-sized): `materialize::decide_modes` (capped at 2,000),
`gas::find`, `interaction.rs`, the history-log filter.

## 5. Verification

- **Work counted at the choke points, not per pass:** a debug counter (`#[doc(hidden)]`)
  incremented by `Tree::nodes_of_type`, `nodes_with`, `relations`, `descendants`,
  `live_nodes`, `bound_slots`, and by iteration over the graph's large maps (`nodes`,
  `dependents`, `natives`, `unneeded`, `stale`) and the store's (`presence`, machines,
  clips); and a counting global allocator in the test binary (bytes and allocations).
- **The scaling test** builds the scenario at +0, +3 and +6 gallery copies and asserts
  **no slope**: each operation's work and allocation equal across the three sizes,
  within a stated constant for operations whose own target grows (showing a tab).
- **Operations covered:** idle; drag; structural preview; constant edit; structural edit;
  reparent; link and unlink; a `let` made (rescope); `fire_at`; a session write; an
  `edit` and a `compose` plan (scratch); undo and redo; a rejected edit; showing and
  hiding a tab; a theme change; playing and scrubbing; a gesture cancelled. Each in a
  document with constraints and one without.
- **Correctness:** the differential suite with the necessity oracle (§4.1) and the
  heights oracle (§4.5) checked after every operation; the plain suite; the §8.8 property
  **with paused pages** (a variant whose pages are not Always, compared with a reference
  stepped on each page's own clock, as `a_hidden_plot_s_transition_holds_part_way` does);
  its 1,000-seed ignored run; the 10,000 random scripts; the host reference.
- **Specific tests:** two hidden pages reading each other pause once unseen; a meter on a
  shown page keeps a hidden laser running (published values); `on_demand` with an
  accumulator; an entity pictured in two pages; a hidden constraint does not run its page;
  after a long script of edits the needed set returns to its baseline (no pin drift).
- **Timing:** `document_scaling_time` (ignored, release): at +6 each operation within
  1.2× of +0. In the app, `single-tab.txt` gains the gallery three times over: `advance`
  and `frame` within 1.5× of the plain demo's.

## 6. Phases

Each phase is committed alone with its tests and its rows of the scaling test, and gets
a council review of its code before the next starts.

| Key | Builds | Exit |
|---|---|---|
| DS-1 | §5's counters, allocator and scaling test over every operation, failing where today's costs are, each failure naming its passes | The test runs; its failures are §3's list |
| DS-2 | Per frame: presence, machines, steps, queries, clock (§4.5); clips of paused owners not sampled, the `Animates` index, clip ends (§4.4, §4.5); the gate's clones and `Clocks::set` by delta (§4.3); the idle sweep as a queue (§4.1) | Idle, drag and playing have no slope |
| DS-3 | Per change: ports, repeats, layout, `regraph`'s scans, heights, per-pipeline kinds, published maps (§4.5) | Structural preview has no slope but for the gate's decision |
| DS-4a | Cell necessity as counts, the old sweep as an oracle (§4.1) | The oracle agrees on every operation of the differential suite and the random scripts |
| DS-4b | `exact()` replaced (§4.2), the 16 host sites on `Store::pull` | Constant and structural edits have no slope but for the snapshot; no pin drift |
| DS-5 | The gate as a composition graph and traversal (§4.3) | Every gate test of §5 and PT-1.2; the gate's whole-document code deleted |
| DS-6 | Roots as consumers (§4.4), after its two prerequisites | The §8.8 property with paused pages; playing and every edit have no slope |
| DS-7 | The snapshot narrowed, `scratch()`, `run`'s scans (§4.6) | Every store operation has no slope |
| DS-8 | The host side (§4.7), one commit per trigger: per frame, per geometry change, per structural change | `single-tab.txt` on the triple gallery passes |

DS-2, DS-3 and DS-7 do not depend on the core; DS-4a precedes DS-4b, DS-5 and DS-6.

## 7. Risks

- **Counts drift:** the oracle (§4.1) after every operation in tests.
- **A read that does not pull** reads a stale value: DS-4b lists every outside-the-graph
  read; the §8.8 property with paused pages checks it.
- **Unrooting** has failed once (RE-5.6): its prerequisites come first, and each kind
  removed names its reader.
- **Scope:** the engine's core again. Anything not in §3 is out.

## 8. Decisions (council round 1's recommendations, adopted)

1. Cell necessity as counts (§4.1); running status by traversal over a composition
   graph (§4.3), not by counts.
2. `exact()` replaced by scoped pulls; a whole-document catch-up kept only for export
   and the agent's whole view (§4.2).
3. The gate from leaf-keyed cross-owner edges and a traversal from the seeds (§4.3).
4. Roots: §4.4's list; constraints everywhere; nothing else in a paused composition but
   scheduled work.
5. No transaction journal: the snapshot narrowed instead (§4.6).

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
