# Patches: one algebra for "the same thing, a bit different"

**Status:** Draft for the user's approval (PT-4a, 2026-11-21). **Not approved; do not
build from it.** Building starts at PT-5 once the user approves this and PT-4b's
storage spec (plans/handoff-patches.md §2). Written from the design note
[`../notes/patches-regions-and-time.md`](../notes/patches-regions-and-time.md), its
council (§11) and the model that tested it, `src/iron_patch` (PT-3). Where this spec and
the note differ, this spec is the later word: the note's laws as written do not all
hold, and §5 here says what does.

**Read first:** the note's §1 to §4 (the motivation and the mapping); its §11 (what the
council found); `src/iron_patch/src/lib.rs` and `tests/laws.rs` (the laws as running
properties). For what is replaced: [`component-model.md`](component-model.md) §5.3,
[`machines.md`](machines.md) §1 and §2, [`variants-and-imports.md`](variants-and-imports.md)
V1, the narrative note's §6.5.

## 1. What this is

Iron has several mechanisms for a thing that differs a little from another: an
instance's overrides, a lesson variant that `extends`, a machine's states as poses, a
clip, a narrative override with an extent, the scratch layer, gesture previews. This
spec replaces them with one: a **patch** is a set of overrides with a **region**, the
set of **points** where it applies, and the value at a point is the base with every
patch whose region contains the point applied over it, weakest first.

What this spec settles: the terms (§2), axes (§3), regions (§4), how a value is
resolved (§5), strength and its laws (§6), editing (§7), arrows (§8), markup (§9),
queries (§10), what each mechanism becomes (§11) and the phases (§12). What it leaves
to PT-4b: storage, loading, the index's shape and sync. What it leaves open: §13.

## 2. Terms

| Word | Means | Example |
|---|---|---|
| **Axis** | a whole degree of freedom | "slide", "mode", "instance", "take" |
| **Value** | one position on one axis | slide 3; `hover`; `bench/o1/j` |
| **Point** | one value on every axis in play | (slide 3, hover, `bench/o1/j`) |
| **Region** | a set of points | "slide 3 on, while hovered, any instance" |
| **Patch** | entries plus a region | "from slide 3 on: the bob is red" |
| **Entry** | one (node, field, value) a patch sets | `#bob` `fill.color` red |
| **Arrow** | a move from one point to another | slide 2 → slide 3 |

The interface says **"Applies to"** for a region and shows none of these words.

There are no parents. "Slide C inherits B's colour" means only that the patch setting
the colour has a region containing both.

## 3. Axes

### 3.1 Kinds

| Kind | Values | A region on it | Examples |
|---|---|---|---|
| **Ordered** | a sequence of values with stable ids | an interval by endpoint ids, each end open or closed, or unbounded | slides, takes (`n`), narrative joints |
| **Unordered** | a set of values with stable ids | a set, or every value but a set | a machine's states, a mode |
| **Instance** | the placements of a definition, each a path of placement steps (definition-local ids, outermost first) | a suffix pattern (`*/o1/j`), less some longer patterns; or a named set (a class) | which meter, which oscilloscope's part |
| **Time** | a clock's time, continuous | an interval, each end open or closed | a clip's span |

- **Values have stable ids** (P2). A region names values by id, so inserting, moving or
  renaming a value never rewrites a region.
- **A deleted value stays in its place** as a tombstone that no point can take (P3). An
  interval whose endpoint is deleted keeps starting or ending where it was: deleting
  slide C leaves `[C, ∞)` starting where C was, and a slide inserted after C's place is
  inside it. Moving a value moves it relative to intervals that do not name it. A
  tombstone with no region naming it may be collected.
- **States are nodes** (`state` nodes under a machine, as today), so a variant or patch
  that adds a state adds an axis value through §5.2's added nodes.
- **Each clock is its own time axis** (P4). A placed instance's clock is its own
  (reactive-engine.md §10.1), so a time region names its clock, and a point has one
  coordinate per clock in view.

### 3.2 Where axes live

- **A component carries its own axes:** its machine's states, its loop's iteration, its
  clock. A placement's point is its value on the instance axis together with its
  coordinates on those local axes.
- **A document carries axes for what spans compositions:** a deck's slides, a lesson's
  narrative joints, takes.
