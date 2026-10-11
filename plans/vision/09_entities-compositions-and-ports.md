# Entities, compositions, and ports

**Status:** Vision draft
**Date:** 2026-10-09
**Scope:** How one thing, such as a lithium niobate modulator, is represented
consistently across many diagrams at different levels of abstraction (an electrical
schematic, a 3D assembly, a cartoon block diagram); how things connect through typed
ports that carry signals and imply placement; and where lessons and annotations sit in
relation to both. Written from a design conversation on 2026-10-04 to 10-09 that
clarified how the tool is meant to be used.
**Relates to:** doc 02 (Model, Projection, Annotation; components; editors as data),
doc 04 (the authored store; views for the model), doc 05 (three kinds of time), doc 06
(units; enumerated choice), and the implementation plans
[`../entity-composition-model.md`](../entity-composition-model.md) and
[`../constraint-system.md`](../constraint-system.md).

---

## 0. Theses

1. **A thing is not its picture.** An *entity* (the modulator) is separate from each of
   its *figures* (a schematic symbol, a CAD body, a cartoon box). Figures point at
   the entity; they never point at each other.
2. **A composition is a world with a notation.** A project holds many compositions in
   parallel, 2D, 3D, or mixed: an electrical schematic, an optical-table assembly, a
   block diagram, a heat-flow diagram, a state machine. A notation's palette and
   conventions push a composition toward one kind of diagram.
3. **Ports are an entity's interface.** A port is a named, typed point where something
   the explanation cares about is exchanged: light, RF, heat, data, or an abstract value
   with no physical connector at all.
4. **One connection, two consequences.** Connecting two ports creates a one-way signal
   edge, directed by the physics, and may imply a geometric constraint, solved in any
   direction by whoever is dragging. Physics flows through signals and models; setup and
   placement go through constraints.
5. **Most geometry is local.** Mate faces, brackets, posts, and screws live only in
   their composition. Only what matters across representations becomes an entity or a
   port, and something gets promoted when an explanation needs it.
6. **Annotation belongs to the lesson, and points at the thing.** An arrow's *owner* is
   a lesson step; its *target* is an entity, port, or signal.
7. **Moving between levels of abstraction has three outcomes.** Going from detailed to
   abstract can always be derived. Going from abstract to detailed needs either a choice
   or new information. The last case becomes a visible obligation, never a silent guess.
8. **Encapsulation is precomposition with an interface.** Wrapping a selection makes a
   parent entity whose ports are the connections that crossed the boundary, and nesting
   behaves the same at every depth.

---

## 1. Usage intent, as clarified

Concrete situations this document must serve:

- **A modulator in four places.** In an electrical schematic it is a symbol with an RF
  input. In a 3D assembly it is a CAD body mated into a fixture. In a lesson diagram it
  is a cartoon box with two light ports and an RF port. There, an animated RF drive
  going in visibly modulates the time-domain light coming out, and the same waveform
  plays wherever else the modulator is shown.
- **Laser → fiber → power meter.** Each is a component built from visual elements with
  a small typed API. Power flows from laser to meter through the fiber's transfer
  function. In its simplest form that is one float; it can grow to a time series or a
  spectrum.
- **A lens focusing a laser, in 3D.** The lens connects to the beam, which constrains it
  to the beam axis. The user drags it along the only line of valid positions while the
  focus at the detector updates.
- **A grounding bolt.** It is a ground symbol in the schematic and a physical bolt in
  the assembly. The schematic says nothing about where the bolt goes, so the tool must
  say "this needs placing" rather than inventing a location.
- **A bracket.** It has several faces that things clamp to and no semantic role. It
  should never generate ports or ghost lines.
- **A pendulum, charged particles.** Mechanisms you can grab anywhere. A simulation
  runs at 60 fps and responds when you take hold of one part (see
  `constraint-system.md`).
- **Two authors.** The user designs, edits, and drags. The LLM authors much of the
  structure, and is told about causes, not every inferred effect.

The direction this sets for the tool: guide users toward **models plus signal-based
inference and visualisation**, keep CAD-style **constraints for setting things up**,
and make **linking representations across levels of abstraction** a first-class
activity, not an afterthought.

