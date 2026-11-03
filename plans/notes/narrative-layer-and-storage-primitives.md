# The narrative layer and storage primitives

**Status:** Note: a design conversation in progress, not a plan. Nothing here is
adopted. It records where the conversation has got to, which way it leans, and what is
still open, before the decisions on saving, a database and a server are taken.
**Date:** 2026-10-25, revised 2026-11-02 (scopes and views, how text is stored, move
hierarchy, templates, the narrative clock, clocks and the kinds of time; open questions
revisited).
**Context:** A conversation about what the primitive saved objects are in each of
Iron's layers: the projection layer, the model layer, and the pedagogy / project /
lab-notebook layer above them. It builds on
[`projects-labs-and-server.md`](projects-labs-and-server.md) (projects, the lab twin,
the server), [`time-machines-and-scenarios.md`](time-machines-and-scenarios.md)
(machines with ports, drivers, the scenario/observation split, the `seq`/`par` time
solver), [`../vision/05_three-kinds-of-time.md`](../vision/05_three-kinds-of-time.md)
(scripted, reactive, transitional), the document spine
([`../specs/document-spine.md`](../specs/document-spine.md) §5–7), the component model
([`../specs/component-model.md`](../specs/component-model.md) §3, §5, §10), the model
interop proposal ([`../specs/model-interop.md`](../specs/model-interop.md)), contexts
([`../specs/contexts.md`](../specs/contexts.md)), vision 02 §2 and §6, and vision 09 §2,
§5 and §9. It revises ECM §10 (the lesson layer as lessons → steps) and vision 09 §9,
and takes `projects-labs-and-server.md` §2 (meaning versus ownership) further.

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
| **Scope** (write boundary) | What syncs and versions together, and who may write it | today, a document (§2.2) |
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

### 2.2 Scopes, and projects as views

`projects-labs-and-server.md` §2 separates **units of meaning** ("everything about
this laser", "this lesson") from **units of ownership and change** (who edits, what
versions together). The leaning here takes that all the way:

**Meaning words carry no storage semantics.** Project, notebook, lesson, course and
document (in the user's sense) are **views**: a row holding a title, framing text,
perhaps a goal, and a membership rule ("everything related to goal g1", "everything
tagged enclosure-v2") plus explicitly pinned members. A project reads naturally as *a
goal plus the neighbourhood of things related to it*. The bin is mostly inherited from
filing cabinets; it stays because new users expect it, not because the model needs it.

**One structural primitive remains: the scope.** A scope has one op log, one permission
set and one history, as a git repository or a Figma file does. Today's `iron_document`
document is the implementation of a scope. A scope cannot be replaced by a query:

| Need | Why a query cannot do it |
|---|---|
| Permissions | access is granted to a place, not to a query result that changes as rows change |
| History and branching | forking and merging need a set with edges |
| Sync, offline | a local-first client syncs a log, and a log belongs to something |
| Deletion | if nothing owns a row, nothing decides when it is garbage |

Users rarely see scopes: a personal scope by default, plus shared ones (a lab library,
a class). Users see projects; the system sees scopes. **Forking a project** computes the
project's closure and copies it on write into a new scope; library references stay
pinned references.

This also answers where individuals and roles live (old Q4): **relations only**. An
individual (motor SN 123) is a row in whichever scope owns the physical fact, usually
the lab's; roles are relations from other scopes.

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

### 3.6 Moves are context-dependent

A move only makes sense given what came before, as a token only makes sense given the
tokens before it. Its meaning comes from two places:

1. **The from-state.** Design state compresses all prior history into one snapshot, as
   an RNN's hidden state does: applying move 301 does not need the 300 before it.
2. **The path so far.** What the reader already knows, what was just tried, why this
   matters now. Design state does not hold this; the narrative does, in its text and in
   the knowledge accumulated along the path.

**Leaning:** the stored move row is the **context-free part** (A → B, kind,
provenance). The **context-dependent meaning lives in the text that cites it.** One move
can appear in two lessons with different explanations.

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
figure), but no individual-versus-role level. Individuals live as rows in the scope
that owns the fact (§2.2).

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
**Leaning:** keep hierarchy only for **scopes** (§2.2), the storage job of deciding who
may write a value and what syncs and versions together. Put all meaning in relations.
"Is a lesson its own document?" then becomes "which scope does it sit in?", settled on
practical grounds.

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

Enhanced markdown using the *generic directives* syntax (`:name[text]{attrs}` inline,
`::name{attrs}` as a block; remark-directive parses it). A **directive** is one of
these marked spans; the "directive vocabulary" is which names exist. `@name` aliases a
state hash; `#name` is an entity, claim or goal id. Abridged:

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

**Both directions.** Reading drives state: the narrative clock (§6.4) turns reading
position into move progress, a `:go` span plays its move, a `::view` reframes. State
drives text: `:val` updates, and a live claim whose condition stops holding after the
model changes is flagged, as stale overrides are. Observations stay fixed: they record
what was true at their state.

**The markdown is one encoding, not the store**, as the XML is for the document
(vision 02's co-equal encodings). Directives are the export form of marks inside rich
text (§5.3).

**Syntax, vocabulary and meaning are separate layers.** What is distinctive here is the
vocabulary (`:go`, `:obs`, `:goal`, …) and its meaning (states, moves, conditions
checked against state, evidence pointing at runs), not the syntax. MyST markdown has a
syntax (roles and directives) and a vocabulary for static scientific documents
(figures, math, citations, cross-references, executable cells); it needs no states or
transitions because its world is a document and at most a kernel. Iron's vocabulary
could be a MyST extension or stand on generic directives; the choice is about LLM
familiarity and export (`mystmd`), not expressiveness. The deciding test is whether
MyST's inline attributes carry `:go[…]{to=@aligned dur=4s}` cleanly
([`../specs/narrative-panel.md`](../specs/narrative-panel.md) §6 Q1).

### 5.2 When to formalise text

Formalising follows one rule: **promote a piece of text when it needs a life outside
its paragraph**, because something will refer to it, check it, answer it or act on it.
This is the same test as for joints (§3.3). Levels:

0. **Prose.** Most text, permanently.
1. **A reference** (mention, live value, view change): no new object.
2. **A node** (claim, result, issue): an object with an id others can cite, answer,
   claim or credit.
3. **A checked node** (a goal or claim with a condition evaluated against state).

Each level is worth its cost only when something uses it. Exploration starts free-form;
promotion arises when a repeatable process appears, or when sense has to be made of a
past free-form exploration.

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

**Leaning: define a minimal vocabulary of Iron's own, built to grow,** rather than
adopting the discourse-graph grammar now. Start with **claim** (with status),
**observation**, **goal** and **question**, plus the grounding references. Export to
discourse graphs later if interoperation matters.

| Family | Types | Needs the engine? |
|---|---|---|
| **Discourse** | question, claim (status: hypothesis … established) | No |
| **Grounding** | entity mention, live value, view, observation, goal with condition, move | Yes: binds text to state and models; Iron's contribution |

In a discourse graph, evidence is text pointing at a paper's figure. In Iron it can
point at a state, a measurement or a model run, so it can be inspected and replayed.

**Should an LLM infer structure at query time instead?** It lowers the cost of
formalising and moves the line, but an inferred node has no stable identity: it cannot
be commented on, claimed, assigned or credited. Leaning: the LLM proposes promotions
("this reads like a hypothesis about `#fiber`; mark it?") and a person accepts.

### 5.3 How text is stored

Text belongs to the scope it was written in: "belongs" means only who may edit it and
whose history records the edit, never meaning. A teacher's lesson sits in the teacher's
or the course's scope and refers to design states in other scopes by global id.

**Leaning: blocks as rows** (the Notion and Roam model):

```
block   (id, scope, parent, order_key, kind, content, author, …)
          kind: paragraph | heading | view | figure | …
          content: rich text whose inline marks carry ids
span    (id, block, start, end, kind, target)          -- derived index
claim   (id, scope, status, about[], defined_by → span, …)
move    (id, scope, from_state, to_state, kind, provenance, …)
```

- **A narrative is a root block**, as a Notion page is a block with children. Fractional
  order keys make a reorder one write.
- **The rich text is authoritative; the span table is derived** on each write. Editing
  prose carries marks with their text. The index answers "every paragraph that mentions
  `#fiber`" or "every lesson citing c1" without parsing.
- **A claim is its own row**, defined by a span; its id survives the sentence being
  rewritten, moved or deleted.
- **Transclusion:** a block has one home; other narratives embed it by reference.
- **Blocks can be nodes in the existing store**, with a text facet, so text gets ops,
  undo and history through `iron_document`. Block-level ops are natural; an op per
  keystroke is not, so text inside a block probably wants a text CRDT (Loro, Yjs) as the
  facet's value, at least once collaboration matters (open question).

## 6. Text, timeline and state machine

| Tool | Decides | Who drives |
|---|---|---|
| **Text** | *what* happens and in *what order*, coupled to reading | the reader's progress |
| **Timeline** | *how* a change looks: easing, staggering, overlap, durations, time maps | the clock |
| **State machine** | *what can happen* when someone acts: branches, try-it-yourself | the viewer's input |

These are vision 05's scripted, (with text) reading-driven, and transitional kinds of
time, all as drivers in the time-machines note's sense.

**A timeline is a path through the move graph with a clock mapped onto it.** Show a
timeline wherever one walk is being played; where the graph branches, show the graph,
or a state machine for a viewer to drive. Rive is the prior art: a state machine whose
transitions hold timelines. "One-dimensional" describes the time axis, not one move at
a time: a walk runs several moves **in parallel** on different parts of the state
(`par`). Timeline tracks are those parallel moves.

**Which one leads can flip.** In a notebook, article or scrolling explainer, text is
the spine and moves hang off spans. In a 3Blue1Brown-style video the picture is the
spine and narration is text anchored to time ranges. Both are one relation between text
spans and time ranges, with either side as the master clock (Descript is the prior art
for editing a transcript to edit the video). **State machines** (Rive) come in where the
author stops driving, such as a learner choosing which lens to try.

### 6.1 Moves within moves: refinement and reaction

Two kinds of hierarchy, kept apart:

| Kind | Example | Mechanism |
|---|---|---|
| **Refinement** (inside) | "swap the lens" is realised by 8 mm out, 11 mm in, realign | a coarse move's interior is a `seq`/`par` arrangement of finer moves; the parent fixes the endpoints, the children fill in the motion |
| **Reaction** (hanging off) | the topic moves from Fourier transform to Fourier series, so graphs animate into a table | a child move is driven by the parent's progress, or triggered by a state change |

**Reaction needs no new hierarchy mechanism.** A move's progress is a signal, `t` from
0 to 1, in the reactive graph Iron already has. A child move binds to it,
`t_child = f(parent.t)` (offset, stagger, remap), or fires on a predicate
(`when topic == series`). A "high-order move" is a move on a more abstract part of the
state, such as the topic under discussion; everything below hangs off through ordinary
bindings.

### 6.2 Derived children, overridden by exception

Packages of sub-moves edited like components would mean constantly editing their
internals. **Leaning:** child moves are **derived by default and overridden by
exception**, the component-instance principle (the expansion is computed, never saved):

1. **The state diff names what changes.** Entity ids persist across states, so A → B
   lists every value that differs (Keynote Magic Move).
2. **Transition rules from context say how each change animates** ("view switches
   cross-fade 400 ms", "entering figures stagger 80 ms"), as CSS transitions declare a
   policy rather than each animation. These rules are context data in the existing
   motion domain (§6.6).
3. **The author sees the derived list** for the current move, filtered to the nodes in
   play, and accepts, retimes, reorders, removes or adds.
4. **Storage holds only the overrides and additions**, keyed by stable id and kept as
   stale if their target disappears.

A timeline is opened only when the default is not enough: several things must be offset
or staggered; the motion itself is the content (a beam path, a trajectory); it must
land on beats of narration or audio; or physical time must be remapped ("slow down
10¹²× here").

### 6.3 Templates: what a "move package" really is

Scenario: a lesson cuts between several cameras in 2D and 3D scenes, and the author
drags a "zoom out and rotate" move onto the timeline. Which camera does it act on, and
what if the camera cannot do what it asks? Three separate problems:

**Binding.** The thing dragged is a **template with typed slots** ("a camera"), a
machine with an input port in the time-machines note's sense. Dropping it resolves the
slot: by **where it was dropped** (onto a camera's track, as After Effects effects
apply to the layer they are dropped on); else by **context at that time** (the solved
timeline knows the active camera at the drop time; if there is exactly one, bind it);
else **ask**, with candidates highlighted in their viewports, or bind all in `par` if
the template declares it applies to each.

**Bind early, remember how.** Store the concrete id *and* the selector that found it
("active camera at t"). If camera cuts are later reordered so that another camera is
active there, the move does not silently retarget: it is flagged, as stale overrides
are ("this was bound to the active camera, which is now cam-B; rebind?"). Silent
retargeting would be the timeline's version of silent keyframe creation (vision 05
§5.2).

**Fitting.** Slots declare **capabilities, not exact types**: *required* zoom,
*optional* orbit. Sub-moves on missing capabilities are dropped and shown as inert, not
hidden; an adapter applies where one exists (orbit on a 2D camera becomes a 2D
rotation); a missing required capability refuses the drop and says why. Values are
**relative to the from-state** ("zoom out 30% from wherever it is", "rotate +40° about
the current target"), which is what makes a template portable. Where no rule covers the
mismatch, the template's **intent** ("pull back to reveal context") lets an LLM propose
a variant the author accepts.

**Ownership after the drop.** A dropped template becomes **ordinary moves**, as After
Effects animation presets become ordinary keyframes: slots bound, expanded into
concrete sub-moves, each with a provenance link ("from template *zoom-and-rotate* v3")
for re-applying or noticing a newer version. Nothing updates live; rearranging the
result is editing one's own timeline. This is copy-on-place, the deviation ECM already
makes in some cases, and clearly right here because animation is what authors tune per
context.

| Thing | What it is | Lives where |
|---|---|---|
| **Template** | intent + typed slots + relative sub-moves | a library, reusable |
| **Rule** | "view switches cross-fade 400 ms", applied automatically to state diffs | context data |
| **Placed moves** | concrete, bound, editable, with a provenance link | the narrative's scope |

Templates are dragged in deliberately; rules happen without asking; placed moves are
what is left after either, and are always one's own to edit.

### 6.4 The narrative clock

The narrative clock is the scripted driver for a narrative. **Leaning: define its
contract now, because it constrains how moves are stored; defer the rest.**

1. **It counts narrative position, not seconds**: a point along the path through the
   text and its moves. Each source maps its input to position:

   | Source | Maps to position by |
   |---|---|
   | Scroll | where the viewport sits in the text |
   | Voiceover / audio | playhead seconds → word alignment → span |
   | Stepping | a discrete "next joint" |
   | Wall clock (video export, live demo) | seconds → the solved timeline |

2. **Moves anchor to spans; the 1D time solver fills the gaps.** The time-machines note
   plans the solver over `seq`/`par` (§4). Narration adds hard anchors ("starts when
   'focus' is spoken"); without audio the same anchors are soft and durations come from
   the moves.
3. **Scripted moves must be seekable**: the state at any position is a function of
   position, never accumulated frame by frame. Scrolling back and scrubbing audio both
   need it. Anything not seekable is event-driven and belongs to the state machine.
4. **One source leads at a time.** While audio plays, scroll follows; if the reader
   scrolls, audio pauses and scroll takes over.

**Tying words to events.** For synthesised voice, TTS engines can report when marked
points are spoken (SSML `<mark>` timepoints; some providers return character-level
timestamps), so each directive span emits a mark and gets back its time. For recorded
voice, forced alignment (WhisperX, Montreal Forced Aligner) gives word times for a
recording and its transcript. Either way the stored relation is span → time range;
re-recording or editing the script re-derives the times and the moves follow.

Deferred: syncing several viewers, the authoring UI. How the clocks relate (seekable
values, the time map, state-machine changes, exploring mid-lesson) is §6.7.

### 6.5 Exploring, authoring, and editing at a point

**The default is exploration.** Every design change is already recorded in the op log,
the finest grain of recorded moves; the question is only whether a change is **cited in
the narrative**, which follows the promotion rule (§5.2). Navigating a view (rotate,
zoom) is not even a design change: it is per-user workspace state, and enters a lesson
only as framing (`::view`). No build/author mode is needed. Ways a change enters the
narrative:

1. **Where you ask decides.** Asking from the narrative (cursor in the lesson, ask in
   place) authors at the cursor; asking from the chat or the canvas just does it.
2. **Capture after the fact.** Explore freely, then "add that to the lesson" or a
   capture button. History holds the op log and view changes, so capture can be
   retroactive. Expected to be the common path.
3. **The LLM follows cues.** "Next, rotate to show…" or "in the lesson…" authors; a
   plain "rotate the view" does it now. When ambiguous, it does the change and offers
   "Add to narrative?".
4. **Recording**, visibly on, for performing a sequence: each change until stop becomes
   a new move at the current point.

**What the canvas shows is a property of the view, not of each property.** A view's
source is either **the live design** (the scope's current state) or **a narrative at a
point** (the narrative's base state with its overrides up to that point). This replaces
After Effects' per-property stopwatch as the marker of "part of the narrative":

- **The view says which state it shows**: a coloured frame and a breadcrumb ("*Getting
  light into a fiber* › at *swap the lens*"), like the component isolation mode, or
  git's "detached HEAD" warning when a past commit is checked out.
- **A view is linked to a narrative or not.** A linked view follows the narrative's
  position (the narrative clock, §6.4): scrolling or scrubbing moves it. Unlinking, or
  Escape, returns it to the live design, so one can explore while reading.
- **Property-level provenance** answers *what the narrative controls*: the inspector and
  the "why is this moving?" probe (vision 05 §5.3) say "set by the narrative at *swap the
  lens*" for values that differ from the base.

A drag then means:

| The view shows | A drag… |
|---|---|
| the live design | edits the design; nothing authored; capturable later |
| a narrative at a point | edits the narrative's state there: the arriving move's end changes and its derived tweens re-derive; no new move |
| a narrative, with recording on | adds a new move at that point |

**Override extent is a mechanism; the default is context data.** An edit at point *k*
is an override with an **extent**: this joint only; from here until the next override
of the same value ("hold"); a named range; the whole narrative. The engine provides
the extents; a narrative's notation chooses the default. A slides-like notation built
in Iron sets "while this slide is on screen" (a joint's extent is its interval); an
animation-like notation sets "hold". A modifier picks the other.

### 6.6 Fitting the existing contexts system

The behaviour choices above should go through the contexts system
([`../specs/contexts.md`](../specs/contexts.md), built 2026-10-26/27), not a parallel
one. What exists: **rules** in a **context** match nodes (`type:`, `self`, `children`,
`state:`) or a **view** (`match="view"`) and declare registered `domain.field` values;
built-in contexts (`std:free-2d`, `std:table`, `std:schematic`, … in
`iron_document/src/std_contexts.xml`) are never saved; a document's contexts are
`context` nodes in its library and may `extends` one built-in; a composition's
`notation` names its context; each view has a **focus context** (`Focus { world, scope,
states }`, with states such as `editing-text` and `main`) whose resolved view fields
drive the shell's palette, keys, inspector sections, tolerances and chrome accents. A
**mode** (affordances §11) is a context applied on top for one view, in the session
tier; presentation and "a lesson step" are its named examples, not yet built.
`contexts.md` §4.1's admission test applies to every new field: a human-interface
choice, wanted differently by at least two contexts, with a mechanism to read it.

How the narrative layer maps onto it (proposals):

| Narrative need | In the contexts system |
|---|---|
| A view showing a narrative at a point (§6.5) | a **mode** on that view, in the session tier, with a new focus state (say `at-point`); its frame colour is the existing `chrome.accent` view field |
| The default override extent (hold, per slide) | a new view field (say `narrative.extent`) resolved for views in the `at-point` state; the extents themselves are the mechanism |
| The palette and keys while at a point (capture, record) | the existing `palette.tools` and `keys.tools` fields under `match="view state:at-point"`, as `state:main` adds the port tool |
| Transition rules for derived sub-moves (§6.2) | the **motion** domain, which already has `motion.placement="<ease> <ms> [<when>]"` and one interpolator; tweens between narrative states extend it rather than adding a rule system |
| Slides-like versus animation-like narratives | **notations**: a deck-like and a timeline-like context bundle, `extends`-ing a built-in, as `focus.txt`'s deck does |
| The panel's layout modes and composer target defaults | view fields on the narrative view (workspace defaults, domain 12, are already listed) |
| What the LLM is told when asked in place | the candidate domain "the model surface per focus context" (§4.2) |

Two limits from the existing plans, kept:

- **No time cascade.** `contexts.md`'s follow-up note does not approve "an arbitrary
  time/space context cascade". A narrative's changes over time are **document data**
  (states, moves, overrides with extents), not rules that vary by time. Where a context
  must change over time (a lesson step making most content inert), it is a mode
  entered and left at joints. `authoring-workflows.md` §7 asks that any time scope name
  its clock, precedence, entry and exit, and what happens to a gesture in progress;
  the **narrative clock** (§6.4) is that clock, and those four answers belong in the
  spec that builds the `at-point` mode.
- **Templates are not context data.** A move template (§6.3) is library content, like
  a definition; its slots' capabilities are facts about facets (a camera has zoom).
  Only choices (default extents, tween timings, palettes) are context data.

### 6.7 Clocks and the kinds of time

Reconciles this note with vision 05 (three kinds of time), the time-machines note and
the time-domain-signals note (conversation of 2026-11-02). Where vision 05 differs, it is
amended to match (its §2.1, §4, §5.2).

**Seekable is a property of values, not only of moves.** The primitive that was missing:
every value depends on some set of sources, and what it depends on decides what time can
do with it.

| Source | Seekable? | Examples |
|---|---|---|
| The document | yes | literals, design values |
| Clocks the narrative owns | yes | narrative position; timeline seconds solved from it (§6.4); a move's progress `t` |
| Free inputs | no | the learner's pointer and edits, wall time, live external data |
| Things with memory | no, unless cached | simulations, integrators, filters (time-domain-signals), model runs |

A seekable value can be **evaluated at any position without running time forward**,
because it is a pure function of position. That is also what seeking is: nothing is
accumulated, so the state at a point is computed, not stored.

**The check is a trial evaluation, not a separate analysis** (user, 2026-11-02). To
place a move ending at a value, evaluate that value with the clocks set to the end
position, beside the live state (as the gesture overlay previews beside the store),
and record which sources the evaluation read. The evaluator already knows each
binding's inputs to update its dependents. The result is seekable when every source
read was the document or a narrative clock. Reading a source matters, not whether the
value happens to come out: a slider the learner has not touched evaluates fine today,
but may differ by the time the move ends. A value with memory cannot be evaluated at
another time at all without a cache. A per-value flag is then only a cache of this
check, for marking values in the UI.

**The narrative's position is an input, not saved state.** What a narrative stores is
the base state, its moves and its overrides with extents (§6.5); `state(p) = base ⊕ the
overrides active at p`. The playhead is per-view workspace state, like a scroll offset.

**The endpoint rule, restated.** Vision 05 §2.1 asked that clip endpoints stay literal,
because an end that depends on a live signal cannot be drawn or dragged. The rule that
matters is weaker: **a move's ends must be seekable.** A literal is the trivial case.
`to=signal.threshold` is allowed when `threshold` resolves through the reactive graph
from a narrative clock: evaluate it at the end position. An end that depends on a free
input or on something with memory is refused, with the reason; following such a value
is a binding, not a move. Authors think in "tween 40 → 200"; the normalised 0→1
parameter is how the engine runs a move, not something to author.

The time-machines note's **closed scenario** is then the case where every value in the
lesson is seekable, and **making free inputs visible** means marking the values that are
not (in the probe, and as the read-only bands below).

**Simulations belong to the model**, out of scope here. A model can make its output
seekable by caching it over time (as data, or as frames), which is the time-machines
note's "record to scrub"; until it does, its outputs count as having memory.

**State-machine changes are held overrides.** A state includes each machine's current
state (`phase: shown`), so a `:go` to a state that differs in a machine's state is an
ordinary override with an extent (§6.5): "from this joint on, `#mount1.mount` is `out`".
This is a `with` block, Python's context manager. Entering applies it, leaving restores
it, even when it is left by a jump, and inner blocks win. The precedent is declarative:

- SVG SMIL `<set attributeName="visibility" to="visible" begin="2s"/>` holds a value
  for its interval, and `fill="remove"` restores it afterwards.
- Unreal Sequencer keys enum and boolean properties as steps; each section chooses
  *Restore State* or *Keep State* when it ends. Unity Timeline's Activation Track has
  the same "post-playback state".
- Reveal.js fragments and `data-state` add a class going forward and remove it going
  back.

The imperative form, "if in A, go to B", is not used: it has no safe inverse. Instead:

- **Playing forward** across the change runs the machine's transition (below).
  **Scrubbing** across it snaps to the state.
- **"If in A" becomes a check** the editor flags ("the machine may not be in A here"),
  not a branch at runtime.

**Exploring mid-lesson.** A learner can pause, explore, then continue. Their changes go
into a **scratch layer** over the narrative's state at that point, held per user in the
session; they edit neither the design nor the narrative. On continue, values that the
coming moves set are taken back by a short **rejoin** tween, from wherever the learner
left them to the narrative's value. A rejoin may start from a live value because only a
move's end must be seekable, and the rejoin is played, never authored. Values the
narrative never touches keep the learner's changes; "reset" drops the scratch layer. (An
author in a view at a point edits the narrative itself, as §6.5 says.)

**Property modes: `const | bound`, with "animated" as a marker.** Vision 05 §5.2's
three-way control answered After Effects' stopwatch question: does a drag at 3 s change
the object or add a keyframe? Here the view's source answers that (§6.5), so `animated`
is not a mode. What stays is the real choice:

- **`const`**: a literal, which a drag edits (or a narrative override edits, at a point).
- **`bound`**: an expression or a signal. A bound value has no drag handle unless its
  binding inverts. This is the spine's existing rule (document-spine.md §5.10): a leaf
  bound linearly to one constant leaf gets a handle that edits that leaf, and any other
  bound axis is locked for the gesture. A ball whose position comes from a model is not
  draggable, and the probe says why.
- **Animated** is shown, not chosen: a marker on the property row meaning "a move sets
  this" (with the narrative's provenance, §6.5), which opens the move.

**Rendering is separate.** The engine knows when something moves: a playing clock, an
active move or rejoin, a running simulation, a free input changing. It draws
continuously only then; otherwise the view stays still, as now.

**Components' arrows are their own.** A component's state machine is part of its
design: states, and transitions that each hold a **sub-timeline**, Rive-style (a
button's hover → pressed). This replaces vision 05 §4's `<play>` sketch:

```xml
<component name="lens-mount">
  <machine name="mount" initial="seated">
    <state name="seated"/>
    <state name="out"/>
    <transition from="seated" to="out" timeline="pull"/>
    <transition from="out" to="seated" timeline="seat"/>
  </machine>
  <timeline name="pull">…</timeline>
  <timeline name="seat">…</timeline>
</component>
```

A narrative never reaches inside. Its move sets the state (`#mount1.mount = out`), and
**the component's transition is the derived sub-move** for that change, by the same
rule as §6.2: derived by default, overridden by exception. Customising it for a lesson
works in two ways:

| Want | How |
|---|---|
| a different timing for this transition, or for every transition in the lesson ("everything at half speed while this step is on") | a context rule in the motion domain, held by the step's mode (§6.6); would need a `transition:` selector beside `state:`, so it must pass `contexts.md` §4.1's admission test |
| a different motion for this one change | an override of the derived sub-move, stored in the narrative, keyed by (instance, transition), kept as stale if the transition disappears |

A component's named timeline that is not a transition (a flourish) can still be played
by a move, as a sub-move whose duration comes from the timeline; it is the same
mechanism, an arrow without a state change.

**Pedagogical components are the exception to "machines describe state".** A machine
in the model describes state, and the narrative supplies the arrows. A component made
for teaching (a hint card, a quiz, a "try it" prompt) has states that are pedagogy:
`hint: hidden | shown`, `answer: none | wrong | right`. **Leaning: this is fine**, and
does not break the layers, because the rule being protected is that **the model never
holds the lesson**, so that one model can be taught by several lessons. It is not that
no component may know about lessons. The test is whether the component makes sense
without the lesson. A lens mount does, so it is model and its state is design. A hint
card does not, so it belongs to the narrative's own compositions, a projection of
narrative state. Its states are narrative values (held by extents, like any other), and
a learner's answers are per-user progress (§10, deferred). A lesson that branches on
the learner ("if they answered wrong, show the hint") is then a machine whose states
live in the narrative, and such components are its surface.

**Rates.** Motion flows from the narrative clock through each move's progress, and
physical time through a per-scene **time map** (physical seconds per narrative second,
"slow down 10¹²× here"), which is usually invisible. Speed is rarely authored directly,
so the tool is a **measurement**: select an object and read its on-screen speed (px/s)
while playing, then retime the move or the map.

**Read-only bands, to be tried.** Vision 05 §5.5: under a timeline's editable tracks,
rows that cannot be dragged show what else is happening over the same time. Each signal
is a sparkline; each machine's state is a coloured band; free inputs and values with
memory are marked. The concern (user, 2026-11-02) is that rows you cannot edit look
wrong in a timeline. Profilers (Chrome's Performance panel) are entirely such rows, and
video editors show waveforms beside clips, but an authoring timeline is different. To
judge in a demo, perhaps only on demand: pin a value from the probe to the timeline.

**The model's TIME manifest** (vision 05 §5.4) gains the narrative's cues: "next", "in
the lesson" and "then" author a move at the cursor (§6.5 item 3); "when the learner…"
is a narrative branch; "follows", "tracks" and "proportional to" are bindings.

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
| Scope | snapshot plus op log, one permission set, one history (spine §7); today's document |
| Projection rows | node, facet, relation, inside a scope, as today |
| State | content-addressed snapshot or delta, unordered |
| Move | row: from-state, to-state, kind, recorded/authored, time, duration, refs to transactions or runs, anchor span |
| Placed move from a template | move rows plus provenance (template, version) and slot bindings (concrete id + selector) |
| Claim | row: status, entities it is about, evidence refs, defining span |
| Goal | row: condition, entities it is about, `serves` / `from` links |
| Text | block rows (parent, order key, kind, rich text with marked spans); span index derived |
| View (project, notebook, lesson, course) | row: title, framing blocks, goals, membership rule, pinned members |
| Identity | global ids (scope id + node id), plus type / individual / role |
| Model definition, run | rows; run outputs in the blob store |
| Assets | blob store by content hash (`projects-labs-and-server.md` §4) |
| Index | relational and derived where possible: scopes and versions, published definitions, cross-scope references, spans by target, claims and goals by entity, users and organisations |

## 9. Where things lean

| Question | Leaning |
|---|---|
| Unit of projection storage | node, facet, relation rows; the scope is the write boundary; components are the unit of reuse |
| Compositions saved? | yes; views are the per-user workspace tier |
| Project, notebook, lesson, document | views (a framing row + membership rule); no storage semantics |
| Structural storage primitive | the scope only: one op log, permissions, history |
| Individuals and roles | relations only; individuals live in the scope that owns the fact |
| Narrative primitive | the move (an arrow between states), not the step |
| States | unordered, content-addressed; overrides over a base are an encoding |
| Move meaning | the row is context-free; context lives in the citing text and the path |
| Learning | a claim with evidence that tightens goals; knowledge kept apart from design state |
| Goals, tests | conditions on states |
| Notebook / lesson / discovery fiction | one schema; provenance (recorded vs authored) and ordering differ |
| Identity | thin; meaning in relations; type / individual / role |
| Hierarchy | only for scopes |
| Substrate | prose as surface and spine, with references into a stored graph |
| Text storage | block rows with marked rich text; derived span index; blocks as store nodes |
| Formalising | on demand, by the "life outside its paragraph" rule; LLM proposes |
| Vocabulary | Iron's own minimal set (claim, observation, goal, question + grounding), grown as needed |
| Timeline | a played path through the move graph; parallel moves are tracks |
| Move hierarchy | refinement (`seq`/`par` inside) and reaction (bound to parent progress) |
| Sub-moves | derived from state diffs and context rules; only overrides stored |
| Reusable moves | templates with typed, capability-based slots and relative values; copy-on-place with provenance |
| Narrative clock | counts narrative position; sources map to it; moves seekable; one source leads |
| Syntax | secondary to vocabulary and meaning; MyST extension or generic directives, decided by inline attributes |
| Exploring vs authoring | exploration by default; ask in place, capture, cues and recording bring changes into the narrative; no global mode |
| Narrative vs design on the canvas | a view's source (live design or narrative at a point), shown on the view; property provenance in the inspector |
| Edits at a point | overrides with an extent; the default extent (hold, per slide, …) is chosen by the narrative's notation |
| Models | definition (row), invocation (document), run (row + blob) |

## 10. Open questions

Ordered roughly by how much each one decides.

1. **Is a condition on states rich enough for goals and tests?** Test it against an
   unplanned goal and a test problem. What is the condition language: the binding
   expression grammar (spine §5.10), extended with quantifiers ("every entity has a 3D
   figure")? To be seen in use.
2. **What exactly is a state, given scopes?** A snapshot of one scope, several, or a
   working set across scopes? Does it include model-run outputs or only reference them?
   Is content addressing over canonical JSON enough, given that `Facet` debug output is
   not content?
3. **Scope defaults and the fork closure.** One personal scope per user, or one per
   project by default? What exactly does "fork a project" copy, and where does the
   closure stop (pinned library references, cross-scope goals)?
4. **Text editing granularity.** Block-level ops in `iron_document` with a text CRDT
   inside each block, or something else? When does collaboration force the choice?
5. **The directive vocabulary in detail.** Which marks are built in, which are
   user-defined types, and how each maps to stored rows (§5.2 lists the starting set).
6. **The template slot language.** How slots, capabilities, adapters and relative
   values are declared, and how selectors are recorded and re-checked.
7. **Clock sources in practice.** Hand-off rules between scroll, audio and stepping;
   whether narration audio is a first-class stored asset with its alignment.
8. **Recorded integrity.** How strictly recorded moves and observations are protected:
   append-only rows, signed entries, timestamps. This is where an *electronic lab
   notebook* (ELN) begins: regulated labs (FDA 21 CFR Part 11, GLP) require audit
   trails, signatures and no silent edits (`projects-labs-and-server.md` §7, §8 Q5).
   Related to library publishing, but a library publishes new versions while a recorded
   observation is never revised, only superseded.
9. **Override extents in detail.** Which extents exist, how "hold" interacts with
   overrides set later or earlier, and how an extent is shown on a timeline.
10. **Moves and the op log.** Is every recorded move a grouping of transactions, and can
   coarse moves be derived from the op log automatically (time gaps, labels, LLM
   summary), or are they always marked by a person?
11. **Seekability in the engine (§6.7).** The trial evaluation's cost, and when it
   reruns (when a source of the end value changes); how a model declares that a cached
   output is seekable; what the probe shows for a value that is not.
12. **The scratch layer and rejoin (§6.7).** How long a rejoin takes and how it eases
   (motion-domain context data?), and whether a learner's scratch layer survives a
   reload.

**Deferred until user flows show which product Iron becomes** (lab notebook,
literature tool, project management):

- **Branching narratives.** Cases seen so far: learner choice that reconverges
  (content addressing makes reconvergence automatic); a counterfactual excursion; an
  adaptive lesson with remediation; parallel design alternatives, including
  agent-proposed edits; depth on demand; a lesson as one path through a notebook's dead
  ends. Two kinds: **design branches** (states differ) and **presentation branches**
  (same states, different order or depth). The store already allows both.
- **Learner progress and goal completion.** When needed, progress is a per-user
  relation (user, node, status, time) in the user's own scope. Project goal completion
  is mostly computed from the goal's condition, with manual check-off only for goals
  without one.

## 11. Prior art

- Discourse graphs: J. Chan, "Discourse Graphs for Augmented Knowledge Synthesis: What
  and Why" (2021); Chan et al., "Steps Towards an Infrastructure for Scholarly
  Synthesis" (arXiv 2407.20666, 2024); the Roam and Obsidian plugins.
- Conklin and Shipman on incremental formalisation.
- Toulmin's argument model; IBIS (Kunz and Rittel); micropublications;
  nanopublications.
- Spivak and Kent, "Ologs" (2012); functorial data migration and CQL; AlgebraicJulia.
- OpenUSD layer stacks and overrides (component-model §12).
- Notion and Roam block models; Loro, Yjs and Automerge for collaborative text.
- Keynote Magic Move; CSS transitions; After Effects animation presets; Descript; Rive
  state machines; manim; 3Blue1Brown.
- SSML `<mark>` timepoints; forced alignment (WhisperX, Montreal Forced Aligner).
- Bret Victor, "Up and Down the Ladder of Abstraction"; explorable explanations.
