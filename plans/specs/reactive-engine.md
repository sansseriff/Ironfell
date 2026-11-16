# The reactive engine: owners, demand, templates and time

**Status:** Approved for building 2026-11-15; the decisions in §19 are settled.
**Nothing is built** except a reference model of the core pass
(`src/iron_engine_kernel`, §6). Work order, the current task and the rules for whoever
builds it: [`../handoff-reactive-structure.md`](../handoff-reactive-structure.md).
What is assumed and not yet verified: §14. How the design was reached:
[`reactive-engine-review.md`](reactive-engine-review.md).

**Replaces** the reactive graph (`graph.rs`), expansion (`expand.rs`) and the settling
and pipeline half of the store (`lib.rs`) with a new engine, written beside the old one
and swapped in when a differential test says the two agree (§16). It supersedes
[`reactivity.md`](reactivity.md) §3.3's "settle in rounds". Everything else in reactivity.md (keys, state, actions,
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
- New constraint kinds, the solver's numerical method, and a live simulation over
  constraints (constraint-system.md §11). §9.4 covers how the solver that exists is
  driven by this engine and where a simulation would attach.
- Ports, anchors and mates in 3D ([`ports-3d.md`](ports-3d.md),
  [`component-3d.md`](component-3d.md)). §8.10 says what this engine must not assume
  so that they fit.
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

**Two things that do not stay:** derived nodes are no longer real document nodes;
they are **addressable, and stored only where they differ** (§5.4). And there is no
full rebuild as a fallback path: loading is the same pass with everything observed.

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
2. A constraint cluster's solved value, if the leaf is one of its variables (§9.4).
   Such a leaf is never bound, so this and the next never meet.
3. The binding's value, if the effective slot is bound.
4. A producer's signal.
5. The effective slot's constant.

### 5.3 Owners