---

## 2. The ontology

| Concept | What it is | Lives in | Drawn? |
|---|---|---|---|
| **Entity** | One thing being described: a modulator, a laser system, a fiber, a variable in a model | The model tree | Never directly; only through figures |
| **Definition** | The reusable type of an entity: its ports, parameters, local state, behaviour, and default figures per notation. Doc 02 §5.3's "Component", renamed (§3) | A library (built-in, user, or LLM-authored) | No |
| **Port** | A named, typed interface point on an entity | Under its entity | Via port anchors on figures |
| **Connection** | Port to port, typed by domain (optical, RF, thermal, data, …) | Between entities | Wherever a composition's notation shows that domain |
| **Signal** | The value on a port: a quantity with units, a structured value (a Gaussian beam), or a time series | Derived at runtime; only sources are authored | Visualised by figures that choose to |
| **Composition** | A 2D, 3D, or mixed world with a notation | A top-level node | Yes, in a panel |
| **Notation** | A palette of figure types, the port domains drawn as wires, layout conventions, and signal visualisations | A library (data) | No |
| **Figure** | A visual node in a composition that depicts an entity and maps its anchors to the entity's ports | Inside a composition | Yes |
| **Local geometry** | Visual content with no entity: brackets, posts, decoration, mate features | Inside a composition | Yes |
| **Refinement** | An entity contains sub-entities, or realises an abstract one (cartoon "laser system" → laser, isolator, collimator) | The model tree, plus `realizes` relations | No |
| **Lesson, step, annotation** | The narrative: ordered steps, each deciding what is visible, highlighted, animated, and said | A lesson tree | Annotations are drawn in the composition a step places them in |

Two rules keep the model honest:

- **Linking is hub-and-spoke.** "Link this symbol to that 3D part" connects both
  figures to one entity, and creates the entity if neither had one. Pairwise links
  between figures would multiply into relationships that can disagree.
- **Promotion is cheap and on demand.** A rectangle drawn in a composition is local
  geometry until someone links it, gives it a port, or connects it. Nobody has to
  create entities up front.

---

## 3. How this maps onto the existing vision

- **Doc 02's three terms.** *Model* is the behaviour of entities: built-in transfer
  functions, simulations, or external call-outs, fed and read through ports. *Projection*
  is compositions, figures, and data-bound visuals. *Annotation* is the narrative
  layer (§9), which doc 02 §2.3 already called "the part of the work that does the
  explaining". The entity is the new piece: the identity that all three share.
- **Doc 02 §5.3 "Components"** (parameterised reusable units with local state) become
  **definitions**. The rename removes the clash with ECS-style `component`s in
  `iron_document`, such as `transform2d`. A definition's parameters plus its output
  ports form the typed interface §5.3 asked for. Its body is one or more internal
  compositions, reached by drilling in.
- **Doc 01 §10's pedagogical links** ("a 2D label targets a 3D part", "an animation
  track coordinates a 2D schematic with a 3D mechanism") are annotations targeting
  entities, plus signals shared across figures.
- **Doc 04's views** remain the *model's reads* of the document. Compositions are not
  views: they hold authored layout. The LLM gets new view relations: the model tree,
  one composition's layout, and one lesson.
- **Doc 06 §5 (units).** Ports make units unavoidable: power in mW, wavelength in nm,
  an RF drive in V. The units decision moves to the front of the queue.

---

## 4. Navigating and authoring across representations

Two directions of movement, kept distinct in the interface:

- **Across**: the same thing in a different notation (schematic ↔ 3D ↔ cartoon).
- **Down and up**: refinement ("laser system" → its parts → individual CAD bodies).

Mechanisms:

