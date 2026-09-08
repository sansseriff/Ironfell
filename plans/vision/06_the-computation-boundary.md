# The computation boundary

**Status:** Vision draft
**Date:** 2026-08-31
**Scope:** What component-local state forces on the projection language, one
change to the build staging of `02_substrate-model-projection-control.md` §13, a
gap in the archetype vocabulary of §9.3, and an honest residual list against the
floor of §6.2.

Companion to `05_three-kinds-of-time.md`, which covers the temporal side. This
document is about §5–6 and §9.3 of doc 02.

---

## 0. What this adds, and what it does not

Doc 02 §5.2 already triages the operations of a component framework into
*already covered*, *evaporates entirely*, and *genuinely lost*, and §6.2 sizes
the floor from the grammar-of-graphics and dataframe-verb traditions. Both hold
up under a concrete test — an XY-graph component with a staggered entrance
animation, worked through in `05` §4.

Three things that test surfaced which the existing documents do not cover:

1. **Component-local state is not optional**, and admitting it decides the order
   in which the remaining node types should be built (§1–2).
2. **Enumerated choice is missing from the archetype vocabulary** of §9.3, which
   is what makes "how would you drag a component between states" feel
   unanswerable (§3). This is the substantive addition.
3. A short **residual list** — most of what looked missing turned out to already
   be in §6.2, and saying which is which matters more than the list (§4–5).

---

## 1. Local state is the forcing function

Doc 02 §5.1 lists reactive-state-that-is-not-derived first among the things
component code does, and §5.3 gives `Component { params, local_state, body }`.
The XY-graph makes concrete *why* the `local_state` field cannot be dropped:

A component with an entrance flourish must record that the flourish has already
played. Without somewhere to hold that, it either replays on every re-evaluation
or requires the parent to remember on its behalf — and a component whose parent
must track its internal lifecycle is not encapsulated.

That is a small requirement with a large consequence. Holding state is a signal,
which the document already has. **Changing** it is not: it needs a statement, not
an expression. §4's `Stmt`/`Handler` forms exist for exactly this, and §13's
staging says to add "bounded imperative handlers next."

The concern that motivated exiling imperative computation — that a mouse cannot
grab it — applies to state changes as much as to arithmetic. §2 and §3 are the
two halves of the answer.

---

## 2. The FSM belongs before general handlers

**Proposed change to the staging in §13:** build the explicit finite-state-machine
node *before* bounded imperative handlers, not after.

The reason is not that handlers are hard. It is that **the FSM absorbs most of
what handlers would be used for, in a form that is directly manipulable.**

| Need | As a handler | As an FSM |
|---|---|---|
| "on mount, play the entrance" | statement list | transition, `on="mount"` |
| "when selected, ease to highlight" | statement list | transition with a guard |
| "when the data changes, replay" | statement list | transition on an input |
| "open, then focus the field" | statement list | genuinely sequential — stays a handler |

The first three are the overwhelming majority of what a component library needs,
and as transitions they are nodes with leaf-only handles: visualisable, editable,
and inspectable by both a human and the model. As statement lists they are small
pockets of imperative code that a visual editor can only present as text.

Build handlers first and every one written before the FSM lands is either
migrated later or left as a permanent exception. Build the FSM first and the
residual handler surface shrinks to genuinely sequential side-effect chains —
which is small enough to be defensible, and which §5.3's enumerated-sink
`Effect` already bounds.

This is the same argument §5.2 makes about `If` and `Each` nodes absorbing
control flow, applied one level up to control *sequencing*.

---

## 3. Enumerated choice: a missing archetype

### 3.1 The gap

§9.3's archetype table is the claim that infinite editors come from infinite
bindings over a small, finite set of manipulation archetypes. Every entry in it
is continuous or structural:

> scalar on a track · point in 2D/3D · interval · set by region · binary relation ·
> sequence · point on a track · containment

