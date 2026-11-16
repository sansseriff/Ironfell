# The reactive engine: owners, demand, templates and time

**Status:** Proposed 2026-11-15. **Nothing here is built; it waits on the user's
approval**, and a council review if the user asks for one (AGENTS.md). Work order and
the next task: [`../handoff-reactive-structure.md`](../handoff-reactive-structure.md).

**Replaces** the reactive graph (`graph.rs`), expansion (`expand.rs`) and the settling
and pipeline half of the store (`lib.rs`) with a new engine, written beside the old one
and swapped in when a differential test says the two agree (§16). It supersedes
[`reactivity.md`](reactivity.md) §3.3's "settle in rounds" and the A/B/C options of the
first version of the handoff. Everything else in reactivity.md (keys, state, actions,
tracked reads, context, queries, presence) is kept as behaviour and re-implemented.

**Read first:** [`reactivity.md`](reactivity.md) §3 and §13;
[`component-model.md`](component-model.md) (instances, overrides, scope);
[`instance-cache.md`](instance-cache.md) §7 (references) and "Open";
[`scopes.md`](scopes.md) §8 (streaming constraints);
[`../notes/narrative-layer-and-storage-primitives.md`](../notes/narrative-layer-and-storage-primitives.md)
§6.4 and §6.7 (the narrative clock, seekable values).

## 1. Why

### 1.1 What is wrong today

| Cost | Where | Measured |
|---|---|---|
| Any change to a repeat's items re-expands the document and rebuilds the graph, evaluating every binding | `Store::settle_repeats` (lib.rs ~1290), `expand::expand`, `Reactive::rebuild` | A plot tick change: 1.3 ms with 1 plot, 7.3 ms with 10 (native); a values-only step is 0.13 ms |
| Any structural transaction does the same | `Store::run` (lib.rs ~2660), `after_transaction` (~2294) | 161.7 ms a structural edit at 1,000 meters (69,072 nodes) |
| Every transaction clones the document | `Store::run`, `let mut work = self.doc.clone()` | 2.2 ms at 14,407 nodes (instance-cache.md); grows with the document |
| Settling polls every repeat on every resolve, values-only steps included | `settle_repeats` reads 4–5 leaves per drawn repeat, compares with a `Debug`-text fallback | Not measured; grows with the number of repeats |
| Each instance stores a full copy of its definition's nodes | `want_entity`, `want_use`, `want_items` clone and rewrite every facet | 69 nodes a plot; 69,072 nodes for 1,000 meters |
| A fixed pipeline of whole-document passes runs after every change | `resolve_physics` (lib.rs ~1794): selection, settle, state, network, theme, layout, contexts | Not split out |
| Everything is evaluated whether or not anything shows it | Every binding, in every composition, hidden tabs included | — |
| Five steppers each keep their own time and scan their node type every frame | `advance_presence`, `advance_machines`, `advance_steps`, `advance_queries`, `clock::advance` (`document_bridge/mod.rs:332`) | — |
| Layout runs once, after structure settles | `resolve_physics` order | Structure cannot depend on a container's size |

### 1.2 Goals

| | Goal | Section |
|---|---|---|
| G1 | Structure costs what changed, for value-driven structure (repeats, conditionals) and op-driven structure (instances, definition edits) alike | §6, §7 |
| G2 | A transaction costs its ops, not the document | §11 |
| G3 | An instance is a reference to a shared template plus its differences, not a copy | §5.4 |
| G4 | Work that nothing observes is not done: hidden compositions, off-screen pictures, collapsed parts of the agent's view. What is observed stays correct | §8 |
| G5 | Structure may depend on layout, as CSS container queries allow | §9.2 |
| G6 | Time is an input the engine schedules, and a dimension a view can be taken along | §10 |
| G7 | "Not loaded" is an ordinary state, so partial loading and sync can be added without another rewrite | §13 |

### 1.3 Not in this spec

- Chunked storage, partial loading over the network, and rooms that hold part of a
  scope (iron-platform). §13 says what this engine provides for them and builds none.
- The renderer's share of a frame, and path data built as text and parsed back each
  frame (`graph::as_leaf_kind`); the handoff lists both as follow-ups.
- The constraint solver. Its clusters stay a transaction-time step (§9.4).
- Level of detail that changes *document structure* by zoom (§8.6 says why).

## 2. What stays

1. Every document mutation is an op in a transaction through `iron_document`.
2. Ops, facets, the registry, the file format (`serial.rs`), the formula language
   (`expr.rs`) and its static dependencies (`Expr::deps`). `deref` stays the one tracked
   read. Cycle refusal at write time, the look-only rule, the wires UI and render
   analysis keep reading static dependencies, observed or not.
3. Derived nodes keep **deterministic path ids** (`#22/1f/~5/1g`, `id.rs`). The same
   document and items give the same ids on every client and every load.
4. Overrides are authored `override` nodes on the outermost owner, keyed by path and
   item key, and survive their target leaving and returning.