| Mechanism | Direction | Behaviour |
|---|---|---|
| **Show in ▸ (notation)** on any figure | Across | Opens that composition centred on the counterpart, highlighted. If there is none, offers to place one |
| **Representations panel** | Across | For the selected entity, lists every composition depicting it, its ports, and its live signals |
| **Linked split view** | Across | Hovering or selecting in one composition lights up the counterpart in the other. The teaching mode |
| **Drill in, breadcrumb out** | Down and up | Double-click an encapsulated block to open its internal composition |
| **Encapsulate selection** | Up | Creates a parent entity. Connections crossing the boundary become its ports; the selection collapses into one block |
| **Unplaced tray** | Across | Entities added in one composition that another notation should show wait in a tray, like KiCad's "update PCB from schematic" |
| **Obligations list** | Across | Connections or entities a composition should realise but does not (§5) |

Refinement and figures combine. A cartoon may show "laser system" as one box while
the 3D view shows its three parts. Selecting the box highlights all three, because they
realise it.

When users and agents cross between representations, and which mechanism serves them:

1. **Identification** — "which box on the table is this symbol?" Show in, highlighting.
2. **Explanation** — abstract and concrete side by side, animated together. Linked views.
3. **Propagating authoring** — "I added an amplifier to the schematic; where does it go
   physically?" The tray.
4. **Checking consistency** — "is everything in the schematic actually cabled?"
   Obligations and ghost lines.