**None of them is "pick one of N declared alternatives."** That is why "how does a
user drag a component between states" has no good answer — a state is not a
continuous quantity and there is nothing to drag.

This is not an exotic omission. Enumerated choice is arguably the most common
leaf kind in any real inspector: text alignment, line cap, blend mode, easing
preset, scale type, arrowhead style, units, and — the case at hand — which state
a component is in.

It also meets §9.3's own admission criterion cleanly. Its value shape is simple,
the manipulation is total over it, and the writeback is unambiguous:

| Archetype | Affordance | Writeback |
|---|---|---|
| **Enumerated choice** | segmented control, cycle-click, hover popup, radial menu | set an enum value |

Being schema-backed is what makes this pay: given a declared enum, the editor
generates its own control with no per-property code, which is §9.3's whole thesis
applied to one more entry.

### 3.2 A state machine decomposes into three archetypes

Once enumerated choice exists, the state machine needs no bespoke editor. It is
three existing archetypes bound to three parts of one structure:

| Part | Archetype | Affordance |
|---|---|---|
| The set of states | enumerated choice *(new)* | pick one; hover popup on the component |
| The transitions | binary relation | drag to connect |
| A transition's progress | scalar on a constrained track | scrub, to preview the blend |

The third row is the answer to the original question. **You cannot drag between
states, because states are discrete. You can scrub through a transition, because
its progress is continuous.** That is what makes a state machine feel manipulable
rather than merely configurable, and it requires no new mechanism — transition
progress is a scalar on a track, exactly like any other.

Guards on transitions are expressions, so they reach the expression editor by the
same route every other binding does.

### 3.3 Pinned state is an editor override, not a document edit

While authoring, a user needs to hold a component in a chosen state to design it
— `hidden`, to see the entrance's first frame.

That pin must be **editor-local and visually loud**. It is not a value in the
document, and it must not be serialised into one. The failure mode is mundane and
common to every tool with variants: pin a state to work on it, forget, ship it
pinned. The affordance should make a pinned component obviously distinct from an
unpinned one at a glance, not merely on inspection.

This is a specific instance of a general rule worth stating: **editor overrides
and authored values must never share a representation.** The document is the
canonical encoding (§3.1); a viewing preference is not part of it.

### 3.4 State must appear in the projection

The model faces the same question a human does and needs the same answer in its
own view:

```xml
<xy-graph id="#g1" data="#ds4" phase="shown"/>
```

Current state is derived, not authored, so it is served on demand and marked as a
snapshot at a version, per doc 04 §4.1. But it must be *available*, or the model
cannot reason about why a component looks the way it does — and will instead
propose edits to the wrong layer.

---

## 4. Residuals in the floor

§6.2 is well-sized and most of what looked missing is already in it. Recording
what is *not* a gap is as useful as recording what is:

| Looked missing | Actually covered by §6.2 |
|---|---|
| Prefix sums for stacked charts | **`stack`**, listed under statistical transforms |
| Running totals, moving averages | **`window`**, listed under wrangling verbs |
| Axis domains and mapping | **Scales** — linear, log, power, sqrt, ordinal, time |
| Binning, regression, density | listed under statistical transforms |

Three genuine residuals:

**Text shaping and measurement.** Absent from §6.2's vocabulary, and it is the
one floor operation that must run *inside* the layout solve rather than upstream
of it — a solver cannot ask an external process how tall a paragraph is, once per
node, per frame. §5.3 has "measurement as read-only derived signals," which
covers how it is *exposed*; §6.2 should also carry it as a floor member, since
its latency requirement is the strictest in the system.

Measurement held outside the solver forces a re-entrant loop: lay out
optimistically, discover which sizes were actually needed, lay out again. That is
tolerable in a batch renderer and unusable in an editor, where it becomes a frame
hitch on every text edit and every resize. In-process shaping is therefore not an
optimisation, it is what makes interactive layout possible at all — and it is why
this operation cannot be deferred to the wasm or call-out tiers of §6.3, which
both sit on the wrong side of the latency criterion.

