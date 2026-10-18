# The component model

**Status:** Approved 2026-10-18. Phases 0–4 done 2026-10-18 (with anchors, §4.2; plugging and mates, §4.3; nested overrides, §5.3); phases 5–6 open.
**Scope:** Reusable components: what a component is, what the document stores for a
placed one, how its insides are derived, its interface, overrides, scope, libraries,
the views a model reads, and the render analysis the component boundary makes possible.
**Amends:** [`entity-composition-model.md`](entity-composition-model.md) (ECM) §4.2–4.3
(definitions and instantiation), §6.1 (parameters become input ports), §9
(encapsulation). Where this spec and ECM disagree, this spec wins once approved.
**Restores:** [`vision/02`](../vision/02_substrate-model-projection-control.md) §5.3
(`Component { params, local_state, body }`, `Instance { component_id, args }`) and
invariant 5 ("the language model reads and writes the document, never the expanded
runtime state"), which the ECM implementation departed from (§1).
**Feeds:** [`rendering/architecture.md`](../rendering/architecture.md) §4–6 and
[`handoff-render.md`](../handoff-render.md) §4 (chunks and subtree baking), through §11.

This spec came out of a conversation on 2026-10-17/18 that started with how to show
links between things on the canvas, found that ports and one-way bindings are the same
thing at evaluation time, and ended at the question of what a placed component should
store. OpenUSD was studied as prior art (§12).

---

## 0. Decisions at a glance

| Question | Decision | Why |
|---|---|---|
| What a Component is | The UI name for a **definition**: an interface, behaviour, and one template per notation | Users will mostly place pre-made components from a library |
| Kinds of component | **Multi-context** (templates in several notations, one entity shared by every picture) and **single-context** (one notation, such as a ground symbol); different icons, one mechanism | A cartoon voltmeter and its 3D model must read one voltage; a schematic ground has no single 3D counterpart |
| Embeds | **Not** components. An embed shows one shared composition elsewhere | A component is a type instantiated many times with separate state; an embed is a window onto one state |
| What the document stores for a placed component | The instance, its arguments, its overrides and the connections to its ports. **Not** its insides | Two lasers are two lines of document, not two copies of a cartoon (vision 02 §5.3) |
| The three layers | **Authored** (stored), **expanded** (derived from definition + arguments + overrides, never stored), and **views** of either | The authored/derived split already governs values (vision 04 §4.1); this extends it to structure |
| Addressing expanded nodes | An **expanded id tier** like the session tier: ordinary `NodeId`s in their own range, never saved, mapped to `(instance, definition-local id)`; shown as `#m/needle` | Every consumer already reads nodes through `store.document()`; `NodeId` stays a `Copy` u64 |
| The interface | **Ports only**, in three kinds: `in` (a value with a default), `out` (derived), and `pair` (an across/through terminal). `param_spec` merges into `port_spec` | One kind of interface member; any setting can later be wired; "expose" is one gesture |
| Raising scope | Give the outer definition a port and connect it to the inner port inside the definition | The UsdShade NodeGraph and Simulink subsystem rule; replaces ECM §9's `exposes` relation |
| Bindings vs ports | Bindings are private wiring inside one scope. Only ports cross a scope boundary | Encapsulation is what makes components replaceable and analysable |
| Instances | **Linked** by default (expanded from the definition); **detach** writes the expansion as authored nodes | Library updates reach linked instances; detach is today's copy |
| Per-instance changes | **Overrides** on the instance, keyed by definition-local id, not by name | Figma's model; Iron's ids are stable, so renames never break an override |
| An override whose target disappears | Kept, inert, and listed as stale | USD's behaviour: nothing breaks, nothing is silently deleted |
| Behaviour | Evaluated from the definition, never copied into an instance | ECM §4.3 said this; the code copies it (§1) |
| Libraries | Imports (source, pinned version, cached copy) and local definitions; **make local** vendors an import | Answers vision 09 §13.3; offline-safe |
| What a model reads | Authored state by default; `expand` in a view query shows an instance's insides, marked derived | Debugging a component should not pollute every read |
| Render analysis | Every template part is classed per channel as definition-static, instance-static or dynamic, from its static dependencies | Bake and share surfaces by knowledge rather than by watching for K quiet frames |
| USD | Borrow its semantics; keep Iron's document; USD as an interchange format at the boundary later | USD has the structural half and none of the reactive half (§12) |
| `Component` in code | Renamed **`Facet`** (`ComponentKind` → `FacetKind`, …) | "Component" becomes a user-facing word |

---

## 1. Where the code is today

Facts this spec changes:

- **Placing a figure copies the template.** `definition::instantiate` and
  `place_figures` ([`definition.rs`](../../src/iron_document/src/definition.rs)) emit
  `Op::Create` for every template node, rewriting references to a `param_spec`'s
  default into references to the entity's `params`, and link each copied root to the
  entity with `figures`. The header says so: "an instance is plain nodes and plain
  bindings: nothing downstream knows about definitions." Editing a definition updates
  no existing instance.
- **Behaviour is copied too.** `instantiate` copies every `port_spec` (as a `port`) and
  every `equation` onto the entity, with their bindings rewritten. ECM §4.3 says
  behaviour "is evaluated per entity from the definition, so editing a definition
  updates every instance." That is the second departure.
- **Two kinds of interface member.** Parameters are a dynamic `params` facet on the
  entity (`component.rs`, `@dynamic`), validated against `param_spec`s. Ports are child
  `port` nodes with `kind`, `dir`, `signal`, `across`, `through`. Parameters arrived in
  ECM phase 1 and ports in phase 3; nothing decided to keep them apart.
- **An unwired in-port already holds a value.** `port.signal` is a constant
  (default 0) until a connection derives it. That constant is where an input's default
  goes (§4).
- **Exposure is a relation in the plan only.** ECM §9 specifies an `exposes` relation
  and `Store::encapsulate`; neither is built.
- **References are global.** An `Expr::Ref` names any `(NodeId, LeafPath)`. Nothing
  enforces a scope; a binding may read the inside of an instance's picture.
- **Relations point at `NodeId`s.** `Relation { from, to }`; nothing can point at a
  node that is not stored.
- **The session tier is the precedent for unsaved nodes.** Ids at or above
  `id::SESSION_BASE` live in the same `Document`, are never saved (`serial.rs`), never
  undone, and session ops are applied through `Store::apply_session`.
- **Every document edit goes through the store.** Transactions through `Store::apply`
  (`run`), gestures through `preview`, `preview_target`, `drag_to` and
  `set_through_handle`, producers through `publish`. The bridge, app and engine API
  build `Op`s directly at about 120 call sites, so any translation of edits must happen
  inside the store at the op level, not at the call sites.
- **Users of definitions:** the `bar` demo, the optics chain (cartoon plus
  schematic figures) and the thermal tab (`document_bridge/demo.rs`), and
  `tests/definitions.rs` and `tests/ports.rs`.
- **The file is schema 5.** `Component` and `ComponentKind` appear about 340 times in
  `src/`; the file's top-level table is `"components"`.

---

## 2. Vocabulary

| Term | Meaning | Stored as |
|---|---|---|
| **Component** | UI name for a definition | `definition` node in `library` |
| **Definition** | Ports, behaviour (bindings and equations), templates per notation, internal connections | subtree under `library` |
| **Template** | A definition's picture in one notation | `template` child of a definition |
| **Entity** | One particular thing in the model, pictured in any number of compositions | `entity` in `model` |
| **Instance** | An entity or a `use` whose `instance.def` names a definition | see §5 |
| **`use`** | A placed picture of a component in one composition (new type) | node in a composition |
| **Figure** | Any visual node with a `figures` relation to an entity, port or connection (unchanged) | relation |
| **Expanded node** | A node derived from a definition for one instance; never stored | expanded tier (§6) |
| **Override** | A per-instance change to an expanded node's leaf | `override` child of the instance |
| **Facet** | Typed data on a node (`transform2d`, `fill`, `port`…); was "component" in code | facet tables |

A **multi-context** component has templates in several notations; each `use` of it
figures one entity, which holds the arguments. A **single-context** component has one
template; its `use` may stand alone, holding its own arguments, with no entity in the
model (vision 09 §7.3's composition-local content). The UI gives them different icons.
Whether a single-context component such as a schematic ground should still get an
entity, so it can raise obligations, is vision 09 §13.6's many-to-many question (§15).

**Embeds stay as they are** (ECM §7.4): one composition, shown by reference wherever it
is embedded, with one state.

---

## 3. The three layers

1. **Authored.** What the file holds and the model edits by default: definitions,
   entities and `use`s with their arguments, overrides, connections, layout of
   top-level things, annotations, lessons.
2. **Expanded.** For every linked instance, its ports (under the entity) and its
   template (under each `use`), built from the definition, the arguments and the
   overrides. Expanded nodes are real nodes in the `Document`, in their own id tier
   (§6): rendering, picking, bounds, the reactive graph and the solver read them like any
   other node. They are never saved, never in undo, never in an authored view.
3. **Views.** Reads of layer 1 by default, or of layer 2 on request (§9).

**Expansion is structural only.** Expanded bindings read the instance's input ports,
so a changed argument flows through the ordinary reactive graph with no re-expansion.
Expansion reruns for one instance when its definition, its overrides or its own
structure change (created, deleted, detached, re-pointed at another definition or
version). A future variant input (§12.2) would add "or a variant input changed".

**Expansion is deterministic** in its inputs, and it reuses the rewrite in today's
`copy_template`: references to the definition's own ports follow to the instance's
ports, references to template nodes follow to their expanded copies.

**Behaviour is shared, values are per instance.** N fibers need N sets of equations,
but not N copies of their expressions: the expanded tier holds each formula once per
definition and evaluates it per instance with that instance's inputs, as a function is
called N times. The solver likewise assembles N clusters from one compiled definition,
whose residuals and sparsity pattern are the same for every instance.

**Expansion runs inside the transaction** that caused it, after the authored ops apply
and before the graph rebuilds, so a transaction that places a component sees its
ports and picture immediately. Expansion produces no ops in history: undoing the
placement deletes the authored instance, and its expansion disappears with it.

---

## 4. The interface: ports

A definition's interface is its `port_spec`s. Each is one of:

| Kind | Holds | When nothing is connected | Examples |
|---|---|---|---|
| `in` | `signal`, a value of the port kind's type and unit | Its **default**, shown as an editable field in the inspector | `range`, `power`, `color`, `voltage in` |
| `out` | `signal`, derived by the definition's behaviour | Still derived | `out`, `reading` |
| `pair` | `across` and `through` | Carries no flow (unchanged) | an electrical pin, a thermal face |

- **Parameters are `in` ports.** A `param_spec` becomes a `port_spec` with `dir = in`,
  kind `value` for plain values or a typed kind, and a default. The entity's `params`
  facet becomes `args`: one slot per `in` port, holding the instance's value, either a
  constant or a binding in the parent scope (`<voltmeter range={#knob.value * 2}>` in
  Svelte terms). A connection to an `in` port compiles to that same binding, as one-way
  connections already do (`port::derived_signals`), and takes the argument's place: the
  argument stays, inert, and the inspector shows the input as driven by its connection.
  (Built this way in phase 1: refusing a set argument would need "set" to differ from
  "default", which a constant slot cannot say.)
- **Outputs** are bindings in the definition over its inputs and internals, including an
  internal leaf (`out = #self/needle.rotation`).
- **Terminals** keep ECM §6.1's equations. They have no default and no field.
- **Kinds and units.** `value` ports carry any value; typed kinds (`optical_power`,
  `rf_voltage`, …) carry their unit, checked at apply time as today. Non-numeric inputs
  (a colour, an enum, a string) use `value` with the default's value kind.

### 4.1 Raising scope

A definition contains instances of other definitions (§5.4). To make an inner port
visible outside, give the outer definition a port and **connect the two inside the
definition**:

| Kind | Inside connection compiles to |
|---|---|
| `in` | the inner input's argument reads the outer input |
| `out` | the outer output reads the inner output |
| `pair` | the outer terminal joins the inner terminal's net |

This is the UsdShade NodeGraph rule (public inputs and outputs connected to internal
nodes) and the Simulink subsystem rule. It replaces ECM §9's `exposes` relation:
encapsulation (§5.5) generates these inside connections.

---

### 4.2 Anchors: where a port sits on a picture (built 2026-10-18)

A port is semantic; where it *is* depends on the picture. A template marks it with an
**`empty`** (a transform that draws nothing, which children can hang off) carrying an
**`anchor`** facet: `port` names the port_spec, `follow` says whether this end is
placed by what it connects to. Expansion carries the anchor's `port` over to the
instance's port.

The graph derives, beside connected ports' signals (`anchor.rs`):

- every anchor's position in its composition, `anchor.wx` / `anchor.wy`: an expression
  over its own and its ancestors' transform leaves (`translate · rotate · scale`, as
  `geom::local_affine`), so a reader can bind to where a port is;
- for a `follow` anchor whose port is connected, its `transform2d.x` / `y`: the
  partner's anchor position in the same composition, taken back into the follower's
  parent space. A follower whose partner also follows stays put, so anchors never loop.

So a fiber's ends sit on the laser and meter it is plugged into, and its curve, bound to
its two anchors, re-draws as either moves. These are ordinary bindings, so dragging a
fiber's end inverts onto what placed its partner: it moves the meter (`handle` now
prefers, among several linear sources, a placed node over a derived one, not the
dragged node's own container, and a leaf of the same kind as the dragged one).

**Empties are also pivots.** The dial meter's needle hangs off an empty at the dial's
centre whose rotation is bound to the reading, rather than giving shapes movable
origins. An empty draws as an editor marker, a small box with a cross (hairline, a
quiet blue; `EMPTY_HALF` = 6 units), which a bare empty is hovered and picked by; an
empty with children is a container whose box includes its marker. Changing how an
empty looks is not built. Anchors exist only in 2D.

### 4.3 Plugging and mates (2026-10-18)

**Ports face a direction.** An anchor's own rotation is the direction the port faces:
its local +x points out of the part, the way a mating connector arrives from. The graph
derives `anchor.wrot` beside `wx`/`wy` (the facing in composition space). The laser's
`out` faces +x; the meter's `in` faces −x (`rot = π`). In 3D an anchor becomes a full
frame (position and orientation; +z out, as in CAD mate connectors).

**What a connection means for geometry** depends on its two anchors, and nothing else is
stored. The connection is the one authored fact; deleting it ends the signal and the
geometry together:

| Ends | Geometry |
|---|---|
| follow ↔ rigid | The follow end sits on the rigid one (§4.2): a fiber's end on a laser |
| rigid ↔ rigid | A **mate**: the two bodies are held port to port, facing each other |
| follow ↔ follow | None: the ends stay where they are |

**A mate is a constraint the solver holds** (constraint-system.md), implied by the
connection rather than stored as a `constraint` node. `solve::clusters` adds one `mate`
term per such connection: the two anchor points coincide and their facings are opposed
(3 equations in 2D). Its operands are the **bodies** the anchors are carried by: each
anchor's outermost authored instance (`NodeId::expanded_root`), because derived nodes are
never solver variables. Solving is flat and authoring is scoped (§8.1): a meter nested in
a component and mated to a laser outside moves the whole component. A connection whose
two anchors share a body (both inside one component) implies nothing; that body is rigid.
With the mate, the bodies are one cluster, and a drag of either moves both (the drag's
finger is strong, the mate required, least change keeps the assembly from turning).

**Plugging is a gesture on the port.** Interaction follows the select tool's scopes:

- A **port is hot** when the pointer is within `PORT_HIT` of its anchor and the anchor
  is one instance level below what a click would select: the ports of top-level
  instances, or of the instances inside an open one. With **Cmd** (the select tool's
  deep-select key, as in Figma) ports at any depth are hot. A hot port lights up with its
  own highlight, distinct from a node's hover outline.
- A **drag that starts on a hot port** connects or disconnects. It never edits the port's
  position inside its instance:
  - From a **follow** end (a fiber's), the end follows the pointer. Released on a
    compatible port it connects; released elsewhere it stays where it was dropped (an
    override of the end's position).
  - From a **rigid** port, the instance the port belongs to follows the pointer by its
    port, keeping its rotation. Near a compatible port it **snaps**: port onto port,
    turned to face it when the target is rigid. Released there it connects, which mates
    when both are rigid.
  - Starting on a connected port disconnects it for the drag. The far end, if it
    follows, stays where it was rather than jumping back to its template position.
    Released back on the same port, nothing changes.
- **Inside the instance's scope** (something in it selected), a port's anchor is an
  ordinary empty: dragging it moves the anchor, as an override.
- A drag **anywhere else** on a mated assembly moves the whole assembly, through the
  solver. Plain drags never make or break mates.
- **Compatible**: the same port kind, opposite one-way directions (out to in) or both
  `pair`, different bodies, and an `in` end not already fed by another connection.
  Candidates are within `SNAP_PX` screen pixels of the dragged port; the nearest wins.
  The rules live in the store (`plug.rs`), so the model surface and the UI share them.
- The release is **one transaction**: the old connection removed (with any far follow
  end pinned in place), the body's or end's new placement, and the new connection. One
  undo step. During the drag, the moved body or end is previewed directly (the store's
  overlay); the solver runs only on release, where it finds the snapped pose already
  satisfies the new mate.

**Toward 3D, and the constraint system.** Snapping and mating are separate on purpose:
the snap is an interaction-time search for a pose (no solver), the mate is a relationship
the solver holds afterwards. In 3D:

- An anchor's frame gives the snapped pose directly: the rigid motion taking one frame
  onto the other with the facing axis flipped. Discrete targets (breadboard holes,
  threaded holes) are snap targets too (constraint-system.md §12, open question U8).
- A port kind (or its anchor) declares its **mate type**, which says which degrees of
  freedom the mate leaves: *fastened* (none; today's only kind), *revolute* (spin about
  the facing axis: a screw in a hole, a rotation mount), *cylindrical* (spin and slide),
  *slider*, *planar*. A mate type is a set of residual rows in the solver. The free
  degrees stay solver variables, so they are dragged like the pendulum's arms. A screw's
  snap keeps the spin angle nearest its current one.
- Mates form a graph across the assembly. A chain is solved as one cluster, which is
  fine at demo scale. A large assembly needs constraint-system.md §5.2's reduction:
  fastened mates in a tree collapse into rigid groups before solving, so only the free
  degrees of mechanisms reach `iron_solve`. A mate that closes a loop which cannot close
  is refused with the solver's culprit message, like any unsatisfiable transaction.
- A mechanism *inside* a component (a revolute joint in its definition) needs solver
  variables on derived nodes. Its solved values then become overrides on the outer
  instance, through §5.3's nested paths. Not built; the path addressing is ready for it.
- Mate features detected on imported CAD (holes, faces, axes) become anchor candidates,
  not ports (§8.1, vision 09 §6).

*Built 2026-10-18,* with these choices:
- **Cmd, not Alt, reaches deep ports.** Cmd is already the select tool's deep-select key
  (Figma's too); Alt during a drag duplicates.
- **Sealed ports are never hot.** Only ports reachable from outside can be connected:
  a top-level instance's, or an inner one its component exposes (the anchor stands for
  the exposing port, so a made component's `out` is hot at its inner fiber's end). An
  inner port that is not exposed stays dark even with Cmd. Connecting to one would need
  "expose this port", which is not built.
- **Two ports on one spot** (a fiber's end on the meter it feeds): the part drawn on
  top is grabbed, as for picking.
- **The rigid drag moves the outermost part.** The part a port drag moves is the
  anchor's outermost authored instance, the same body a mate holds.
- **A mated part holds its rotation when dragged.** Without that, least change traded
  a slight turn of the assembly against moving the far part; the drag's finger now
  also holds the grabbed part's rotation when it is in a mate (`Finger::hold_rot`).
  The committed pose meets the drag's target within the solver's strong-over-weak
  weighting (thousandths of a unit), while the mate itself holds exactly.
- **Not built:** a part with several ports snaps only by the port grabbed; 3D anchors
  and mate types.

## 5. Instances

### 5.1 What is stored

```text
model/
  entity "#m"  instance.def=→voltmeter  instance.mode=linked
               args { range: 10 V }
    override …                         (optional, §5.3)
composition "cartoon"/
  use "#u1"   use.notation="cartoon"   transform2d …   figures → #m
    override …
composition "3d"/
  use "#u2"   use.notation="3d"        transform3d …   figures → #m
connection  connects a=#probe/out  b=#m/in
```

- An **entity** instance holds the arguments; each `use` that figures it picks a
  notation and a placement. Its expanded ports live under the entity; each `use`'s
  expanded template lives under that `use`.
- A **standalone `use`** (single-context, no entity) holds `instance.def`, `args` and
  its expanded ports itself.
- An entity or `use` without `instance.def` is not an instance: a generic `thing`
  from Link, with hand-made authored ports, keeps working as today.

### 5.2 Linked and detached

- **Linked** (default): expanded from the definition, as §3.
- **Detach** (`Store::detach(instance)`): one transaction that writes the current
  expansion as authored nodes with fresh ids (today's `copy_template`), applies the
  overrides into them, removes `instance.def`, and repoints relations that targeted
  expanded nodes. It is undoable like any transaction. Detached instances keep their
  entity and figures relations, so they still read as "a voltmeter" in the model view
  through `instance.origin` (informational, not live).

### 5.3 Overrides

An `override` is a child node of the instance:

| Field | Meaning |
|---|---|
| `override.target` | The definition-local id of the overridden node (§6.2) |
| `override.leaf` | The leaf path, such as `fill.color` |
| `override.value` | A constant |

- Applied during expansion, after the template is copied, in child order.
- Overrides are allowed on template nodes, not on ports: a port's value is its
  argument (§4).
- Structural overrides (add, delete, reparent inside an instance) are not supported.
  The answer is to detach, or to edit the definition. Hiding a part is an override of
  its visibility leaf.
- **Stale overrides.** If the definition no longer has `override.target`, or the node
  no longer has the leaf, the override is kept, applies nothing, and is listed by
  `Store::stale_overrides()`. The inspector shows them with a clear action. Updating a
  definition's pin (§10) reports which overrides it made stale.
- Constant only in this spec. A bound override would be a binding from the parent
  scope into the inside of an instance, which §8 forbids; a need for one is a missing
  `in` port.
- **Nested targets** (built 2026-10-18; open question 6). A part of an instance nested
  inside a component, `#bench/m/needle`, is overridden from the outermost instance, as
  USD writes an `over` at `/bench/m/needle`: the override sits on `#bench` with a
  **path target** `m/needle` (`override.target` holds the path as an id, shown
  `#m/needle`; its steps are definition-local ids, so renames break nothing). The
  bench's definition may also override its inner `m` (an override inside the
  definition, target `needle`). The strongest opinion wins: the outermost instance's,
  then each enclosing definition's from outside in, then the definition's own value.

### 5.4 Nesting

A definition's templates may contain `use`s of other definitions, and its body may
contain inner entities and connections. Expansion recurses. An inner instance's
interface is visible only inside its parent definition; outside, only the parent's ports
are (§4.1, §8). A definition may not contain itself, directly or indirectly; the cycle is
refused at apply time, as for embeds.

### 5.5 Encapsulation

ECM §9's `Store::encapsulate(selection, name)` becomes **make component**: one
transaction that creates a local definition from the selection (a template from the
selected figures, inner instances for the selected entities, a port for every connection
with exactly one end inside, and the inside connections of §4.1), and replaces the
selection with one linked instance of it. ECM §9's laws (associativity, black-boxing,
undo) carry over unchanged as tests.

---

## 6. Identity and addressing

### 6.1 The expanded tier

- Expanded nodes take ids from a new range, `id::EXPANDED_BASE`, between content ids
  and `SESSION_BASE`. They are filtered out of the file and of history exactly as
  session ids are.
- `Store` keeps a table `ExpandedId ↔ ExpandedPath`, where
  `ExpandedPath { instance: NodeId, local: Vec<LocalId> }` (one entry per nesting
  level). An id is kept for the session while its path exists, so selection, hover and
  Bevy provenance survive a re-expansion. Ids are not stable across reloads; nothing
  saved holds them.
- Shown as `#m/needle` (names when unique, else local ids), nested as `#bench/m/needle`.
- Relations held in memory point at expanded ids, so consumers are unchanged. **On
  save**, a relation endpoint in the expanded tier is written as its `ExpandedPath`; on
  load it resolves after expansion. An endpoint whose path no longer resolves (the
  definition dropped the port) is a broken link, shown as relations to tombstones are
  today.

### 6.2 Definition-local ids

A `LocalId` names a node inside its definition. For a local definition it is that
node's own `NodeId` in the `library`, which is stable and never reused. For an imported
definition it is the node's id in the source document, kept in the import (§10). Every
override, saved relation endpoint and render surface key (§11) uses local ids, never
names, so renaming a part breaks nothing.

---

## 7. Editing expanded nodes

Edits aimed at expanded nodes are translated inside the store: in `run` for
transactions and in `preview`/`preview_target` for gestures, which are the only places
edits enter (§1). No call site changes.

| Edit | Becomes |
|---|---|
| Set or drag a leaf that is bound to the instance's inputs | The input's argument, by handle inversion (`handle.rs`). Dragging the needle sets an unwired `in` |
| Set or drag a constant leaf of a linked instance | An override (create or update) |
| Set a port's value | Its argument |
| Create, delete, reparent or reorder an expanded node | Refused: "detach #m, or edit voltmeter" |
| A leaf driven by a connected input, a clip or a producer | Locked, with its reason, as today |
| Session ops (selection, hover) | Applied directly; both tiers are unsaved |

Translated edits are ordinary authored ops in history, so undo works unchanged. A
translated transaction reports what it became, so the model surface can tell an LLM
"set `#m.args.range`" when it asked to change `#m/needle`.

---

## 8. Scope rules

1. **Inside a definition**, bindings may reference only `#self`: the definition's own
   ports and nodes, and the ports of instances inside it. Anything else is refused when
   the definition is validated. USD drops relationships that leave a referenced
   subtree with a warning; Iron refuses them when written.
2. **Outside**, an authored binding may not depend on an expanded leaf except an
   instance's port. A request to bind to `#m/needle.rotation` is refused, naming the
   ports that exist and suggesting an `out` port.
3. **Reading and pointing are allowed.** Annotation `targets`, `highlights`, selection
   and views may name expanded nodes: a lesson may point at the needle.
4. **Nesting** follows rule 2 at every level: something outside the bench reaches the
   voltmeter's `in` only if the bench has a port connected to it.

### 8.1 Constraints

Scope limits what a constraint may **name**, never how far solving reaches.

- **Inside a definition**, a constraint's operands are its own nodes and ports (a
  needle's travel, a linkage inside a mechanism). It is part of the component's
  behaviour, private, and expanded per instance like any other part.
- **Outside**, a constraint may name an instance's ports, and its **exposed anchors**:
  named geometric points or features a template declares per notation (a mounting
  hole, a rail axis). Anchors are the geometric counterpart of ports: vision 09 §6
  keeps mate features out of ports, so they need their own interface. For imported CAD,
  detected features become anchor candidates (vision 09 §6, tier 1).
- **Solving is flat**, as in Modelica: once expanded, a cluster spans every equation its
  unknowns connect, across component boundaries. Encapsulation restricts authoring and
  reading, not the solution.
- **Requirements across the model.** A range or limit on a value elsewhere (a laser's
  output must stay within 2–8 mW) is a constraint in the parent scope naming
  `#laser.out`. Dragging a voltmeter's needle inverts to its `in`, and through the
  wired chain to the laser's power; with the limit in place, the drag must stop at it.
  That needs two things not yet built: **inequality constraints** (`iron_solve`
  projects onto equality rows only), and **chain drags through the solver** whenever a
  constraint touches the chain, instead of linear inversion alone (handoff-product §4,
  "pins on solved values"). Both are general CS work; this spec only requires that
  they respect §8's naming rules.

---

## 9. Views

- `Tree` and `Model` show authored state only. An instance reads as one element with
  its arguments, port connections and overrides:

  ```xml
  <entity id="#m" def="voltmeter" range="10 V">
    <port name="in" from="#probe/out" signal="3.2 V"/>
    <override target="needle" leaf="fill.color" value="#c22"/>
    <figures compositions="#c1 #c2"/>
  </entity>
  ```
- `Composition(id)` shows `use`s as one element each, with placement and overrides.
- `ViewQuery` gains `expand: Vec<NodeId>`: those instances' expanded subtrees,
  marked `derived="true"`, with compound ids. For debugging a component; never the
  default.
- `definition` views show a component's ports, behaviour and templates, which is how a
  model learns what a component does without expanding an instance.
- Edits arriving through the model surface are translated by §7; the reply names what
  each became.

---

## 10. Libraries

The `library` root holds two kinds of definition:

- **Local**: a subtree, as today.
- **Import**: `import.source` (a library URL or project path), `import.version` (a
  pinned version or content hash), and a cached copy of the definition subtree, kept so
  the project opens offline. Cached nodes keep their source local ids (§6.2); how they
  sit in the project's id space without colliding is part of this phase's design.

Verbs:

- **Update** an import: one undoable transaction changing the pin and the cache; its
  result lists the overrides it made stale.
- **Make local** (vendor): turn an import into a local definition. Instances stay
  linked, now to the local copy.
- **Detach** (§5.2): flatten one instance.

This waits on the project container (handoff-product §4, clipboard-and-assets.md §6),
which decides where cached imports and shared libraries live.

---

## 11. Render analysis

The component boundary makes caching decidable. Dependencies are static
(`Expr::deps`, graph.rs), so for every definition:

1. For each template node and each **channel** (shape, placement, effects), compute
   the set of the definition's inputs it depends on, transitively through internal
   bindings.
2. Per instance, combine with which inputs are **live** there: connected, bound in the
   parent, or driven by a clip or producer.

| Class | Depends on | Surface | Shared by |
|---|---|---|---|
| **Definition-static** | Nothing | Keyed by (definition, version, run index, scale bucket) | Every instance |
| **Instance-static** | Inputs that are constant here | Keyed by (definition, version, part, **the values of the inputs it depends on**); rebaked when one of those values changes | Every instance whose relevant inputs are equal |
| **Dynamic** | A live input | Drawn live | None |

The instance-static key holds input *values*, not the instance id. A thousand fibers of
three lengths share three surfaces for each length-dependent part, and one surface for
each part that does not depend on length at all.

**Raster sharing is what makes N identical instances cheap.** `vello_gpu` builds strips
in device space (architecture.md §3), so sharing a part's *vector* description between
instances at different positions saves path building and memory but not flattening and
strip generation, which run once per placement. Sharing one baked surface, drawn N
times as a texture, does save it, within the scale-bucket quality envelope.

- **Shape and placement are separate**: the needle's shape is definition-static and
  its rotation dynamic, so it is baked once and moved ("move a baked group",
  architecture.md §6).
- **Painter order splits static content into runs**: dial face (static), needle
  (dynamic), glass (static) is two surfaces with the live needle between. The analysis
  reports the runs.
- **An override** moves the part it touches to instance-static.
- **3D**: the same classes choose static batching and merged meshes (vision 10 §5.3).
- **Explaining cost**: the class carries its reason ("needle depends on `in`"), which is
  the cost guidance vision 10 §5.2 asks the editor to show.

The analysis is produced by `iron_document` per definition and per instance, and
handed to the render plan as partitions with their classes and keys. The render track
owns how they are drawn (handoff-render §4).

---

## 12. Relation to USD

### 12.1 What maps

USD was designed for film pipelines: many artists editing one shot non-destructively,
assets referenced thousands of times. Its composition, overrides, instancing and
encapsulation solve the structural half of this spec. It has no reactive half: no value
expressions, no dataflow evaluation, no solver, no two-way editing.

| Iron | USD |
|---|---|
| Authored layer | Layers holding opinions |
| Expanded tier | The composed stage; instance proxies |
| Definition | A referenced asset |
| Linked instance | A referencing prim, instanceable |
| Override | An `over` opinion |
| Stale override | An `over` with no defining opinion: kept, skipped by traversal |
| Ports, §4.1 | UsdShade NodeGraph interface inputs and outputs |
| Scope rule 1 (§8) | Relationship encapsulation |
| Detach | Flatten |
| Definition-static surface sharing | Prototypes shared by instancing key |

**Where Iron differs:** USD addresses by name paths, so renames break overrides; Iron
keys by stable id (§6.2). USD forbids edits under an instance; Iron allows overrides,
as Figma does (§5.3).

### 12.2 Variant sets (planned extension)

A USD variant set offers alternative structures for one prim. In Iron that is an `enum`
`in` port that selects among a definition's alternatives: a component's "cartoon /
simplified / exploded" picture, or a part's sizes. It fits vision 02's rule that
choosing among predefined options is a leaf. It makes expansion depend on an input value,
so it is deferred until the base model is built.

### 12.3 Interchange

USD becomes an import and export format at the boundary, as SVG is today, when 3D
components are worked on. An imported asset becomes a definition: prim paths seed its
local ids (the importer mints stable ids and matches by path on reimport), variant sets
become enum inputs, and ports and behaviour are added on top (vision 09 §6's two-tier
tagging). Candidates: the pure-Rust `openusd` crate (reads usda, usdc and usdz, has a
composed stage; pre-1.0), and `lunco-usd-bevy` as a reference for mapping prims to Bevy.
Not part of this spec's phases.

---

## 13. Migration (schema 6)

- `param_spec` → `port_spec` with `dir = in` and the default; `params` → `args`.
- Existing instances become **detached** instances: their copied figures and ports are
  already authored nodes, so nothing visible changes. `instance.def` moves to
  `instance.origin`.
- The demos (`bar`, optics, thermal) are rebuilt with linked instances; the smoke
  script's expected output changes accordingly.
- The file's `"components"` table becomes `"facets"`; files of schema 5 and older load.

---

## 14. Phases and exit criteria

**Phase 0 — Rename `Component` to `Facet`.** Mechanical, its own commit.
*Exit:* tests and the smoke script pass unchanged.
*Done 2026-10-18.* Rust names only: serde renames keep `components`, `add_comp`,
`remove_comp` and the `component_*` error kinds on the wire and in the file, and string
literals (messages) are unchanged. Those move to the new names with schema 6 (phase 1),
when the model surface's manifest changes anyway.

**Phase 1 — One interface.** `in` ports with defaults replace parameters; `args`;
schema 6 migration; the inspector shows unwired inputs as fields.
*Exit:* the demos run unchanged; the laser's power is an `in` port edited from its field
and wireable from a slider; schema 5 files load.
*Done 2026-10-18.* `param_spec` is gone: an input is a `port_spec` with `dir = in` of a
one-way kind, its default in `port.signal` (`definition::inputs_of`). The entity's
`params` facet is `args`. Instantiation still copies every port (phase 2 stops that); a
copied `in` port's `port.signal` is bound to `#entity.args.<name>`, and a connection's
derived binding replaces it in the graph. Handle inversion and inlining now read the
graph's binding before the stored one, so a connected input inverts through its
connection rather than its argument. Schema 6 (`serial::migrate_inputs`) turns
`param_spec` into `in` port_specs, `params` into `args`, rewrites `.param_spec.default`
and `.params.` references, and renames the file's `components` table to `facets`;
entities from older files have no `in` port nodes and keep working through their
arguments. The inspector's Component section (`inspect_component`) lists the inputs of
the instance a selection is or pictures, edits unwired ones and locks wired ones with
their source. The demo's ids after the bar shift (each instance mints its `in` ports);
the smoke script's hard-coded ids follow. A follow-up commit moved the model-facing
names too: ops are `add_facet { facet }`, `remove_facet` and `create { facets }`, error
kinds `facet_*`, `document_schema()` returns `facets`, and the manifest says "facet", so
to a model "component" now means only a definition. The inspector's internal JSON keeps
its `components` key.

**Phase 2 — The expanded tier and linked instances.** `EXPANDED_BASE`, the path table,
`use`, expansion in the transaction, behaviour evaluated from the definition, saved
relation endpoints as paths, `expand` in views, `detach`. The demos become linked.
*Exit:* editing the voltmeter definition's template updates both placed voltmeters in
the same transaction; the model view shows each as one element; saving writes no
expanded node; undo of a placement removes its expansion; the optics chain's meter
reading is unchanged.
*Done 2026-10-18, with these deviations from §6:*
- **Expanded ids are computed, not tabled.** `NodeId::expanded(owner, local)` packs the
  instance and the definition node into one u64 (top bit set; 31 + 32 bits), shown and
  parsed as `#owner/local` (`#3B/1A`), and accepted in expressions. So ids are stable
  across re-expansion *and reloads*, saved relations and bindings simply name them, and
  no path table or save-time translation exists. Names in displayed ids, and nesting
  deeper than one level (an expanded id cannot be an owner), are left for phase 4.
- **Expansion** (`expand.rs`) computes every linked instance's wanted nodes and
  reconciles the document through the op applier inside `Store::run`, after the
  authored ops and before the end-of-transaction checks; it runs when a transaction is
  structural or touches an instance, a `use` or a library node. Its ops travel in
  `Transaction::expanded` (followed by the bridge and geometry revisions, never undone
  or replayed). Load expands in `Store::with_document`. Authored ops on expanded nodes
  are refused, naming the instance, until phase 3 translates them; a link *to* one
  needs only its instance to be live.
- **`instance.mode`** (`linked` | `detached`) says whether an entity derives; schema 7
  makes every older entity `detached`. `definition::instantiate` makes linked
  instances (an entity plus a `use`); `instantiate_static` / `place_static_figures`
  keep the copying path as "load as static asset". `detach_ops` (inspector button,
  `document_detach`) turns a linked instance static, re-pointing relations and
  bindings that named its derived nodes.
- **Copy and paste** leave derived nodes out of a fragment and map a reference to one
  through its instance, so a copied chain keeps its connections.
- **The existing demo scenes stay static** for now: the smoke script exercises binding
  inversion, Link, shape conversion and gesture ownership on nodes inside them, which
  needs phase 3's edit translation once they are linked. The new **Components** tab
  places the optics definitions as linked instances (source, fiber, meter wired port
  to port, a power slider, a spare laser and two loose fibers), checked by the smoke
  script's Components section.
- Not done: sharing one expression per definition across instances (§3, "Behaviour is
  shared"); each expanded node still holds its own rewritten copy. Model-view noise: an
  `in` port that a connection feeds still shows its inert argument (`in="0 W"`).

**Phase 3 — Overrides, edit translation and scope.** `override`, stale overrides, §7's
translation in `run` and `preview`, §8's validation.
*Exit:* dragging the needle of an unwired voltmeter sets `in`; recolouring one instance's
needle survives an edit to the definition; deleting the needle from the definition
leaves a stale override listed by `stale_overrides()`; binding a text to
`#m/needle.rotation` is refused with a message naming the ports.
*Done 2026-10-18.* `override` nodes (`target`, `leaf`, constant `value`) under an
entity or `use`, applied by expansion, adding a facet the node type admits;
`Store::stale_overrides()` lists inert ones with a reason. `expand::translate` runs in
`Store::run` before any op applies, so gestures, the inspector and the model surface all
get it: a constant set on a derived leaf that inverts (`handle`) becomes a set on its
source (an argument, a slider, held to its bounds), a constant set on a leaf of its own
becomes an override (created, or updated when one already targets that leaf), a
binding set on one is refused. `expand::check_scope` adds `out_of_scope` errors for
§8's rules 1 and 2. The exits run natively (`tests/definitions.rs`) and in the
Components tab, where a `dial meter` definition (dial, needle, `fmt` readout) replaces
the optics meter: setting its needle to full scale drives the chain back to the
slider, which stops at 20 mW. Not done: structural edits inside an instance stay
refused (detach first); the inspector does not yet list an instance's overrides or
stale ones.

**Phase 4 — Make component.** §5.5, replacing ECM phase 4's `encapsulate`.
*Exit:* ECM §9's three law tests pass, with the laser and fiber made into a "source"
component and the meter reading unchanged.
*Done 2026-10-18.* `make::make` / `Store::make_component` (a Make component button in the
inspector; `document_make_component`). The new definition holds the selected instances
as inner entities, still linked to their own definitions, their pictures as template
`use`s, and connections wholly inside; a crossing connection gets a port (`out`: its
signal reads the inner port; `in`: the inner argument reads it; a pair: a connection
inside), an argument bound outside becomes an `in` port whose binding the outer
instance takes, and every new port has an **`exposes`** relation to the inner port, so
anchors follow through component boundaries. Fresh ids are renumbered in creation
order. Expansion recurses (§6 ids are now registry paths, so they nest): an instance
derives its definition's inner entities (`#S/E`), their ports (`#S/E/out`), inner
connections and `exposes` as derived relations (`RelationId::expanded`), and nested
pictures; a transaction's inverse leaves derived nodes and relations to re-expansion.
The model view lists authored instances only (not a definition's inner entities, nor
derived ones unless expanded). Tested natively (`tests/ports.rs`: black-boxing, the
slider still driving through `power`, an edit to the laser definition reaching the
laser inside the source, save/load with the same nested ids, undo) and by the smoke
script's example 2. The associativity law test is not written. Not yet: detaching a nested component
(refused) and make component from plain shapes. Overriding a part of a nested instance
was refused here and is built since (§5.3, "Nested targets").

**Phase 5 — Libraries.** §10. Waits on the project container.
*Exit:* a project opens offline with an imported component; updating its pin is one
undoable transaction listing the overrides it made stale; make local keeps instances
linked.

**Phase 6 — Render analysis.** §11, consumed by the render track's chunk and bake work.
*Exit:* two voltmeters share one dial surface; driving `in` from a slider rebakes
nothing and redraws only the needle.

Phases 0–4 depend on nothing outside this spec and repair both of §1's departures.
Phase 6 depends on phase 2 only, so it can run alongside 3–5.

---

## 15. Open questions

1. **Does a single-context semantic component get an entity?** A schematic ground has
   meaning (it raises obligations, vision 09 §8) but no one 3D counterpart. Tied to
   vision 09 §13.6.
2. **Local state.** Vision 02 §5.3 gives components `local_state` (a toggle, an open
   panel). Is it an `in` port the component writes itself, session state, or a new kind?
3. **Import id space.** How cached import nodes avoid colliding with project ids (§10).
4. **Prototype sharing at node level.** USD shares one scenegraph per instancing key;
   this spec expands every instance. Revisit if instance counts make memory or
   expansion time show up in measurement.
5. **Definition editing in place.** Editing a definition from one of its instances
   ("edit main component" in Figma) needs a UI and decides which instance is shown while
   editing.
6. ~~**Overrides on nested instances.**~~ Decided 2026-10-18: both, the outermost
   instance's opinion strongest (§5.3, "Nested targets").
7. **Anchors.** How a template declares exposed anchors (§8.1), and whether ports may
   imply anchors (ECM's "a port kind declares the mate it implies").
8. **Inequality constraints and limits on signals** (§8.1): owned by
   `constraint-system.md`, needed for requirement-style limits across components.
