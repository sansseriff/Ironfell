# Patches, regions and time

**Status:** Note: a design conversation, not a plan. Nothing here is adopted; do not
build from it. It leans hard enough that a spec should follow it, and the user has
said breaking changes to the UX and the internals are welcome to get one coherent
system (2026-11-20).
**Date:** 2026-11-20.
**Context:** A conversation that began with a hidden tab's plot costing frames (its
tick labels kept coming and going) and went on to time, takes, slides and state
machines. It builds on and revises:
- [`narrative-layer-and-storage-primitives.md`](narrative-layer-and-storage-primitives.md):
  states, moves (arrows), override extents (§6.5), clocks (§6.4, §6.7);
- [`time-machines-and-scenarios.md`](time-machines-and-scenarios.md): machines with
  ports, drivers, the time map;
- [`../specs/machines.md`](../specs/machines.md): states as poses, transitions over
  `progress`;
- [`../specs/variants-and-imports.md`](../specs/variants-and-imports.md): variants
  that `extends`;
- [`../specs/component-model.md`](../specs/component-model.md) §5.3: overrides;
- [`../specs/timeline.md`](../specs/timeline.md) §4.1: the clock node;
- [`../specs/reactive-engine.md`](../specs/reactive-engine.md) §8.3, §10: what stays
  necessary, clocks, slices.

---

## 1. The question

Iron has several mechanisms for "the same thing, a bit different":
- component instances over a definition;
- lesson variants that `extends` a definition;
- machine states, each a pose;
- clips over time;
- a narrative's overrides with extents;
- the learner's scratch layer;
- gesture previews.

The user's examples add three more:
- **A deck whose slides build on each other.** Editing slide A should reach B, C and
  D unless they change that value.
- **A machine replayed as takes.** An explainer replays the same mechanism over and
  over, adding a part, changing a speed.
- **A state that takes some properties from one state and others from another**, in a
  machine whose states form a cycle, so that no state is simply "the parent".

The goal is one rule set from which all of these follow: "all of these things are X",
with firm laws.

## 2. The algebra

### 2.1 Terms

- **An axis** is something that varies:
  - a deck's slide index;
  - a machine's state;
  - a loop's iteration `n`;
  - time;
  - which instance;
  - a variant dimension (size, detail).

  Axes are ordered (slides, iterations, time) or not (machine states, instances), and
  discrete or continuous (time).
- **A point** is a coordinate on every axis in play.
- **A region** is a set of points. On an ordered axis it is usually an interval:
  - `{B}`;
  - `[B, ∞)`;
  - `[B, D]`.

  On an unordered axis it is a set, such as `{A, B}`. Across axes it is a product, such
  as `{hover} × {every instance}`.
- **A patch** is a partial set of overrides plus a region. The overrides are:
  - values for some fields of some nodes (a value may be a function of the
    coordinates, as a clip's is of `t`);
  - nodes added;
  - nodes hidden.
- **The value at a point** is the base with every patch whose region contains the
  point applied over it, weakest first:

  `value(x) = base ⊕ p₁ ⊕ p₂ ⊕ …` for every `pᵢ` with `x ∈ region(pᵢ)`, in strength
  order.

There are no parents. "B inherits its colour from A" means only that the patch which
sets that colour has a region containing both A and B. The UI may still say "inherits
colour from A" when the patch was authored while editing A.

### 2.2 The laws

1. **Identity and associativity.** An empty patch changes nothing, and
   `(p ⊕ q) ⊕ r = p ⊕ (q ⊕ r)`: overrides are a monoid.
2. **Disjoint patches commute.** Order matters only where two patches set the same
   field.
3. **Locality.** Editing a patch changes exactly the points in its region. Its blast
   radius is its region, so it can be shown before the edit is committed.
4. **Specificity decides strength.** If `region(p) ⊂ region(q)`, p wins where both
   apply.
   - So a later slide's `[C, ∞)` beats an earlier `[B, ∞)`, which is what "builds on"
     means.
   - "This only" (`{C}`) beats "from here on", which beats "always".
5. **Incomparable overlaps are resolved by a declared axis order, and flagged.** For
   example, a slide patch and a hover patch both set a colour at (slide 3, hover).
   This is the CSS cascade: specificity, then order. It replaces a hand-made list of
   layer kinds.
6. **Stale entries are inert and kept.** An entry whose node does not exist at a point
   does nothing there and is listed as stale. This is the component model's rule,
   unchanged.

### 2.3 The edit operations

| Operation | Does | The user's words |
|---|---|---|
| Edit | changes a value in one patch | |
| Widen | adds points to a patch's region | "bubble up" |
| Narrow | removes points | "bubble away", push down |
| Split | moves some of a patch's entries into a new patch with another region | "partially bubble up" |
| Merge | joins two patches with the same region | |
| Reorder | the tie-break of law 5 | rare |

All of them are ops in transactions through `iron_document`, undone like any other.

### 2.4 Arrows

The narrative note's primitive is the **move**, an arrow between states (§3.3). Here a
state is a point, and an arrow `x → y` is determined by the patches:
- those that **enter** (`y` in their region, `x` not);
- those that **leave** (`x` in their region, `y` not).

So:
- **On an ordered axis built "from here on", arrows and patches coincide:** the arrow
  `B → C` is exactly the patch `[C, ∞)`. Authoring a move in a deck or a lesson *is*
  making a patch.
- **On a graph** (a machine), arrows are derived from the patches. Arrows are what you
  play and talk about; patches are what is stored.
- **A move's derived sub-moves** (narrative note §6.2, Keynote Magic Move) are the
  field diff between the two points, animated by the context's transition rules and
  overridden by exception.
- **Moves still carry** what the narrative note gives them: a kind, provenance
  (recorded or authored), a duration, and references to what realised them.
  - These are properties of the arrow, not of the patch.
  - A recorded move's patch is never edited in place (the note's integrity rule).