- **The session axis** (P8) has one value per session layer (the scratch layer, a
  gesture's previews). Its patches are never saved or synced.

### 3.3 Coordinates

What sets a point's coordinate is the axis's **driver**: an argument (a machine's
current state is an in port, machines.md M2), the narrative's position, a deck's
current slide, a loop's `n`, a clock's time. A driver is never a patch entry (P9): a
patch does not set coordinates, so a patch cannot switch itself on or off. A binding
that drives a coordinate from values a patch sets is allowed, and reads the values at
the current point; the axes' dependency graph must be acyclic, and a cycle is an error
on the driver.

## 4. Regions

- **A patch's region is one box** (P5): one selection per axis, the axes it does not
  name being whole. A strength order that extends inclusion exists for boxes and not
  for unions of boxes (two unions can match on every axis's projection while one lies
  inside the other: note §11.1). Where a patch is wanted over a union, it is several
  patches.
- **Unions of boxes are answers.** A query's result (§10), a winning region, a blast
  radius: unions, closed under intersection and difference, computed axis by axis
  without enumerating points (`iron_patch::region`).
- **Selections by kind:**

  | Selection | Means |
  |---|---|
  | (absent) | every value |
  | `[a, b]`, `(a, b]`, `[a, ∞)`, … | an interval (ordered, time) |
  | `{a, b}` | these values |
  | `not {a, b}` | every value but these (grows as values are added) |
  | `*/o1/j` | instances whose path ends in these steps |
  | `*/j not */o2/j` | …less those ending in a longer pattern |
  | a class name | a named set of instances (§11.1) |

## 5. The value at a point

### 5.1 Resolution

For a point x and one (node, field):
1. **Presence first** (P6). Whether a node exists at x is a field like any other
   (`present`), resolved as below, and a node exists where it is present and its parent
   exists. A patch that adds a node sets its presence; a base node is present unless a
   patch removes it.
2. **Then each field:** among the patches whose region contains x and that set the
   field, the strongest's entry; else the base's value.
3. **Hiding cascades** (P6): a node is shown where it exists, its own `hidden` resolves
   false, and its parent is shown. A stronger patch can set `hidden` back to false; it
   cannot show what a hidden ancestor hides.
4. **An entry on a node that does not exist at x does nothing there** (law 6).

A value that is a function of the coordinates (a clip's, of `t`) is evaluated at x; a
bound value (a formula) is evaluated at x as any binding is. Values with history (an
integrator, a trace) are not patch values: they are state kept per placed point and per
run of its clock (the note's §2.5), outside this algebra.

### 5.2 Structure

- **Added nodes** are base nodes whose presence is false, which a patch makes present.
  Their ids are allocated like any node's, so an added node is the same node at every
  point that has it, and an entry may name it.
- **Removed nodes** are nodes a patch makes absent. What reads them where they are
  absent reads nothing, as a deleted node's reader does today.
- **Child order** is the base's: a patch does not reorder (P7). A patch that wants a
  different order sets a layout field.
- **Hidden nodes** exist: they are laid out where layout says so, readable by bindings,
  and not drawn. Absent nodes are none of these.

### 5.3 Stale entries

An entry is **stale** when its node is absent at every point of its winning region
(§6.2), which is non-empty: it does nothing anywhere it would win. Stale entries are
kept, apply nothing and are listed (`Store::stale()`, replacing `stale_overrides`). An
entry absent at only some of the points where it wins is normal (a part only take 3
adds), and is not listed.

## 6. Strength and the laws

### 6.1 Strength

Strength is a **stored rank** on each patch (P1), an order key like child order's, so
a patch fits between two others without renumbering.

- **A new patch takes its rank from specificity** (P1): just above every patch whose
  region is no more specific, by the **axis keys** below, and below the rest. So when
  patches are made, a narrower region is stronger, and "this only" beats "from here
  on" beats "always".
- **Editing a region does not change its rank.** Derived strength would: widening
  `{C}` to `[C, ∞)` would make an existing `[C, D]` the more specific, and C would
  change though it was in the patch throughout (PT-3 found this at seed 53, and in the
  example `apply_from_b_on_and_the_widening_flaw`). Stored rank keeps region edits
  local (law 3).
- **A rank that contradicts inclusion is a lint**, not an error: a patch whose region
  lies strictly inside another's and that sets a field the other sets, and is the
  weaker (`World::contradictions`). Its fix is a reorder.

**The axis keys** (P10), each a total preorder on one axis's selections in which a
selection strictly inside another always has a larger key:

| Kind | Key, compared in this order (larger is more specific) |
|---|---|
| Ordered, time | its first value later; then its last value earlier; then fewer values |
| Unordered | fewer values |
| Instance | more trailing steps shared by every instance it selects; then fewer instances |

A region's key is its axes' keys **in the declared axis order** (P11), compared
lexicographically, then the rank breaks what remains. This is a strict total order
that extends inclusion over boxes (PT-3: 10,000 scripts).

**The declared axis order** (P11), first deciding first: **session, instance, a
component's local state axes, clocks, then the document's sequence axes** (narrative,
slide, take). So an instance's override beats its definition's hover pose at
(this instance, hover), as it does today and as a CSS id beats `:hover`; and a hover
pose beats a slide's patch at (slide 3, hover). A document may reorder its own axes
among themselves; the session axis stays first.

### 6.2 The laws, as they hold

1. **Identity.** A patch with no entries, or an empty region, changes no value.
2. **Order of entries is irrelevant across fields.** Exchanging two patches adjacent in
   strength that set no field in common changes no value.
3. **Locality.**
   - Editing an entry changes exactly the points of its **winning region** (its patch's
     region less the regions of stronger patches setting that field) where the value
     differs. This is an edit's blast radius, and it can be shown before the edit.
   - Editing a region changes only the points it adds or removes.
   - Neither covers values with history: an edit on `t ∈ [2, 4]` changes an
     integrator's state at every later `t`, and a blast-radius display says so.
4. **Specificity gives a new patch its strength** (§6.1).
5. **Ties of specificity are broken by rank and flagged.** Where two patches' keys are
   equal at an overlap and both set a field, the rank decides and the lint lists it
   (a class whose instances all end in `j` against `*/j`: note §11.4). Where the keys
   decide, nothing is flagged: a slide patch under a hover pose is the order working.
6. **Stale entries are inert and kept** (§5.3), and axis values are kept as tombstones
   (§3.1).

Values do not depend on the order patches are stored in. These are properties in
`src/iron_patch/tests/laws.rs`; PT-5 runs them against the store.

## 7. Editing

### 7.1 The operations

| Operation | Does | Ops |
|---|---|---|
| **Edit** | changes an entry's value | set an entry |
| **Widen, narrow** | adds or removes points of a patch's region, on one axis | add or remove values or move an endpoint, per axis (PT-4b: they merge) |
| **Split** | moves some entries to a new patch on another region; on the same region the new patch sits just above, and every value stays | add a patch, clear entries |
| **Merge** | joins two patches on equal regions into the stronger. **Refused** where a patch between them in strength overlaps them and sets a field the weaker sets: one rank cannot keep both results | set entries, remove a patch |
| **Reorder** | changes a rank | set a rank |
| **Add, remove** | a patch | |

All are ops in transactions through `iron_document` (AGENTS.md), undone like any other;
each has an inverse (PT-3's `undo_restores_everything`).

### 7.2 The edit region and the drag

**Each view has an edit region** (the note's §5): "from here on" in a deck or takes,
"this state" in a machine, "the definition" inside a component; a modifier picks "this
only". The view's frame shows it ("Editing: B → end").

**A drag at x, in edit region E, setting a field to v** (P12):
- If the value at x is already v, nothing is written.
- If nothing sets the field at x: the patch on exactly E takes it, made if there is none.
- If the patch that wins at x lies inside E and may be written: it takes the entry.
- Otherwise: a patch on **E ∩ the winner's region** takes it, the existing one if it is
  stronger than the winner, else a new one ranked just above the winner.

So a drag always shows what it set, never changes a point outside E, and never writes a
patch that may not be written (a recorded move's, an imported or `std:` definition's).
These are lens laws, PT-3's `a_drag_obeys_the_lens_laws`: after a drag the value at x
is v; dragging to the value shown writes nothing; a second drag at x writes the patch
the first made, and adds none.

**A value that is a function of the coordinates** (a clip's) is written by kind: a drag
over a clip writes a key at the current time, as autokey does, not a constant over the
clip.

**Shadowing is said, not hidden.** Editing a value that stronger patches override at
other points says so ("Slides C and D keep their own colour"), marks those points, and
offers one action to drop their entries so they follow.

## 8. Arrows

An arrow x → y is determined by the patches: those whose region holds y and not x
**enter**, those holding x and not y **leave**, and the field diff between the two points
is what moves.
- On an ordered axis built from "from here on" patches alone, the arrow B → C is
  exactly the patches starting at C. In general it is every patch with a boundary
  between B and C.
- A move's derived sub-moves are that field diff, animated by the context's transition
  rules, refined by tweens (machines.md M3, M4).
- Arrows carry what the narrative note gives a move: a kind, provenance (recorded or
  authored), a duration, references to what realised it. These belong to the arrow,
  not to any patch.
- **A point mid-arrow is not a point.** A transition at progress 0.4 interpolates two
  points' values; which transition, and where it started (an interrupted one reverses
  from where it is drawn, machines.md M8), is the driver's state. Locality and "a view
  shows one point" are about points, not interpolations.

## 9. Markup

```xml
<axis name="slide" kind="ordered">
  <value id="$a" name="A"/><value id="$b" name="B"/><value id="$c" name="C"/>
</axis>

<patch applies="slide in [$b, ∞)">
  <set target="#title" leaf="text.content" value="Two slits"/>
</patch>
<patch applies="slide = $c">
  <set target="#bob" leaf="fill.color" value="#d33"/>
  <add target="#arrow"/>                 <!-- presence: an added node -->
  <hide target="#legend"/>
</patch>
<patch applies="vis = hover &amp; instance ~ */o1/j" rank="b4">
  <set target="#needle" leaf="stroke.color" value="token:accent"/>
</patch>
```

- **`applies`** is a selector (P13): selections joined by `&`, one per axis, in §4's
  forms: `axis in [a, b]` (any end `(`, `)`, `∞`), `axis = v`, `axis in {a, b}`,
  `axis not in {a, b}`, `instance ~ pattern [not pattern …]`, `instance in class`.
  Values are referred to by id, written by name where the name is unique. The same
  grammar is the query language's (§10).
- **`rank`** is the order key (P1). Markup without one takes §6.1's default when it is
  applied.
- **Entries** are `set` (a field: a constant, a bound value `{= …}`, or a clip),
  `add` and `remove` (presence), `hide` and `show`.
- **A machine** keeps its markup (machines.md §2) but for poses: `state` nodes carry no
  fields, and each pose is a `patch applies="vis = shown"` inside the definition, so a
  field several states share is one patch over them (`applies="vis in {shown, compare}"`).
- **An instance's overrides** are `patch applies="instance ~ */o1/j"` children of the
  definition or instance where they are written today (component-model.md §5.3's path
  targets become patterns; the outermost instance's patch fixes the most steps).

## 10. Queries

A region is also a **selector**: a condition on a point. Queries use §9's grammar, and
answer in regions (unions of boxes), per node, without enumerating points.

| Question | Answer |
|---|---|
| Find: where is this field this value? | for each entry or base value that matches, its winning region ∩ the query |
| Why is this value what it is here? | the patches containing the point that set the field, strongest first, the losers struck through |
| What would this edit change? | the entry's winning region (§6.2, law 3) |
| What changes between these two points? | the arrow (§8) |
| On which slides is anything blue? | roll-up: project the find's regions onto one axis |
| Lint | rank ties (law 5), ranks against inclusion (§6.1), dead patches (an empty winning region for every entry), stale entries (§5.3) |

- A search result can become a patch's region: "make all of those red" is one op.
- **The agent** reads patches with their regions instead of every point's values,
  explains a value by its patch stack, and states an edit's blast radius before making
  it.
- **Where symbolic answers stop:** a bound value is evaluated at the points asked
  about (slices, reactive-engine.md §10.5, which PT-5 takes over); values with history
  answer "not seekable"; "blue" matches theme tokens as well as literal colours.

## 11. What each mechanism becomes

| Today | Becomes | Deleted when it lands |
|---|---|---|
| An instance's overrides (component-model.md §5.3), nested targets | patches on the instance axis; a nested target's path is a suffix pattern, and outermost-wins is the keys' "more fixed steps" | `override` nodes, their application in expansion, `stale_overrides` (PT-7) |
| A machine's states and poses (machines.md M1, M3) | an unordered local axis of `state` nodes; each pose a patch on it, shared between states where wanted | the fields on `state` (PT-6) |
| A machine's current state (M2) | the coordinate, set by the in port as today | nothing |
| Transitions, `progress`, tweens (M4–M6, M8) | arrows; `progress` and interruption are the driver's | nothing; tweens refine the arrow's diff |
| M7, "one writer per leaf" | replaced: a leaf may be set by several patches, resolved by strength; a leaf both bound in the base and set by a patch takes the patch's value where it applies | M7 (PT-6) |
| A lesson variant that `extends` (variants-and-imports.md V1) | a **class** (P14): a named set on the instance axis; `extends` chains are nested classes, so C's region covers D's when D extends C. A variant that adds a state adds a `state` node | `extends` (PT-7) |
| A narrative override's extent (narrative note §6.5) | a region on the narrative axis; a "hold" is `[k, ∞)`, and "a later start wins" ends it at the next hold | the extent kinds (PT-9) |
| A deck | an ordered document axis; slide patches default to `[k, ∞)` | |
| Takes of a loop | the iteration axis, driven by the loop's `n` | |
| A clip | a patch on its clock's time axis whose values are functions of `t`, with region `[k₀, ∞)` (a clip holds after its last key); a staggered target is its own patch | clips' separate resolution (PT-9) |
| The scratch layer, gesture previews | patches on the session axis, first in the order | the overlay's separate precedence (PT-5) |
| `ROOTED`'s list of facet kinds | the visibility gate on composition clocks (PT-1, done) | `ROOTED` (PT-10) |

**Two instance relations stay apart.** Several entities from one definition share
design and have their own state: the instance axis. One entity in several pictures
shares state (reactive-engine.md §8.4): a **picture axis**, on which a region may set
picture-side fields only (projection, style), never the body's, so the body computes
once.

**The boundary stays.** Whether patches from outside a definition may set a part's
fields is a policy per definition (today: any template node's leaf, component-model.md
§5.3); a definition may later declare which fields it exposes.

## 12. Phases

Each ends committed, recorded in the roadmap, with its exit met.

| Phase | Builds | Exit |
|---|---|---|
| **PT-5. Patches in `iron_document`** | Axes, patches and regions as document data; the ops of §7.1; resolution at a point (§5); the stored rank and its default; the lints; evaluation at an observer's point (taking over reactive-engine.md §10.5's slices); a schema bump. No mechanism moves yet | Values agree with `iron_patch` on every script of a differential test (the crate's scripts, run through the store); a patch edit at 700 meters is within 2× of a constant edit; §6.2's properties hold over the store's random scripts |
| **PT-6. Machines on axes** | §11's machine rows; `std:plot`'s `vis` first; the state editor edits patches on the state axis | machines.md's poses deleted; `std:plot` looks and animates as before (its tests and `plot-view.txt`) |
| **PT-7. Instances on the instance axis** | overrides as patches, classes, `extends` as nested classes | every override test passes as patches; `override` deleted; a differential test of old against new over the suite, as the engine plan's was |
| **PT-8. Search and provenance** | §10; provenance chips, the blast radius shown while dragging, the lints in the inspector | the agent answers "why is this blue" and "what would this change" from regions |
| **PT-9. Decks, takes, time** | ordered axes in the UI; clips as patches; narrative extents as regions; `now()` and `clock()` | a three-slide deck and a take series authored and played; clips resolve as patches |
| **PT-10. Clock operators** | rate, offset, loop, ping-pong, gate, hold, remap; ghosts, coverage | per the note's §3; `ROOTED` deleted |
| **PT-11. Storage and the server** | PT-4b's spec | PT-4b's exits |

## 13. Open

- **Relative regions:** "each instance in its own hover state" is a union over
  instances of (that instance, its state = hover): a diagonal no box writes, and the
  commonest machine region. Until it is designed, a pose patch lives in the definition,
  where its region is on the local state axis alone and holds for every instance, and
  a per-instance pose is a patch on (that instance) × (that state).
- **Rank ties as a lint:** whether a tie between a class and a pattern with equal keys
  is worth flagging, or noise (law 5).
- **Continuous time in the index:** intervals of reals with open ends are modelled
  (`iron_patch`); PT-4b says how they are stored.
- **Cost:** the model enumerates by design; PT-5 measures value-at-point and
  arrow size against the number of patches.

## 14. Decisions

| # | Decision |
|---|---|
| P1 | Strength is a stored rank (an order key), given from specificity when a patch is made; a region edit does not change it; a rank against inclusion is a lint |
| P2 | Axis values have stable ids, and regions name values by id |
| P3 | A deleted axis value stays in its place as a tombstone |
| P4 | Each clock is its own time axis; a time region names its clock |
| P5 | A patch's region is one box; unions are answers, not regions |
| P6 | Presence is a field resolved before the rest; hiding cascades and a hidden node exists |
| P7 | A patch does not reorder children |
| P8 | Session layers are patches on a session axis, first in the order, never saved |
| P9 | Coordinates are set by drivers, never by patch entries; the axes' dependencies are acyclic |
| P10 | The axis keys of §6.1 |
| P11 | The declared axis order: session, instance, local states, clocks, sequences |
| P12 | The drag writes into E ∩ the winner's region, by §7.2's rule |
| P13 | One selector grammar for regions and queries |
| P14 | Variants become classes on the instance axis |
