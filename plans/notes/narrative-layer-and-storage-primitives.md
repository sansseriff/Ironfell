# The narrative layer and storage primitives

**Status:** Note: a design conversation in progress, not a plan. Nothing here is
adopted. It records where the conversation has got to, which way it leans, and what is
still open, before the decisions on saving, a database and a server are taken.
**Date:** 2026-10-25
**Context:** A conversation about what the primitive saved objects are in each of
Iron's layers: the projection layer, the model layer, and the pedagogy / project /
lab-notebook layer above them. It builds on
[`projects-labs-and-server.md`](projects-labs-and-server.md) (projects, the lab twin,
the server), [`time-machines-and-scenarios.md`](time-machines-and-scenarios.md)
(machines with ports, drivers, the scenario/observation split), the document spine
([`../specs/document-spine.md`](../specs/document-spine.md) §5–7), the component model
([`../specs/component-model.md`](../specs/component-model.md) §3, §5, §10), the model
interop proposal ([`../specs/model-interop.md`](../specs/model-interop.md)), vision 02
§2 and §6, and vision 09 §2, §5 and §9. It revises ECM §10 (the lesson layer as
lessons → steps) and vision 09 §9.

---

## 1. The question

The long-term goal is a saving, database and server architecture. Before choosing
one, decide what the minimum stored unit is in each domain: something like "a row".

- **Projection layer** (compositions, figures, components): the starting intuition was
  "a component is a row".
- **Model layer** (external computation, from an equation edited live to an agent that
  designs a CAD part): the intuition was that models are fairly clearly rows, as API
  calls with schemas.
- **The layer above both** (lessons, tests, project goals and timelines, lab notes):
  unclear. It is about *how the things below change and are navigated*, not the things
  themselves. Teaching and project management look structurally alike: both move assets
  through states and up and down levels of abstraction (sketch ↔ 3D ↔ schematic), and
  neither does it in a straight line.

This note calls the third layer the **narrative layer**. Earlier documents call it the
lesson layer or the annotation layer. It covers lessons, discovery fiction, lab
notebooks and project processes.

## 2. Projection layer: four different units

"The minimum unit of saving" covers four separate units. Keep them apart:

| Unit | Job | In the projection layer |
|---|---|---|
| **Row** | Storage granularity | node, facet, relation (the file's flat tables, `serial.rs`) |
| **Change** | Sync, undo, history | an op inside a transaction |
| **Write boundary** | What syncs and versions together, and who may write it | a document: a project, a library |
| **Reuse** | What is published, pinned and updated | a component definition |

**Leaning:** a component is the unit of **reuse**, not the storage row. A definition is
a subtree, and a placed instance stores only its definition, arguments, overrides and
connections, with the insides expanded and never saved (component-model §3, §5.1).
Making a component one row would invite storing the expansion.

The document is already relational in shape: canonical JSON of three flat tables. The
XML is only the language model's *view* (`view.rs`) and is never stored (spine §7).

### 2.1 Compositions and views

**Leaning:** compositions are part of the saved schema. A composition is the world
itself (authored layout: figures, local geometry, annotations, constraints), not a
window onto one. The "views of compositions" are the **workspace** tier (panel layout,
pans, zooms), saved per user beside the document, never shared
([`../specs/panels-and-session.md`](../specs/panels-and-session.md)).

## 3. The narrative layer: arrows, not steps

### 3.1 Why not steps

The first proposal was a **step**: a node holding a state reference, an observation,
annotations, drivers and an optional criterion, with lessons as graphs of steps. It was
rejected as not general enough:

- A step is a node, a place to stop. What this layer describes is *movement between*
  nodes.
- Real work is not linear. Projects bounce between sketch, schematic and 3D, and
  sometimes return to the start because something was learned. "A pinned version in a
  history" assumes a line.
- It is not obvious how long a step is. In a 3Blue1Brown video the content is
  continuous transformation, and divisions are not natural.

### 3.2 States

**Leaning:** a **state** is a snapshot of the projection documents, kept in an
**unordered, content-addressed map** (hash → state). No time order is imposed on it.

- Two paths that arrive at the same design reach the same key, so returning to an
  earlier design returns to the same node, and the graph may have loops.
- Expressing one state as **overrides over another** is a storage encoding, not
  meaning. A lesson can start from a finished product with a stack of overrides that
  take it back to its incomplete start, and proceed by removing them one at a time. It
  reuses the component model's override rules: overrides keyed by stable id, kept as
  stale rather than deleted when their target disappears.

### 3.3 Moves

**Leaning:** the primitive of this layer is the **move**, an arrow from one state to
another. It carries:

- a kind: designed, tried, measured, explained, abstracted, refined, …;
- provenance: **recorded** (it happened) or **authored** (made up for teaching);
- optionally a duration: a continuous animation is one move interpolating from A to B;
- references to what realised it: transactions in the op log, a model run, a
  measurement.

Moves compose: a path of fine moves can be summarised as one coarse move ("built
prototype v1"). The op log is the finest grain of recorded moves. An authored move may
have no op path at all (a lesson cutting from a broken design to a fixed one).

**Steps dissolve.** The joints between moves are wherever someone might want to land.
The criterion for a joint is **referencability**: could someone link to it, comment on
it, grade it or return to it? Granularity is chosen per narrative, and coarse and fine
narratives can share the same moves.

### 3.4 Knowledge separate from design

Design state can loop. **Knowledge mostly accumulates.** When a project "goes back to
the beginning" because of a discovery, the design returns to the sketch but the
knowledge does not. So knowledge is kept apart from design state:

- A **claim** is a statement about entities: hypothesis, observed, established. An
  **observation** is a claim pinned to one state with evidence (a measurement or a
  run).
- A **goal** is a **condition on states**, not a point on a path. Tests and lesson
  exercises are goals with a check.
- Learning something usually **tightens a goal's condition**: states that used to
  satisfy it no longer do. That is why returning to a sketch need not be regress: the
  target region shrank, and the reason is recorded. Learning may also spawn a subgoal
  ("find out why"), linked to the claim that prompted it.

There is a resemblance to vision 09's **obligations** (abstract → detailed needs new
information, shown as a visible gap). "Put a circuit board inside the enclosure" reads
like an obligation on the enclosure entity, plus a criterion.

### 3.5 Narratives

A **narrative** is an ordering over moves, claims and states, with framing. The same
schema covers each use, differing in provenance and ordering:

| Narrative | Moves | Ordered by |
|---|---|---|
| Lab notebook | recorded | wall-clock time |
| Lesson | authored | teaching |
| Discovery fiction | authored, shaped like a notebook (idea → try → observe → learn) | teaching |
| Project timeline | recorded, plus goals not yet met | time and dependency |

"Teaching as an idealised simulation of the real world" becomes literal: discovery
fiction is a notebook with the provenance flag flipped. Turning a real notebook into a
lesson is editing a narrative: choosing, reordering, smoothing away failed attempts.

**Integrity rule (leaning):** recorded moves and observations are never edited in place;
authored ones are freely editable.

## 4. What a "thing" is

Example: a motor from a humanoid robot's leg reused as an actuator in a car. The same
physical part has entirely different meaning. Three kinds of sameness:

1. **Type:** the motor model, in Iron a definition.
2. **Individual:** this unit, serial number 123, whose identity survives being moved.
3. **Role:** "knee actuator", "steering actuator": a slot in one wiring diagram.

**Leaning:** meaning is **relational**. A role is a map from a slot's interface into the
part's ports, usually injective (the car uses power and torque and ignores the
encoder). Moving the motor changes which maps point at it, not its identity. Identities
stay thin and meaning lives in relations.

Iron today has type (definition) and the thing-versus-its-picture split (entity versus
figure), but no individual-versus-role level. Where individuals live relates to the lab
twin question in `projects-labs-and-server.md` §3.

### 4.1 Category theory, as a design check

Used to check designs, not as user vocabulary:

- **An object is known by its relations** (the Yoneda idea): thin identity, thick
  relations. This is the motor case, and why teaching feels "reference-y".
- **Two kinds of composition.** Things compose by **wiring** (components with ports;
  operads and wiring diagrams; "everything is components connected by signals"), which
  Iron has. Processes compose by **sequencing** (moves chained into paths, `seq`/`par`),
  which the time-machines note sketches. The narrative layer is the second over the
  first. Narratives compose too (a course of lessons, a coarse move broken into fine
  ones), but by paths, not containment, and one move can belong to many narratives.
- Prior art: Spivak and Kent's *ologs* (2012), a categorical framework for scientific
  knowledge representation; functorial data migration / CQL (a schema as a category,
  for "what a thing is changes"); AlgebraicJulia.

### 4.2 Hierarchy and "ownership"

Words like "owns" and "contains" imply hierarchies the narrative layer should not have.
**Leaning:** keep hierarchy only for the storage job of **write boundaries** (who may
write a value, what syncs and versions together), which something must decide. Put all
meaning in relations. "Is a lesson its own document?" then becomes "which write
boundary does it sit in?", settled on practical grounds.