5. The model and the picture stay separate: an entity's body computes, a `use`'s
   picture shows (component-model.md; reactivity.md §13's "template only" context rule).
   §8.4 depends on this.
6. Behaviour as specified in reactivity.md §13: keys, `when`, `let.keep`, `on`,
   `deref`, `@name`, traces, accumulators, queries, presence, drag and wheel actions.
7. Errors propagate; problems are data on nodes.
8. No backwards compatibility (AGENTS.md). No file-format change is needed: derived
   nodes were never saved.

**Changed from the first handoff's "fixed" constraints**, with the user's agreement
(2026-11-15): derived nodes are **addressable, and stored only where they differ**
(was: real document nodes); a full rebuild survives only as loading with everything
observed (was: a fallback path).

## 3. Vocabulary

"Scope" already means the unit of storage and history (scopes.md), and "context" means
behaviour rules (contexts.md) and `@name` reads. This spec avoids both for new things.

| Term | Is |
|---|---|
| **Owner** | What reactivity.md §3.1 calls a scope: something that owns derived structure and state. An instance's body, a picture, a repeat or conditional, one of its items |
| **Template** | The immutable, shared description of what an owner derives, compiled from a definition or a repeat's children |
| **Cell** | One value the engine holds: a binding, a published signal, a structure decision, a layout result, a clock |
| **Observer** | A declared reader: a view's visible region, an inspector, an agent's look, a narrative position, a pin |
| **Necessary** | A cell some observer needs, directly or through other cells. Only necessary cells are kept current |
| **Slice** | One assignment of clock values (and an override layer) at which values are read. The live slice is "now" |
| **Summary** | The few facts kept about an owner whatever its state: bound, counts, what it exports and reads, a hash |
| **Tree** | The read interface that answers for authored and derived nodes alike (§12.1) |

## 4. The shape

```
            observers (views, looks, inspectors, narratives, pins)
                 │ interest: owners, cells, a slice, a level
                 ▼
  ops ──► Document (authored nodes only) ──► templates (compiled, shared)
   │                                              │
   └──► Engine ◄──────────────────────────────────┘
          owner tree     one record per instance body, picture, repeat, item
          cells          dense arrays; heights; necessary counts; stamps
          scheduler      clocks and the next time each time-dependent cell changes
          stabilise()    one pass: values, structure, layout, in height order
                 │
                 ▼
        Tree (read)  +  Changes (what moved since last taken)
```

New module tree `iron_document::engine` (names are a proposal): `cell.rs` (arena,
heights, necessity), `eval.rs` (stabilise), `owner.rs`, `template.rs`, `structure.rs`
(reconcile), `observe.rs`, `clock.rs` (scheduler), `slice.rs`, `tree.rs`,
`producers/` (clips, layout, network, machines, steps, queries, presence).

## 5. Data

### 5.1 Cells

A cell exists for a leaf only when something needs one: it is bound, published,
derived from a relation, or read by another cell. A constant nobody reads has no cell.

```rust
struct Cell {
    leaf: Leaf,            // (node id, facet.field)
    owner: OwnerId,
    kind: CellKind,        // Input | Binding | Structure | Layout | Solve | Stepper | Clock
    deps: SmallVec<CellId>,    // static, plus tracked reads from the last evaluation
    readers: SmallVec<CellId>,
    height: u32,
    necessary: u32,        // observers and necessary readers that need it
    value: Slot,           // Good(Value) | Failed(String) | Pending(Why)
    changed_at: Stamp,     // when the value last changed
    computed_at: Stamp,    // when it was last brought up to date
    clocks: ClockSet,      // which clocks it depends on, from its deps (§10.5)
}
```

Cells live in dense arrays indexed by `CellId`, with a per-owner map from
(local node, leaf path) to cell. Today's graph is `BTreeMap<Leaf, BoundNode>` with a
map lookup per read and an `Expr` clone per evaluation (`graph.rs:468`).

### 5.2 What a leaf reads as

Highest first. Today's order is overlay, binding, signal, constant (`graph::read_with`);
overrides are baked into copied facets at expansion.

1. The gesture overlay (a preview).
2. A binding's value.
3. A producer's signal.
4. An override on an owner along the path, outermost first (component-model.md's rule).
5. The template's or the authored node's constant.

### 5.3 Owners

```rust
struct Owner {
    id: NodeId,                 // the instance, picture, repeat, or item scope `#r/~k`
    parent: Option<OwnerId>,
    kind: OwnerKind,            // Root | Body | Picture | Collection | Item
    template: Option<Arc<Template>>,
    state: OwnerState,          // Unloaded | Dormant | Live
    pins: u32,                  // selection, an outside reader, an open inspector
    children: Vec<OwnerId>,     // for a Collection, in key order, leaving items last
    keys: Vec<Key>,             // a Collection's items now
    cells: Vec<CellId>,
    diffs: Diffs,               // overrides and other per-node differences from the template
    session: BTreeMap<Leaf, Value>,   // `let.keep = session` cells
    presence: Option<Presence>, // an Item entering or leaving
    problems: Vec<Problem>,
    summary: Summary,
}

struct Summary {
    bound: Option<Rect>,        // last known, in the parent's space; or a declared size
    nodes: u32,                 // derived nodes it would hold if loaded
    exports: Vec<Leaf>,         // what something outside reads from it
    reads: Vec<Leaf>,           // what it reads from outside
    as_of: Stamp,               // when its values and problems were last current
}
```

| Kind | Made from | Template |
|---|---|---|
| Root | the document | none; authored nodes |
| Body | an entity linked to a definition | the definition's ports, equations, inner entities, connections, channels, machines, lets, traces, accumulators, queries (`expand::is_body`) |
| Picture | a `use` that figures a linked entity, or a main | the definition's template for that notation |
| Collection | a group with a `repeat` or `when` facet | its children that are not items |
| Item | one key of a Collection | the Collection's template |

An authored repeat's own children (the template as drawn in the document) are the
Collection's **prototype**: evaluated like any owner when something observes them (an
inspector, a look, a fit measurement, §9.2), not otherwise. Today they are evaluated
always, reading item 0.

### 5.4 Templates: instances as references

A template is compiled once per (definition, notation) or per Collection, and shared by
every owner made from it.

```rust
struct Template {
    version: Stamp,                    // bumps when its source is edited
    nodes: Vec<TNode>,                 // local id, parent, type, order, facets; parents first
    rels: Vec<TRel>,                   // relations among its nodes
    bindings: Vec<TBinding>,           // (node, leaf, expr, refs)
    children: Vec<(Local, ChildKind)>, // nested owners: inner entities, uses, repeats, conditionals
    hash: u64,
}

enum Ref {
    Inside(Local, LeafPath),   // another node of this template: this owner's own copy
    Item(ItemLeaf),            // value, index, presence, leaving: the enclosing Item's
    Arg(Name),                 // the instance's argument, or what feeds its in port
    Outside(Leaf),             // an authored leaf outside: read as it is
    Context(Name),             // `@name`: the nearest provider above this owner
}
```

- **A derived node is (owner, local node).** Its type, parent, order and facets are the
  template's. Its constants are the template's unless an override differs. Its bound
  leaves evaluate the template's expression with `Ref`s resolved against this owner, so
  no expression is cloned or rewritten per copy (today `map_refs` runs per copy,
  `expand.rs:426`).
- **Nothing is stored per derived node** except cells for its bindings (while the owner
  is Live or Dormant) and its entries in `diffs`.
- **Nested owners** come from `Template::children`: a repeat inside a picture is a
  Collection owner whose template is compiled with the picture's.
- **A definition edit** recompiles its templates. The compiler returns what changed
  (nodes added and removed, slots changed). Each Live or Dormant owner of that template
  applies the difference: new cells, disposed cells, readers of a changed slot marked
  dirty. Unloaded owners only take the new version. An index holds template → owners.
- **Relations** between derived nodes (a picture's `figures`, inner connections) are
  answered from `Template::rels`, relocated to the owner. Saved relations *to* a derived
  node (a connection to an instance's port) stay authored, as now.
- **Ids** keep `id.rs`'s interned paths. The path registry is process-wide and
  append-only with no eviction, which value-keyed items that come and go will grow
  without bound; decision D5 (§19).

Render-side precedent: instance-cache.md §7's references are already `use` nodes drawn,
picked and indexed through their template with nothing expanded (`render.rs:577`,
`picking.rs:161`, `scene.rs:519`). An Unloaded picture is drawn the same way (§8.5).

### 5.5 Overrides

Unchanged as authored data: `override` nodes under the outermost owner, `target` the
remaining path (`expand::translate`, `overrides`). The engine indexes them per owner by
target, as layer 4 of §5.2. Creating, changing or deleting one marks the target's cell
dirty; no re-expansion. Stale overrides (`expand::stale_overrides`) are found per owner
when its template's version changes.

## 6. Evaluation

Jane Street's Incremental is the model: heights, a recompute heap, necessity, and
structure as cells whose evaluation makes and disposes other cells. Dependencies stay
static, which Incremental's are not.

### 6.1 Necessity

`necessary` counts the observers that need a cell and its necessary readers. It is
maintained when an observer is added or dropped and when an edge is made or removed.

- A **necessary** cell is always current after `stabilise()`.
- An **unnecessary** cell is not touched by changes at all. When it becomes necessary it
  is brought up to date by pull: each dependency first, then itself if any dependency's
  `changed_at` is later than its `computed_at`.

So a change costs the necessary cells it reaches, and looking at something costs its
cone once.

### 6.2 One pass

```
stabilise():
  while let Some(cell) = heap.pop_lowest():
      match cell.kind:
          Binding   => evaluate; if the value or its state changed, push necessary readers
          Structure => reconcile the owner (§7); new necessary cells are pushed
          Layout    => lay out one container (§9.2); push readers of placements that moved
          ...
```

Everything a cell makes sits above it, so the pass never revisits a height. There are
no rounds: nested structure settles in the one pass, where `settle_repeats` loops once
per nesting level, re-expanding each time.

### 6.3 Heights

Set on insertion, adjusted locally: when an edge is added and the dependency's height is
not below the reader's, raise the reader and, recursively, its readers. Meeting the
starting cell during the raise is a cycle. Today `compute_heights` relaxes the whole
graph to a fixpoint (`graph.rs:405`), including from inside `evaluate` when a tracked
read finds a new dependency.

- A cell an owner makes is placed above that owner's Structure cell.
- A reader outside an Item of something inside it (`#g/~3/7`, a node list, `deref`) is
  raised above the Collection's Structure cell, and pins the Item (§7.4).
- Heights are never lowered while running. Loading computes them fresh.

### 6.4 Value states

| State | Readers | Drawn as |
|---|---|---|
| `Good(v)` | compute from it | `v` |
| `Failed(why)` | fail, "depends on …" naming the root (as now) | the last good value, marked stale |
| `Pending(why)` | are pending, naming what is awaited | the last good value, marked pending |

`Pending` is new. Its sources are a `query` awaiting an answer (today a `status` leaf
only) and, later, an authored chunk that is not loaded (§13). A derived owner that is
Unloaded is never a source: loading it is synchronous.

### 6.5 Kept as they are

- Equality cutoff, with NaN the same data as NaN (`value.rs`).
- Tracked reads: the leaves a `deref` read become dependencies until the next
  evaluation; one found late in a pass is evaluated first; one that reaches its own
  binding is `Failed("… a cycle")`.
- Type checks at a boundary leaf (`types::declared`), and `as_leaf_kind`.
- Determinism: cells at one height are taken in `Leaf` order.

## 7. Structure

### 7.1 Value-driven: Collections

A Collection has one Structure cell. Its dependencies are `each`, `count`, `key`, `max`,
`when.is`, `enter_ms`, `exit_ms`. It is necessary when the Collection is Live. When it
runs:

1. Compute elements and keys exactly as `settle_repeats` and `item_keys` do now
   (node references, record ids, distinct words, else the index; duplicates fall back to
   index keys with a problem; `max` truncates with a problem).
2. Diff against `keys`: kept, added, removed.
3. **Removed:** if `exit_ms` is over 0 the Item becomes leaving and stays, after the
   wanted ones, with the element and index it had; otherwise dispose it (§7.3).
4. **Added:** make an Item owner. Its `item.value` and `item.index` are Input cells the
   Collection writes. Make cells for the template's bindings that are necessary; make
   its nested owners. With `enter_ms`, start its presence at 0.
5. **Kept:** write `item.value` and `item.index` where they changed. Readers follow by
   the ordinary pass.

Nothing outside the Collection is visited.

### 7.2 Op-driven: instances and definitions

A transaction's ops reach the engine one at a time (§11). Each maps to the owners and
cells it affects:

| Op | Affects |
|---|---|
| Create, delete, reparent an authored node outside a definition | Its own cells; its owner if it is an entity, a `use`, or holds a repeat or `when`; context readers below a moved or removed provider |
| Set a constant | That leaf's cell, if it has one |
| Set a formula, or clear one | That cell: its dependencies, height and readers |
| Set on `instance`, `use` (def, mode, notation), link or unlink `figures` | That Body or Picture owner: made, disposed, or given another template |
| Any op inside a definition | Its templates recompile; their owners apply the difference (§5.4) |
| Create, change or delete an override | The target's cell |
| Set `let.provide`, a provider's name; move a provider | Readers of that `@name` below the old and new place (index: name → readers) |
| Link or unlink a connection or an anchor's relation | The derived cells at its ends (today `port::derived_signals` and `anchor::derived` are recomputed for the whole document on every rebuild) |
| Add or remove a `repeat` or `when` facet | A Collection owner made or disposed |

### 7.3 Disposal

Deepest first. An owner's cells are removed from their dependencies' reader lists; a
leaf that stops being bound reads as its constant again and is reported changed (today
`rebuild`'s `old_bound` loop). Session cells are dropped (they reset, reactivity.md
§12.1); overrides are kept. **Unloading is not disposal:** an Unloaded owner keeps its
`session` values and `diffs` (decision D8).

### 7.4 Readers from outside

An index holds owner → cells outside it that read into it. Such a reader pins the owner
(it cannot be Unloaded) and keeps what it reads necessary while it is itself necessary.
When the owner is disposed, the reader's dependency no longer exists and the reader is
`Failed`, as a read of a missing leaf fails now.

### 7.5 Problems and limits

Problems are held per owner; `Store::problems()` is the union, maintained as owners
change. A Dormant or Unloaded owner's problems are those of `summary.as_of`
(decision D3).

- Nesting past 16 owners is a problem and still expands; 128 is refused.
- The node budget (50,000) counts derived nodes of **loaded** owners.
- Counts that keep changing each other cannot occur within a pass (a Collection's
  inputs may not read its own items, refused at write time). Feedback through layout is
  bounded in §9.2.

## 8. Observation and owner states

### 8.1 States

| State | Template | Cells | Evaluated | Stored |
|---|---|---|---|---|
| **Unloaded** | referenced | none | no | `summary`, `diffs`, `session` |
| **Dormant** | referenced | exist | no; values as of `summary.as_of` | the same, and cells |
| **Live** | referenced | exist | necessary ones, every pass | the same |

An owner is at least as awake as its most awake child, and a child is never more awake
than an Unloaded parent. Loading a derived owner is synchronous: instantiate cells from
the template, apply `diffs`.

### 8.2 Observers

```rust
enum Interest {
    Region { composition: NodeId, rect: Rect, zoom: f64 },  // a 2D view; a 3D view gives a frustum
    Nodes(Vec<NodeId>),          // an inspector, a selection, the layers panel's open rows
    Look(ViewQuery),             // the agent's XML view: root, depth, expand, fidelity
    Pin(NodeId),
}
fn observe(&mut self, interest: Interest, slice: SliceId) -> ObserverId;
fn reobserve(&mut self, id: ObserverId, interest: Interest);
fn unobserve(&mut self, id: ObserverId);
```

- A **Region** makes Live the picture owners whose `summary.bound` meets the rectangle
  grown by a margin, found through the spatial index's entry for each owner (§12.3).
  Leaving by more than a second, larger margin makes them Dormant; Unloading is a
  separate, slower policy (memory pressure, or a count of Dormant owners).
- A **Look** is short-lived: it makes necessary exactly the leaves it prints. Skeleton
  fidelity is answered from summaries and loads nothing. `expand="#x"` loads `#x`.
- A **narrative** is an observer that also knows what comes next: it may observe the
  next position's interest ahead of time so that it is loaded and current on arrival.
- With no observer on a composition, all its pictures are Dormant.

**Everything-observed** (`Store::observe_all`) makes every owner Live and every cell
necessary. That is today's behaviour, and the mode the differential test runs in (§16).

### 8.3 What stays necessary without an observer

Necessity flows backwards from what is observed, so most of this is automatic: a visible
readout that reads an off-screen machine's port keeps that port's whole upstream chain
current, and nothing else of the machine. Beyond that, these are always necessary:

- A stepper with memory whose policy is `run` (§10.4).
- An `on` whose trigger is a condition (reactivity.md §5.2; not built yet).
- What a solver cluster or the network solve reads, while the cluster or island is
  necessary.

### 8.4 Bodies and pictures

A Body owner is never Unloaded while its entity exists: it is small, and saved
connections attach to its ports. Its cells are evaluated only when necessary. Pictures
are what go Dormant and Unloaded. One entity pictured in two compositions computes once;
each picture wakes with its own view.

### 8.5 Drawing what is not Live

- **Dormant:** drawn from its last values. The render cache's tiles and shared drawings
  are already keyed by content and stay valid.
- **Unloaded:** drawn as a reference to its template through the owner's placement,
  with `diffs` applied; if `diffs` cannot be applied without loading, it is loaded when
  it comes into view instead.
- **Bound:** `summary.bound` is the last computed bound, or the picture's declared size
  (`use` with a `size` facet). A Dormant picture whose parts would have moved outside
  that bound is not noticed until it wakes; the wake margin covers small motion.
  Decision D2 offers an owner-level opt-out.

### 8.6 Levels

A view asks for a level by an owner's size on screen: **full**, **reduced rate**
(sampled every nth frame, §10.3), **frozen** (Dormant) and **bound only** (Unloaded).
Phase 3 builds full and frozen. Levels change how often and how much is computed, never
which nodes the document has: two views can show one composition at different zooms, and
structure is shared by both.

### 8.7 Waking without a hitch

- A per-frame budget (a time, default 4 ms) for bringing owners up to date; the rest
  continue next frame, nearest the view centre first, drawn frozen meanwhile.
- An **idle sweep** brings Dormant owners up to date a few per idle frame, so problems
  and bounds do not go stale indefinitely.

### 8.8 The consistency property

For any sequence of edits, publishes and clock moves, and any way of observing during
it: **after observing everything, every value equals what observing everything
throughout would have given**, except values downstream of a stepper that was paused
by policy. §16.3 tests this directly.

## 9. The pipeline becomes cells

`resolve_physics` and the per-frame steppers become cells and scheduler entries, ordered
by dependency. Host producers (`register_producer`, `publish`, `retract`) keep their
interface: publishing writes Input cells.

### 9.1 What each becomes

| Today | Whole-document today | Becomes |
|---|---|---|
| `settle_repeats` | reads every repeat | A Structure cell per Collection (§7.1) |
| `publish_selection`, `publish_state`, interaction, events, theme | per call | Input cells written by the host or an action |
| `resample` (clips, `time.rs`) | all clips, any change | One cell per animated leaf, reading the clock and its clip's keys (§10) |
| `solve_network` | `port::network(doc)`, one Newton solve | A Solve cell per connected island of connections |
| `resolve_layout` | `layout::compute` over all groups and tables | A Layout cell per container (§9.2) |
| `port::derived_signals`, `anchor::derived` | all connections and anchors, each rebuild | Cells made and removed with their relation (§7.2) |
| `advance_machines`, `advance_steps`, `advance_queries`, `advance_presence`, `clock::advance` | each scans its node type each frame | Stepper cells on the scheduler (§10) |
| `resolve_contexts` (behaviour rules) | when markup changed | Unchanged; not reactive values |
| `rebuild_clusters`, `post_apply` (constraint solver) | all constraints | Unchanged in this spec (§9.4) |

### 9.2 Layout, and structure that depends on it

CSS allows a descendant's style to depend on a container's size only when the
container's size does not depend on its contents (containment); Flutter's
`LayoutBuilder` and Compose's `SubcomposeLayout` are the same rule. That makes the
dependency one-way, so it resolves top-down. Three mechanisms, in order of preference:

1. **Fitting is layout's job.** Wrapping, and showing only the children that fit, are
   layout modes that publish a `fits` flag per child. No structure changes.
2. **Containment.** A Layout cell covers one container's subtree down to the next
   contained box. A container whose size is fixed or comes from its parent is
   *contained*; a Collection inside it may read its size (`count="{= floor(#box.size.w
   / 80)}"`). The order falls out of heights: size the container, settle the structure
   inside, lay out inside.
3. **Measuring the prototype.** "As many as fit" reads the size of the Collection's
   prototype (§5.3), which exists without any item.

True feedback (a count that reads its own items' laid-out size) is refused at write
time where it is static, and where it arises through a tracked read it is cut after one
extra pass per frame, with a "did not settle" problem and the rest deferred to the next
frame, as `ResizeObserver` does (decision D6).

### 9.3 Machines

A machine's state is an argument (document data). A running transition is a function of
its start time and the clock, so it is a clock-reading cell, not an integrator
(§10.4). `machine.at` and `machine.running` stay as leaves.

### 9.4 The constraint solver

Out of scope. `post_apply` validates and solves at commit and during gestures
(`resolve_clusters_live`), reading and writing through the engine as it does now. Its
whole-document `rebuild_clusters` after every transaction is the next whole-document
cost once this lands; its clusters should later be indexed by owner.

## 10. Time

### 10.1 Clocks

A clock is a cell. The **root clock** is presentation time, which the host supplies.
Any other clock is a parent clock through a mapping: offset, rate, paused, loop. The
session `clock` node (`clock.t`, `playing`, `rate`, loop bounds; timeline.md §4.1) is a
child of the root. Scrubbing writes it; playing lets the mapping run.

A formula reads time through the **nearest clock above its owner**, resolved the way
`@name` is. An owner provides one with a `clock` facet. With no providers every reader
gets the document's clock, which is today's behaviour, and gives timeline.md's "a
content binding on the clock needs a stable name" its answer. Uses: the narrative clock
(position, not seconds), a preview at another time, a component in slow motion, and
reduced update rate for something small on screen (a coarser mapping).

### 10.2 One entry point

```rust
fn advance(&mut self, now: f64) -> Wake;
struct Wake { next: Option<f64>, live: BTreeSet<NodeId> }   // compositions that must redraw
```

`advance` replaces the five `advance_*` calls and the host's clock player. `Wake`
replaces each producer's bookkeeping in `LiveSources` (`liveness.rs`): a composition is
live while a necessary cell that changes continuously is drawn in it.

### 10.3 The scheduler

Each time-dependent cell says when it next changes:

| Cell | Next change |
|---|---|
| An animated leaf between keys | Continuous inside the clip's span; then never, until a key or the clock is edited |
| A machine's tween | Continuous until its end |
| An item's presence | Continuous until it reaches 1, or 0 (then the Item is disposed in that pass) |
| A `trace`'s `out` | At the arrival of the next recorded change |
| A `trace`'s `dash` (the picture of what is in flight) | Continuous while any change is in flight |
| A query's debounce and timeout | At that time |
| An `accumulate` | Continuous while its `next` differs from its value |

A queue per clock holds the timed entries. Each `advance` pops what is due and marks it
dirty; Continuous cells are evaluated only if necessary. Frame cost follows what is
moving *and* observed. Timed events fire whether or not their source is on screen,
which is what delivers an off-screen machine's pulse to a visible meter.

### 10.4 Memory

| Class | Examples | Unobserved | At another time (§10.5) |
|---|---|---|---|
| **Pure** in its clocks | clips, tweens, presence, `ease` of a clock | skipped; computed when read | valid |
| **Logged** | `trace` (change events with times) | events still recorded while what it follows is necessary | valid within its window |
| **Integrating** | `accumulate`, a host simulation | by policy: `run` (default), `pause`, or `coarse` (larger steps) | not valid unless recorded |

A stepper with policy `run` is always necessary, with everything it reads. That is the
cost of keeping it right; a closed form is preferred where one exists (the laser's pulse
phase is `frac(t · hz)`, not an accumulator). Decision D1.

### 10.5 Slices: time as a dimension

A value is a function of the document, the clocks and the free inputs. A **slice** fixes
the clocks (and may carry an override layer). The live slice is now. Another slice is
equally valid for every Pure and Logged value, which is the notes' "seekable"
(narrative note §6.7), and its "trial evaluation at the end position" is reading a
value in another slice.

- Each cell's `clocks` is the union of its dependencies'. A value with no clock
  dependency is the same in every slice and is stored once.
- A slice stores values only for cells that depend on a clock it sets differently.
  Everything else reads through to the live slice.
- An Integrating cell read in a slice other than the live one is `Failed("not
  seekable: …")`, naming the stepper, unless a recording covers that time.
- An observer names its slice (§8.2), so two views may show one composition at two
  times, paying for the time-dependent part twice and nothing else.
- The gesture overlay is the same idea (values read in place of the document's while a
  drag runs) and may later become an override layer on the live slice. It stays as it
  is until phase 6.

## 11. Transactions

`Store::run` no longer clones the document.

1. Translate edits aimed at derived nodes (`expand::translate`: a source's `Set`, an
   override, or a refusal), reading templates through the Tree.
2. Apply each op to the document in place, collecting inverses. On an error, keep
   going to report every error, as now.
3. Tell the engine each op (§7.2) and `stabilise()` structure, so the checks that read
   derived structure see it (`check_created` for a connection to a derived port,
   `check_scope`, `check_context`).
4. If anything failed: apply the inverses in reverse, tell the engine each, and
   `stabilise()`. The engine's state is a function of the document and its inputs, so it
   returns to where it was at a cost proportional to the change.
5. Otherwise `post_apply` (the solver), and on its failure the same rollback (today:
   put the old document back and rebuild the graph, `lib.rs:2423`).

Consequences:

- `Transaction::expanded` goes away. It is never replayed today; undo re-derives.
  `ops`, `derived_ops` (solver writes) and `inverse` stay.
- `attachment::validate(&self.doc, &work)` compares the documents before and after and
  must instead work from the ops and their inverses.
- Undo, redo, replay and ops arriving from a server take the same path. A rejected
  push rolls back by step 4.
- A gesture's previews are unchanged: overlay values, committed as one transaction.

## 12. The host contract

The host has no op-by-op path today: `sync_document` turns ops and resolved changes into
a set of node ids and rebuilds those by reading the store (`document_bridge/mod.rs:520`,
`collect_ids`, `materialize::reconcile`). It does not treat derived ids differently. So
the contract is small.

### 12.1 The Tree

`Store::tree()` answers for any live node id, authored or derived, with `Document`'s
read methods under the same names: `live`, `node`, `parent`, `children`, `depth`,
`facet`, `facets_of`, `get::<T>`, `leaf`, `relations_from`, `relations_to`,
`nodes_of_type`. For a derived node it answers from the owner's template and `diffs`,
loading the owner if needed. `Store::document()` remains and holds authored nodes only.

Callers that can meet a derived id move to `tree()` (about 230 uses of `document()`
outside the crate; most are mechanical). Enumerations (`nodes_of_type`, a full preorder)
on the Tree visit **loaded** owners only; a caller that needs everything says so, and
is a place to review.

### 12.2 Changes

```rust
fn take_changes(&mut self) -> Changes;
struct Changes {
    nodes: BTreeSet<NodeId>,       // a resolved value, a facet or a relation of theirs moved
    structure: bool,               // something was made, removed or reordered
    loaded: Vec<NodeId>,           // owners that came in
    unloaded: Vec<NodeId>,         // owners that went
}
```

It replaces `take_derived_ops`, `drain_changes`, `has_changes`, `take_touched` and
`take_geometry_changes`, and the chaining of `tx.expanded` into `collect_ids`.
`geom::Revisions` is fed from it.

### 12.3 What each consumer does

| Consumer | Today | With owners |
|---|---|---|
| `materialize::reconcile` | per id, reads the store | The same, through the Tree. An unloaded owner removes its nodes' drawables |
| Spatial scene (`iron_spatial`) | full preorder on any structure change; copies indexed as leaves | One entry per owner with its bound, as references are (`Embed2d`); an owner's contents indexed while it is loaded; rebuilt per owner |
| Drawing (`render.rs`, `chunks.rs`) | walks `doc.children`; instance runs from copies | The Tree for loaded owners; the reference path for Unloaded ones (§8.5) |
| Picking | spatial candidates, then a chain | A hit on an Unloaded owner's entry loads it, then picks inside |
| Selection (`Reach` chains ending in derived ids) | ids | Holding a derived id pins its owner |
| Render analysis (`analysis.rs`) | per instance, per call | Per template and set of wired inputs, cached by template version |
| `document_view` (`<look>`) | reads everything | A Look observer (§8.2) |
| `document_inspect`, `document_component`, `document_computed` | read derived nodes | A Nodes observer while the inspector shows them |
| Layers panel, `narrative_targets` | exclude derived ids | Unchanged |
| `paint_order` | walks the whole tree on any structure change | Per owner; a follow-up for the render track |

## 13. Ready for partial loading and sync (not built here)

What scopes.md §8 asks of "later designs", and what this engine supplies:

| Need | Supplied by |
|---|---|
| "Not loaded" is ordinary | `Pending` (§6.4); owner states (§8.1); the Tree can answer "not loaded" for an authored chunk as it answers from a template for a derived one |
| A loading unit smaller than a scope | The owner, and for authored content a subtree with a stored `Summary` |
| Knowing what a visible part needs | `summary.reads` and `exports`, computable from static dependencies without evaluating |
| Ops that apply to a partly loaded store | An op on something not loaded updates its summary's stamp; nothing else is derived until it loads |
| Remote edits that cost what they touch | §7.2 and §11 |
| Derived structure agrees between clients | It is never synced; ids are computed |

The platform's rooms each hold a whole `Store` (iron-platform README). A room over a
partly loaded scope, summaries as nodes of a hash tree in storage, and fetching by
interest are that repository's work.

## 14. Answers to the first handoff's questions

| # | Question | Answer |
|---|---|---|
| 1 | Expanding one scope exactly as a full expansion would | Expansion is one function per owner over a compiled template; expanding the document is that function from the roots (§5.4, §7) |
| 2 | Problems per scope | Held per owner (§7.5) |
| 3 | The template's own bindings | The prototype, evaluated only when observed (§5.3) |
| 4 | Index or value keys for plot ticks | Decision D7 |
| 5 | Edges across a scope boundary | Inward readers indexed per owner; they pin it (§7.4) |
| 6 | Heights | Local adjustment on insert; one heap (§6.3) |
| 7 | Context reads | Resolved at cell creation; an index name → readers (§7.2) |
| 8 | Derived signals | Per relation (§7.2) |
| 9 | Removing a binding | Per cell on disposal (§7.3) |
| 10 | When to fall back to a full rebuild | Never; loading is the same path over everything |
| 11 | Transactions on a working copy | In place, with rollback (§11) |
| 12 | Presence | A scheduled cell on the Item; disposal in the pass where it reaches 0 (§10.3) |
| 13 | Session lets | Dropped on disposal, kept on unload (§7.3) |
| 14 | Render analysis and sharing | Per template (§12.3); `Changes` names what moved (§12.2) |
| 15, 16 | Structure inside the graph | Yes (§6.2). No ops are emitted for derived structure at all |

## 15. Targets

Proposed; each is measured before and after with the probes in the handoff's §4.

| Measure | Today | Target |
|---|---|---|
| Pan or zoom step that changes the tick count, native, 1 and 10 plots | 1.3 / 7.3 ms | Within 2× the values-only step at both sizes, so about 0.3 ms |
| Values-only pan step, native | 0.13 ms | No slower |
| `action` p95 in `plot-perf.txt` (wasm-bench): slow pan, wheel zoom | 6.7, 6.4 ms | Under 1.5 ms |
| Structural edit at 1,000 placed meters (`instance_cost`, `bench-instances.txt`) | 161.7 ms | Under 2 ms |
| Placing one more meter at 1,000 | 79.9 ms | Under 1 ms |
| Memory per placed meter beyond the first | to be measured in phase 0 | Under a tenth of today's |
| Evaluations per frame for a composition in a hidden tab | all its bindings on any change | 0 |
| Loading the demo document | to be measured in phase 0 | No slower |

## 16. Verification

### 16.1 The old engine is the reference

Both engines are compiled into `iron_document` until the swap. A differential harness
drives one script through both and compares after every step, with the new engine in
everything-observed mode:

- the set of live node ids, authored and derived;
- each node's type, parent, order, and facets (constants; bound expressions after
  relocation);
- relations;
- the resolved value and error of every leaf;
- problems, as (node, kind);
- what a `ViewQuery` prints, at full fidelity.

Not compared: ops emitted for derived structure, evaluation counts, heights.

Scripts: every existing test's sequence of calls, and a generator of random ones
(transactions, previews, publishes, clock moves, undo and redo) over documents with
repeats in instances and instances in repeats, conditionals toggling, keys changing
kind, overrides on items that leave and return, enter and exit in flight, context
providers moved, connections to instance ports, definition edits.

### 16.2 The existing suite

`cargo test -p iron_document` (316 tests in 49 files on 2026-11-15) runs against the new
engine. Tests that read engine internals are rewritten to the public surface or to
counters the new engine keeps for tests:

- `reactive.evaluations`, `height`, `transitive_deps` (`tests/reactive.rs`,
  `tests/reactivity.rs`);
- `take_derived_ops` and counts of ops (`tests/reactivity.rs`, `tests/plot_perf.rs`,
  `tests/constraints.rs`, `tests/gestures.rs`, `tests/handles.rs`, `tests/signals.rs`);
- `expand::` helpers (`tests/compose.rs`, `authoring`, `machines`, `repeat`, `standard`).

Then `bun test`, the fast smoke, and `plot-view.txt`, `actions.txt`, `query.txt`,
`fiber.txt`, `pulses.txt` (run-iron skill).

### 16.3 What has no reference

Dormancy, unloading, the scheduler and slices are new behaviour. They are tested against
the engine itself by §8.8's property: a random script run while observing a random,
changing subset, then observing everything, must equal the same script observed in full
throughout. A stepper with policy `pause` is excluded by construction.

## 17. Phases

Sequence and the current task are in the handoff. Each phase ends committed, pushed,
measured and recorded in the roadmap's Status table.

| Phase | Builds | Exit |
|---|---|---|
| **0. Baselines and the harness** | Memory and load-time baselines (§15). The differential harness, run old against old. The script generator | The harness is green on old-vs-old across the existing suites' scripts and 10,000 random ones |
| **1. The host contract, on the old engine** | `Store::tree()` and `take_changes()` (§12) implemented over today's document and graph. Every outside caller that can meet a derived id uses them. `attachment::validate` works from ops | No behaviour change: smoke output identical; `take_derived_ops`, `drain_changes`, `take_touched` have no callers outside the crate |
| **2. The core, with everything observed** | Cells, owners, templates as references, one-pass structure, transactions in place (§5–§7, §11). The pipeline's producers still run in today's order and write Input cells. Then the swap: the old `graph.rs`, `expand::expand`, `settle_repeats` are deleted | The differential test and the suite are green; §15's plot and meter targets are met; the roadmap row "Plot pan and zoom performance" is closed |
| **3. Observation** | Observers, Dormant and Unloaded pictures, owner entries in the spatial index, drawing Unloaded owners as references, the wake budget, the idle sweep (§8, §12.3) | §8.8's property test is green; a hidden tab's composition costs 0 evaluations; panning away from 1,000 animated meters drops store time per frame to that of what is in view, measured |
| **4. Time** | The clock tree and nearest-clock reads, the scheduler, `advance` and `Wake`; the steppers as scheduled cells; memory classes and policies (§10.1–§10.4) | `fiber.txt` and `pulses.txt` pass with the laser off screen and the meter on; an idle document with a trace and an accumulator at rest schedules no wake |
| **5. The pipeline as cells** | Layout cells with containment and `fits`; the network solve per island; clips as cells (§9) | A repeat whose count reads its container's width adds items during a resize drag; `layout`, `table`, `ports`, `schematic` suites green; a layout change in one container lays out only that container |
| **6. Slices** | §10.5: per-slice values for clock-dependent cells, an observer's slice, "not seekable" errors; timeline.md phase 5's seekable endpoints use it | Two views of one composition at two clock times, both correct; a clip end bound to a clock-derived value is accepted and one bound to an accumulator is refused with the reason |

Phases 3, 4 and 5 do not depend on one another and may be reordered. Chunked storage
(§13) is separate work after 3.

## 18. Risks

| Risk | Mitigation |
|---|---|
| A caller still assumes a derived node is in `Document` | Phase 1 moves every caller onto the Tree while the old engine still stores copies, so a miss shows as a compile error or a failing smoke before the engine changes |
| The new engine differs subtly from the old in a case no test covers | The random differential test (§16.1); the swap waits on it |
| Reading a derived node's facets through the Tree is slower than reading a stored copy | Cells hold resolved values, which is what drawing reads; relocated facets are cached per loaded owner on first read; measured in phase 2 |
| A Dormant picture's bound is stale and it fails to wake | The wake margin, the idle sweep, the declared size; decision D2 |
| Waking a large region stalls a frame | The per-frame budget and frozen drawing meanwhile (§8.7) |
| The interned path registry grows without bound | Decision D5; measured in phase 0 on a script that churns value keys |
| Phase 2 is large and lands as one swap | Phases 0 and 1 land first and are useful alone; within phase 2 the new engine runs under the differential test from its first commit |

## 19. Decisions for the user

Each has a default the spec assumes until told otherwise.

| # | Decision | Default |
|---|---|---|
| D1 | What an unobserved integrator does | `run`: stay necessary and correct. An author may set `pause` or `coarse` per node |
| D2 | A Dormant picture's bound | Last known, or its declared size. Later, an owner may ask to keep its placement and shape live while its look sleeps (render analysis already separates those channels) |
| D3 | Problems in owners that are not Live | As of their last evaluation, with the idle sweep refreshing them. Not guaranteed current |
| D4 | The words "owner" and "slice" | As in §3; reactivity.md's "scope" is renamed where the two specs meet |
| D5 | Reclaiming interned id paths | Measure in phase 0. If growth matters, count references per path from owners and free at zero |
| D6 | Feedback from measured layout into structure | One extra pass per frame, then a problem and the rest next frame |
| D7 | `std:plot` tick labels keyed by value (labels slide and fade) | Yes, after phase 2 makes it cheap; plot.md and reactivity.md §13 change with it |
| D8 | Session lets when an owner is unloaded | Kept (unloading is not disposal) |
| D9 | The node budget | 50,000 loaded derived nodes; unloaded owners do not count |