```rust
struct Owner {
    id: NodeId,                 // the instance, picture, repeat, or item scope `#r/~k`
    parent: Option<OwnerId>,
    kind: OwnerKind,            // Root | Composition | Body | Picture | Collection | Item
    stateful: bool,             // its template holds state, here or below (§8.3)
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
    bound: Option<Bound>,       // a rectangle or a box (§8.10), in the parent's space; or a declared size
    nodes: u32,                 // derived nodes it would hold if loaded
    exports: Vec<Leaf>,         // what something outside reads from it
    reads: Vec<Leaf>,           // what it reads from outside
    as_of: Stamp,               // when its values and problems were last current
}
```

| Kind | Made from | Template |
|---|---|---|
| Root | the document | none |
| Composition | a composition | none; its authored nodes. The unit a view observes, and later the unit of loading (§13) |
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
    Body(Local, LeafPath),     // a picture's read of its definition's body: the figured entity's Body owner
    Item(u8, ItemLeaf),        // value, index, presence, leaving: the nth enclosing Item's
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
- **The key names shared syntax, not one owner's records.** What an expression's
  `Ref`s resolve to comes from the owner's **environment**: the entity it figures, its
  arguments, its enclosing items, and the relations that cross its boundary
  (`figures`, `connects`, `exposes`).
- **A Collection inside a template** is compiled as a sub-template of the enclosing
  one (today `template_of_repeat` builds it from one picture's copies, that picture's
  overrides baked in, `expand.rs:349`). The plot's tick labels are this case.
- **The engine stores nothing per derived node** except cells for its bindings and its
  entries in `diffs`. **The Tree** (§12.1) materialises a derived node's facets, with
  references relocated and overrides applied, the first time something reads them:
  each loaded owner has one write-once slot per template node (`OnceLock`), allocated
  when it loads, so a read through `&Store` can fill a slot and hand out `&T`. An
  owner's slots are cleared when it unloads, its template's version changes, or an
  override, an argument or a boundary relation of its environment changes.
- **What this saves:** drawing and picking read every visible node's facets, so a
  visible owner is materialised in full, as today. The saving is in owners nothing
  reads (off screen, in hidden tabs, collapsed in the agent's view), and in the engine
  not cloning expressions.
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

`src/iron_engine_kernel` is this section and §6.1, §6.3 and §7.1 as running code: about
700 lines, its rules numbered R1 to R8 in `engine.rs`, tested against recomputation
from scratch on random programs with everything observed and with a changing observed
subset. Read it before writing `eval.rs`. It is a model of the algorithm, not a start
on the engine: its values and expressions are toys (§14, A5).

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
| **Unloaded** | referenced | none | no | `summary`, `diffs`, `session`, and for a stateful owner its parked state (§8.3) |
| **Dormant** | referenced | exist | no; values as of `summary.as_of` | the same, and cells |
| **Live** | referenced | exist | necessary ones, every pass | the same |

An owner is at least as awake as its most awake child, and a child is never more awake
than an Unloaded parent. **A stateful owner (§8.3) is Unloaded only when it is at
rest**, and then it is *parked*: its state is kept and it is loaded again when
something it waits on happens. Loading a derived owner is synchronous: instantiate cells from
the template, apply `diffs`.

### 8.2 Observers

```rust
enum Interest {
    Region { composition: NodeId, rect: Rect, zoom: f64 },  // a 2D view
    Volume { composition: NodeId, frustum: Frustum, pixels_per_unit: f64 },  // a 3D view (§8.10)
    Nodes(Vec<NodeId>),          // an inspector, a selection, the layers panel's open rows
    Look(ViewQuery),             // the agent's XML view: root, depth, expand, fidelity
    Pin(NodeId),
}
fn observe(&mut self, interest: Interest, slice: SliceId) -> ObserverId;
fn reobserve(&mut self, id: ObserverId, interest: Interest);
fn unobserve(&mut self, id: ObserverId);
```

- A **Region** makes necessary the drawn leaves of every node the spatial index finds
  in the rectangle grown by a margin: authored nodes of the composition as much as
  derived ones. Picture owners whose bound meets it are made Live through the index's
  entry for each owner (§12.3). Which cells of a Live owner are necessary is the same
  rule: those a drawn leaf in the region needs.
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
current, and nothing else of the machine.

**State is told by the kind of node, not by where it sits.** Pictures can hold state
by design: a `let` in a template is per picture (reactivity.md §5.1, the accordion's
open flag), any node may be a template's child, and an entity inside a repeat's
template gives each item an instance. So "the body computes, the picture shows" cannot
be the line. Instead a template is **stateful** if it, or any template under it, holds
a session `let`, a `trace`, an `accumulate`, a `machine`, a `query`, or an `on` with a
condition trigger. That is known when the template is compiled. An `on` for pointer
events is not state: it only runs while something is interacting with it.

For a stateful owner, always, whatever is observed:

- **Its state is never lost.** While anything it follows is moving it is at least
  Dormant, so its session cells and logs exist and its events are recorded.
- **At rest it may be parked.** An owner is at rest when no recording is pending, it
  has no continuous scheduler entry, and no integrator in it is running. Parking
  unloads its cells and keeps, beside `session`: each machine's state and the tail of
  its log that a tween could still read, each `trace`'s changes still in flight, each
  `accumulate`'s value, and its timed scheduler entries. It is loaded again, with that
  state, when a leaf in `summary.reads` changes, when one of its timed entries is due,
  or when it is observed. The first two are how it would have noticed the event had
  it stayed loaded, so parking changes nothing that can be seen (§8.8). A thousand
  machines far from every view, with still inputs, cost a summary and a few values
  each.
- **What cannot be parked** is an owner whose inputs keep moving: a machine whose
  state argument reads a running clock, an integrator with policy `run`. It stays
  Dormant, evaluating only what its state needs, unless its author sets `pause` or
  `coarse`, or the application sets a **simulation range** (below).
- **Its lifecycle is exact.** Every Collection above it keeps its Structure cell
  necessary, so it is made and disposed when it would have been and its state reset
  when it would have been. A leaving item's exit is two recorded times (when it began
  to leave, and whether it turned back), and its disposal a scheduled event at the
  deadline; nothing is sampled per frame.
- **Events that start something are recorded when they happen** (§10.4): a machine's
  state argument changing, `machine.at` changing position or direction, the value a
  `trace` follows changing. What they read stays necessary. The tween or the picture
  of the pulse that follows is computed only when observed.
- **An integrator** (`accumulate`) with policy `run` runs, with what it reads.

**A simulation range** is the application's choice to stop state that is running and
far from every observer, as a game stops the world beyond some distance: an observer
may carry a range, and a running stateful owner outside every range is treated as
`pause`. This is outside §8.8's property by design (the owner resumes where it
stopped, not where it would have been) and is off unless set. Catching up on waking,
by replaying inputs or taking large steps, is not designed here.

For every loaded owner, stateful or not: **its extent** (§8.5), the inputs of the
network solve when it runs (§8.9), and **each constraint cluster with an input that
changed** (§9.4): a cluster's pose depends on the pose before it, so it is solved when
its inputs move, seen or not.

A pure owner (no state at any depth: the plot's tick labels, a table's cells, most
drawings) keeps nothing else. On waking, a pure Collection reconciles once: its items
appear and go without their enter and exit, and a layout glide is at its target. That
is what "snaps" means, and it is exact, since nothing else depended on those.

**What this costs.** A component with a machine is stateful, and so is everything
above it; `std:plot` has one (`vis`). A lesson of many such components keeps their
structure current and records their events, but evaluates none of their pictures. A
machine's state argument that reads a running clock is evaluated every frame, unless
the formula is one the scheduler can solve for its next crossing (a threshold on a
clock); phase 4 measures this case.

### 8.4 Bodies and pictures

A Body owner is never Unloaded while its entity exists: it is small, and saved
connections attach to its ports. Its cells are evaluated only when necessary. Pure
pictures go Dormant and Unloaded freely; stateful ones go Dormant, and are parked
when at rest (§8.3). One entity
pictured in two compositions computes once; each picture wakes with its own view.

### 8.5 Drawing what is not Live

- **Dormant:** drawn from its last values. The render cache's tiles and shared drawings
  are already keyed by content and stay valid.
- **Unloaded:** drawn from what the render cache kept for it (tiles, or its last
  display list), keyed by owner. Only an owner with no arguments, overrides or item
  value, so identical to its template, may be drawn as a reference
  (`render.rs:573`); a reference draws the template's own values, which is wrong for
  anything else. An Unloaded owner with nothing cached is loaded when it comes into
  view.
- **Bounds stay right.** A view must never miss something that has moved into it.
  Each loaded owner, and each authored subtree the index holds as one entry, has an
  **extent cell**: the union of its nodes' bounds, reading what `geom::local_bounds`
  reads (geometry, placement, stroke width, shadow padding, measured text) and its
  children's extents. It is always necessary while the owner is loaded, so an edit or
  a moving input updates `summary.bound` whether or not anything is drawn. Render
  analysis cannot supply this: its channels put fill and stroke with geometry, shadows
  with effects, and treat layout as opaque (`analysis.rs:57`).
- **What that costs, and the way out.** A static extent costs nothing between edits.
  An owner whose geometry moves keeps that geometry current while Dormant, which for
  an unclipped plot of live data is most of its work. An owner with a declared size
  that **clips** has that size as its extent and nothing inside is needed: standard
  components should clip. Only an owner whose extent is static, clipped or declared
  (§8.10) may be Unloaded. Keeping an extent current needs only the cells the extent
  reads: a part that moves as a whole needs its placement, not its insides. Phase 4 measures both cases.

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
- a stepper an author set to `pause` or `coarse`, or one outside a simulation range
  (§8.3);
- answers from outside (a `query`'s request times and replies);
- the network solve where more than one solution exists, since it starts from its last
  answer (`lib.rs:2146`); §9.1 keeps its inputs and warm start per island.

§16.3 tests the property directly. Whether a transaction is accepted never depends on
what is observed (§11).

### 8.9 Reading

A read through `&Store` never loads or evaluates. Reading a cell that is not current
returns its last value and says so (`as_of`). Drawing a Dormant owner reads such
values on purpose, so that is not an error; a debug build checks only the readers
below, which must not see one.

Anything that writes what it reads into the document or the overlay first makes what it
reads current, through `&mut Store`: `expand::translate` and handles (inverting a value
onto its source), the solver's commit step (§9.4, §11.2), `fire` (an action's parallel
reads). Views, picking and drawing observe first, in the frame's
update step, then read.

### 8.10 Three dimensions

The engine's core does not count dimensions: a cell is a leaf's value, and
`transform3d` leaves are bound, animated, overridden and observed like any other.
Owners, templates, time and transactions are the same in a 3D composition, and
expansion already copies a template's subtree whatever its node types
(component-3d.md §1). A port's signal, a connection's derived binding and the network
solve read no geometry at all. What this spec has written in 2D terms, and what each
must become, so that nothing built here has to be undone for ports-3d.md:

| Written here as 2D | For a 3D composition |
|---|---|
| `Interest::Region { rect, zoom }` | A second variant, `Volume { composition, frustum, pixels_per_unit }`. Built in phase 4 with Region, since 3D compositions exist today |
| `Summary::bound: Rect`, the extent cell reading `geom::local_bounds` | `Bound` is an enum of a rectangle and a box; the extent cell reads the mesh's box through `geom::world_affine3`. A 3D owner cannot clip, but it can **declare** its extent: a box its author states it stays inside, as game engines' fixed bounds do. A declared box is the extent, nothing inside is needed, and the owner may sleep or be unloaded. Content that leaves the box is not found by a view until the box is in view (a problem in debug builds when the computed extent is seen to exceed it). Without a declaration the extent is static or stays current, as in 2D |
| The spatial index (`iron_spatial`, an R-tree of rectangles) | Owner entries need a 3D index for 3D compositions. Today the 3D side has none and picks against mesh boxes; phase 4 may keep a walk over owner bounds until counts need more |
| A 2D composition shown on a plane in 3D (`planes.rs`) | The 3D view's frustum, cut by the plane, gives that composition a Region with the zoom its texture is drawn at. One rule covers a panel, an embed and a draped plane |
| Levels by size on screen (§8.6) | The same measure: projected size. Swapping meshes by distance is the host's (`VisibilityRange`), below this engine |
| Layout cells (§9.2) | 2D only, by nature |
| Anchors' derived `wx`, `wy`, `wrot` (`anchor.rs`); mates of 3 equations (`plug.rs`); the solver's `transform2d` variables | ports-3d.md's work: a 3D frame, 6-equation mates, lifted anchors. Each is a derived cell or a Solve cell of the kinds already here (§9.1, §9.4) |

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
| Pins (`layout::constrained`, `layout.rs:443`), which overwrite placements and sizes after layout | all `use` nodes | Part of the Layout cell of the frame they pin to, after its children are placed |
| Table tracks (`resolve_layout` marks `table.columns` so the grid redraws) | per table | Outputs of the table's Layout cell; a change is reported as a mark (§12.2) |
| `resolve_contexts` (behaviour rules) | when markup changed | Unchanged; not reactive values |
| `rebuild_clusters` (constraint clusters) | all constraints, every transaction | A Clusters cell per composition (§9.4) |
| `resolve_clusters_live`, called by hand from eleven places, and the solve in `post_apply` | every cluster, every call | A Solve cell per cluster, evaluated when an input changed; its result becomes `derived_ops` at commit (§9.4) |

### 9.2 Layout, and structure that depends on it

CSS allows a descendant's style to depend on a container's size only under **size
containment**: the container takes no size from its contents on that axis. Flutter's
`LayoutBuilder` and Compose's `SubcomposeLayout` are the same rule. That makes the
dependency one-way, so it resolves top-down. Three mechanisms, in order of preference:

1. **Fitting is layout's job.** Wrapping, and showing only the children that fit, are
   layout modes that publish a `fits` flag per child. No structure changes.
2. **Containment.** A structural input (a Collection's `each`, `count`, `key`, `when`,
   at any distance) may read a box's size only on an axis where the box is contained:
   its size there does not come from its contents. That holds without declaring
   anything for a sized `use` (`std:plot`'s ticks read their template's width, which is
   the `use`'s) and for a group with a fixed size on that axis; a group that fills or
   hugs must declare `contain`, and then contributes no content size to its parent.
   Reading an uncontained size from a structural input is refused at write time.
3. **Measuring the prototype.** "As many as fit" reads the size of the Collection's
   prototype (§5.3), which exists without any item.

Today one Taffy run lays out each outermost container's whole nested tree, measuring
text inside it (`layout.rs:318`), and a table's tracks are solved together. That stays
the unit, cut only at contained boxes, each of which gets two cells:

- **Allocation:** the outer run treats the box as a leaf of known size and publishes
  that size.
- **Interior:** a run rooted at the box places its children.

Structure inside the box sits between the two in height. Layout reads beneath its own
published placements, as now (`layout.rs:33`); a glide is presentation over the target
(§9.1), which is what other cells read.

**Limit in phase 5: a box that structure reads is contained on both axes.** CSS also
allows containment on one axis with the other hugging (`inline-size`): the width is
allocated, the contents are laid out and structured at that width, and their height
goes back to the outer layout. That is a third step in the middle of the outer run,
which Taffy would reach through a measure callback that may ask at several widths. It
is the next step after phase 5, not part of it.

**Reaching CSS** needs more than this engine: `iron_layout` has only `Auto`, `Px` and
`Fill` for a dimension (`iron_layout/src/lib.rs:68`), so minimum and maximum sizes,
percentages, aspect ratio, wrapping and growth weights are layout-engine work with
their own spec. What this spec provides is the ordering that lets structure follow a
container.

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

**Today** the solver sits beside the graph. `solve::clusters` groups every constraint
in the document after every transaction (`solve.rs:130`) and orders the clusters
against bindings with a sort of its own. `resolve_clusters_live` is called by hand
from eleven places in `lib.rs` (a gesture's move, the playhead, a session change) and
solves every cluster each time. constraint-system.md §6 planned a cluster as a node
in the graph's height order; that was not built, and `graph.rs` knows nothing of
clusters. Here it is built.

**Why a cluster fits.** A constraint has no direction, but a cluster does once its
roles are assigned: it reads its **knowns** (bound and animated leaves, fixed nodes'
transforms, ancestors' transforms, constraint values) and writes its **variables** (the
free transform leaves of its operands). The rule that a binding may feed a cluster or
read from it, not both, is what makes that a function. Three things set it apart from
a binding, and each has a place in this engine:

| Difference | Place |
|---|---|
| Its result is authored data: the saved document is always solved | The commit step writes it as `derived_ops` (§11.2, step 5) |
| A solve starts from the pose before it and moves as little as it can | The pose is state: memory class Integrating (§10.4) |
| Which side moves is chosen per solve (a finger, a transaction's `Set`) | Drivers are Input cells |

**The cells.**

- **A Clusters cell per composition.** Constraints, their operands and the mates that
  connections imply (`plug::implied_mates`) all lie in one composition
  (constraint-system.md §0), so the grouping is per Composition owner. It is a
  Structure cell (§7): it reads the composition's `constraint` nodes and `constrains`
  relations (told by ops, §7.2), each constraint's `enabled`, the anchors and
  connections that imply mates, and which operand leaves are free (a constant slot,
  not bound, not driven by a clip, not on a fixed node). It makes one Solve cell per
  cluster, keyed as now (the parent of the cluster's lowest constraint). Adding a
  constraint regroups one composition. `constraint::validate_all` becomes this cell's
  problems, per composition.
- **A Solve cell per cluster.** Its inputs are the cluster's knowns and its drivers;
  it has one output per variable. Its height is one more than its highest input, so
  bindings that feed it run first and bindings that read a solved leaf run after, in
  the same pass. A binding that reads a variable and feeds the same cluster, and
  clusters that feed each other in a ring, are ordinary cycles (§6.3), reported with
  `ErrorKind::Cycle` and the cluster's name as now. The separate sort in
  `solve::clusters` goes away.
- **A solved layer in what a leaf reads as** (§5.2), between the gesture overlay and
  the binding: a variable reads as its Solve cell's output while that differs from its
  constant. Today these values are written into the overlay with `preview`; a layer of
  their own means a gesture's cancel does not have to tell its previews from the
  solver's.
- **Drivers.** A finger (the grabbed anchor at the cursor, strong priority) and a
  transaction's explicit `Set` on a variable are Input cells that the cluster's Solve
  cell reads. They exist for one gesture or one transaction.

**When it solves.** When an input changed, and only then. This rests on the solver
being at a fixed point: solving again from a converged pose with the same inputs gives
that pose. `resolve_clusters_live` assumes the same today (it solves everything and
writes only what differs), and phase 5 tests it directly: solving every cluster on
every call and solving only dirty ones must agree on every script.

**The pose is state.** The Solve cell keeps its last converged pose as its warm
start. A trial inside a transaction reads that pose and does not replace it; commit
adopts the result, as `record(now)` adopts events (§10.4), so `stabilise()` stays pure
and step 6 of §11.2 restores the pose by restoring the inputs. A solve that does not
converge keeps the previous pose and reports it, as now (constraint-system.md §5.5).

**Unobserved clusters.** Because the answer depends on the pose before it (an elbow
that is up stays up), skipping solves while an input sweeps and solving once on waking
can land on another branch. So a cluster follows D1's rule for integrators: it solves
whenever an input changes, observed or not, unless its author sets `pause` or
`coarse` (decision D11). The saving against today is that a cluster whose inputs are
still costs nothing, where today each of the eleven call sites solves all of them. At
commit every cluster the transaction touched is solved whatever is observed, since a
transaction's acceptance never depends on observation (§11).

**Kept as behaviour:** the three priority tiers, culprits and `Unmet` in errors,
`exact` transactions, `SolverSummary` and what the model is told (causes, not
effects), gestures (`begin_grab`, fingers, release committing one transaction that
reproduces the previewed pose), undo and replay applying recorded `derived_ops`
without solving.

**Later, and why this shape is ready for it.** A live simulation (XPBD,
constraint-system.md §11) is the same cluster with a stepper on presentation time
(§10.3) in place of a solve on change, its positions and velocities in a simulation
layer below the solved one. Its policy when unobserved (`run`, `pause`, `coarse`) is
the one above. The solver's variables are 2D today (`transform2d.x`, `.y`, `.rot`);
ports-3d.md's mates add six-equation frames. Nothing in these cells counts dimensions:
a variable is a leaf and a residual is the solver's business.

## 10. Time

### 10.1 Clocks

A clock is a cell. There are **two root clocks today, and they stay two**:

| Clock | Is | Read by today |
|---|---|---|
| **Presentation time** | the host's wall clock; only moves forward | steps (traces, accumulators), queries, presence, layout glides, machines' own transitions |
| **The playhead** | the session `clock` node: `t`, `playing`, `rate`, loop bounds (timeline.md §4.1). It is scrubbed, looped and paused, so it jumps both ways | clips; machines whose `progress` a clock clip drives (the player takes both, `lib.rs:1940`) |

Steppers stay on presentation time: pausing the timeline does not freeze a pulse.

Formulas have no way to read time today except a reference to the session `clock` node
(`expr.rs` has no time function; timeline.md §11 notes the gap). Two new functions:

- **`now()`** is presentation time. It is what a free-running source reads (the laser's
  pulse phase), and it does not stop when the timeline does.
- **`clock()`** is the **nearest scripted clock**, and the playhead where none is
  provided. A node provides one with a `clock` facet, defined as another clock through
  a mapping (offset, rate, paused, loop). Uses: the narrative clock (position, not
  seconds), a preview at another time, a component in slow motion.

**A model has one time.** A body resolves `clock()` along the model's own tree (its
entity's ancestors), never through a picture, because one entity may be pictured under
several clocks and computes once (§8.4). A clock provided on a picture's side reaches
pictures only: what the body computes is the same in every picture, and each picture
may show it at its own clock. This is the context rule already in force (`@name` may
be read in a template only, reactivity.md §13), applied to time.

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
| **Logged** | `trace` (changes with times); a machine's transitions (start time, starting values, and for `machine.at` the position and direction it came from, `machine.rs:309`); a stateful item's entering and leaving | the **event** is always recorded (§8.3); nothing is sampled | valid within what the log holds |
| **Integrating** | `accumulate`; a host simulation; the network solve's warm start; a constraint cluster's pose (§9.4) | by policy: `run` (default), `pause`, or `coarse` (larger steps) | not valid unless recorded |
| **Presentation** | a pure item's enter and exit; layout glides | not kept; absent on waking (§8.3) | not valid |

**Recording is a step of its own, not part of `stabilise()`.** `stabilise()` is a pure
function of the document, the inputs and the logs. `record(now)` runs after it, at a
commit and at each `advance`: it compares what each logger follows with what it last
recorded and appends events. So a trial inside a transaction records nothing (§11),
and a value that changes and changes back between two recordings leaves no event,
as today, where steppers poll once a frame.

Recording costs when an event happens, not every frame, so a thousand machines off
screen cost nothing between their state changes. An `accumulate` with policy `run` does
cost every frame it moves; a closed form is preferred where one exists (the laser's
pulse phase is `frac(now() · hz)`). Decision D1.

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

Sources are found **by evaluating in the slice**, as the note says ("a trial
evaluation, not a separate analysis"): the evaluation records what it read, including
what a `deref` reached, which provider an `@name` resolved to, and whether a producer
held a leaf. A static check from `Expr::deps` is only a conservative first answer for
refusing an edit (a clip's end bound to an accumulator); it cannot be exact, since a
host may publish onto any leaf.

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

`Store::run` no longer clones the document. This rests on three things. The facts
about today's code in each were checked; what is assumed beyond them is A1 to A3
(§14).

### 11.1 What it rests on

**A document journal, not op inverses.** Op inverses do not put the document back as it
was: three ops change it and then run a check that can fail, returning no inverse
(`op.rs:524`, `:615`, `:690`); a create's inverse is a delete, which leaves a saved
tombstone stamped with a version that never committed (`document.rs:502`,
`serial.rs:45`); id counters advance. So `Document`'s mutators write an exact undo
record (the node, facet or relation as it was, or that it was absent, and the
counters) to a journal while a transaction is open. Rolling back to a mark restores
the document bit for bit, including its indexes. Op inverses remain what undo replays.

**Validation is a function of the document and templates.** The checks before commit
are `check_binding` (existence, `binding_cycles`, the look-only rule
`reads_interaction`), `check_created` and `validate_connection`, `signal_loop`,
`check_scope`, `check_context` and `attachment::validate`. None reads a value. They do
read derived structure, in two kinds:

- *Op-driven structure* (an instance's ports, a picture's parts): a function of the
  document and templates. It is brought up to date for the transaction's ops **before**
  the checks, as `expand` is today, so creating an instance and connecting to its port
  in one transaction works, and so does undoing the deletion of a connected instance
  (`link` accepts a derived end whose instance exists, `op.rs:720`).
- *Value-driven structure* (which items a Collection has): today the checks use the
  items as they were before the transaction (`expand(&mut work, stamp,
  &self.repeats)`), which would make acceptance depend on what is observed once a
  Collection can be dormant. Instead **a derived id inside an item is valid if its
  path is valid in the template, for any key.** A binding that reads `#g/~3/7` is
  accepted whether or not key 3 exists now, and is `Failed` while it does not (§7.4).
  This is more permissive than today, and is an intended difference (§16.1).