## 5. Text as the surface

A goal of Iron is for narrative text to drive, or be closely linked to, state and
models. Narrative expresses intent, reasoning, worries and plans, usually in prose.

**Leaning:** written text is the **authored surface and the spine** of the narrative
layer: its linear order is the narrative order, and the joints are wherever a reference
sits in it. Underneath it, the graph of states, moves, claims and goals is what is
stored and queried. Text marks spans that refer to those objects; the objects live
outside any one text, so one claim can be cited by a notebook, several lessons and a
project page.

### 5.1 A sketch

Enhanced markdown using the generic-directives syntax (`:name[text]{attrs}` inline,
`::name{attrs}` as a block; remark-directive parses it). `@name` aliases a state hash;
`#name` is an entity, claim or goal id. Abridged:

```markdown
---
title: Getting light into a fiber
provenance: authored
start: @bare
---

We have a :e[10 mW laser]{#laser} feeding a :e[single-mode fiber]{#fiber} and a
:e[power meter]{#meter}. :goal[Get at least half the light through]{#g1
when="#meter.in.power >= 5 mW"}.

::view{show=bench-3d}

:go[We put an 8 mm lens in front of the fiber]{to=@lens8 dur=3s} and
:go[walk the fiber end into the focus]{to=@aligned dur=4s}.
:obs[The meter reads 3.2 mW]{#o1 of=#meter.in.power at=@aligned evidence=run:7f3a}.

:claim[Maybe the alignment is off]{#h1 status=hypothesis about=#fiber}.
:go[Scan the fiber end across the focus]{scan=#fiber.end.x range=±5µm dur=6s}:
:obs[the power peaks where we already were]{#o2 evidence=run:7f3b}, so
:refute[it isn't alignment]{#h1 by=#o2}.

::view{show=beam-sketch}

:claim[A single-mode fiber only accepts a spot matching its mode, about 10 µm
across]{#c1 about=#fiber status=established cites=datasheet:SM980}. Ours is
:val{#beam.waist fmt="0.0 µm"}. :goal[Match the spot to the mode]{#g2 serves=#g1
from=#c1 when="abs(#beam.waist - 5 µm) < 0.5 µm"}.

:run[The mode-overlap model]{model=gauss-overlap in=#beam,#fiber out=#coupling}
:pred[predicts about 62% with an 11 mm lens]{#p1 at=@lens11}.

:go[Swap in the 11 mm lens and realign]{from=@aligned to=@lens11 dur=4s}.
:obs[6.1 mW]{#o3 of=#meter.in.power at=@lens11 evidence=run:7f3c}:
:met{goal=#g1} :met{goal=#g2}, and :support[the model was right]{#p1 by=#o3}.
```

| Symbol | Primitive | While reading |
|---|---|---|
| `:e` | entity reference | highlights the entity's figures in visible compositions |
| `::view` | framing change | changes what is shown; design state unchanged (moving the eye between abstraction levels) |
| `:go` | move | transforms design state; its span is the move's duration, so reading plays it |
| `:obs` | observation | a claim pinned to a state with evidence; fixed even if the model changes |
| `:claim` | claim | hypothesis or fact; with `when=` it is checked live |
| `:refute`, `:support` | evidence → claim | makes "what we learned" queryable |
| `:goal` | goal | a condition on states; `serves=` / `from=` record an unplanned subgoal and its cause |
| `:run`, `:pred` | model invocation, prediction | a prediction sits beside an observation for comparison |
| `:val` | live value | text whose value comes from state |

**Both directions.** Reading drives state: scroll position acts as a playhead, a `:go`
span plays its move, a `::view` reframes. State drives text: `:val` updates, and a live
claim whose condition stops holding after the model changes is flagged, as stale
overrides are. Observations stay fixed: they record what was true at their state.

