# The reactive engine: owners, demand, templates and time

**Status:** Proposed 2026-11-15; **revised the same day after a council review**
(Claude Opus 5.5 and GPT-6.1 Sol, both "rework"; §20 records each finding and what
changed). **Nothing here is built; it waits on the user's approval.** Work order and
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

Two steps. Today overrides are baked into the copied facets as constants
(`expand::apply_overrides`), and the read order is overlay, binding, signal, constant
(`graph::read_with`). Both are kept.

**The effective slot** of a derived leaf is the override's constant if an override
along the owner path targets it (outermost wins, component-model.md §5.3), else the
template's slot. An override **replaces** the slot: an overridden leaf whose definition
later gains a formula stays the override's constant, and an override may add a facet the
template node lacks, where the type admits it.

**The value**, highest first:

1. The gesture overlay (a preview).
2. The binding's value, if the effective slot is bound.
3. A producer's signal.
4. The effective slot's constant.

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

- **A derived node is (owner, local node).** Its parent and order are the template's.
  Its effective slots are §5.2's. Its bound leaves evaluate the template's expression
  with `Ref`s resolved against this owner, so the engine clones and rewrites no
  expression per copy (today `map_refs` runs per copy, `expand.rs:426`).
- **What the compiler must reproduce from today's expansion** (`want_entity`,
  `want_use`, `rewritten`):
  - a body's `port_spec` becomes a `port`; each `in` port's `signal` is a synthesized
    binding to the instance's argument (`Ref::Arg`);
  - node references held as constants (`Value::Ref`, an anchor's `port`) are
    relocated per owner, like references in formulas;
  - a picture's references into the definition's body go to the entity's body owner,
    or stay on the definition for a main; in a sized `use`, references to the template
    root go to the `use`. So the key is **(definition, notation, sized, main)**;
  - a machine's states, transitions and tweens are copied into the body.
- **Nested owners are found from effective slots, not fixed by the template.** An
  override may change an inner entity's `instance.def` or an inner `use`'s notation
  (`expand.rs:487`), so which template a child owner gets is read from the child node
  after overrides; such an override is a structure change to that child (§7.2).
- **The engine stores nothing per derived node** except cells for its bindings and its
  entries in `diffs`. **The Tree** (§12.1) materialises a derived node's facets, with
  references relocated and overrides applied, the first time something reads them, and
  keeps them per loaded owner until it unloads or its template's version changes. So
  readers keep getting whole facets by reference, and only what is read is built.
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
target. Creating, changing or deleting one changes the target's effective slot (§5.2):
its binding cell is removed or restored, readers are marked dirty, and if the leaf is
one that chooses a nested owner's template, that owner is reconciled. No re-expansion.
Stale overrides (`expand::stale_overrides`) are found per owner when its template's
version changes.

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

Everything a cell makes sits above it, so structure settles in the one pass, where
`settle_repeats` loops once per nesting level, re-expanding each time. A cell runs
again within a pass in one case, as now (`graph.rs:530`): a tracked read, or a read
into an item that has just come to exist, finds a dependency that has not run yet. The
dependency is pulled first (which may reconcile a Collection that was not necessary),
the reader's height is raised and it is queued again. A budget bounds this, as now.

### 6.3 Heights

Set on insertion, adjusted locally: when an edge is added and the dependency's height is
not below the reader's, raise the reader and, recursively, its readers. Meeting the
starting cell during the raise is a cycle. Today `compute_heights` relaxes the whole
graph to a fixpoint (`graph.rs:405`), including from inside `evaluate` when a tracked
read finds a new dependency.

- A cell an owner makes is placed above that owner's Structure cell.
- A reader outside an Item of something inside it (`#g/~3/7`, a node list, `deref`) is
  raised above the Collection's Structure cell, and pins the Item (§7.4).
- A Structure cell is a dependency of every cell its owner makes, in the height graph.
  So two Collections whose inputs read each other's items are found as a cycle when the
  second edge is made, and both get a problem (today the loop is cut after 129 rounds,
  `lib.rs:1389`).
- Heights are never lowered while running, so they can drift upward; loading computes
  them fresh, and a compaction pass may when the maximum passes a limit.

### 6.4 Value states

| State | Readers | Drawn as |
|---|---|---|
| `Good(v)` | compute from it | `v` |
| `Failed(why)` | fail, "depends on …" naming the root (as now) | the last good value, marked stale |
| `Pending(why)` | are pending, naming what is awaited | the last good value, marked pending |

`Pending` is new and **has no source in this spec's phases**: it is for an authored
chunk that is not loaded (§13). A `query` keeps today's behaviour: a `status` leaf, and
`value` holding the initial or last good answer (`query.rs:103`). A derived owner that
is Unloaded is never a source: loading it is synchronous.

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
`Failed`, as a read of a missing leaf fails now. A reader of an item that does not
exist yet (`#g/~3/7` before key 3) is held in an index by the id it waits for, and is
linked and re-run when that item is made.

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
current, and nothing else of the machine. Beyond that, the rule is **the model keeps its
history; the picture may lose its own.**

Always necessary, whatever is observed:

- **Lifecycle of anything that holds model state.** A Collection or conditional whose
  template holds a session `let`, a `trace`, an `accumulate`, a `machine` or a `query`
  (known from the template) keeps its Structure cell necessary, so its items are made
  and disposed when they would have been, and their state reset when it would have
  been. A Collection of pure pictures (the plot's tick labels) does not.
- **Events that start something.** A machine's state argument changing, and the value a
  `trace` follows changing, are recorded with their time when they happen (§10.4). The
  tween or the picture of the pulse that follows is computed only when observed.
- **An integrating stepper** whose policy is `run` (§10.4), with what it reads.
- **An `on` whose trigger is a condition** (reactivity.md §5.2; not built yet).
- **What bounds a loaded picture**, when that can move (§8.5).
- **Inputs to the constraint solver and the network solve**, when they run (§8.9).

Presentation state in a picture is not kept: an item's presence (enter and exit), a
layout glide, a tween inside a picture. On waking these **snap to where they would have
settled**; nothing replays what was not seen.

### 8.4 Bodies and pictures

A Body owner is never Unloaded while its entity exists: it is small, and saved
connections attach to its ports. Its cells are evaluated only when necessary. Pictures
are what go Dormant and Unloaded. One entity pictured in two compositions computes once;
each picture wakes with its own view.

### 8.5 Drawing what is not Live

- **Dormant:** drawn from its last values. The render cache's tiles and shared drawings
  are already keyed by content and stay valid.
- **Unloaded:** drawn from what the render cache kept for it (tiles, or its last
  display list), keyed by owner. Only an owner with no arguments, overrides or item
  value, so identical to its template, may be drawn as a reference
  (`render.rs:573`); a reference draws the template's own values, which is wrong for
  anything else. An Unloaded owner with nothing cached is loaded when it comes into
  view.
- **Bounds stay right.** A view must never miss something that has moved into it. The
  cells that decide a loaded picture's bound (its placement, and its shape where that
  can change) stay necessary while the picture is Dormant if they depend on anything
  that can change without an edit: a clock, a signal, a stepper. Render analysis
  already classes a part's placement and shape apart from its look (`analysis.rs`). A
  picture whose bound is static costs nothing. A picture is Unloaded only if its bound
  is static, or it declares a size that clips it.

### 8.6 Levels

A view asks for a level by an owner's size on screen: **full**, **reduced rate**
(sampled every nth frame, §10.3), **frozen** (Dormant) and **bound only** (Unloaded).
Phase 4 builds full and frozen. Levels change how often and how much is computed, never
which nodes the document has: two views can show one composition at different zooms, and
structure is shared by both.

### 8.7 Waking without a hitch

- A per-frame budget (a time, default 4 ms) for bringing owners up to date; the rest
  continue next frame, nearest the view centre first, drawn frozen meanwhile.
- An **idle sweep** brings Dormant owners up to date a few per idle frame, so problems
  and bounds do not go stale indefinitely.

### 8.8 The consistency property

For any sequence of edits, publishes and clock moves, and any way of observing during
it: **after observing everything, every value that is a function of the document, the
clocks, the recorded events and the running steppers equals what observing everything
throughout would have given.** Outside the property, by design:

- presentation state in pictures, which snaps (§8.3);
- a stepper an author set to `pause` or `coarse`;
- answers from outside (a `query`'s request times and replies);
- the network solve where more than one solution exists, since it starts from its last
  answer (`lib.rs:2146`); §9.1 keeps its inputs and warm start per island.

§16.3 tests the property directly. Whether a transaction is accepted never depends on
what is observed (§11).

### 8.9 Reading

A read through `&Store` never loads or evaluates. Reading a cell that is not current
returns its last value and says so (`as_of`); in a debug build it is an error unless
the caller asked for stale values.

Anything that writes what it reads into the document or the overlay first makes what it
reads current, through `&mut Store`: `expand::translate` and handles (inverting a value
onto its source), `post_apply` and `resolve_clusters_live` (the solver), `fire` (an
action's parallel reads). Views, picking and drawing observe first, in the frame's
update step, then read.

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
| `solve_network` | `port::network(doc)`, one Newton solve | A Solve cell per algebraic island: unknowns joined by any equation they share, a definition's equations included (`port.rs:274`), not by connections alone. Each keeps its own warm start |
| Layout glides (`motion`, `advance_motion`, `lib.rs:2052`) | per call | Presentation state on the picture: a scheduled cell from the drawn position to the target; snaps on waking (§8.3) |
| `resolve_layout` | `layout::compute` over all groups and tables | A Layout cell per container (§9.2) |
| `port::derived_signals`, `anchor::derived` | all connections and anchors, each rebuild | Cells made and removed with their relation (§7.2) |
| `advance_machines`, `advance_steps`, `advance_queries`, `advance_presence`, `clock::advance` | each scans its node type each frame | Stepper cells on the scheduler (§10) |
| `resolve_contexts` (behaviour rules) | when markup changed | Unchanged; not reactive values |
| `rebuild_clusters`, `post_apply` (constraint solver) | all constraints | Unchanged in this spec (§9.4) |

### 9.2 Layout, and structure that depends on it

CSS allows a descendant's style to depend on a container's size only under **size
containment**: the container takes no size from its contents on that axis. Flutter's
`LayoutBuilder` and Compose's `SubcomposeLayout` are the same rule. That makes the
dependency one-way, so it resolves top-down. Three mechanisms, in order of preference:

1. **Fitting is layout's job.** Wrapping, and showing only the children that fit, are
   layout modes that publish a `fits` flag per child. No structure changes.
2. **Declared containment, per axis.** A group may declare `contain` on width, height
   or both. On a contained axis it contributes no content size to its parent, and its
   size is fixed or given by the parent. Only then may a Collection inside read that
   size (`count="{= floor(#box.size.w / 80)}"`); reading the size of an uncontained
   ancestor from a structural input is refused at write time. Being sized by a parent
   is not enough: a fill child still feeds a parent that hugs its contents.
3. **Measuring the prototype.** "As many as fit" reads the size of the Collection's
   prototype (§5.3), which exists without any item.

Today one Taffy run lays out each outermost container's whole nested tree, measuring
text inside it (`layout.rs:318`). That stays the unit, cut only at contained boxes, in
two cells each:

- **Allocation:** the outer run treats a contained box as a leaf of fixed size on its
  contained axes and publishes the box's size.
- **Interior:** a run rooted at the contained box places its children.

Structure inside the box sits between the two in height. Layout reads beneath its own
published placements, as now (`layout.rs:33`).

True feedback (a count that reads its own items' laid-out size) is refused at write
time where it is static. Where it arises through a tracked read it is a cycle, reported
as a problem on the Collection, which keeps its last items (decision D6).

### 9.3 Machines

A machine's state is an argument (document data). The player has memory today: it
notices a state change by polling, records when the transition started and the channel
values it started from, including a transition interrupted midway (`machine.rs:282`,
`:331`). So a machine is **Logged** (§10.4): the change of its state argument is an
event recorded when it happens, whatever is observed, and the tween is a pure function
of that log and the clocks. Kept: a transition driven by a clock clip (`progress`),
`machine.at` and `machine.running`, a child never a frame behind its parent (today a
4-round loop, `lib.rs:1939`; here ordinary dependency order), and undo snapping the
instances it touches instead of playing back (`lib.rs:2481`).

### 9.4 The constraint solver

Out of scope. `post_apply` validates and solves at commit and during gestures
(`resolve_clusters_live`), reading and writing through the engine as it does now. Its
whole-document `rebuild_clusters` after every transaction is the next whole-document
cost once this lands; its clusters should later be indexed by owner.

## 10. Time

### 10.1 Clocks

A clock is a cell. There are **two root clocks today, and they stay two**:

| Clock | Is | Read by today |
|---|---|---|
| **Presentation time** | the host's wall clock; only moves forward | steps (traces, accumulators), queries, presence, layout glides, machines' own transitions |
| **The playhead** | the session `clock` node: `t`, `playing`, `rate`, loop bounds (timeline.md §4.1). It is scrubbed, looped and paused, so it jumps both ways | clips; machines whose `progress` a clock clip drives (the player takes both, `lib.rs:1940`) |

Steppers stay on presentation time: pausing the timeline does not freeze a pulse.

Formulas have no way to read time today except a reference to the session `clock` node
(`expr.rs` has no time function; timeline.md §11 notes the gap). New: `clock()` in a
formula reads the **nearest clock provided above its owner**, resolved the way `@name`
is, and the playhead where none is. An owner provides a clock with a `clock` facet,
defined as another clock through a mapping (offset, rate, paused, loop). Uses: the
narrative clock (position, not seconds), a preview at another time, a component in slow
motion, a reduced update rate for something small on screen.

### 10.2 One entry point

```rust
fn advance(&mut self, now: f64) -> Wake;
struct Wake { next: Option<f64>, live: BTreeSet<NodeId> }   // compositions that must redraw
```

`advance` replaces the five `advance_*` calls and the host's clock player. `Wake`
replaces each producer's bookkeeping in `LiveSources` (`liveness.rs`): a composition is
live while a necessary cell that changes continuously is drawn in it.

### 10.3 The scheduler

Each time-dependent cell says, for its clock, when it next changes:

| Cell | Next change |
|---|---|
| An animated leaf | At the clip's first key (before it the clip does not drive the leaf, `time.rs:204`); continuous between keys; at each `hold` key; after the last key, never |
| A machine's tween | Continuous from the recorded start until its end |
| An item's presence, a layout glide | Continuous until it settles |
| A `trace`'s `out` | At the arrival of the next recorded change |
| A `trace`'s `dash`, `ages`, `values` | Continuous while any change is in flight |
| A query's debounce and timeout | At that time |
| An `accumulate` | Continuous while its `next` differs from its value |
| A formula that reads `clock()` | Continuous while that clock runs |

A queue per clock holds the timed entries. Each `advance` pops what is due and marks it
dirty; continuous cells are evaluated only if necessary. Frame cost follows what is
moving *and* observed. Timed events fire whether or not their source is on screen,
which is what delivers an off-screen machine's pulse to a visible meter.

A clock that jumps (a scrub, a loop, a rate or direction change, a gesture's preview of
`clock.t` and its cancel, `sync_clock`) discards its queue's entries and asks each of
its cells again. Entries are in the clock's own time, so a rate change only changes when
the parent clock reaches them.

### 10.4 Memory

| Class | Examples | Unobserved | At another time (§10.5) |
|---|---|---|---|
| **Pure** in its clocks | clips; `ease` of a clock; a tween given its recorded start | skipped; computed when read | valid |
| **Logged** | `trace` (changes with times); a machine's transitions (start time and starting values) | the **event** is always recorded (§8.3); nothing is sampled | valid within what the log holds |
| **Integrating** | `accumulate`; a host simulation; the network solve's warm start | by policy: `run` (default), `pause`, or `coarse` (larger steps) | not valid unless recorded |
| **Presentation** | presence, layout glides | not kept; snaps on waking | not valid |

Recording an event costs when the event happens, not every frame, so a thousand
animated machines off screen cost nothing between their state changes. An `accumulate`
with policy `run` does cost every frame it moves; a closed form is preferred where one
exists (the laser's pulse phase is `frac(clock() · hz)`). Decision D1.

### 10.5 Slices: time as a dimension

A value is a function of the document, the clocks, the recorded events and the free
inputs. A **slice** fixes the clocks (and may carry an override layer). The live slice
is now. This is the notes' "seekable" (narrative note §6.7), and its "trial evaluation
at the end position" is reading a value in another slice.

Whether a value is valid in another slice is decided by its **sources**, kept per cell
as the union of its dependencies':

| Source | In another slice |
|---|---|
| The document | the same value, stored once |
| A clock | recomputed at the slice's time |
| A log (trace, machine) | recomputed, within what the log holds |
| A free input (the pointer, a host signal, selection, session state), an integrator, presentation state | `Failed("not seekable: …")`, naming the source |

Depending on no clock is not enough to be valid: a host signal has no clock edge and is
still "now" only.

**Phase 6 builds value slices only.** A slice stores values for cells that depend on a
clock it sets differently, and reads through to the live slice for the rest. It does
**not** hold its own structure: if a Collection's items, a layout or a solve depends on
a clock the slice changes, everything below it is "not seekable" in that slice, and the
way to see that time is to move the live clock. Structure per slice (an owner tree per
slice below a clock-dependent Collection) is the full form of "any time is equally
valid"; it is possible in this design and is left open (decision D10).

An observer names its slice (§8.2). The gesture overlay stays as it is; it is shared by
the store and holds solver results, so it belongs to the live slice only.

## 11. Transactions

`Store::run` no longer clones the document. The council found that rolling back by
inverses alone does not work as first written; this section is the answer.

### 11.1 What must be true first

- **Each op is atomic.** Today `set`, `add_comp` and `reparent` change the document and
  then run a check that can fail, returning no inverse (`op.rs:615`, `:690`, `:524`).
  Every op must check before it changes anything, or undo its own change on failure.
- **Counters are saved.** A create advances the id counters (`next_ids`, `next_rels`);
  they are saved at the start and put back on rollback, so a failed transaction leaves
  no gap.
- **Validation is a pure function of the document and its templates.** The checks that
  read derived structure (`check_created` for a connection to a derived port,
  `check_scope`, `check_context`) are answered by the Tree from the document and
  compiled templates, without the engine's values. So nothing is evaluated, disposed,
  scheduled or sent before a transaction is known to be good, and acceptance never
  depends on what is observed. `attachment::validate` compares the documents before and
  after (and scans every attachment); it works from the ops and their inverses.

### 11.2 The steps

1. Translate edits aimed at derived nodes (`expand::translate`), making current what it
   reads (§8.9).
2. Apply each op in place, collecting inverses. On an error keep going, to report every
   error, as now.
3. Validate (§11.1). On any failure: apply the inverses in reverse, restore the
   counters, discard templates compiled in step 2. The engine was never told.
4. Tell the engine each op (§7.2) and `stabilise()`. **Owners disposed in this step are
   held, with their state, until step 6.**
5. `post_apply` (the solver). On failure (today: put the old document back and rebuild
   the graph, `lib.rs:2423`): apply the inverses, tell the engine, `stabilise()`; held
   owners are re-attached as they were.
6. Commit: drop held owners; release buffered effects (a query's request, `on.emit`);
   publish `Changes`.

### 11.3 Consequences

- `Transaction::expanded` goes away. It is never replayed today; undo re-derives.
  `ops`, `derived_ops` (solver writes) and `inverse` stay.
- Undo, redo, replay and ops arriving from a server take the same path. Undo still
  snaps machines (`machines.snap`).
- `apply` still suspends a running gesture (overlay cleared), classifies the
  transaction against it, and resumes or cancels; idempotency keys are checked before
  step 1, as now.
- **`apply_session`** (`lib.rs:1216`) is the second way the document changes: session
  ops (selection, the clock, the timeline and Library compositions) applied outside
  history, which today rebuilds the whole graph on any structural one. It takes the
  same steps, without history.
- If atomic ops prove hard to guarantee, the fallback is a document of shared,
  copy-on-write chunks, so a working copy costs what changed. It is not the first
  choice because every read pays for the indirection.

## 12. The host contract

The host has no op-by-op path today: `sync_document` turns ops and resolved changes into
a set of node ids and rebuilds those by reading the store (`document_bridge/mod.rs:520`,
`collect_ids`, `materialize::reconcile`). It does not treat derived ids differently. So
the contract is small.

### 12.1 The Tree

`Store::tree()` answers for any live node id, authored or derived, with `Document`'s
read methods under the same names: `live`, `node`, `parent`, `children`, `depth`,
`facet`, `facets_of`, `get::<T>`, `leaf`, `relations_from`, `relations_to`,
`nodes_of_type`. For a derived node it answers from what the owner has materialised
(§5.4). `Store::document()` remains and holds authored nodes only.

- **A read never loads** (§8.9). A derived node of an Unloaded owner is "not loaded";
  the caller observes or pins first, in a `&mut` step.
- **Callers inside the crate move too**, not only the host: `layout::compute` and
  `constrained`, `port::network`, the machine player, steps, queries, the palette,
  `publish_selection`, `Store::selection` (which today filters by `doc.is_live` and
  would silently drop every selected derived node), `solve::clusters`,
  `check_created`, `translate` and handles, `view`, `analysis`. They take a `Tree`
  where they take `&Document` now.
- Outside the crate, about 260 uses of `document()`. Keeping `document()` means a
  missed caller still compiles, so phase 1 renames it (`authored()`), which makes every
  use a decision.
- Enumerations on the Tree visit loaded owners only.

### 12.2 Changes

```rust
fn take_changes(&mut self) -> Changes;
struct Changes {
    values: BTreeSet<Leaf>,              // leaves whose resolved value moved
    created: Vec<NodeId>,                // authored and derived, parents first
    removed: Vec<(NodeId, NodeId)>,      // (node, the parent it had), deepest first
    moved: Vec<(NodeId, NodeId)>,        // (node, old parent): reparented or reordered
    relations: Vec<(NodeId, NodeId)>,    // ends of relations linked or unlinked
    structure: BTreeSet<NodeId>,         // owners and authored parents whose children changed
    loaded: Vec<NodeId>,
    unloaded: Vec<NodeId>,
    revisions: Revisions,                // authored, session and resolved, as `ui::publish` reads them
}
```

It replaces `take_derived_ops`, `drain_changes`, `has_changes`, `take_touched` and
`take_geometry_changes`, and the chaining of `tx.expanded` and `tx.inverse` into
`collect_ids` (which uses inverses to find a node's former parent and an unlinked
relation's ends, `mod.rs:520`).

- `values` are leaves, not nodes: `geom::Revisions` tells a move from a change of shape
  by the leaf (`geom.rs:399`).
- `removed` lists derived ids as well, since a disposed owner's nodes can no longer be
  enumerated afterwards; `materialize::reconcile` needs them to drop drawables.
- `structure` is per parent, so paint order and the spatial index can update one
  subtree. Paint order must read the Tree from phase 2c on: once copies leave the
  document, a walk of `document()` gives derived nodes no paint index (`mod.rs:675`).
- An owner in `unloaded` drops its nodes' drawables; a selection chain or a
  history-restored selection that names a derived node pins its owner first.

### 12.3 What each consumer does

| Consumer | Today | With owners |
|---|---|---|
| `materialize::reconcile` | per id, reads the store | The same, through the Tree. An unloaded owner removes its nodes' drawables |
| Spatial scene (`iron_spatial`) | full preorder on any structure change; copies indexed as leaves | One entry per owner with its bound, as references are (`Embed2d`); an owner's contents indexed while it is loaded; rebuilt per owner |
| Drawing (`render.rs`, `chunks.rs`) | walks `doc.children`; instance runs from copies | The Tree for loaded owners; cached tiles or display list for Unloaded ones (§8.5) |
| Picking | spatial candidates, then a chain | A hit on an Unloaded owner's entry loads it, then picks inside |
| Selection (`Reach` chains ending in derived ids) | ids | Holding a derived id pins its owner |
| Render analysis (`analysis.rs`) | per instance, per call | Per template and set of wired inputs, cached by template version |
| `document_view` (`<look>`) | reads everything | A Look observer (§8.2) |
| `document_inspect`, `document_component`, `document_computed` | read derived nodes | A Nodes observer while the inspector shows them |
| Layers panel, `narrative_targets` | exclude derived ids | Unchanged |
| `paint_order` | walks the whole document on any structure change | Reads the Tree (required from phase 2c); per changed parent from `Changes::structure` |
| UI events (`ui::publish`) | authored, session and resolved revisions | `Changes::revisions` |

## 13. Ready for partial loading and sync (not built here)

What scopes.md §8 asks of "later designs", and what this engine supplies:

| Need | Supplied by |
|---|---|
| "Not loaded" is ordinary | `Pending` (§6.4); owner states (§8.1); the Tree can answer "not loaded" for an authored chunk as it answers from a template for a derived one |
| A loading unit smaller than a scope | The owner, and for authored content a subtree with a stored `Summary` |
| Knowing what a visible part needs | `summary.reads` and `exports`, from static dependencies without evaluating. A `deref` is found only by running, so it may ask for a chunk nobody predicted: that read is `Pending` until it arrives |
| Ops that apply to a partly loaded store | An op on something not loaded must be kept (or fetched again) to apply when it loads; a summary's stamp alone is not enough. Unsolved here, as in scopes.md §8 |
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

Hypotheses, not measurements; each is measured before and after with the probes in the
handoff's §4. The meter rows cannot be met by the engine alone: a **constant** edit at
1,000 meters already costs 14.1 ms with no expansion (instance-cache.md), which is the
document clone plus whole-document passes that outlive phase 2 (`rebuild_clusters`,
`attachment::validate`, `layout::compute`, `port::network`, the palette, contexts).
Phase 0 profiles that edit; the meter targets are for the end of phase 5, and phase 2
reports the engine's share.

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
everything-observed mode.

Through phase 2b the new engine still stores copies in the document (§17), so the
comparison is exact:

- the set of live node ids, authored and derived; each node's type, parent, order and
  facets; relations;
- the resolved value and error of every leaf; problems, as (node, kind);
- what a `ViewQuery` prints, at full fidelity;
- **effects, not only final state:** the query requests issued, `on.emit` events,
  session cells made and dropped, the revisions reported, the selection after undo;
- **a failed transaction leaves nothing:** the document, counters, values and effects
  are identical to before it.

Not compared: ops emitted for derived structure, evaluation counts, heights.

**Intended differences** are listed in one reviewed file, each with a test of the new
behaviour: the prototype is not evaluated unless observed; the old engine's text
comparison of NaN inputs in `settle_repeats`; its 4-round machine loop; its 129-round
cut for Collections that read each other.

For phase 2c the reference is phase 2b's engine: the Tree's materialised nodes must
equal the copies it stored.

**A host reference.** The store comparison does not show whether the host updated
correctly from `Changes`. A second harness rebuilds the host's derived state from
scratch at each step (drawables, paint order, the spatial scene) and compares it with
the state kept incrementally.

Scripts: every existing test's sequence of calls, and a generator of random ones
(transactions including failing ones, previews, publishes, clock moves, undo and redo)
over documents with repeats in instances and instances in repeats, conditionals
toggling, keys changing kind, overrides on items that leave and return and on nested
instances' definitions, enter and exit in flight, context providers moved, connections
to instance ports, definition edits.

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
measured and recorded in the roadmap's Status table. The council asked for phase 2 to
be cut and for time to come before observation; both are done here.

| Phase | Builds | Exit |
|---|---|---|
| **0. Baselines and the harness** | Memory and load-time baselines; a profile of the constant edit at 1,000 meters (§15); growth of interned id paths. The differential harness and the host reference (§16.1), run old against old. The script generator | Both harnesses green on old-vs-old across the existing suites' scripts and 10,000 random ones |
| **1. The read contract, on the old engine** | The `Tree` and `take_changes()` (§12) over today's document and graph. Every caller that can meet a derived id uses them, **inside the crate and outside**; `document()` is renamed | No behaviour change: smoke output identical; `take_derived_ops`, `drain_changes`, `take_touched` have no callers |
| **2a. Transactions in place, on the old engine** | Atomic ops, saved counters, validation from the document and templates, `attachment::validate` from ops, `apply_session` on the same path (§11) | No document clone in `run`; a random failing transaction leaves the store bit-identical; the constant edit at 1,000 meters measured again |
| **2b. The new graph, copies still stored** | Cells, heights, necessity (everything necessary), owners, compiled templates, Structure cells that reconcile one owner and **write its copies into the document as today**. Replaces `graph.rs`, `expand::expand` and `settle_repeats`. The pipeline's producers run in today's order and write Input cells | The differential test is exact and green; **the plot targets of §15 are met**; the roadmap row "Plot pan and zoom performance" is closed |
| **2c. Copies leave the document** | Derived nodes answered by the Tree from owner, template and diffs, materialised on read (§5.4); paint order and the spatial scene from the Tree | The Tree equals 2b's stored copies on every script; memory per instance measured against phase 0 |
| **3. Time** | Two root clocks, `clock()` and clock providers, the scheduler, `advance` and `Wake`; steppers as scheduled cells; events recorded for traces and machines; memory classes (§10.1–§10.4) | `fiber.txt` and `pulses.txt` pass; an idle document with a trace and an accumulator at rest schedules no wake; a machine's transition matches the old player's, interrupted transitions included |
| **4. Observation** | Observers, Dormant and Unloaded pictures, what stays necessary (§8.3), bounds kept right (§8.5), owner entries in the spatial index, the wake budget, the idle sweep, reads that never load (§8.9) | §8.8's property test is green; a hidden tab's composition of pure pictures costs 0 evaluations; a pulse from an off-screen laser reaches an on-screen meter on time; something moving into the view from outside it appears |
| **5. The pipeline as cells** | Layout cells with declared containment and `fits`; the network solve per algebraic island; clips as cells; `rebuild_clusters` and the per-transaction scans measured and, where they dominate, indexed (§9) | A repeat whose count reads a contained box's width adds items during a resize drag; `layout`, `table`, `ports`, `schematic` suites green; the meter targets of §15 |
| **6. Value slices** | §10.5: sources per cell, per-slice values, an observer's slice, "not seekable" errors; timeline.md phase 5's seekable endpoints use it | Two views of one composition at two clock times, both correct where structure does not depend on the clock; a clip end bound to a clock-derived value is accepted and one bound to an accumulator is refused with the reason |

Phase 3 comes before 4 because what may sleep depends on how steppers and events are
classified. Phase 5 does not depend on 3 or 4. Chunked storage (§13) is separate work
after 4.

## 18. Risks

| Risk | Mitigation |
|---|---|
| A caller still assumes a derived node is in `Document` | Phase 1 moves every caller onto the Tree while the old engine still stores copies, so a miss shows as a compile error or a failing smoke before the engine changes |
| The new engine differs subtly from the old in a case no test covers | The random differential test (§16.1); the swap waits on it |
| Reading a derived node's facets through the Tree is slower than reading a stored copy | Cells hold resolved values, which is what drawing reads; relocated facets are cached per loaded owner on first read; measured in phase 2 |
| A Dormant picture's bound is stale and it fails to wake | What can move a bound stays necessary (§8.5, decision D2) |
| Waking a large region stalls a frame | The per-frame budget and frozen drawing meanwhile (§8.7) |
| The interned path registry grows without bound | Decision D5; measured in phase 0 on a script that churns value keys |
| Phase 2 is large | Cut into 2a, 2b and 2c (§17), each useful alone; 2b keeps today's representation so its comparison with the old engine is exact |
| A stale value is written into the document | Reads that feed writes make their inputs current first (§8.9); a stale read is an error in debug builds |

## 19. Decisions for the user

Each has a default the spec assumes until told otherwise. D1, D2, D6 and D9 changed
after the council; D10 is new.

| # | Decision | Default |
|---|---|---|
| D1 | What unobserved state does | **The model keeps its history; pictures snap** (§8.3, §10.4). Events (a machine's state change, a traced change) are always recorded. An integrator runs unless its author sets `pause` or `coarse`. Presence, glides and tweens inside pictures are not kept. The reviewers disagreed (one wanted everything to run, one wanted integrators to run only when their output leaves their owner); this is between them |
| D2 | A Dormant picture's bound | **Kept right**: whatever can move its bound without an edit stays necessary (§8.5). A picture with a static bound costs nothing. The first draft's "last known bound" could hide something that moved into view |
| D3 | Problems in owners that are not Live | As of their last evaluation, with the idle sweep refreshing them. Whether a transaction is accepted never depends on this |
| D4 | The words "owner" and "slice" | As in §3; reactivity.md's "scope" is renamed where the two specs meet |
| D5 | Reclaiming interned id paths | Measure in phase 0. Counting references from owners is not enough: relations, history and a selection can hold a derived id after its owner has gone |
| D6 | Feedback from measured layout into structure | Refused where static; a cycle with a problem where found at run time. No deferral to the next frame, which would make the result depend on frame timing |
| D7 | `std:plot` tick labels keyed by value (labels slide and fade) | Yes, as its own change after phase 2b |
| D8 | Session lets when an owner is unloaded | Kept (unloading is not disposal) |
| D9 | Limits | 50,000 loaded derived nodes; also a limit on owners in total, loaded or not, since each keeps a summary |
| D10 | Structure per slice | Not built: phase 6 is value slices, and a time whose structure differs is seen by moving the live clock. The full form is left open until something needs two structures at once (a side-by-side "before and after", say) |

## 20. Council review (2026-11-15)

Claude Opus 5.5 and GPT-6.1 Sol, both at high effort, each returned "rework". Every
claim about the code below was checked before it changed the spec.

| Finding | From | Checked | Result |
|---|---|---|---|
| Rollback by inverses leaves changes behind: ops mutate before a check that can fail; counters advance; disposal drops session, presence, trace and machine state; effects escape | both | True (`op.rs:524`, `:615`, `:690`; `lib.rs:1476`) | §11 rewritten: atomic ops, saved counters, pure validation before the engine is told, disposal held until commit. Sol's full transaction journal is not adopted, because validation no longer touches engine state |
| Overrides replace slots (a binding included) and can add facets; they can change a nested instance's definition; constant references are relocated; the `sized` flag and `port_spec` → `port` | both | True (`expand.rs:647`, `:487`, `:706`) | §5.2, §5.4, §5.5 |
| The crate's own readers walk derived nodes through `&Document`; `Store::selection` would drop derived nodes | Opus | True (`lib.rs:2251`) | §12.1; phase 1 covers the crate |
| §8.8 is false for history: a dormant conditional that toggles, traces sampled per frame, machines started by polling, presence integrating `dt`, glides, queries | both | True (`machine.rs:282`, `step.rs:134`, `lib.rs:1436`) | §8.3, §8.8, §10.4, D1 |
| A last-known bound can hide content that moved into view | Sol | Judgment; agreed | §8.5, D2 |
| Reading a cell that is not current can write stale values into the document; reads through `&Store` cannot load | Opus | True (`post_apply`, `translate`, `resolve_clusters_live`) | §8.9 |
| An Unloaded picture drawn as a reference shows the template's values, wrong with arguments | Opus | True (`render.rs:573` draws the template's own nodes) | §8.5 |
| Value slices cannot hold structure that depends on time; no clock edge does not mean seekable | both | Judgment; agreed | §10.5, D10 |
| Two clocks are merged; formulas cannot read time today; undo snaps machines | Opus | True (`lib.rs:1940`, `:2481`; no time function in `expr.rs`) | §10.1, §9.3 |
| The scheduler misses clip starts, hold keys, jumps, `ages`, layout motion; machines are not pure | Sol | True (`time.rs:204`) | §10.3, §9.1, §9.3 |
| "Sized by its parent" is not containment; one layout cell cannot both size the box and place its contents | both | True of today's single Taffy run (`layout.rs:318`) | §9.2 |
| Network islands follow equations, not connections; the solve warm-starts | Sol | True (`port.rs:274`, `lib.rs:2146`) | §9.1, §8.8 |
| `Changes` is too coarse; `apply_session` is a second path | both | True (`geom.rs:399`, `mod.rs:520`, `lib.rs:1216`) | §12.2, §11.3 |
| The meter targets cannot be met in phase 2 | Opus | True: a constant edit is 14.1 ms at 1,000 meters | §15, phase 5 |
| "Never revisits a height"; references to items that do not exist yet; Collections reading each other | both | Agreed | §6.2, §6.3, §7.4 |
| The harness misses effects, failed transactions, the host, and intended differences | both | Agreed | §16.1 |
| Phase 2 is too large; lifecycle before dormancy | both | Agreed | §17 |
| Patch the old graph first (the first handoff's option A) | both | Not adopted as such: 2b writes the new cell graph directly but keeps today's stored copies, which gives the same early plot fix and an exact comparison without code that is thrown away |
| A copy-on-write document instead of rollback | Opus | Kept as the fallback (§11.3) |
| Materialise derived records per loaded owner so reads keep returning whole facets | Opus | Adopted for the Tree (§5.4); the engine itself still evaluates from the shared template |
