# Handoff: costs that scale with the document

**Date:** 2026-11-21 · **Branch:** `master` · **Keys:** `DS-n` (plans/README.md, "Work
keys") · **Status:** plan, for council review before any code. The user asked for it
(2026-11-21): "I want all costs that scale with document size to be gone", councils on
this plan and through the code.

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

Found by sampling `document_scaling_time` at +3 (`sample`, macOS) per operation, and by
reading the code. Host-side items (§3.6) are from a separate read-only audit.

### 3.1 Necessity is a sweep, not counts

- `Store::necessity` / `Reactive::necessity` (lib.rs ~2940, graph.rs ~694) lists every
  hidden composition's cells and walks back from what is needed. It runs whenever the
  graph is `relaxed`.
- **Every transaction relaxes:** `exact()` before a transaction, an action, a scratch
  plan, and a session write that is not a constant (lib.rs `apply_session`) makes every
  cell needed and evaluates every stale one, and the next `advance` sweeps again.
  Constant edit at +3: `necessity` is 1,155 of 1,434 samples of `apply` and `advance`.
- **Structure no longer relaxes** (`need_what_reads_reach`, 2026-11-21, fd2db00), but a
  cell only a removed one needed stays needed until the next sweep.
- The spec (§6.1) defines necessity as counts kept per observer and per edge. It was
  never built that way.

### 3.2 The visibility gate is a second sweep

- `Store::paused` (PT-1.2) decides which compositions and entities to pause by walking
  every hidden composition's subtree and readers. Now cached (`Clocks::gated`) and one
  pass (8148122), it still runs in full on any structural change or rewiring anywhere:
  0.9 ms at +0, and it grows with the hidden document.

### 3.3 `ROOTED` keeps hidden pages computing

- `ROOTED` (lib.rs ~269) makes every cell of 29 facet kinds needed wherever it sits:
  `let`, `args`, repeats, conditionals, items, machines, ports, clips and more, because
  the store reads them outside the graph.
- So a value reaching a hidden page (a clip while playing, a publish, a hidden plot's
  argument) evaluates that page's lets, args and repeat inputs. Playing at +3: 1,669 of
  2,227 samples in `evaluate`, mostly hidden gallery pages driven by their clips.
- The store already has the means to read on demand: a read of a stale or unneeded cell
  from outside the graph is recorded (`take_escaped`), made needed (`Reactive::need`)
  and the caller runs again (`settle_reads`, `current`).

### 3.4 Per-frame scans

Each frame, whatever changed:
- `advance_presence` and `publish_presence` walk every repeat's items in the document
  (`self.presence`), interning item scopes (`NodeId::item_scope`) for each.
- `advance_machines` (`machine::Player::advance`) visits every machine and computes its
  poses (`machine::pose`, `channels`, `children`) every frame.
- `clock()` is `live_of_type("clock")`: a scan of every live node, several times a frame
  (`advance_playhead`, `set_time`).
- `advance_steps`, `advance_queries`: smaller, same shape (scan all, skip most).

Idle at +3: `advance_presence` 1,201 and `advance_machines` 1,079 of 1,298 samples of
`advance`.

### 3.5 Per-change scans

- `port::network` / `network_now`: every pipeline with a change scans every port in the
  document (`nodes_of_type`), even when no port is near the change. RE-5.3 put the
  network's solve on per-island natives; the scan for islands did not go.
- `settle_repeats`: after structure, `repeat_inputs` is reset and every drawn repeat's
  inputs are read and compared (`expand::drawn_repeats`).
- `published_by` clones a producer's whole published map to compare (items, presence).
- **Every transaction snapshots the store for rollback** (`Saved::of`, lib.rs ~370):
  a clone of the whole `Reactive` graph, the clusters, geometry revisions, repeats,
  item memo and presence, dropped when the transaction commits. Constant edit at +3:
  `Saved` drop 432 and `Reactive::clone` 217 samples.
- `after_transaction`'s remaining passes (to be split in DS-1).

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

### 4.1 Necessity as counts (spec §6.1)

Each cell keeps a **need count**: how many needed readers it has, plus one for each root
that holds it. A cell is needed when its count is above zero.

- **Roots:** the cells of shown compositions (an observer added or dropped adds or
  removes them, which costs that composition); the cells outside every composition and
  every hidden entity (loose roots, the timeline, unpictured model nodes); the roots of
  §4.3; a gesture's previewed leaves; pinned cells (`pinned`, as today).
- **An edge made** from a needed reader raises its source's count; **an edge removed**
  lowers it. A cell whose count rises from zero becomes needed: its reads' counts rise in
  turn, and if it is stale it is evaluated once they are current (pull, §6.1's second
  bullet). A count falling to zero makes the cell unneeded and lowers its reads' counts.
- **Cycles:** binding cycles are errors and their cells fail; a native cycle (R6) can
  keep itself needed. Accepted: extra work, never a stale read. Stated as a limit.
- **Replaces:** `Store::necessity`, `Reactive::necessity`, `relaxed`, the idle sweep's
  role of catching up (it stays for errors of unneeded cells, §8.7), and
  `need_what_reads_reach`.
- **`exact()` goes.** A transaction, an action, a scratch plan and a session write read
  what they read through the same pull: a read of a stale cell from outside the graph is
  escaped, made needed and read again (`current`), as handles already do. What validation
  or expansion reads is made needed the same way.

### 4.2 The gate from counts

A hidden composition's clock runs when something running needs a cell in it. With counts
kept per composition:

- `held_from_outside[H]`: how many cells of H are needed by readers outside H.
- H runs when it is shown, set to run always, or `held_from_outside[H] > 0`; an entity
  likewise, by its pictures and its readers outside it.
- Edges into and out of H move the count; nothing walks H.
- `Store::paused`, `Clocks::gated`, `Reactive::reader_nodes`, `Docs::reshaped`,
  `Reactive::wired` and `op::reshapes` are deleted.

### 4.3 `ROOTED` shrinks to what is running

`ROOTED` exists because the store reads those cells outside the graph: steppers read
`next` formulas and clocks, the machine player reads states and transitions, expansion
reads repeat inputs, the network reads ports.

- **In a running composition** (shown, always-run, or held from outside), state facets
  stay roots: `accumulate`, `trace`, `machine`, `query`, session `let`s, constraints.
  Their values move on time and must not be lost (§8.3).
- **In a paused composition** nothing is a root. Its steppers hold (PT-1.2), so nothing
  reads it from outside the graph; a value reaching it marks cells stale and stops.
- **Structure** (repeats, conditionals, items, embeds) is never a root: expansion reads a
  repeat's inputs through the escaped-read path, so a repeat in a shown composition is
  needed through what draws it, and a hidden composition's structure is settled when it
  is shown (G4; "hidden compositions are still laid out", RE-5.6's unmet item).
- **Ports and equations** are roots only in a running composition (the network of a
  paused one holds, as its clocks do).
- The list that remains is written down with each entry's reason; every other kind is
  needed only when read.

### 4.4 Indexes for the per-frame and per-change scans

- **Presence:** keep the set of repeats with items entering or leaving; `advance_presence`
  and `publish_presence` visit only those, and only on running clocks.
- **Machines:** keep the set of machines mid-transition or whose state changed;
  `advance_machines` visits only those. A machine at rest costs nothing a frame.
- **Steps and queries:** the same: the running accumulators and traces, the queries
  with a request out or a debounce running.
- **The clock:** its id cached, kept by the ops that create or delete it.
- **The port network:** islands kept up to date by the ops that touch ports,
  connections and equations; `network_now` visits the islands whose natives ran.
- **Repeats:** `settle_repeats` reads the inputs of the repeats whose natives ran and of
  those structure made or removed; `repeat_inputs` is never reset whole.
- **Published maps:** compared by the entries that changed, not cloned whole.

### 4.5 Transactions roll back by journal

`Saved::of` is replaced by a journal: each structure a transaction changes (the graph's
cells and edges, clusters, geometry revisions, repeats, item memo, presence) records
what it replaced, as `Docs` does for the document (`begin`, `rollback`, `commit`).
A rollback replays the journal backwards; a commit drops it. Cost: what the transaction
changed.

### 4.6 Host side

Each pass of §3.6 is limited to the shown compositions and to what changed; the store's
`Changes` already name the nodes.

- **Embeds:** an index of embeds by the composition they sit in and the one they show,
  kept from `Changes`; every embed scan reads it (one per view per frame becomes one per
  change).
- **Controls:** `reachable_controls` from an index of `empty` nodes by composition.
- **Spatial:** `begin_changes` and `commit` only on the scopes a change touched;
  `rebuild_root` without the full scans; a template change rebuilds what places it.
- **Paint order:** per composition, kept incrementally: a structural change renumbers
  the parent's children range, not the document.
- **3D:** entities of hidden 3D compositions not visited (`InComposition` filtered by the
  shown set, or the entities parked while hidden).
- **Timeline:** `sync_themed` only when clips or the clock changed.
- **Planes:** only for embeds in shown 3D compositions.
- `gas::find` and `interaction.rs`: indexed, or left with a note if cheap (DS-1
  measures).

## 5. Verification

- **A scaling test that fails, by counts, not times.** `iron_document` gains a debug
  counter of work done: nodes and cells visited by store and graph passes
  (`Store::work()`, `#[doc(hidden)]`). A test builds the scaling scenario at +0 and +3
  and asserts each operation's count is equal at both sizes (or within a stated constant
  for the observer change itself). A new whole-document pass fails it.
- **The timing probe** `document_scaling_time` stays, ignored, release: at +3 each
  operation within 1.2× of +0.
- **Correctness:** the differential suite, plain suite, the §8.8 property
  (`observing_part_of_the_time_changes_nothing_once_all_is_observed`, and its 1,000-seed
  ignored run), the 10,000 random scripts, the host reference, and the app scripts
  (`smoke`, `regression`, `fiber`, `pulses`, `idle`, `single-tab`, `gestures` with a
  check-in).
- **In the app:** `single-tab.txt` gains a document with the gallery three times over,
  and asserts `advance` and `frame` within 1.5× of the plain demo's.

## 6. Phases

Each phase is committed alone, with its tests, the scaling count for what it covers,
and a council review of the code before the next starts.

| Key | Builds | Exit |
|---|---|---|
| DS-1 | The work counter and the scaling count test (§5), failing where today's costs are; `after_transaction` and the host passes split and listed (§3.5, §3.6) | The test runs and names each failing operation's passes |
| DS-2 | Per-frame indexes (§4.4: presence, machines, steps, queries, the clock) | Idle and drag frames pass the scaling count |
| DS-3 | Per-change indexes (§4.4: ports, repeats, published maps) | Structural preview passes, but for necessity and the gate |
| DS-4 | Necessity as counts, `exact` removed (§4.1) | Constant and structural edits pass, but for the snapshot |
| DS-5 | The gate from counts (§4.2) | Structural preview passes; the gate's code deleted |
| DS-6 | `ROOTED` shrunk (§4.3) | Playing passes; hidden structure settles when shown |
| DS-7 | Rollback by journal (§4.5) | Every store operation passes |
| DS-8 | The host side (§4.6) | `single-tab.txt` on the triple gallery passes |

DS-2, DS-3 and DS-7 are independent of the core (DS-4 to DS-6) and can go first; DS-4
must precede DS-5 and DS-6.

## 7. Risks

- **Counts drift** (an edge path that forgets to adjust): the differential suite and a
  debug check that recomputes necessity from scratch after every operation in tests
  (the old sweep, kept as a test oracle only) catch it.
- **Pull everywhere** changes when stale cells are brought up to date; a read that does
  not go through the escaped path reads a stale value. Every outside-the-graph read in
  the store is listed in DS-4 and DS-6 and checked by the §8.8 property.
- **`ROOTED`'s reasons** are partly undocumented; each kind removed needs its reader
  found.
- **Scope:** this is the engine's core again. Fight additions: anything not on §3's list
  is out.

## 8. Decisions to make in review

1. Counts (§4.1) against keeping the sweep but running it only on observer changes and
   containing every other trigger. Counts are the spec's design; the sweep is simpler.
2. Whether `exact()` can go entirely (§4.1), or transactions keep reading exactly by a
   narrower pull.
3. The gate from per-composition counts (§4.2), or derived otherwise.
4. Which facet kinds remain roots in a running composition (§4.3).