**Curve and path interpolation.** §6.2's geometric operations cover project,
slice, contour, sample/decimate and transform, but not turning a point sequence
into a path with an interpolation mode (linear, step, monotone, basis). Every
line and area mark needs it.

**Seeded, deterministic randomness.** Procedural and organic motion need noise,
and it must be reproducible: same seed, same output. An ambient RNG breaks
undo/redo determinism and makes two renders of one document disagree — a
correctness bug, not a quality one. `random(seed, …)` as a pure floor function;
never an implicit source.

---

## 5. Residuals in the language

**Multi-way branching.** `Expr` has `If`, and nothing else. Twelve-case
conditional formatting becomes twelve nested `If` nodes — technically declarative
and practically unreadable, in text and much worse in a visual editor. A
`Match`/lookup-table node keeps the flat cases flat. This is the most likely
single cause of documents that satisfy every architectural rule and are still
incomprehensible.

**Recursion, for structure of unknown depth.** `Each` handles flat collections.
An org chart, a file tree, a nested outline need depth that is not known when
authoring. The clean recovery is **allowing a component to instantiate itself**,
bounded by a depth budget — recursion over a data structure rather than an
unbounded loop, which keeps the frame-safety argument of §4 intact and stays
editable. This should be decided deliberately: "components may not self-reference"
is a natural-looking restriction that silently rules out a whole category of
visualisation.

**Units as part of the value type.** For a scientific tool this is not a
nicety. Values are 60.7 K, 11 in, 2 GHz, 9.58 mW. If a value is a bare float,
conversion and unit-aware formatting become string manipulation scattered through
the document, and comparison across units is silently wrong. If dimension is part
of the value, conversion and comparison are floor operations and a unit change is
a display property — which also makes it an *enumerated choice* leaf (§3.1),
directly manipulable. The decision is much cheaper before a corpus of documents
exists than after.

---

## 6. A test for any proposed subset

Before admitting or excluding a capability, run it against these. Each is drawn
from a case that actually broke something above.

1. **Can a mouse grab its result?** If the output has leaf handles, it can live
   inside. If it is opaque, it is a Model (§2.4).
2. **Must it run inside a continuous loop?** If yes it is floor, whatever else it
   is (§6.1). Text measurement is the strict case.
3. **Is it pure and total?** The price of admission to the floor (§6.2), because
   reactivity assumes purity and the frame budget requires bounded cost.
4. **Does it stay legible at scale?** A construct that is correct at N=2 and
   unreadable at N=12 — nested `If` — fails, even though nothing is formally
   wrong with it.
5. **Does it have a stable archetype?** If a user cannot manipulate it with one
   of the §9.3 affordances, either it needs a new archetype (and enumerated
   choice shows those exist) or it does not belong in the document.
6. **Is it deterministic under undo?** Anything drawing on ambient state — time,
   randomness, external reads — must take its source as an explicit input.

---

## 7. What this asks of the rest of the architecture

| Requirement | Where it lands |
|---|---|
| FSM node built before bounded imperative handlers | doc 02 §13 staging |
| Enumerated choice added to the archetype vocabulary | doc 02 §9.3 |
| Transition progress exposed as a scalar on a track | doc 02 §9.3 |
| Editor overrides (pinned state) never share representation with authored values | doc 02 §3.1 |
| Current state served in projections, marked derived | doc 04 §4.1, §6.3 |
| Text shaping named as a floor member | doc 02 §6.2 |
| Path interpolation and seeded random added to the floor | doc 02 §6.2 |
| `Match` node; component self-instantiation with a depth budget | doc 02 §4, §5.3 |
| Units as a value-type decision | new; decide early |

None of this contradicts the existing documents. The staging reorder in §2 and
the archetype addition in §3 are the two that change work already planned; the
rest are gaps to fill in place.