### 2.5 The boundary: values with history

A simulation, a trace or an integrator depends on the **path** through the space, not
only on the point: it is a path function, where the algebra's values are state
functions. These values stay outside the algebra, as state kept per placed point and
per run of its clock (§3). They are made seekable by storing the path (a trace) or by
making it reproducible (deterministic fixed steps and checkpoints).

## 3. Time

### 3.1 Clocks are an axis with a driver

- **Presentation time** is the master: monotonic, never scrubbed, and never shown with
  a playhead.
- **The document clock** (timeline.md §4.1's `clock` node) is the first scripted clock
  beneath it: played, paused, scrubbed, looped.
- **Each composition, and any placed instance, may have its own clock**, defined from a
  parent clock by a mapping. This is reactive-engine.md §10.1's clock providers,
  deferred in phase 3 (step 4).
- **Time flows along connections**, like position or heat. Operators sit between a
  clock and what it drives:

| Operator | Does |
|---|---|
| Rate | slow motion, fast forward |
| Offset | start later |
| Loop | `t` within a range, and `n`, the iteration count |
| Ping-pong | forward, then back |
| Gate | paused while a condition holds |
| Hold / step | freeze; stop-motion steps |
| Remap | a curve from outer to inner time |

- **Free-running or scripted.** A composition clock's parent is either presentation time
  (free-running, the default: exploration pages) or the document clock or a loop under
  it (scripted: narratives, explainers). It becomes scripted by being placed under a
  loop, a take or a narrative: placement gives the clock its parent. The UX of that move
  is open (§7).
- **The visibility gate.** Every composition clock's default gate is "not observed, and
  not read by anything that runs". So a hidden tab pauses, and a hidden simulation
  feeding a visible meter runs.
  - An author may rebind the gate: `false` keeps the page running in the background;
    another condition hands it to something else.
  - This generalises `Store::paused` (accumulators in a composition no observer
    reaches hold still, demo.md) to everything that runs on time.
- **UI runs on unscaled time.** Hover fades and enter or exit transitions use
  presentation time even while a composition's clock is paused or slowed. This is a
  pause menu's rule in games.
- **Steppers take fixed steps** (Glenn Fiedler's "Fix your timestep"): a few capped
  steps per frame, not one step of the frame's length. This makes rate changes,
  resuming and determinism correct, and determinism is what checkpoints, rewind and
  branches need.

### 3.2 Time and necessity are different things

- **Time decides whether state changes.** It is meaning, chosen by the author.
- **Necessity decides whether derived values are computed.** It is an optimisation that
  must never be visible (reactive-engine.md §8.8).

With a hidden tab's clock paused nothing in it moves, so necessity has nothing to
compute and the tab costs nothing. With the gate overridden, necessity still skips its
pictures, and only its state and recorded history advance.

This also removes the reason for `ROOTED` making every `let`, repeat and conditional
needed in a hidden composition (`iron_document/src/lib.rs`). That shortcut is why a
hidden `std:plot` still expands its ticks.

### 3.3 Loops and takes

- **A loop is a reset boundary.** At each wrap, state inside starts from its initial
  value, so each iteration replays deterministically.
- **`n` is an axis.** A take is a point on it, and a take's changes are patches with
  regions on it: `[3, ∞)` for "from take 3 on", `{3}` for "take 3 only".
- **Ghosts and comparisons are placements, not features.**
  - A ghost is another point of the same space (take 2) placed underneath with a ghost
    style, sharing the local clock.
  - A chaos comparison is two points that differ in ρ by 0.001, overlaid on one clock.

### 3.4 Coverage

A scripted clock's simulations are integrated as far as they have been played. The
timeline shades that coverage, as Blender's physics cache bar and After Effects' cached
frames do:
- **solid:** checkpointed, so seeking there is instant;
- **beyond:** integrated forward in fixed steps over a few frames when seeked to, the
  fill growing as it goes;
- **after a parameter edit:** coverage is invalidated after the edit takes effect;
- **a branch:** a second lane from the fork point.

## 4. Where everything lands

| Today | In this model |
|---|---|
| A component instance's overrides (component-model §5.3) | patches on `{that instance}` of the instance axis; the definition is the base |
| A lesson variant that `extends` (variants-and-imports V1) | a variant axis; the variant's differences are patches on its point. `extends` chains become nested regions |
| A machine's states as poses over channels (machines.md M1, M3) | a discrete, unordered axis. Each state's look is patches with regions on it, shared between states where wanted. Channels may survive as the numbers a transition interpolates |
| A machine's transitions and `progress` (M4–M6) | arrows between points. `progress` is a point moving along the arrow, driven by a clip, the narrative clock, or the machine's own player. Tweens refine the derived diff |
| A narrative override's extent (narrative note §6.5: this joint, hold, a range, all) | a region on the narrative-position axis |
| A deck | an ordered axis with next and previous edges; slide patches default to `[k, ∞)` |
| Takes of a loop | the iteration axis, driven by the loop clock's `n` |
| A clip or keyframes | a patch on a time interval whose values are a function of `t` |
| A machine's current state as an in port (M2) | the coordinate on that axis, set by an event, a binding, a clip key or a narrative patch |
| Orthogonal behaviour (hover and open/closed) | two axes, a product (Harel's orthogonal regions), not one machine of four states |
| The scratch layer, gesture previews | session patches on the current point, strongest by axis order |
| Placements, ghosts, comparisons | several points of the same space shown at once |

A view shows one point. This is reactive-engine.md §10.5's slice, and the user's "time
as one slice through a high-dimensional space", made literal: the document is a function
over a coordinate space, and every view is a slice of it.

## 5. Editing

- **Each view has an edit region.** Its default comes from the notation:
  - "from here on" for a deck or takes;
  - "this state" for a machine;
  - "the definition" inside a component.

  A modifier picks the other common choice ("this only").
- **A drag always changes what you see.** If a more specific patch already sets the
  field at this point, writing to the wider edit region would change nothing visible.
  So the edit goes to the patch that currently wins for that field, and the editor says
  so, with one step to widen: "Edited slide C's own colour. Apply from B on?"
- **Editing a value that later points override** says so: "Slides C and D keep their
  own colour".
  - Those points are marked as shadowed.
  - One action drops their overrides so they follow.
- **Lenses are the precedent for the reverse direction.** Writing through a view to the
  patch that supplies the value is a lens's `put`. The spine's binding-inversion rule
  (document-spine.md §5.10) already does this for bindings.

## 6. UX ideas

Each is a projection of the algebra, not a separate feature.

1. **The region editor for an ordered axis is the timeline.** The dope sheet already
   shows rows per object and property, and bars over an axis. Give it a discrete axis
   too, with slides, takes or narrative joints as columns:
   - each patch is a bar from where its region starts to where it ends;
   - dragging a bar's end widens or narrows it;
   - dragging a field's row out of a bar splits it.

   One panel, for time and for sequences.
2. **The state map for an unordered axis.** States are nodes and transitions are arrows,
   as in a machine's graph view.
   - Patches are translucent hulls around the states they cover (bubble sets).
   - Dragging a state into or out of a hull widens or narrows that patch; dragging a
     field chip from the inspector onto a state does the same for one field.
   - Clicking an arrow opens what it changes (the fields entering and leaving), its
     derived animation, and a scrubber for its progress.
3. **A grid for product axes**, like Figma's variant sets (size × state). A patch
   covering a row, a column or a cell is a band on the grid.
4. **Provenance chips in the inspector.** Every field shows a coloured chip for the
   patch that supplies it, with its region in words ("B →", "hover", "this instance",
   "keyframed").
   - Its menu has: widen to…, narrow to here, split out, reset to inherited.
   - Selecting a chip lights its region in the timeline or state map.
5. **A live blast radius.** While dragging on the canvas, a filmstrip of the axis's
   other points updates live:
   - points that will change show it;
   - points that won't are dimmed;
   - shadowed ones are hatched.
6. **The edit region on the view's frame.** A pill such as "Editing: B → end", coloured
   like the frame and the breadcrumb (narrative note §6.5's "a narrative at a point"),
   with the other regions one click away.
7. **Compare two points.** Onion skin, a field diff, and "make equal" per field (a patch
   operation). A ghost in a take is the same thing kept on while playing.
8. **Capture after exploring.** Explore in the scratch layer, then "keep as a patch" and
   choose its region; the narrative note expects capture to be the common path.
9. **The agent speaks regions.** "From slide 3 on", "only while hovered", "for every
   take after this one" map to regions directly, and its view of a point lists each
   value's patch.

## 7. Precedents

| Precedent | What it gives |
|---|---|
| CSS: pseudo-classes, media and container queries, the cascade | Patches with regions over state and viewport axes; specificity then order. The everyday form of this model |
| Harel statecharts, orthogonal regions | Several state axes at once, as a product |
| Figma variant sets | A discrete grid of properties; reset overrides; push changes to the main component |
| USD composition (layer stacks, variant sets, strongest opinion per field) | Per-field resolution over layered opinions; film pipelines' show → sequence → shot inheritance is the deck's chain |
| Delta-oriented programming, feature algebra (Schaefer et al.; Apel and Lengauer, *An Algebra for Features and Feature Composition*) | A product is a core plus deltas with application conditions over features: a patch with a region, with published laws |
| Darcs and Pijul patch theory (Mimram and Di Giusto, *A Categorical Theory of Patches*) | A branch is a set of patches; independent patches commute. Here conflicts resolve per field by laws 4–5, which is simpler |
| Unity and Unreal animation layers with masks | An override layer applying to part of a rig: a patch's fields are its mask |
| Lenses (bidirectional transformations) | Where an edit through a view lands |
| Blender's NLA editor, After Effects precomps and time remapping | Clock mappings on placed strips; nested time |
| Blender's physics cache, After Effects' cache line | Coverage shading |
| Games' time scale, per-actor dilation, fixed timestep, replays | The clock tree, unscaled UI time, determinism |
| Racing games' ghost cars | Ghosts as a placement |

## 8. What this revises (breaking changes welcome)

- **machines.md**, which the user judges over-built before its value was shown. States
  become points on an axis; poses become patches; transitions become arrows. Channels
  and `progress` may survive.
- **variants-and-imports.md V1:** `extends` chains become regions on a variant axis.
- **The narrative note §6.5:** extents become regions on the narrative-position axis,
  and its states (content-addressed snapshots, §3.2) are points.
- **component-model.md §5.3:** instance overrides become patches on the instance axis.
  The "outermost instance's opinion is strongest" rule must come out of laws 4 and 5,
  or be stated as the axis order.
- **reactive-engine.md §8.3 and `ROOTED`:** hidden compositions pause by their clock
  gate (§3.1), not by a list of facet kinds.
- **timeline.md:** the panel becomes the region editor for any ordered axis (§6 item 1),
  and its clock one node of the clock tree.

## 9. Open

- **The axis order** for incomparable overlaps (law 5). One rule, but its order must be
  written down and tested.
- **Structure in patches:** added and hidden nodes, their stale rules, and what they
  cost. Phase 2c's "exceptions as effective state" is the starting point.
- **One source leads per axis.** If a loop's `n` and the narrative both drive the take
  index, the narrative note's "one source leads at a time" (§6.4) applies, but per axis.
- **Moving a system from free-running to scripted time.** Placement gives the clock its
  parent, but how that is offered and shown needs sketching.
- **Events across clocks:** what a running meter reads from a paused page, and how a
  machine's timed transitions count paused time (they should not).
- **Recorded moves:** how an op-log path summarises to a patch, and whether a recorded
  patch can be widened (an authored copy, probably).
- **What persists per point:** simulation state per placed point, and whether takes
  share a checkpoint at their start.

## 10. A building order, if adopted

1. **Composition clocks with the visibility gate and fixed-step steppers.** This fixes
   the hidden-tab cost this began with, and everything after builds on the clocks
   without changing them.
2. **The algebra as a toy with property tests:** identity, associativity, commutation of
   disjoint patches, locality, specificity, stale entries. These are random scripts, as
   the store's tests already are. Then a spec.
3. **Machines rebuilt on axes and patches**, replacing machines.md's poses; the plot's
   `vis` is the first user.
4. **Decks and takes:** ordered axes, the timeline as region editor, provenance chips.
5. **Clock operators, loops with `n`, ghosts; then coverage and checkpoints.**