The cycle, look-only and signal-loop checks walk templates with item references
abstracted, so they cover owners not yet made. `attachment::validate` compares before
and after and scans every attachment; it works from the journal's touched nodes.

**`stabilise()` is pure; recording and effects wait for commit** (§10.4). A trial
stabilise changes values and structure and nothing else: no event is logged, no
request sent, no `Changes` published.

### 11.2 The steps

1. Check the idempotency key; suspend a running gesture and classify the transaction
   against it, as now. Translate edits aimed at derived nodes (`expand::translate`),
   making current what it reads (§8.9).
2. Open the journal. Apply each op; on an error keep going, to report every error.
   Bring op-driven structure up to date for these ops.
3. Validate (§11.1). On failure: roll the journal back, discard templates compiled in
   step 2. The engine's values were never touched.
4. Tell the engine each op (§7.2), set the transaction's drivers (each explicit `Set`
   of a number on a constant leaf, as `post_apply` collects them now), and
   `stabilise()`. Owners it disposes are **held**, with their state. Clusters cells
   and Solve cells are part of this pass (§9.4), so the solve happens here, once, in
   dependency order.
5. **The solver's commit step.** Every cluster the transaction touched is made
   current whatever is observed. A Clusters cell's problem (a malformed or duplicate
   constraint), a cycle through a cluster, a solve that did not converge, or an
   `exact` transaction with an unmet request fails the transaction with the errors
   `post_apply` gives now. Otherwise each variable whose solved value differs from its
   constant is written to the document as a `derived_op`, through the journal. The
   constant now equals the solved layer, so telling the engine changes no value.
   Until phase 5 this step is `post_apply` as it is, reading through the engine and
   stabilising after each cluster's writes (`lib.rs:2609`).
