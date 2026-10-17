# The engine boundary: what Iron evaluates and what Bevy runs

**Status:** Agreed 2026-10-17. Phases 1 and 2 of §7.1 are built (`rendering/render-benchmark.md`
§10–11); phases 3–5 are not yet scheduled. Settles CS open question I5 and roadmap §5's ownership question.
**Date:** 2026-10-17
**Scope:** Where resolved state (world transforms, solved poses, evaluated bindings) is
computed; what a Bevy entity is for; how values flow from Bevy and from Models back into
the document; and what this means for the bridge, the spatial index and the rendering
plan. Amends [`01`](01_layered-architecture-overview.md) §3 (layer 2 is now a retained
Iron layer), [`02`](02_substrate-model-projection-control.md) §8 (writeback has two
channels), [`07`](07_current-2d-backends-and-hybrid-architecture.md) (baking is one of
several presentation modes) and
[`../specs/constraint-system.md`](../specs/constraint-system.md) §18.2 I5 and §11.

---

## 0. Decisions at a glance

| Question | Decision |
|---|---|
| Who computes world transforms | **Iron**, for every document node, 2D and 3D. Bevy entities that mirror document nodes receive a world transform and never compute one. There is no third computation; `understory_box_tree` stays rejected |
| Where the dependency graph is evaluated | In the store's evaluator (`graph.rs`): overlay → binding → clip → simulation → authored. World transforms are inputs inside that graph, which is why they live with it |
| What produces values | Anything: clips, the kinematic solver, a Bevy physics engine, a Model session, a sensor. Producers publish into **runtime layers** the evaluator reads beneath bindings. They never write transactions per frame |
| What a Bevy entity is | A **runtime resource**, spawned for what Bevy acts on: 3D renderables, views and targets, cameras, and document nodes in the working set. Not the document's identity; `NodeId` is |
| How many entities a node has | 0..N. A baked 2D group has 0; a baked 3D group has 1; a live node has 1 (flat, Iron-driven); a glTF asset has N; a particle has no node at all |
| Entities for 2D shapes | Only for subtrees in **Live** mode (the working set), flat, with `GlobalTransform` written by the reconciler. Retained and baked subtrees have none |
| Physics on document shapes | A Bevy-side producer (avian or similar) over Live entities, publishing poses into the simulation layer. The dynamics engine does not enter `iron_document` |
| Bevy → document | Signals per frame (runtime layers); ops only at explicit commit points (drag release, freeze) |
| Role of Bevy | Renderer, host (schedule, input, window, GPU, assets) and the runtime for populations that are not document nodes |

---

## 1. The problem this settles