5. **Agent work** — the LLM works mostly at the entity level ("connect the RF amplifier
   to the modulator's RF port") and asks about layout only when needed.

---

## 5. Consistency between levels of abstraction

Every connection, and every entity, has one of three statuses in each composition whose
notation should show it:

| Status | When | What the tool does |
|---|---|---|
| **Derivable** | The composition is less detailed than where the fact was authored (a fiber placed in 3D implies the optical connection in the cartoon) | Computed automatically. Nothing to ask |
| **Realisable with a choice** | A rule or default could fill the gap (the connection is obvious from port types, but a position is a guess) | Offered as a suggestion; applied on confirmation |
| **Unrealised** | New information is needed (the ground symbol says nothing about where the bolt goes) | An obligation: "ground connection #c7 has no physical realisation in the table assembly", shown in a list and optionally as a ghost line |

Obligations are derived, not stored. Only an explicit "intentionally omitted" mark is
stored, because a simplified teaching diagram often leaves things out deliberately. How
strictly to enforce consistency (warn, block, or only hint) is deferred until the system
is built far enough to try it.

This is the lens pattern from bidirectional transformations. Deriving the abstract view
from the detailed one is *get*. Pushing an edit made in the abstract view back to the
detailed one is *put*, which needs the old detailed state to fill in what the abstract
view does not say. When there is no old state to draw on, put cannot run, and that is
exactly when an obligation appears.

---

## 6. Ports in general

**A port is semantic, not geometric.** It is where something the explanation cares
about crosses an entity's boundary. A **mate feature**, such as a face, axis, or hole
used for positioning, is geometric. It lives only in its 3D composition, belongs to the
constraint system, and never appears anywhere else.

- Some features are both. A fiber connector's face is where light crosses (a port) and
  where the connector seats (a mate). The port kind declares the mate it implies, so
  connecting the ports creates the mate.
- Most mate features have no port. A bracket has several and none.
- **Whether something is a port depends on the explanation.** In a lesson about heat,
  the face a bracket clamps to becomes a thermal port, because heat flows through it and
  the lesson is about that. So promotion is cheap, and nothing is decided at import time.
- An abstract API, such as a model's output value, is also a port, with no visual
  connector at all.

**A port kind** (built-in vocabulary, extensible) declares:

- **Domain and data type.** Optical power in W; a Gaussian beam {waist, waist position,
  wavelength, M², power}; RF voltage as a time series; heat flow in W; a plain number.
- **Direction.** Out, in, or both (for conservative exchange such as heat; see §13).
- **Compatible partners**, and adapters (a constant power lifts to a flat time series).
- **Implied constraints per notation.** In 3D, `fiber_fc` implies coincident and
  concentric; `optical_axis` implies "on the beam line, parallel"; in 2D schematics,
  nothing.
- **Default glyph per notation.** An arrow, a connector, or a pin.

**CAD import is tagged in two tiers.** Geometric features (cylindrical holes, planar
faces, axes) are detected automatically and become mate candidates. Ports are semantic
tags that someone authors, or that an LLM proposes from part names and catalogue data:
a vendor part number says a lot about what a part is for.

**Ghost lines stay sparse.** Only entity-level connections can be unrealised, and mates
are never entity-level, so an assembly of brackets produces no rat's nest.

---

## 7. Signals and models

- **Signals are one-way and typed.** A connection is a signal edge from an out-port to
  an in-port. An entity's behaviour maps in-port values and parameters to out-port
  values. The fiber's attenuation is a floor function; a modulator's response may be a
  floor function or a Model.
- **Built-in model primitives run in the frame loop.** Optical propagation, Gaussian
  beam transforms, linear circuits, and rigid-body or particle dynamics are floor-tier:
  pure, bounded, in-process, 60 fps (doc 02 §6). External Models remain the slow,
  cost-tagged tier.
- **Signals are visible everywhere.** Because a signal lives on the entity's port, every
  figure can visualise it. The RF waveform animates in the cartoon and in the
  schematic from one source.
- **Authored versus derived.** Source values (the laser's set power) are authored
  parameters. Propagated signals are derived and never written into the file. Time
  series and spectra are runtime data, too large and too derived to store.
- **Ordering follows the physics.** In an optical train, signals flow with the light.
  Constraint outputs (the lens's position along the beam) feed signal inputs (the spot
  size at the detector), and signal outputs (the beam axis) feed constraint inputs (the
  line the lens may slide along). The light's direction keeps this acyclic. A
  configuration that truly loops, like a cavity, needs a simulation, not a binding.

---

## 8. Constraints

Constraints position things within a composition. A drag assigns roles at interaction
time: grabbed, free, fixed, bound. Connections can imply constraints (§6), so connecting
a lens to a beam creates the rail it slides on. Constraints are composition-local, like
mate features: a mate in the 3D view says nothing to the schematic. The design is in
[`../constraint-system.md`](../constraint-system.md).

---

## 9. The narrative layer

Annotation is the third term of doc 02 and deserves its own structure.

- **A lesson** is an ordered set of **steps**, which is scripted time (doc 05). Each step
  decides which compositions are visible and how they are framed, what is highlighted,
  which annotations appear, and which signals animate.
- **An annotation has two relationships.** Its *owner* is the step it belongs to. Its
  *target* is an entity, a port, a signal, or a region. Targets go through entities,
  not figures, which gives annotations useful behaviour for free:
  - they re-anchor when layout changes;
  - they can be *carried* into another composition ("show this callout in 3D too")
    by resolving the target to its figure there;
  - they can be queried by subject: "what does this lesson say about the modulator?"
- **They do not spread automatically.** An annotation appears where its step places it,
  not in every composition that depicts its target. Placement is part of the lesson's
  design.
- **Two kinds of annotation.** A *data-bound* annotation shows a value ("beam waist
  30 µm", bound to a signal) and is a binding into the model. A *claim* asserts meaning
  ("the modulator converts RF into optical phase") and links to an entity, and
  optionally to sources (doc 04's `explains` and `cites` links).

---

## 10. Notations as data

A notation bundles:

1. a palette of figure types (with default figures per definition);
2. the port domains it draws as connections, and how (orthogonal wires on a grid,
   physical cables in 3D, animated arrows);
3. its layout and constraint conventions (snap grids, 3D mates, free placement);
4. how it visualises signals (a scope trace, a pulsing beam, a coloured heat arrow).

Notations are data built from shared primitives, so users and the LLM can define new
ones, just as they can build their own editors (doc 02 §9). A composition can mix
notations, which is how a diagram becomes partly 2D and partly 3D.

---

## 11. Laws

From applied category theory: string diagrams, operads and wiring diagrams, black-boxing
functors, and lenses. These are design rules and a test list, not user-facing
vocabulary.

1. **Typed composition.** A connection exists only between compatible port kinds, or
   through a declared adapter.
2. **Associative encapsulation.** Encapsulating A and B, then that with C, is the same
   as encapsulating A, B, and C at once, in ports and in behaviour.
3. **Figures preserve connections.** If two entities are connected and both are
   depicted in a composition whose notation shows that domain, their figures are
   connected, or the connection shows as an obligation.
4. **Lens round trips.** Pushing an unchanged abstract view back changes nothing (*get*
   then *put*), and an edit pushed back is what you see when you derive again (*put*
   then *get*).
5. **Black-boxing respects wiring.** An encapsulated entity's port behaviour equals the
   composition of its parts' behaviours. Replacing a subsystem with its black-box model
   changes nothing outside.
6. **Identity is stable.** An entity keeps its id across compositions, encapsulation,
   refinement, and model edits. Figures and annotations never lose their target.

---

## 12. What the language model sees

- **Mostly entities.** The model tree view, with entities, ports, connections, and
  signal summaries, is compact and semantic. Composition views are layout, read when
  the task is about placement.
- **Causes, not effects.** A human drag reports the driver and the re-solved clusters
  (`constraint-system.md` §9). A propagated signal change is never pushed.
- **Obligations are queryable.** "Realise the open items in the table assembly" is a
  well-formed task.
- **Lessons are a view.** The model can read and write steps and annotations without
  touching layout.

---

## 13. Open questions

1. **Strictness of consistency**: warn, block, or hint. Deferred until there is a
   working system to try (§5).
2. **Conservative ports.** Heat and current are exchanged, not sent. Modelica handles
   this with across and through variables. Do we need it, or are directed signals plus
   explicit models enough for teaching?
3. **Definition scope.** Built-in definitions in code, user and LLM definitions in a
   project library, a shared library across projects. Where do they live, and how are
   they versioned?
4. **Default figures.** When a definition is placed in a new notation, does it get a
   generated default (a labelled box with its ports) or require one?
5. **CAD pipeline.** Which formats (STEP, glTF from SolidWorks), and where feature
   detection runs (in the worker, or offline at import).
6. **Many-to-many realisation.** One schematic ground realised by a chassis plus three
   bolts: is `realizes` enough, or does it need structure?
7. **Lesson branching.** Linear steps first. Do lessons ever branch or respond to the
   learner's interaction?
8. **Naming.** "Composition", "entity", "definition", "depiction". Settle them before
   they reach the UI. **Decided 2026-10-11:** depiction was renamed figure on
   2026-10-11 (the relation is `figures`; a definition's template child is a
   `template`); the other three stay.

---

## 14. Prior art

| Source | Taken |
|---|---|
| **KiCad and PCB tools** | Symbol and footprint as two figures of one component (reference designator, pin-to-pad map); cross-probing; "update PCB from schematic"; the ratsnest |
| **Revit / BIM** | One model, many views; families with per-view, per-detail-level geometry |
| **SysML** | One model, many diagram types; ports and item flows |
| **Capella / Arcadia** | Levels of abstraction with "realizes" links between them |
| **Simulink** | "Create subsystem from selection": boundary signals become ports |
| **After Effects** | Precomposition: the gesture, minus the interface |
| **SolidWorks** | Mate references and SmartMates: a part pre-authored with how it mates |
| **Modelica** | Typed connectors whose connection generates equations |
| **Moody, "The Physics of Notations"** (2009) | Principles for designing each notation, including cognitive integration across diagrams |
| **Ainsworth, DeFT** (2006) | Learning with multiple representations: learners need the correspondence made visible, through linked highlighting and synchronised animation |
| **Fong & Spivak; Baez & Fong; Coecke & Kissinger** | Wiring diagrams, black-boxing functors, string diagrams for physics |
| **Lenses** (bidirectional transformations) | get and put, and the round-trip laws |

---

## 15. Amendments

- **Doc 02 §5.3**: "Components" are renamed **definitions**. Their body is reached by
  drilling into internal compositions.
- **Doc 02 §2.3 and doc 01 §10**: annotation is realised as the narrative layer of §9,
  with owner and target kept separate.
- **Doc 06 §5**: the units decision is required before ports. See
  `entity-composition-model.md` §2.
- **Doc 04 §6**: views gain model-tree, composition, and lesson relations.
