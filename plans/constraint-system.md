# Constraints and interaction-time causality

**Status:** Proposed — implementation plan with open questions (§18)
**Date:** 2026-10-08
**Scope:** How to add a constraint system to `iron_document` and the Bevy bridge so that
a human can grab any part of a mechanism and the rest responds, while the document stays
the single authority, undo stays exact, and the language model stays informed without
being flooded. Ends in a demo: an inverse-kinematics pendulum you can drag by the ball
or by the platform.
**Builds on:** [`document-spine.md`](document-spine.md) (store, ops, gestures, bindings).
**Fits into:** [`roadmap.md`](roadmap.md) stages C2, D, and I;
[`vision/09_entities-compositions-and-ports.md`](vision/09_entities-compositions-and-ports.md)
§8. Constraints are for setting up and placing things inside one composition; physics
flows through signals and models. Connections between ports may imply constraints
([`entity-composition-model.md`](entity-composition-model.md) phase 7).
**Amends:** `document-spine.md` §5.10 ("a bound leaf has no drag handle") and vision
[`02`](vision/02_substrate-model-projection-control.md) §8.2 (writeback policy) — see §17.
**Prior art read for this plan:** Inkling's `Constraints.ts`, SolveSpace's `system.cpp`,
the `ezpz` and `arael-sketch` crate docs, Blender's XPBD solver design notes (§19).

---

## 0. Decisions at a glance

| Question | Decision |
|---|---|
| What a constraint is | A relationship that holds in every direction. Which side moves is decided per solve, not when it is authored |
| How constraints are stored | A `constraint` node with a `constraint` component (kind, value, enabled), plus `constrains` relations to its operand nodes |
| What the solver moves | Free transform leaves (`transform2d.x/y/rot`) of operand nodes. Bound leaves and fixed nodes are inputs, never moved |
| Constraint dimensions | `constraint.value` is an ordinary slot, so a distance or angle can be bound to a slider: a driving dimension |
| Where solving runs | A new pure crate `iron_solve` (no Bevy, no `iron_document`), driven by an adapter in `iron_document` |
| Numerical method | Damped least squares (Levenberg–Marquardt), warm-started from the last pose, with three priority tiers: required, strong, weak |
| How a drag works | The cursor is a temporary *strong* constraint on the grabbed point. The solver writes results through `Store::preview`; release commits one transaction |
| Is the saved document always solved | Yes. Every transaction is followed by a solve of the clusters it touched; the solver's writes are recorded in the same transaction as `derived_ops` |
| Bindings vs constraints | Both stay. A solved cluster is one node in the reactive graph's height order; a binding may feed a cluster or read from it, not both |
| What the LLM is told | Causes, not effects: who drove what, and which clusters re-solved. Values the solver inferred are available on demand, never pushed (§9) |
| First proof | A 2D pendulum: drag the ball (arms follow), drag the platform with the ball fixed (arms bend), arm length bound to a slider |
| Scope | Composition-local: constraints and mate features live in one composition and never appear in others or in the model tree |
| Later | Live simulation with grab (XPBD), 3D assembly mates, constraint-backed paths, constraints implied by port connections |

---

## 1. Why, in one paragraph

Bindings are one-way: `x = f(y)`. They are what an LLM writes well and what a machine
evaluates cheaply, and they stay. But a human's instinct is to grab the thing they see and
pull. A mechanism's parts are related in every direction: drag the ball and the arms
follow; drag the platform with the ball pinned and the arms bend. Today `iron_document`
answers every such drag with "that value is derived; snapping back". This plan adds the
second mechanism: relationships declared once, solved in whatever direction the hand
chooses at the moment of the grab. The LLM authors relationships; the hand assigns
causality.

---

## 2. Vocabulary

- **Constraint** — a declared relationship between operand nodes (coincident, distance,
  angle, fix, pin, …). It has no direction.
- **Operand** — a node a constraint refers to, plus an **anchor**: a point in that node's
  local coordinates (the end of an arm, the centre of a ball).
- **Variable** — a leaf the solver may change. Initially only `transform2d.x`, `.y`,
  `.rot` of operand nodes.
- **Known** — a leaf the solver reads but may not change: bound leaves, leaves of fixed
  nodes, constraint values, ancestors' transforms.