Three places compute where a node is: the store (`geom::world_affine`, for the solver,
copy/paste, SVG and hit tests), Bevy (`ChildOf` + `GlobalTransform`, one entity per
node, for rendering and picking) and the spatial index (which reads Bevy's result).
Rotating a group of 10,000 shapes recomputes 10,000 Bevy transforms and 10,000 bounds,
which is most of the measured frame (`../handoff-render.md` §4, finding 6). The
question was which one should be the authority.

The answer follows from what the transforms are *for*. The solver reads world positions
of anchors; a binding can feed a cluster's input and another can read its output; a
clip can drive a leaf the solver treats as known; the simulation layer sits between
them. That is one height-ordered dependency graph, evaluated with equality cut-off,
with world transforms as inputs in the middle of it. A graph with one scheduler can be
evaluated exactly once per change. A graph split between the store and Bevy's schedule
has to be iterated to a fixpoint every frame, or re-implemented as Bevy systems, whose
order is declared statically and cannot follow the data. The project met this once
already: when Bevy owned the drag, nothing bound to the dragged shape moved until
release, and the fix was to move the drag to where the bindings are (CS §8.2).

Two further reasons, each sufficient on its own:

- **Transactions are atomic.** `apply` yields a solved pose or fails and touches nothing.
  A solve that ran in Bevy after the ops landed would discover "unsatisfiable" a frame
  late, after the history already recorded the unsolved state.
- **The return path would be ops.** A system that "writes the document after Bevy
  computes" turns every frame of a drag or a simulation into a transaction, which
  pollutes undo, the version counter and what the language model is told.

So world transforms are computed where the graph is evaluated, which is Iron, and
Bevy's propagation is not used for document nodes.

### 1.1 What this does not say

It does not say Bevy is the wrong place for a constraint solver in general. avian and
rapier are Bevy-native solvers. They are dynamics engines over runtime bodies, and they
give none of: atomic solved transactions, exact undo, derived ops in the log, cycle
detection against bindings, "who drove what" for the model. Those are document
properties. Dragging a partially mated assembly and having the rest follow, with one
undo step on release, is a document operation (CS §12), so it runs in Iron, in 2D and
in 3D. Dynamics over a population of bodies is a runtime operation and runs in Bevy
(§4).

---

## 2. Evaluator and producers

The distinction that makes the rest fall out:

- The **evaluator** is the one component that answers "what is this leaf's value now".
  It is `Reactive` in `graph.rs`, with its read order overlay → binding → clip →
  simulation → authored constant. It owns world transforms because the solver inside it
  needs them and the renderer reads them.
- A **producer** supplies values to one of the evaluator's layers. Today's producers:
  the gesture (overlay), `time.rs` (clips), the kinematic solver (overlay during a drag,
  derived ops on apply), the port network (`physics`). Planned: the simulation layer
  (CS §11), Model sessions (`../specs/model-interop.md`).

The rule: **the evaluator lives in Iron; producers live wherever fits.** A producer in
Bevy, in a worker, or across a socket is as legitimate as one in the store, provided it
publishes into a layer rather than writing transactions. The three existing maps
(`animated`, `physics`, the planned `simulation`) are special cases of one mechanism,
which §4 names.

---

## 3. Transforms

### 3.1 2D

The store's `geom` module gains a retained, revision-keyed world-transform and bounds
pass: vision 01's "derived world model" (layer 2), which has only ever existed as an
uncached walk. A subtree carries a **local revision** (its own content changed) and a
**placement revision** (its position in its parent changed). Rotating a group bumps one
placement; its descendants' local-space bounds are untouched. This is handoff item 3
(`../handoff-render.md` §4) and it is what the spatial index, the chunk recorder and
the bake policy all key on.

`iron_spatial` indexes each group's contents in group-local space with one placement
affine per occurrence, as embeds already do. Picking maps the pointer into local space
on the way down. The drift check `check_world_affine` in `document_bridge/mod.rs`, which
compares the store against Bevy every frame, is deleted once nothing reads Bevy's value.

### 3.2 3D

`geom` gains an `Affine3` (translation, quaternion, scale) alongside `Affine2`; CS §12
already plans the quaternion migration (I6). Iron computes the world affine of every 3D
document node the same way.

3D entities that mirror document nodes become **flat**: no `ChildOf` between them, and
the reconciler writes Iron's world affine into `GlobalTransform` directly. Bevy's
propagation then has no document nodes to propagate. Bevy's own propagation keeps
running for **Bevy-native hierarchies**: a glTF asset's internal node tree, a particle
system, a gizmo, each under one document node that holds the placement. (That
`GlobalTransform` can be set directly on an entity without a `Transform`, and that Bevy
0.20's renderer and picking read it as-is, is to be verified in the first phase.)

### 3.3 Precision

Iron computes in f64. f32 at a document coordinate of 1,000,000 px resolves to about
0.06 px, which an infinite canvas reaches. Bevy receives f32 only at the end.

---

## 4. Bevy → Iron: the return channel

Two channels, never confused:

| Channel | Carries | Frequency | In history | Example |
|---|---|---|---|---|
| **Runtime layer** | derived values the evaluator reads beneath bindings | every frame | no | body poses from a simulation, a Model's output frame, a collision count |
| **Ops** | authored changes | at commit points | yes | drag release, "freeze" a simulation, "bake" a Model result into values |

The runtime layer generalises `animated`, `physics` and the planned `simulation`
map into one: **named runtime signals**, published by any producer, read by bindings
like any leaf, carrying a producer id and a frame stamp, never serialised. A leaf claimed
by two producers is a reported error, as `time.rs` already does for clip versus binding.

Worked example, from the conversation: gas particles in a box, and a protein whose shape
responds to how often it is hit. The particles are Bevy entities with colliders and no
document nodes. The protein is a document node whose shape parameter is bound to a
runtime signal `hits`. A Bevy system counts collisions and publishes `hits` each frame;
the binding re-evaluates; the protein's geometry updates; nothing enters the history.
"Freeze" commits the resolved value as one transaction, which is the model-interop
spec's shape (`immutable runtime outputs → projections → rendering`; commit explicitly).

---

## 5. Entities

### 5.1 The invariant

**A document node maps to 0..N Bevy entities; a Bevy entity maps to 0..1 document
nodes.** Identity is `NodeId`; an entity is a resource that records which node it
serves (`Provenance`). Picking and selection of document nodes go through Iron's index,
never through entity hits. This already holds for 2D and for embedded planes; it is
made the rule.

### 5.2 Presentation modes

Every subtree is in one mode. The mode decides both what Bevy holds for it and how it
is drawn:

| Mode | Entities per node | Encoded | Pickable per node | Typical use |
|---|---|---|---|---|
| **Live** | one, flat (`Provenance`, `GlobalTransform`, draw data) | every change | yes | the working set: what is selected, being edited, simulated or animated finely |
| **Live + simulating** | the same entity, plus the producer's components (collider, body) | every change | yes | a running physics simulation |
| **Retained vector** | none | dirty chunks only | yes, via the index | everything else on screen |
| **Baked** | none (2D); one per subtree (3D) | no; drawn from a texture or merged mesh | enter to edit | static, complex, or small on screen |
| **Compiled** | none | no | no | raster or video layers made deliberately from a subtree |

An entity exists for what Bevy acts on. In Live mode that is any Bevy system (physics,
an inspector, a plugin). A flat entity with two or three components is a table row;
tens of thousands are cheap. What was expensive was hierarchy propagation and
change-detection scans over one entity per node, and no mode has those.

Mode is chosen by policy, not authored: Live for the selection and its neighbourhood,
for subtrees with a running simulation or a fine-grained clip; Retained by default;
Baked for subtrees that are static, complex or small on screen (the Flutter rule:
unchanged for K frames and expensive enough). A simulation or fine animation forces its
subtree Live, which has a visible cost at scale; the editor shows the cost and offers
freeze, bake or compile. That is the intended guidance toward separating dynamic from
static content (conversation 2026-10-17), expressed as a mechanism rather than a rule.

### 5.3 3D symmetry

A Bevy mesh must be an entity, so baked 3D ends at **one** entity per subtree rather
than zero: a merged static mesh (static batching), an impostor (the subtree rendered to
a texture on a billboard, which is `planes.rs` in reverse), or an LOD mesh. Entering the
subtree expands it again. Baked 2D ends at zero only because the 2D compositor is
Iron's display list, where a baked group is one draw command; Bevy's 3D compositor is
its render world, where the equivalent is one entity. The invariant of §5.1 covers both.

---

## 6. Bevy's role

| Iron | Bevy |
|---|---|
| Identity, hierarchy, authored values | Frame loop, schedule, input, windowing, DPI |
| Bindings, clips, constraints, gestures, undo, history | GPU device, render graph, textures, compositing views |
| Resolved state: world transforms, solved poses, evaluated signals | 3D rendering: meshes, materials, lights, cameras, glTF |
| Spatial index, culling, picking of document nodes | Asset loading |
| Draw intent: display lists, chunks, modes | Executing draw intent: Vello, 3D passes |
| The model's projection, serialisation | Runtime populations: particles, bodies, Model output projections |

Bevy is the renderer, the host and the runtime for everything that is not a document
node. Its role shrinks relative to "the entity table is the document", which the spine
already abandoned, and grows with the parts of the roadmap that are furthest out:
populations, Models projecting bulk data, 3D assemblies, compiled layers. This is the
way game engines serve non-game tools, with the authoritative data in a domain model
outside the engine.

---

## 7. What changes in the code

What is rewritten is the glue, not the foundation. The store, ops, reactive graph,
solver, gestures, SVG, the Vello backend, views and textures and the UI protocol stay.

| Area | Change |
|---|---|
| `iron_document/geom.rs` | `Affine3`; a retained world-transform and bounds pass keyed on local and placement revisions |
| `iron_document/graph.rs` | runtime signal layer replacing the `animated`/`physics`/`simulation` special cases (clips keep their own stamp semantics) |
| `iron_spatial` | local-space indexing per group with placements; 3D bounds for document nodes |
| `document_bridge/materialize.rs` | mode-driven: spawns flat entities for Live subtrees and 3D renderables only; writes `GlobalTransform` from Iron |
| `document_bridge/mod.rs` | `check_world_affine` removed; a `publish` pass drains Bevy-side producers into the runtime layer |
| `document_bridge/render.rs`, `spatial.rs`, `highlight.rs`, `nest.rs` | read Iron's world affines, not `GlobalTransform` |
| `bevy_app/select.rs`, `picking.rs`, `manipulate.rs`, `pen.rs`, `shape_tool.rs` | same: Iron affines for 2D; the 3D gizmo keeps Bevy transforms for its own entities |
| `vector/hybrid.rs` | chunk recorder and bake policy consume subtree revisions (the render handoff's items 3–4) |

### 7.1 Phases

Each phase leaves the app working and the smoke script passing.

1. **Retained transforms in Iron** (2D). **Done 2026-10-17** (commit `161648c`):
   `geom::Revisions` (placement, local, structure), `Store::local_affine`,
   `local_bounds` and `take_geometry_changes`; `iron_spatial::Scene` in local space
   plus placements; `prepare_spatial` reads no Bevy transform. Bevy still spawns and
   propagates; the drift check stays. *Exit met:* rotating the 10k field rewrites one
   entry (`render-benchmark.md` §10). Not done: a cached world-affine table in the
   store itself (`world_affine` still walks the chain; the scene caches placements
   per group, which is what phase 2's readers need).
2. **Bevy consumes.** **Done 2026-10-17** (commit `26de602`): no `ChildOf` on
   document entities; a 2D entity carries no transform (its readers use
   `nest::world_2d`: the scene's cached placements, else the store); a 3D entity
   receives `GlobalTransform` from `Store::world_affine3` (`geom::Affine3`), written
   once as its `Transform` at spawn so Bevy's simple-transform sync never overwrites
   it, and a moved 3D ancestor rewrites its few materialised descendants. The drift
   check is deleted; `propagate` is timed. *Exit met:* 10k rotation's changed frame
   67 → 27 ms on the container, `propagate` 0.1 ms (`render-benchmark.md` §11). The
   gizmo and 3D picking passed the smoke on Bevy 0.19; the 0.20 check moves to the
   upgrade. *Amendment to §5.2:* a Live 2D entity gets a `GlobalTransform` only once
   a Bevy-side producer needs one (phase 4), since keeping every descendant's
   current would be the per-node cost this phase removed.
3. **Modes.** The reconciler consults a mode per subtree; Retained subtrees have no
   entities. *Exit:* a 100k-shape composition idles with entities only for the
   selection's neighbourhood.
4. **Runtime signal layer and a first Bevy producer.** A collider/body producer over
   Live entities publishing into the layer; freeze commits. *Exit:* the gas-and-protein
   example runs with zero transactions per frame.
5. **Chunks and bakes** keyed on the revisions from phase 1, per the render handoff.
   Baked 3D (merged mesh or impostor) follows when a 3D scene needs it.

Phase 1 is handoff item 3 and can start now. Phases 2–3 are the bridge rewrite. The
Bevy 0.20 / `vello_gpu` 0.3 upgrade is independent and is easier after phase 3, since
fewer systems touch entities.

---

## 8. Open questions

- **Q1. The kinematic simulation (CS §11).** The spec puts XPBD in `iron_solve` so the
  sim and the kinematic solver share residuals per constraint kind. Under §2 it could
  equally be a Bevy-side producer with joint constraints. Decide when phase 4 lands,
  from whether grab-and-release of a constrained mechanism needs the solver's
  residuals or only its pose.
- **Q2. Mode policy thresholds.** How large a selection neighbourhood is Live; how many
  frames unchanged before a bake; whether the user can pin a mode. Measure first.
- **Q3. Headless use.** A collaboration server, CLI export and replay validation want
  the evaluator without a GPU. §3 makes that possible; nothing here schedules it.
- **Q4. Runtime signals in the model's view.** The language model reads authored values
  and, on demand, resolved ones (vision 04 §4.1). A runtime signal is resolved state
  from a producer it cannot see; whether the view names the producer is open.
