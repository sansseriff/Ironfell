# Bevy events, simulations and runtime drivers

**Status:** Note, not adopted. An answer to a question asked on 2026-10-15; nothing
here should be built until a plan adopts it.

**The question.** Iron is built on constraints and reactive signals. Can ordinary
Bevy events and listeners still take part? The listeners would see what document
entities do (position, collisions, anything a game listens to) and act on the
document in turn. The example: a gas of many particles simulated in Bevy systems,
too many to be document nodes, and a protein, which is a document node, that
responds when a particle hits it.

**Short answer: yes.** Three rules keep it consistent with the spine:
1. **Bevy reads the document freely.**
2. **Bevy writes it only through the store**, on one of three channels.
3. **High-count simulation state stays in Bevy.** Only what the document cares
   about crosses: aggregates per document node, and declared intent.

## 1. Reading: the document is already in Bevy

- **Positions are already there.** Every node is materialised as an entity with
  `Provenance(NodeId)`, and `NodeMap` maps both ways. Resolved values (after
  bindings, constraints, clips, previews and the physics layer) are written into
  its components every frame. A Bevy system can read a protein's
  `GlobalTransform` today, or `Store::resolved` for any leaf.
- **What is missing is change *events*.** The bridge already computes them every
  frame: `DocumentChanges` (the nodes rewritten), `ResolvedChange` (leaf and new
  value), and the log of applied transactions. Publishing them costs little:
  - a buffered Bevy message, `LeafChanged { node, entity, leaf, value }`, for
    systems that scan;
  - an entity event on the materialised entity, so an observer on the protein
    hears only about the protein;
  - `Transacted { label, actor, nodes }` for authored edits, so a listener can
    tell "the model changed this" from "a binding moved it".
- **Listen to the store's feed, not to `Changed<Transform>`.** A Bevy `Transform`
  is an output the reconciler overwrites; the store's feed says why something
  changed.

## 2. Simulations that are not document nodes

The particles live only in Bevy: spawned, stepped and despawned by Bevy systems,
never saved, never in history. They are *runtime* state, like the physics solution
and clip samples are derived state.

- **The document holds the intent.** A `gas` node (or a definition instance)
  declares the simulation: count, temperature, the region it fills, and which
  document entities it interacts with. Those parameters are ordinary leaves:
  bindable, constrainable, editable by a person or the model, saved.
- **A bridge system reads the declaration** and runs the particles. The particles
  collide with document entities through those entities' materialised colliders,
  built from their document geometry and kept in step by `Provenance`, as meshes
  are.
- **The language model sees the declaration** and the drivers' names (below),
  never particle state.

## 3. Writing back: three channels

| Channel | What it is | Saved | Undo | Right for |
|---|---|---|---|---|
| Transaction (`PendingTransactions`) | An authored edit with a label and an actor | yes | yes | Discrete, meaningful changes: "the protein denatured", "bound a ligand" |
| Gesture preview (`Store::preview`) | One owner's in-progress values (`gesture`) | no | commits as one | The pointer only |
| **Runtime driver (proposed)** | Values a named driver writes every frame into a derived layer | no | no | Continuous influence from a simulation: hit counts, pressure, temperature, a jiggle |

The third channel does not exist yet.

- **It already has two siblings.** The reactive graph has an *animated* layer
  (clips, `Reactive::set_animated`) and a *physics* layer (`set_physics`), read
  beneath gesture previews and above authored constants. A runtime layer is a
  third: `Store::drive(driver, leaf, value)` and `Store::release(driver)`.
- **Behaviour follows from that.**
  - Bindings, constraints, signals and port behaviours downstream react in the
    same frame, as they do to a clip.
  - The model's view marks the leaf `driven-by="gas"`, as it marks clip targets
    (`<driven-by>`).
  - A driven leaf has no direct handle, as a clip-driven one has none. A gesture
    on it is refused, or pins it, which is the same open question as for clips
    (doc 05 §5.2).
  - Nothing is saved; stopping the driver drops its values and the authored ones
    show again.
- **It is the "driver on a free input port" of
  [`time-machines-and-scenarios.md`](time-machines-and-scenarios.md) §2.** The
  simulation clock is one kind of driver; the gas is a driver on the protein's
  input port.

## 4. The protein, end to end

1. **The protein's definition** declares an input port, say `impacts`: a one-way
   signal of collision rate or impulse (ECM §6.1). Its behaviour reads the port
   through bindings or equations. A `strain` parameter might follow the impulse,
   a colour might follow the strain, and a clip might play a wobble when the rate
   crosses a threshold.
2. **A Bevy collision system** steps the particles and, per frame, sums the hits
   on each document entity that has an `impacts` port. Particle-level work stays
   in Bevy; only one number per protein crosses.
3. **It writes the sum through the runtime layer**:
   `store.drive("gas", Leaf(protein.impacts.signal), value)`. Everything bound to
   it updates this frame, in every composition that pictures the protein: the
   cartoon, the schematic, the 3D view.
4. **A discrete outcome is a transaction.** If a hit should change authored state
   (the protein unfolds and stays unfolded), a listener issues one: labelled,
   rate-limited, actor `System`. It is undoable, and the model hears about it as a
   cause.
5. **Other Bevy listeners** (a sound, a particle burst at the impact point) observe
   the same collision event directly and never touch the document.

## 5. What would need building

- **The change feed as Bevy messages and entity events** (§1). Small: the data is
  already computed in `sync_document` and `reconcile_gesture`.
- **The runtime layer** in `iron_document::graph` and `Store`, keyed by driver:
  `driven-by` in views, handle locking, and clearing on stop (§3).
- **Collider sync** for document geometry, whatever physics crate is chosen.
- **Simulation time:** which clock a driver runs on (the playhead or free-running),
  and recording runs so a simulation with memory can be scrubbed (time-machines
  §1).

**Open:** whether a driver may write *authored* leaves through the runtime layer
(overriding them while it runs), or only port signals and leaves declared as
drivable. Ports only is cleaner, because a definition then says what can be driven.