- **Cluster** — a set of constraints connected through shared variables. Clusters solve
  independently (Inkling's term; physics engines call these islands).
- **Driver** — whatever pushes on a cluster for one solve: a cursor, a transaction's
  explicit `Set`, a bound input that changed, later a simulation. Drivers are never
  stored as constraints.
- **Finger** — the driver for a drag: a temporary constraint "this anchor is at the
  cursor", at strong priority. The name and idea come from Inkling.
- **Inferred value** — a variable the solver moved because of a driver, not because
  anyone asked for that leaf by name. The arms of a dragged pendulum are inferred.
- **Derived ops** — the `Set` ops the solver produced while applying a transaction.

---

## 3. The core model: roles are assigned per solve

Every leaf that participates in a constraint is still an authored `Slot::Const`. What
changes is that, for each solve, the leaf gets a **role**:

| Role | Decided by | Solver may move it | Example |
|---|---|---|---|
| Fixed | a `fix` constraint on its node | No | the pendulum platform, when fixed |
| Bound | the slot is `Slot::Bound` | No — read through the reactive graph | `x = #slider.value * 300` |
| Animated | a clip drives the leaf at the current playhead (`time.rs`) | No — read from the animated layer | a clip sliding the platform |
| Driven | the current driver (finger, explicit `Set`) | Pulled toward the driver's target at strong priority | the grabbed ball |
| Free | everything else in the cluster | Yes, as little as possible | the arms |

The same document gives different roles in different moments: grab the ball and it is
driven while the arms are free; mark the ball fixed and grab the platform, and now the
platform is driven and the arms are free. Nothing is restructured, which is the property
the user asked for ("instant switching from N dependent variables to one dragged variable
and N−1 dependent variables").

Priorities (from constraint hierarchies, the Cassowary/DeltaBlue lineage):

1. **Required** — the constraints themselves. Must hold within tolerance after a
   successful solve.
2. **Strong** — drivers. Satisfied as closely as the required constraints allow. If the
   cursor is out of reach, the ball stops at full extension under it: the "rubber band".
3. **Weak** — least change: every free variable prefers its previous value. This is what
   makes motion continuous and stops elbows flipping between solutions.

---

## 4. Document changes (`iron_document`)

### 4.1 Constraint nodes

A new node type in `registry.rs`:

```rust
TypeSpec {
    name: "constraint",
    components: &[(K::Constraint, true), (K::Name, false)],
    default_host: Host::Canvas,      // never materialised as content; see §8.4
    children: ChildPolicy::None,
}
```

A new component in `component.rs`:

```rust
Constraint / "constraint" => Constraint {
    kind:    Enum   = Value::Enum("coincident".to_owned()),
    value:   Number = Value::Number(0.0),   // distance or angle; ignored by some kinds
    enabled: Bool   = Value::Bool(true),
}
```

Because `value` is a slot, `constraint.value` can be `Slot::Bound("#slider.slider.value * 200")`.
That is a driving dimension, the CAD concept, with no new machinery.

**Operands are relations,** using a new `RelKind::Constrains`, from the constraint node to
each operand node. The relation's `data` carries `role` (`"a"`, `"b"`, …) and `anchor`
(a `Vec2` in the operand's local space; `[0, 0]` by default).

Why relations rather than `Ref` fields on the component:

- Relations already validate that both ends are live on `Link`, have inverses, and are
  indexed both ways. `relations_to(node)` answers "which constraints touch this node"
  with no new index, which is also the "why is this moving?" query.
- Constraint kinds have different numbers of operands; a relation per operand handles
  that without optional fields.
- Operands are stored exactly once. Inkling's main lesson was that duplicated state
  makes solvers "floaty"; Ref fields plus relations would have duplicated it.

Where constraint nodes live in the tree: as children of the group that owns the
mechanism, by convention. Deleting the group tombstones its constraints with it, views
elide them with the group, and undo restores both together.

**Tombstones.** A constraint whose operand is tombstoned becomes inactive (reported in
views, not an error). Undoing the delete restores the operand with the same id, and the
constraint becomes active again. This falls out of the existing tombstone design.

### 4.2 Constraint kinds for the first slice

Let `p(n, anchor)` be the anchor's world position under node `n`'s resolved world
transform. Residuals:

| Kind | Operands | Residual (= 0) | Equations |
|---|---|---|---|
| `coincident` | a, b | `p(a) − p(b)` | 2 |
| `distance` | a, b | `‖p(a) − p(b)‖ − value` | 1 |
| `pin` | a | `p(a) − data.at` (a world point) | 2 |
| `fix` | a | none: removes `a`'s variables from the problem | 0 |
| `angle` | a, b | `wrap(rot_w(b) − rot_w(a) − value)` | 1 |
| `horizontal` / `vertical` | a, b | `p(a).y − p(b).y` / `p(a).x − p(b).x` | 1 |

A hinge is `coincident` (rotation stays free). `fix` is implemented by moving the node's
leaves to *known*, not as an equation, which is cheaper and better conditioned
(SolveSpace and Inkling both treat locks this way).

The kind vocabulary is code, like the archetypes of vision doc 02 §9.6. A
`ConstraintSpec` table beside `TypeSpec` declares, per kind, its operand roles, whether it
uses `value`, and its units, so validation, views, and the model manifest read one source.

### 4.3 World transforms inside the store

The solver needs world positions of anchors on nodes nested in groups. Today only Bevy
computes world transforms (`ChildOf` + `GlobalTransform`). Add a small `geom` module to
`iron_document`:

- `Affine2` (6 floats) and composition `T · R · S`, matching Bevy's
  `Transform::compute_affine` order exactly.
- `world_affine(doc, reader, id)`: walks the parent chain, reading resolved values
  through the same reader the reactive graph uses, so previews and bindings are honoured.

Only operand nodes' own leaves are variables. Ancestors are known for the solve:
dragging an arm never moves the group it sits in (open question U7).

A bridge test asserts that `world_affine` equals Bevy's `GlobalTransform` for random
trees, so the two never drift.

### 4.4 Floor functions

`expr.rs` gains `sin cos tan atan2 sqrt pow hypot` and the constant `pi`. Forward
kinematics as plain bindings needs them, and so does any binding that reads a solved
angle. One match arm each, plus round-trip tests.

### 4.5 Validation at apply time

New `ErrorKind`s:

- `BadConstraint` — wrong operand count or roles for the kind, missing anchor, `value`
  of the wrong kind.
- `Unsatisfiable` — the post-transaction solve could not meet the required constraints.
  The error lists the constraints most likely at fault (§5.6).
- `Cycle` (existing) — extended to binding↔cluster cycles (§6).

Duplicates (two `coincident` constraints between the same anchors) are rejected as
`BadConstraint` with "already constrained by #c12". Inkling instead unified duplicates
silently; for an LLM author, an explicit error teaches more.

### 4.6 Serialisation

No format change beyond new type, component, and relation-kind names; the canonical JSON
of `document-spine.md` §7 already covers nodes, components, and relations. The schema
number bumps because old readers would reject the new kind names.

---

## 5. The solver crate (`src/iron_solve`)

A new workspace member with no dependency on Bevy or `iron_document`. It knows only
numbered variables and residual functions. This follows Inkling's finding that
"analysis and reduction techniques are separable from the choice of solver", and it lets
us swap in another solver (for example `ezpz`) behind the same interface.

### 5.1 Problem shape

```rust
pub struct Problem<'a> {
    pub vars: Vec<f64>,          // initial values (warm start)
    pub scale: Vec<f64>,         // characteristic size per var (px for x/y, L for rot)
    pub terms: Vec<Term<'a>>,    // residual blocks
}
pub struct Term<'a> {
    pub tier: Tier,              // Required | Strong | Weak
    pub vars: SmallVec<[usize; 6]>,
    pub residual: &'a dyn Fn(&[f64], &mut [f64]),  // writes r(x)
    pub len: usize,              // number of equations
}
pub struct Solution { pub vars: Vec<f64>, pub status: Status, pub iterations: u32,
                      pub max_required_residual: f64 }
```

The `iron_document` adapter builds a `Problem` per cluster: reads knowns, maps free
leaves to variable indices, and wraps each constraint kind's residual as a closure over
the knowns.

### 5.2 Reduction before solving

In order (the steps Inkling and SolveSpace share):

1. **Knowns.** Fixed nodes, bound leaves, and constraint values are read and removed
   from the variable set.
2. **Propagation.** A `pin` or `coincident` against a fully known point with a free
   point on the other side solves directly. Repeat to a fixpoint (Inkling's
   `computeKnowns`).
3. **Singletons first.** Equations that mention only one remaining variable are solved
   alone (SolveSpace: "this can be a huge speedup").
4. Whatever is left goes to the numerical solve.

### 5.3 Clustering

Union-find over constraints that share a variable. Nodes with no constraints are
singleton clusters, and "solving" one is just setting its position; one code path
serves constrained and unconstrained drags. Clusters are cached and rebuilt on the same
trigger as `Reactive::rebuild` (any structural transaction).

### 5.4 Numerical method

Levenberg–Marquardt on the stacked, weighted residual vector:

- Required rows weighted heavily (start at 10³), strong rows at 1, weak rows
  `(xᵢ − xᵢ_prev) / scaleᵢ` at 10⁻². These are starting points to tune by feel.
- Column scaling by `scale` so a radian and a pixel are comparable. A rotation's scale
  is the node's half-diagonal: turning an arm by Δθ moves its tip by about L·Δθ.
  SolveSpace uses the same column-scaling trick to make dragged parameters move less.
- Jacobians by central finite differences at first (clusters are tiny); forward-mode
  dual numbers later if profiling says so.
- Dense `f64` linear algebra. A small hand-rolled Cholesky or `nalgebra`; sparse only
  if clusters grow past a few hundred variables (open question I4).

### 5.5 Continuity and stability

- **Warm start** from the previous frame's values. Never re-solve a drag from the
  authored pose.
- **Step limit:** if the driver jumped far (a fast mouse), approach in sub-steps within
  the frame budget. Large single steps are where solvers switch branches.
- **Give up gracefully:** if required residuals are not under tolerance at the iteration
  cap, keep the previous pose and report `NotConverged`. Inkling does the same ("if
  maxit, return" without writing values); a frozen frame is better than a jump.
- **Branch flip check:** if any free angle changes by more than π/2 in one solve, treat
  it as a flip and retry from the previous pose with smaller sub-steps.

### 5.6 Analysis (degrees of freedom and conflicts)

On structural change only (the `ezpz` docs warn that this analysis is expensive):

- Jacobian rank of the required rows gives the cluster's remaining degrees of freedom.
- Redundant or conflicting constraints are found SolveSpace's way: drop one constraint at
  a time and re-test rank (`FindWhichToRemoveToFixJacobian`). The result feeds
  `Unsatisfiable` errors and the view's diagnostics.

### 5.7 Budgets and determinism

- Per-frame budget: 2 ms for all clusters touched by a gesture, in wasm in the worker.
  Measured, not assumed; the solver reports iterations and time per cluster.
- Deterministic: iterate in `BTreeMap` order, no hash-order dependence, no threads.
  Replay does not depend on this, because replay applies recorded `derived_ops` (§7), but
  tests are much easier when the same inputs give bit-identical outputs.

---

## 6. Constraints and the reactive graph

A cluster becomes a node in `graph.rs`'s height-ordered evaluation, alongside bindings:

```rust
enum GraphNode { Binding(BoundNode), Cluster(ClusterNode) }
// ClusterNode.inputs  = known leaves it reads (bound leaves, constraint values, fixed transforms)
// ClusterNode.outputs = its free leaves
// height(cluster) = 1 + max height of inputs
```

- A binding that reads a solved leaf (a label showing the ball's y) has a greater
  height than the cluster and updates in the same pass.
- A binding that feeds a cluster input (arm length bound to a slider) runs first; the
  cluster re-solves when it changes.
- A binding that reads a cluster's output and feeds the same cluster's input is a cycle,
  rejected at apply time with `ErrorKind::Cycle`, naming the cluster. This keeps the
  graph a DAG, which the existing equality-cutoff evaluation relies on.

Where a cluster's outputs go depends on context:

- **During a gesture** they go to the overlay via `preview`, like a slider scrub today.
- **During `apply`** they become `derived_ops` (§7).
- **While a live simulation runs** (§11) they go to a separate simulation layer.

`read_with` today (with spine step 6) is: overlay → binding → animated (clips) →
authored constant. It becomes: overlay → binding → animated → simulation → constant. A
leaf claimed by two drivers (a clip and a simulation, or a binding and a clip) is a
reported error, never silently resolved, as `time.rs` already does for clip versus
binding.

---

## 7. Transactions: solve-on-apply

**Invariant:** after every successful transaction, every enabled, active constraint
holds within tolerance. A saved file is always a solved pose.

`Store::apply` becomes:

1. Run the requested ops on the working copy (as now).
2. Update bindings on the working copy.
3. Find clusters whose inputs, variables, or constraints changed. Solve each, treating
   leaves explicitly `Set` by this transaction as **driven** (strong): what the
   transaction asked for wins wherever the constraints allow.
4. Turn solver changes into `Set` ops, apply them, and record them as the transaction's
   `derived_ops`. The transaction's `inverse` covers both kinds of ops.
5. If any cluster is `Unsatisfiable`, fail the whole transaction and touch nothing.

```rust
pub struct Transaction {
    // existing: version, label, actor, ts, idempotency_key, ops, inverse
    pub derived_ops: Vec<Op>,     // new: solver output, applied after `ops`
}
```

- **Undo** applies `inverse`, restoring the requested and the inferred values exactly.
  No re-solve is needed, because the old pose was already solved.
- **Redo and replay** apply `ops` then `derived_ops` without solving. The log is the
  ground truth, so replay is independent of solver changes between versions.
- **Gesture commits** pass through the same path. After a drag the overlay already holds
  a solved pose, so the post-solve finds nothing to change and `derived_ops` is empty.

The cost is that a transaction which disturbs a large cluster records every leaf the
solver moved. That is the honest record of what changed, and it is what makes undo exact.

---

## 8. Gestures through the solver

### 8.1 Store API

```rust
impl Store {
    /// Start a drag of `anchor` (local coordinates) on node `id`.
    pub fn begin_drag(&mut self, id: NodeId, anchor: [f64; 2]) -> DragHandle;
    /// Move the finger. Solves the node's cluster and writes results via `preview`.
    pub fn drag_to(&mut self, h: &DragHandle, world: [f64; 2]) -> SolveReport;
    // release: existing `commit_gesture(label, actor, ts)`; escape: `cancel_gesture()`
}
```

The finger is held in the gesture state, never written to the document. Several
`DragHandle`s may be live at once (two-handed manipulation, which Inkling built around;
open question U9). Inkling also showed something worth copying: two fingers on both ends
of a fixed-length arm can be read as "change this length". That is a role override at
interaction time, and it belongs on the open-questions list rather than in slice one.

### 8.2 Bridge changes (`src/bevy_app/interaction.rs`)

- On press, compute the grab anchor in node-local coordinates (the inverse of the
  entity's `GlobalTransform` applied to the pointer) and call `begin_drag`. This replaces
  the `grab_offset_2d` bookkeeping.
- `drag_apply_system`'s `Overlay2D` branch stops writing Bevy `Transform`s and calls
  `drag_to`. The slider branch is untouched. Every position drag now goes through the
  store, which also fixes a current gap: things bound to a dragged shape didn't follow
  until release.
- `drag_commit_system` sends `GestureRequest::Commit("move <type>")` for all drag kinds,
  instead of building `Set` ops itself.
- **Latency:** `sync_document` runs in `PreUpdate`, but drags apply in `Update`. Without
  a fix, every solved drag would lag a frame behind the pointer. Add a second
  reconcile pass right after `drag_apply_system` in `Update` that drains changes and
  rewrites affected entities. It must run before `TransformSystems::Propagate` in
  `PostUpdate`, so that `render_document_layer` sees the new transforms the same frame.
- **Axis locking.** The working tree's `interaction_decide_system` already locks each
  axis whose leaf is bound or clip-driven, for the whole gesture. `begin_drag` absorbs
  that logic: locked leaves are simply knowns for the solve. A node whose `x` and `y`
  are both known has no free variables, so `begin_drag` returns a handle that does
  nothing and reports why.

### 8.3 What a drag touches, in order, each frame

pointer → `drag_to` → cluster solve → `preview` writes (overlay) → bindings re-evaluate →
`drain_changes` → reconcile affected entities → render. One undo step on release.

### 8.4 Drawing constraints

Constraint nodes are not document content, so the reconciler skips them. Constraint
glyphs (hinge circles, dimension lines) belong in an editor overlay layer driven by the
same views, in a later phase.

---

## 9. What the LLM is told

This is the user's specific requirement: when the solver moves inferred nodes, decide
what the model hears about it.

**Principle: report causes, not effects.** The constraints are in the document, so the
model can always recompute or re-read the effects. Pushing every inferred value at it
wastes context and invites the model to "fix" values that the solver will just move back.

### 9.1 Four kinds of change, four policies

| Change | Reaches the model as | Values included |
|---|---|---|
| Per-frame previews during a drag | Nothing. Not document state | — |
| A committed human gesture | One entry: `human moved #ball (finger) — cluster #pendulum re-solved: 4 nodes` | Only the driven node's new values |
| A transaction the model itself sent | `Applied` gains a `solver` summary: clusters re-solved, nodes moved, and **any requested value the solver did not honour** | Requested-vs-final for driven leaves only |
| Simulation ticks (later) | Nothing per tick; only start, pause, and freeze events | Frozen values on freeze, summarised |

### 9.2 The model's own edits

If the model sends `<set id="#ball" x="900"/>` and the arms only reach 612, the
transaction succeeds (the strong driver is satisfied as far as possible) and the
response says so. `document_apply` today answers `{"ok":true,"version":N}`; it gains a
`solver` field, and `edit.ts` renders it as markup for the model:

```json
{"ok": true, "version": 214,
 "solver": {"clusters": ["#pendulum"], "moved": ["#arm1", "#arm2", "#arm3"],
            "unmet": [{"leaf": "#ball.transform2d.x", "requested": 900, "solved": 612,
                       "reason": "out of reach: cluster at full extension"}]}}
```

A transaction may opt into strictness with `"exact": true`, which turns any `unmet`
entry into an `Unsatisfiable` failure. That suits a model that wants all-or-nothing
edits. Whether strict should be the default for model edits is open question U2.

### 9.3 Views mark solver-maintained values

Just as a bound slot renders as `x="…" x.bind="…"`, a free variable in a cluster renders
with the cluster that maintains it:

```xml
<rect id="#arm2" x="140" y="212" rot="0.61" x.by="#pendulum" y.by="#pendulum" rot.by="#pendulum"/>
<constraint id="#c3" kind="coincident" a="#arm1@[0,120]" b="#arm2@[0,0]"/>
<constraint id="#c5" kind="distance" a="#base@[60,0]" b="#ball@[0,0]" value.bind="#len.slider.value * 2"/>
```

The `.by` marker tells the model that writing that leaf directly makes it a driver whose
effect spreads through `#pendulum`. Clusters render as a summary line with degrees of
freedom and status, elided by default like any other subtree.

### 9.4 The context ledger

Vision doc 04 §8 plans a ledger of what the model has read. Solver changes plug in at
cluster granularity: if the model has not read inside `#pendulum`, re-solving it needs
no notice beyond the cause line. If it has, the notice names the cluster, not each leaf.
Until the ledger exists (doc 04 Phase 3 uses blanket invalidation), the cause line and
the version bump are the whole notification.

### 9.5 "Why is this moving?"

The doc 05 probe extends naturally: for a leaf, answer with its role and the chain of
causes, for example `#arm2.rot — free in #pendulum; driven by finger on #ball`, or
`bound: #slider.value`, or `fixed by #c1`. `relations_to(#arm2)` lists the constraints;
the gesture state lists the active drivers.

---

## 10. Views for humans

The inspector (Svelte chrome) shows, for a selected node: the constraints touching it,
its role per leaf (fixed, bound, free, driven), and its cluster's degrees of freedom.
Conflicts from §5.6 highlight the culprit constraints. This is phase 4, but the data
comes from the same view renderer the model uses.

---

## 11. Live simulation with grab (phase 5)

The second kind of behaviour: things that move on their own (pendulum swinging under
gravity, charged particles in a box) and respond when grabbed.

- **Method:** XPBD (extended position-based dynamics) over the same constraint kinds.
  Each kind provides its residual; XPBD's projection uses the residual and its gradient,
  so the kinematic solver and the simulation share one definition per kind.
- **Document:** a `simulation` component on a group (`running`, `substeps`, `gravity`),
  a `body` component on simulated nodes (`mass`), and force kinds (`gravity`, `spring`,
  `coulomb`, `damping`) as a code vocabulary, the built-in "model primitives" that run at
  60 fps.
- **State:** authored leaves are the initial conditions. While running, positions and
  velocities live in a simulation layer between the overlay and resolved bindings. They
  are not transactions, not undoable, and not reported per tick.
- **Grab:** the finger sets the grabbed body's inverse mass to zero and its position to
  the cursor. Every other body keeps responding through the same step. Release restores
  the mass. That is the "N dependent becomes 1 driven plus N−1 dependent" switch.
- **Freeze:** an explicit command commits the current simulation state as authored
  values in one transaction (the model hears "simulation frozen", §9.1).
- **Time:** simulation time is doc 05's third kind of time. Recording a run into a clip
  is open question U6.

---

## 12. 3D assembly mates (phase 6)

Toward the optical table.

- `transform3d.rot` is currently Euler XYZ (`EulerRot::XYZ` in `materialize.rs`). Euler
  angles make a poor solver variable (gimbal lock), so add `Value::Quat` and migrate
  `rot` to a quaternion, with a schema bump. The solver works in a local 3-vector
  rotation increment and re-normalises.
- Kinds: `coincident` (point–point), `concentric` (axis–axis), `coplanar` (plane–plane,
  with flip), `distance`, `angle`, `point_on_plane`. Anchors become frames (point plus
  axis) on each operand.
- Breadboard holes are discrete. Model them as snapping that creates a real `pin` on
  release, not as a continuous constraint (open question U8).
- **Mate features versus ports.** Faces, axes, and holes used for mating are local
  geometry on the CAD depiction, detected automatically at import. They are not ports.
  A port kind may *declare* the mate it implies (a fiber connector implies coincident
  plus concentric; an optical axis implies "on the beam line, parallel"), so connecting
  two ports creates the constraint. See vision 09 §6.

---

## 13. Paths (phase 7, coordinated with the pen-tool plan)

Anchor points become child `point` nodes of a `path` node, each with its own
`transform2d`. Static paths are just authored points. A "flexible" path (a rope that
gravity acts on) is the same points with `distance` constraints between neighbours inside
a running simulation. Both kinds requested earlier come from one representation.

---

## 14. Phases and exit criteria

### Phase 0 — Groundwork (each item useful on its own)

- Floor trig and roots in `expr.rs` (§4.4).
- `geom` module and `world_affine`, with the Bevy equivalence test (§4.3).
- All position drags through `preview`, with the post-drag reconcile pass (§8.2).
- `Applied` gains an (initially empty) change summary.

*Exit:* dragging a rect moves a second rect whose `x` is bound to the first in the same
frame; release produces one "move rect" transaction; undo restores both.

### Phase 1 — `iron_solve`

*Exit:* native unit tests pass for:

- a three-link chain reaching a target (converges, required residual < 1e-6 px);
- an unreachable target (full extension, `max_required_residual` small, strong residual
  equal to the gap);
- a four-bar linkage driven through a full crank turn without a branch flip;
- redundant and conflicting systems, with the culprit identified;
- bit-identical results on repeated runs.

It also needs a benchmark: a 12-variable cluster at 20 iterations, timed natively and in
wasm.

### Phase 2 — Constraints in the document

Type, component, relation kind, `ConstraintSpec`, validation, clusters, solve-on-apply,
`derived_ops`, views with `.by` markers, `Applied.solver` summaries.

*Exit* (headless, in `tests/constraints.rs`):

- a pendulum built from ops alone is solved on apply;
- setting `constraint.value` re-solves, with derived ops recorded; undo restores
  exactly; redo matches;
- save → load → replay from empty gives identical canonical JSON;
- a contradictory constraint is rejected as `Unsatisfiable` naming the culprit;
- a binding↔cluster cycle is rejected as `Cycle`;
- the tree view of the pendulum matches a golden file.

### Phase 3 — The inverse-kinematics demo (the proof)

Scene, built in `demo.rs` from ops: a platform (rect, fixed), three arms (rects) joined
by `coincident` hinges, a ball (circle) at the end of arm 3, a slider whose value drives
arm length through a bound `distance` value, and a bar whose height is bound to the
ball's `y`.

*Exit criteria, in order:*

1. Drag the ball: the arms follow every frame, with no visible lag behind the pointer;
   the bar tracks the ball's height in the same frame.
2. Drag beyond reach: the chain straightens and the ball stays as close to the pointer
   as it can get. No jitter, no flipping.
3. Release: exactly one transaction, "move ball", whose `derived_ops` list the arms.
   Undo returns every part; redo matches.
4. Move `fix` from the platform to the ball (one model-style transaction), then drag
   the platform: the arms bend and the ball stays put.
5. Scrub the length slider: the chain re-solves live, and one transaction on release.
6. Try to drag a node whose position is bound: nothing moves, and the reason shows.
7. A scripted model edit (`Set #ball.transform2d.x` out of reach) returns an `unmet`
   entry; the same edit with `exact: true` is rejected.
8. The cause-only notification of §9.1 appears for the human drag, and a tree view read
   afterwards shows `.by="#pendulum"` on the arms.
9. Solve time per frame stays under 2 ms in the wasm build (logged).

### Phase 4 — Diagnostics and authoring

Degrees-of-freedom display, conflict highlighting, constraint glyphs, gesture creation
of constraints (drop an arm's end on another to create a hinge), modifier keys for role
overrides (open question U5), and model-surface support for creating constraints inside
`<create-subtree>`.

### Phase 5 — Live simulation (§11)

*Exit:* charged particles in a box run continuously; grabbing one moves it with the
cursor while the others react; releasing resumes; no transactions while running; freeze
commits one. The pendulum's "swing" mode uses the same hinges as phase 3.

### Phase 6 — 3D mates (§12)

*Exit:* a lens on a post on a breadboard; drag the post and the lens follows; a
concentric mate between two lenses holds while one slides along the rail.

### Phase 7 — Paths (§13), after the pen tool exists.

---

## 15. Change map

| Where | Change | Phase |
|---|---|---|
| `src/iron_document/src/expr.rs` | `sin cos tan atan2 sqrt pow hypot pi` | 0 |
| `src/iron_document/src/geom.rs` (new) | `Affine2`, `world_affine` | 0 |
| `src/bevy_app/interaction.rs` | drags via store; anchor in local space; unified commit | 0, 3 |
| `src/document_bridge/mod.rs` | post-drag reconcile pass | 0 |
| `src/iron_solve/` (new crate) | problem, reduction, clustering, LM, analysis | 1 |
| `Cargo.toml` (workspace) | add `src/iron_solve` member | 1 |
| `src/iron_document/src/registry.rs` | `constraint` type; `ConstraintSpec` table | 2 |
| `src/iron_document/src/component.rs` | `Constraint` component | 2 |
| `src/iron_document/src/document.rs` | `RelKind::Constrains`; cluster cache | 2 |
| `src/iron_document/src/op.rs` | new error kinds; `derived_ops` on `Transaction` | 2 |
| `src/iron_document/src/lib.rs` | solve-on-apply; `begin_drag` / `drag_to`; `Applied.solver` | 2, 3 |
| `src/iron_document/src/graph.rs` | `GraphNode::Cluster`; layered reads | 2 |
| `src/iron_document/src/view.rs` | constraint nodes; `.by` markers; cluster summaries; probe | 2 |
| `src/iron_document/src/serial.rs` | schema bump; new names | 2 |
| `src/iron_document/tests/constraints.rs` (new) | phase 2 exit tests | 2 |
| `src/document_bridge/demo.rs` | pendulum scene | 3 |
| `src/document_bridge/materialize.rs` | skip `constraint` nodes | 2 |

---

## 16. Testing strategy

- **Invariant test helper:** after every transaction in every test, every enabled,
  active constraint holds within tolerance. Add it next to `Document::check_invariants`.
- **Random drags:** generate pendulums and linkages with random dimensions, drag random
  anchors along random paths, and check the invariant, the absence of flips (no free
  angle jumping more than π/2 per frame), and that undo restores exactly.
- **Replay:** every test that commits also replays the log from empty and compares
  canonical JSON.
- **Golden views:** tree views of demo scenes are checked into `tests/golden/` so changes
  to what the model sees show up in review.
- **Bridge equivalence:** `world_affine` versus Bevy `GlobalTransform` (§4.3).

---

## 17. Amendments to existing plans

- `document-spine.md` §5.10, "a bound leaf has no drag handle" (in the working tree: a
  bound or clip-driven axis is locked for the gesture): still true for those leaves.
  Leaves that are free in a cluster now have handles through the solver, and the
  cluster is the disambiguation policy that vision doc 02 invariant 2 requires.
- Vision doc 02 §8.2's policy table gains a row: **constrained leaf → solve the cluster,
  least change, deterministic.** The model-mediated repair of §8.3 remains the fallback
  for bound expressions, where no constraint declares the intent.
- Vision doc 09: constraints are its geometric half (§8). Annotation arrows can use
  `coincident` constraints to stay attached to their targets
  (`entity-composition-model.md` §10).
- Vision doc 06 (the floor): the solver is floor-tier. It is pure, bounded by an
  iteration cap, and in-process, so it satisfies doc 02 invariants 7 and 8.

---

## 18. Open questions

Each question has a proposed default so work can proceed; the default is what the
phases above assume.

### 18.1 Intended use (product)

- **U1. What is the first real use: explanatory mechanisms, layout, or simulation?**
  Partly answered (2026-10-09): constraints are mainly for *setting things up*, like
  SolidWorks assemblies and optical-table layout, while physics goes through models and
  signals. The pendulum stays the first demo because it exercises every piece, but
  assembly mates and layout constraints (alignment, equal spacing) rank above
  mechanism-style kinds afterwards.
- **U2. Should the solver adjust model edits, or reject them?** Default: adjust and
  report (`unmet`), with `exact: true` available. Alternatively, strict by default for
  the model and lenient for humans.
- **U3. What happens when a drag is impossible** (everything fixed, or the grabbed node
  is fully determined)? Default: nothing moves and a hint says which constraint
  prevents it. Alternatives: offer to suspend the blocking constraint, or move the
  whole cluster rigidly.
- **U4. What is grounded by default?** Default: nothing. An unfixed mechanism moves
  rigidly when dragged, as in SolveSpace. SolidWorks instead fixes the first part.
- **U5. Interaction-time role overrides.** Should modifiers change roles mid-drag:
  temporarily fix the grabbed node's neighbours, drag ignoring constraints, or (as in
  Inkling) two fingers on both ends of a dimension to change it? This is the
  fine-grained mixing you described; the gesture vocabulary needs a decision.
- **U6. What becomes of simulation runs?** Discarded, frozen into authored values, or
  recorded as a clip (scripted time) for playback in a lesson?
- **U7. Do constraints cross group boundaries, and can a drag move a parent?** Default:
  yes and no. Constraints may reference nodes in different groups, but only operands'
  own transforms are variables. Should a group ever act as one rigid body in a cluster?
- **U8. Snapping vs. constraints.** Should snapping (grids, breadboard holes, alignment
  guides) create real constraints on release, as Onshape and SketchUp do, or only
  position the node?
- **U9. Multi-touch.** Is two-handed manipulation (two fingers at once) a goal on
  iPad-class devices? The API allows it; the bridge's input does not yet.
- **U10. How visible are constraints?** CAD-style glyphs always on, only on selection,
  or only in the inspector?
- **U11. Precision.** CAD-grade (1e-9) or visual (0.01 px)? This affects tolerances,
  iteration caps, and whether f32 anywhere is acceptable.
- **U12. Constraints on non-geometric numbers.** Should the solver work on any number
  leaf (`a + b = c`, a colour's hue tied two-way to an angle), not just transforms?
  Default after 2026-10-09: no. Non-geometric relationships belong to signals and
  models, and the cheap two-way case is covered by linear binding inversion
  (`roadmap.md` stage A). The design would still allow it later.
- **U13. Scale.** Rough targets for constrained nodes per document and per cluster, and
  particles per simulation. This decides when sparse algebra and spatial acceleration
  for forces (Coulomb is N²) are needed.
- **U14. Reference dimensions.** Should a measurement that follows the geometry (shown
  but not enforced) exist? Inkling's unlocked lengths are this. It would be derived
  state, not authored, so it would live in the reactive layer, not in
  `constraint.value`.

### 18.2 Implementation

- **I1. Nodes plus relations for constraints** (§4.1) versus a dedicated constraint
  table outside the node tree. Nodes give ids, undo, views, and tombstones for free; a
  table would be leaner if constraints ever number in the tens of thousands.
- **I2. Solve-on-apply inside `apply`** (§7) versus a follow-up system transaction. Inside
  is atomic and makes undo one step; a follow-up is simpler to implement but leaves a
  window where the document is unsolved.
- **I3. Build or adopt the solver.** Default: build the small LM core in `iron_solve`
  and keep `ezpz` (MIT, Rust, 2D, Gauss–Newton, from KittyCAD) as a reference and a
  possible backend. SolveSpace is GPL-3.0 and FreeCAD's planegcs is LGPL, so read
  them for ideas only. Inkling's repository has no licence file, so the same applies.
- **I4. Linear algebra.** Hand-rolled dense versus `nalgebra` versus `faer`; finite
  differences versus dual numbers. Decide from the phase 1 benchmark.
- **I5. Who owns world transforms.** The store now computes them for the solver, Bevy
  computes them for rendering, and the caching plan's Understory box tree would compute
  them a third time. Long term, one should be the authority and the others consumers.
- **I6. Quaternion migration** (§12): the timing and the file migration for existing
  documents.
- **I7. Cross-platform floating point.** Replay is safe because it applies recorded ops.
  But re-solving the same document natively and in wasm may differ in the last bits,
  because transcendental functions differ between platforms. Does anything need
  bit-identical solves across platforms?
- **I8. Model edits that arrive mid-drag.** A transaction from the model applies in
  `sync_document` while a gesture overlay exists. Options: queue it until release,
  apply it and re-solve the drag on top, or cancel the gesture. Default: queue.
- **I9. Per-frame invalidation cost.** A running simulation changes many leaves every
  frame; the reactive graph's invalidation and the reconciler's affected set must stay
  cheap at that rate. Measure in phase 5.
- **I10. Weights and tolerances.** The tier weights and damping in §5.4 are starting
  points. Tune them by feel in the demo, and record the final values with the reasons.
- **I11. Rotation dragging.** Grabbing a point off-centre rotates a free body. Is the
  half-diagonal rotation scale (§5.4) the right feel, or should rotation be stiffer
  unless a rotation handle is grabbed?
- **I12. Where cluster analysis results live.** Cached in the store (rebuilt on
  structural change) versus computed when a view asks.

---

## 19. Prior art and what this plan takes from it

| Source | Taken |
|---|---|
| **Inkling** (Ink & Switch), `App/src/app/Constraints.ts` | The drag as a constraint (`Finger`); propagating knowns before solving (`computeKnowns`); clusters by shared variables; absorbing equalities so state is never duplicated; keeping the old values when the solver hits its iteration cap; animation as a temporary lock that is released afterwards (`VarMover.ts`), which is the model for drivers in §11. Its solver minimises a sum of squared errors with a general-purpose minimiser (g9 / numeric.js `uncmin`) and is called once per frame for every cluster. No licence: ideas only. |
| **SolveSpace**, `src/system.cpp` | Weighting dragged parameters through column scaling; substituting `a = b` equalities; solving single-variable equations first; Newton steps with a minimum-norm least-squares solve; rank tests for degrees of freedom; dropping one constraint at a time to find the conflicting ones. GPL-3.0: ideas only. |
| **ezpz** (KittyCAD, MIT) | Confirms Gauss–Newton is enough for 2D sketches; run degree-of-freedom analysis only on structural change. A candidate backend. |
| **arael-sketch** | Levenberg–Marquardt with drift regularisation and "robust drag constraints"; a whole drag recorded as one undoable action. The same choices as §5.4 and §8. |
| **Blender XPBD solver** | Pin constraints, substeps, Gauss–Seidel iteration, residual reporting. The template for phase 5. |
| **Cassowary / DeltaBlue** | Constraint strengths (required, strong, weak) and choosing direction per edit. The tier model of §3. |
| **Modelica** | Equations whose causality is assigned when compiled. Confirms "declare relations; assign direction late" scales to engineering models. |