6. **On any failure in 4 or 5:** roll the journal back to the mark, tell the engine
   the reverse changes, `stabilise()`, re-attach held owners, discard the pending
   `Changes`. Values, structure and solved poses are then what they were, since `stabilise()` is pure;
   stamps are newer, which costs a recomputation that finds nothing changed.
7. **Commit:** close the journal, drop held owners, `record(now)`, adopt each solved
   pose as its cluster's warm start and clear the drivers, release buffered
   effects (a query's request, `on.emit`), publish `Changes`, push history.

### 11.3 Consequences

- `Transaction::expanded` goes away. It is never replayed today; undo re-derives.
  `ops`, `derived_ops` (solver writes) and `inverse` stay.
- Undo, redo and replay take steps 1 to 4 and 7 without the solver, as now, and undo
  still snaps the machines it touches. Ops from a server take the same path.
- **`apply_session`** (`lib.rs:1216`) is the second way the document changes: session
  ops (selection, the clock, the timeline and Library compositions), outside history.
  Today it runs no validation, no expansion and no solver, and rebuilds the whole
  graph on a structural op. It keeps its rules (session-tier ids only, its own version
  counter) and takes steps 2, 4 and 7: journal, tell the engine, commit. No solver
  writes reach it.
- If the journal proves awkward, the fallback is a document of shared, copy-on-write
  chunks, where rollback is keeping the old root. It is not the first choice because
  every read pays for the indirection.

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
    slots: BTreeSet<Leaf>,               // a slot or facet changed with no change of value:
                                         // a binding became an equal constant, a facet was added
    marks: BTreeSet<Leaf>,               // reported as changed on request (a table's tracks, a
                                         // text whose font arrived: `Reactive::mark` today)
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
- `slots` covers what the host reconciles today from `AddFacet`, `RemoveFacet` and
  `Set` ops whatever the value (`mod.rs:646`): editability and render analysis change
  when a binding does, even to an equal value.
- Phase 1 lists, for each field, every consumer it feeds, before the old drains go.
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

## 14. Assumptions not yet verified

Everything else in this spec that states a fact about today's code was checked against
it. These are the claims the design rests on that were **not** checked by running
anything. Each names the phase that settles it and what to do if it is false. If one
fails, stop and report before designing around it (handoff §5).

| # | Assumption | Where | Settled in | If false |
|---|---|---|---|---|
| A1 | Every change to a `Document` goes through its mutators (`insert_node`, `restore_node`, `tombstone`, `set_parent`, `set_order`, `insert_facet`, `remove_facet`, `facet_mut`, `insert_relation`, `remove_relation`, the id counters, `set_version`), so a journal written there restores it bit for bit, indexes included. `facet_mut` hands out `&mut Facet` and must record the facet first | §11.1 | 2a: a random failing transaction leaves the document identical, compared field by field | The copy-on-write document of §11.3 |
| A2 | No validation check reads a resolved value | §11.1 | 2b: the checks compile against a document and templates with no engine in reach | Each check that does is named, and moved after `stabilise()` with the solver's |
| A3 | Cycle, look-only and signal-loop checks can be made over templates with item references abstracted, and accept exactly what today's checks over expanded copies accept, apart from the listed intended differences | §11.1 | 2b: the differential test's failing transactions | Validate against materialised structure for the owners the ops touch |
| A4 | A lazily materialised derived node can be handed out as `&T` from `&Store` (write-once slots per loaded owner) without a lock a reader could contend on | §5.4 | 2c | Materialise a loaded owner in full when it loads |
| A5 | The one pass with structure inside it gives the old engine's results on real templates. The kernel (`src/iron_engine_kernel`) shows the algorithm agrees with recomputation from scratch on a toy language; it has no overrides, exits, `deref` into items, or shared templates | §6, §7 | 2b: the differential test | Report which rule failed; the rules are numbered in the kernel |
| A6 | Solving a cluster from a converged pose with unchanged inputs returns that pose, so solving only dirty clusters equals solving all | §9.4 | 5: both run on every script | Solve every cluster of a composition whenever any of its inputs changed |
| A7 | A machine's tween, a trace's `out` and a stateful item's exit can each be computed from a log of events and the clocks, matching the old steppers' sampled results within the differences §16.1 lists | §9.3, §10.4 | 3 | That stepper stays a per-frame stepper while necessary, and is Integrating |
| A8 | An extent cell reading what `geom::local_bounds` reads is cheap enough to keep necessary for every loaded owner | §8.5 | 4: measured on the demo document and at 1,000 meters | Extents only for owners above a size; smaller ones stay Live |
| A9 | Taffy can lay out a contained box's interior as a separate run that gives the same placements as the single nested run today | §9.2 | 5: `layout` and `table` suites | Containment only at sized `use` boundaries, which are already separate templates |
| A10 | The interned path registry's growth under churned value keys is small enough to leave unreclaimed for now | §5.4, D5 | 0: measured | Reclaiming becomes part of 2c |

Not reviewed by the council (the handoff's pre-build review covers them): §9.4, §8.10,
and parking in §8.3.

## 15. Targets

Hypotheses, not measurements; each is measured before and after with the probes in the
handoff's §6. The meter rows cannot be met by the engine alone: a **constant** edit at
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
| A drag in one cluster with 100 other clusters at rest in the document | to be measured in phase 0 | The cost of the one cluster; no other cluster solved or regrouped |
| Structural edit at 1,000 placed meters (`instance_cost`, `bench-instances.txt`) | 161.7 ms | Under 2 ms |
| Placing one more meter at 1,000 | 79.9 ms | Under 1 ms |
| Memory per placed meter beyond the first, **for a meter nothing reads** (off screen, unloaded) | to be measured in phase 0 | Under a tenth of today's. A visible meter is materialised in full, as today |
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

**Intended differences** are listed in one file,
`src/iron_document/tests/differential/intended.rs`, as data the harness reads: each
entry names the difference, where the comparison is relaxed for it and how narrowly,
and the test of the new behaviour. A difference not in that file fails the harness;
adding an entry is a stop-and-report unless it is one of these:

- the prototype is not evaluated unless observed;
- a derived id inside an item validates by its template path, whatever keys exist
  (§11.1);
- a failed transaction leaves no tombstone and no gap in the id counters;
- an inner picture's definition follows an override of its entity's `instance.def`
  (today the body does, `expand.rs:487`, and the picture does not, `expand.rs:587`);
- the old engine's text comparison of NaN inputs in `settle_repeats`, its 4-round
  machine loop, and its 129-round cut for Collections that read each other;
- from phase 3: events are recorded at each commit and advance, where the old steppers
  sample once a frame (`step.rs:136`).

For phase 2c the reference is phase 2b's engine: the Tree's materialised nodes must
equal the copies it stored.

**A host reference.** The store comparison does not show whether the host updated
correctly from `Changes`. A second harness rebuilds the host's derived state from
scratch at each step (drawables, paint order, the spatial scene) and compares it with
the state kept incrementally. `materialize::reconcile` takes Bevy `Commands` and mesh
assets (`materialize.rs:352`), so this is a test in the app crate over a headless Bevy
`World`, with no window or GPU; phase 0 builds that fixture first.

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
measured and recorded in the roadmap's Status table.

| Phase | Builds | Exit |
|---|---|---|
| **0. Baselines and the harness** | Memory and load-time baselines; a profile of the constant edit at 1,000 meters (§15); solver time per gesture step and per transaction with many clusters at rest; growth of interned id paths. The differential harness and the host reference (§16.1), run old against old: a `differential` feature makes `Store` a facade over two stores, so every existing test is a differential script (handoff §4.1). The script generator | Both harnesses green on old-vs-old across the existing suites' scripts and 10,000 random ones |
| **1. The read contract, on the old engine** | The `Tree` and `take_changes()` (§12) over today's document and graph. Every caller that can meet a derived id uses them, **inside the crate and outside**; `document()` is renamed | No behaviour change: smoke output identical; `take_derived_ops`, `drain_changes`, `take_touched` have no callers |
| **2a. Transactions in place, on the old engine** | The document journal (§11.1). `run` applies in place; `expand::expand` still runs before the checks, its writes journaled and `expansion_problems` deferred to commit; a failure rolls the journal back and, after `post_apply`, rebuilds the graph as now. `attachment::validate` from the journal. Validation is otherwise unchanged | No document clone in `run`; a random failing transaction leaves the document bit-identical, tombstones and counters included; the constant edit at 1,000 meters measured again |
| **2b. The new graph, copies still stored** | Cells, heights, necessity (everything necessary), owners, compiled templates, and §11's steps in full, with validation from templates. Cells evaluate from templates. Owners **write their copies into the document**: op-driven ones in step 2, before the checks; value-driven ones in step 4. Those writes go through the journal, are not transaction ops, bump no version, and stay out of history and sync; they reach the host only as `Changes`. Replaces `graph.rs`, `expand::expand` and `settle_repeats`. The pipeline's producers run in today's order and write Input cells; the solver stays `post_apply` and `resolve_clusters_live`, reading through the engine | The differential test is exact and green; **the plot targets of §15 are met**; the roadmap row "Plot pan and zoom performance" is closed |
| **2c. Copies leave the document** | Derived nodes answered by the Tree from owner, template and diffs, materialised on read (§5.4); paint order and the spatial scene from the Tree | The Tree equals 2b's stored copies on every script; memory per instance measured against phase 0 |
| **3. Time** | Two root clocks, `clock()` and clock providers, the scheduler, `advance` and `Wake`; steppers as scheduled cells; events recorded for traces and machines; memory classes (§10.1–§10.4) | `fiber.txt` and `pulses.txt` pass; an idle document with a trace and an accumulator at rest schedules no wake; a machine's transition matches the old player's, interrupted transitions included |
| **4. Observation** | Observers, Dormant and Unloaded pictures, what stays necessary (§8.3), bounds kept right (§8.5), owner entries in the spatial index, the wake budget, the idle sweep, reads that never load (§8.9) | §8.8's property test is green; a hidden tab's composition of pure pictures costs 0 evaluations; a pulse from an off-screen laser reaches an on-screen meter on time; something moving into the view from outside it appears |
| **5. The pipeline as cells** | Layout cells with declared containment and `fits`; the network solve per algebraic island; clips as cells; the constraint solver as Clusters and Solve cells with a solved layer and a commit step (§9.4); the remaining per-transaction scans measured and, where they dominate, indexed (§9) | A repeat whose count reads a contained box's width adds items during a resize drag; solving only dirty clusters agrees with solving all on every script, and a drag in one cluster solves no other; `resolve_clusters_live` has no callers; `layout`, `table`, `ports`, `schematic`, `constraints`, `gestures` and `handles` suites green; the meter targets of §15 |
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

## 19. Decisions

Settled by the user on 2026-11-15 as written here. A builder who finds one of them
blocking does not change it; that is a stop-and-report (handoff §5).

| # | Decision | Settled as |
|---|---|---|
| D1 | What unobserved state does | **State keeps its history wherever it sits; pure pictures do not** (§8.3, §10.4). An owner holding a session `let`, a trace, an accumulator, a machine or a query has an exact lifecycle and its events recorded. It may sleep, and when at rest it is parked: unloaded with its state kept, and loaded again when an input it reads changes or a timed event is due. Only state whose inputs keep moving stays loaded, and the application may set a simulation range beyond which that is paused. An integrator runs unless its author sets `pause` or `coarse`. A pure owner's enter, exit and glides are simply absent on waking. The cost is that a component with a machine keeps the structure above it current |
| D2 | A Dormant owner's bound | **Kept right**, by an extent cell that is always current while the owner is loaded (§8.5). Free when static or clipped; an unclipped owner with moving geometry keeps that geometry running. Standard components should clip |
| D3 | Problems in owners that are not Live | As of their last evaluation, with the idle sweep refreshing them. Whether a transaction is accepted never depends on this |
| D4 | The words "owner" and "slice" | As in §3; reactivity.md's "scope" is renamed where the two specs meet |
| D5 | Reclaiming interned id paths | Measure in phase 0. Counting references from owners is not enough: relations, history and a selection can hold a derived id after its owner has gone |
| D6 | Feedback from measured layout into structure | Refused where static; a cycle with a problem where found at run time. No deferral to the next frame, which would make the result depend on frame timing |
| D7 | `std:plot` tick labels keyed by value (labels slide and fade) | Yes, as its own change after phase 2b |
| D8 | Session lets when an owner is unloaded | Kept (unloading is not disposal) |
| D9 | Limits | 50,000 loaded derived nodes; also a limit on owners in total, loaded or not, since each keeps a summary |
| D11 | A constraint cluster nothing observes | **Solves whenever an input changes**, as an integrator runs (D1), because its pose depends on the pose before it. A cluster at rest costs nothing. `pause` and `coarse` are for the author to set, on the composition; they are named here and built with the live simulation, not before |
| D10 | Structure per slice | Not built: phase 6 is value slices, and a time whose structure differs is seen by moving the live clock. The full form is left open until something needs two structures at once (a side-by-side "before and after", say) |

## 20. History

How this design was reached is in
[`reactive-engine-review.md`](reactive-engine-review.md): the two council rounds with
every finding and what changed, the first handoff's open questions and their answers,
and the changes made after the council. Nothing in it is needed to build from this
spec.