**The markdown is one encoding, not the store**, as the XML is for the document
(vision 02's co-equal encodings). Ids in the directives keep references and comments
stable while the prose around them is rewritten.

### 5.2 When to formalise text

Formalising follows one rule: **promote a piece of text when it needs a life outside
its paragraph**, because something will refer to it, check it, answer it or act on it.
This is the same test as for joints (§3.3). Levels:

0. **Prose.** Most text, permanently.
1. **A reference** (mention, live value, view change): no new object.
2. **A node** (claim, result, issue): an object with an id others can cite, answer,
   claim or credit.
3. **A checked node** (a goal or claim with a condition evaluated against state).

Each level is worth its cost only when something uses it.

**Discourse graphs as evidence** ([Chan et al. 2024](https://arxiv.org/html/2407.20666)).
Joel Chan's discourse graphs use Question, Claim, Evidence and Source, linked by
informs, supports and opposes, formalised incrementally (select text, hotkey, pick a
type). Findings from a deployment of about three years:

- Matt Akamatsu's cell biology lab added **HYP**, **RES** (result), **CON**
  (conclusion) and **ISS** (issue, open work). **Experiments were pages, not nodes**:
  each held its motivating questions, with outputs summarised as result nodes.
- A PhD student at first resisted making result nodes. After a synthesis session the PI
  said the process exposed what the student had been "keeping everything in his head",
  with confusions and duplicates.
- An issue node was claimed by another lab member about 30 seconds after it was made:
  identity enables action.
- Explicit relations stayed sparse; most connections came from informal links.
- The design follows Conklin and Shipman: early or required formal structure harms
  open-ended work, so the vocabulary is small, formalising is incremental and optional,
  and users can add types.

Older lineage: Toulmin's argument model; IBIS (issues, positions, arguments; 1970s
design rationale, relevant to project management); micropublications and
nanopublications.

**Two families of types (leaning):**

| Family | Types | Needs the engine? |
|---|---|---|
| **Discourse** | question, claim, hypothesis, conclusion, issue, source | No: close to discourse graphs' grammar |
| **Grounding** | entity mention, observation/result, prediction, goal with condition, move, view | Yes: binds text to state and models; Iron's contribution |

In a discourse graph, evidence is text pointing at a paper's figure. In Iron it can
point at a state, a measurement or a model run, so it can be inspected and replayed.

**Should an LLM infer structure at query time instead?** It lowers the cost of
formalising and moves the line, but an inferred node has no stable identity: it cannot
be commented on, claimed, assigned or credited. Leaning: the LLM proposes promotions
("this reads like a hypothesis about `#fiber`; mark it?") and a person accepts.

## 6. Text, timeline and state machine

| Tool | Decides | Who drives |
|---|---|---|
| **Text** | *what* happens and in *what order*, coupled to reading | the reader's progress |
| **Timeline** | *how* a change looks: easing, staggering, overlap, durations, time maps | the clock |
| **State machine** | *what can happen* when someone acts: branches, try-it-yourself | the viewer's input |

**Leaning: a timeline is the inside of a move.** A `:go` with nothing else is a default
tween between two states: entity ids persist across states, so a state diff names the
values that change, and they interpolate (Keynote's Magic Move is the prior art). A
timeline is opened only when the default is not enough:

- several things must be offset or staggered;
- the motion itself is the content (a beam path, a trajectory);
- it must land on specific beats of narration or audio;
- physical time must be remapped ("slow down 10¹²× here").

This is progressive refinement, as for text: a move stays coarse until refined. It
reuses Iron's clips and the time-machines note's `seq`/`par` timing.

**Which one leads can flip.** In a notebook, article or scrolling explainer, text is
the spine and moves hang off spans. In a 3Blue1Brown-style video the picture is the
spine and narration is text anchored to time ranges. Both are one relation between text
spans and time ranges, with either side as the master clock (Descript is the prior art
for editing a transcript to edit the video). **State machines** (Rive) come in where the
author stops driving, such as a learner choosing which lens to try. Linear text cannot
express that.

## 7. Model layer

The range from an equation edited in a composition to an agent designing a SolidWorks
part is one idea already in vision 02 §6.3: one call node whose identifier resolves to
the floor (built-in Rust), user wasm, or a remote call, with a cost tag. Model interop's
three tiers (jobs, live sessions, local acceleration) cover the remote end.

**Leaning:** store three things:

| Thing | Where | Holds |
|---|---|---|
| **Model definition** | a registry row | identity, version, input/output schema, tier, cost; effectively a component definition whose behaviour runs elsewhere |
| **Invocation** | in the document | which model is bound to which ports; versions with the project |
| **Run** | a row, outputs in the blob store | model version + hash of inputs → status and result; cacheable and reproducible |

Long-running models return more than signals:

- **signals:** runtime values, never saved;
- **assets:** a CAD file by hash;
- **definitions:** a returned part enters the library;
- **proposed edits:** a branch the user accepts or rejects, which fits the
  branches-as-proposals option in `projects-labs-and-server.md` §8 Q4.

Runs are also evidence: an `:obs` or `:pred` cites a run (§5.1).

## 8. Storage shape this points toward

Not decided; the shape the leanings imply.

| Primitive | Shape |
|---|---|
| Projection document | snapshot plus op log per document (spine §7), as today |
| State | content-addressed snapshot or delta, unordered |
| Move | row: from-state, to-state, kind, recorded/authored, time, duration, refs to transactions or runs |
| Claim | row: text, entities it is about, evidence refs, status |
| Goal | row: condition, entities it is about, `serves` / `from` links |
| Narrative | authored text with stable ids in its references; many-to-many with moves, claims, states |
| Identity | global entity ids (document id + node id), plus type / individual / role |
| Model definition, run | rows; run outputs in the blob store |
| Assets | blob store by content hash (`projects-labs-and-server.md` §4) |
| Index | relational and derived where possible: documents and versions, published definitions, cross-document references, claims and goals by entity, users and organisations |

## 9. Where things lean

| Question | Leaning |
|---|---|
| Unit of projection storage | node, facet, relation rows; the document is the write boundary; components are the unit of reuse |
| Compositions saved? | yes; views are the per-user workspace tier |
| Narrative primitive | the move (an arrow between states), not the step |
| States | unordered, content-addressed; overrides over a base are an encoding |
| Learning | a claim with evidence that tightens goals; knowledge kept apart from design state |
| Goals, tests | conditions on states |
| Notebook / lesson / discovery fiction | one schema; provenance (recorded vs authored) and ordering differ |
| Identity | thin; meaning in relations; type / individual / role |
| Hierarchy | only for write boundaries |
| Substrate | prose as surface and spine, with references into a stored graph |
| Formalising | on demand, by the "life outside its paragraph" rule; LLM proposes |
| Vocabulary | discourse types (from discourse graphs) plus grounding types (Iron's) |
| Timeline | the inside of a move; opened only when the default tween is not enough |
| Models | definition (row), invocation (document), run (row + blob) |

## 10. Open questions

Ordered roughly by how much each one decides.

1. **Discourse graph grammar: adopt or define?** Adopt it directly, so lab notes
   interoperate with tools labs already use (Roam, Obsidian plugins), or define Iron's
   own and export to it.
2. **Is a condition on states rich enough for goals and tests?** Test it against an
   unplanned goal and a test problem. If it holds, goals, tests and lessons share one
   mechanism. What is the condition language: the binding expression grammar (spine
   §5.10), extended with quantifiers ("every entity has a 3D figure")?
3. **What exactly is a state?** A snapshot of one document, of several, or of a working
   set across documents? Does it include model-run outputs, or only reference them? Is
   content addressing over canonical JSON enough, given that `Facet` debug output is
   not content (see memory on content hashing)?
4. **Where do individuals and roles live?** In a lab-level document, a library, or as
   relations only? This overlaps `projects-labs-and-server.md` §8 Q1 (who holds the
   fact "the laser is on bench 3").
5. **Which directive vocabulary?** §5.1 is a first draft. Which symbols are core, which
   are user-defined types, and how do directives map to stored objects?
6. **Branching narratives.** How alternative moves out of one state appear in text
   (links?) and where they become a state machine.
7. **The text-to-time relation.** One mechanism for text-led and picture-led narratives,
   with either as the master clock. Does narration audio need first-class alignment?
8. **Recorded integrity.** How strictly recorded moves and observations are protected:
   append-only rows, signed entries, timestamps. This is where an electronic lab
   notebook's requirements begin (`projects-labs-and-server.md` §7, §8 Q5).
9. **Write boundaries for narratives.** Since hierarchy is only for writes: does a
   lesson, a notebook or a project goal set share a write boundary with the design
   documents it refers to? Goals spanning several documents (enclosure plus circuit
   board) suggest not.
10. **Is the record in v1?** Learner progress, goal completion and notebook entries
    bring users and permissions in early.
11. **Moves and the op log.** Is every recorded move a grouping of transactions, and
    can coarse moves be derived from the op log automatically (by time gaps, labels,
    LLM summary), or are they always marked by a person?

## 11. Prior art

- Discourse graphs: J. Chan, "Discourse Graphs for Augmented Knowledge Synthesis: What
  and Why" (2021); Chan et al., "Steps Towards an Infrastructure for Scholarly
  Synthesis" (arXiv 2407.20666, 2024); the Roam and Obsidian plugins.
- Conklin and Shipman on incremental formalisation.
- Toulmin's argument model; IBIS (Kunz and Rittel); micropublications;
  nanopublications.
- Spivak and Kent, "Ologs" (2012); functorial data migration and CQL; AlgebraicJulia.
- OpenUSD layer stacks and overrides (component-model §12).
- Keynote Magic Move; Descript; Rive state machines; manim; 3Blue1Brown.
- Bret Victor, "Up and Down the Ladder of Abstraction"; explorable explanations.
