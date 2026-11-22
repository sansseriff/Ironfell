# Patches: one algebra for "the same thing, a bit different"

**Status:** Draft for the user's approval (PT-4a, 2026-11-21), revised after council
round 1. **Not approved; do not build from it.** Building starts at PT-5 once the user
approves this and PT-4b's storage spec, [`patch-storage.md`](patch-storage.md)
(plans/handoff-patches.md §2). Written from the design note
[`../notes/patches-regions-and-time.md`](../notes/patches-regions-and-time.md), its
council (§11) and the model that tested it, `src/iron_patch` (PT-3). Where this spec and
the note differ, this spec is the later word: the note's laws as written do not all
hold, and §6 here says what does.

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
| **Layer** | where a patch is written: a definition, one that places it, the document, the session | an instance's override is the document's |
| **Arrow** | a move from one point to another | slide 2 → slide 3 |

The interface says **"Applies to"** for a region and shows none of these words.

There are no parents. "Slide C inherits B's colour" means only that the patch setting
the colour has a region containing both.

## 3. Axes

### 3.1 Kinds

| Kind | Values | A region on it | Examples |
|---|---|---|---|
| **Ordered** | a sequence of values with stable ids | an interval by endpoint ids, each end open or closed, or unbounded; or a set | slides, takes (`n`), narrative joints |
| **Unordered** | a set of values with stable ids | a set, or every value but a set | a machine's states, a mode |
| **Instance** | the placements of a definition, each a path of placement steps (outermost first) | a suffix pattern (`*/o1/j`), less some longer patterns; or a named set (a class) | which meter, which oscilloscope's part |
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
- **A take axis is a sequence axis, wherever it is declared:** a component's loop
  declares its iteration as a local axis, a document's take series is a document axis,
  and either orders among the sequences (P11). There is no third kind of take.

### 3.2 Where axes live

- **A component carries its own axes:** its machine's states, its clock, and its loop's
  iteration if it has one. A placement's point is its value on the instance axis
  together with its coordinates on those local axes.
- **A document carries axes for what spans compositions:** a deck's slides, a lesson's
  narrative joints, a take series.
- **The session is not an axis** but the outermost layer (§6.1, P8).

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
- **Regions are compared as written** (P16). Whether one region is within another,
  equals it, or meets it, which strength, the drag, merge and the lints all ask, is
  decided from the selections as written, including values the axes do not have yet.
  `[C, ∞)` is not `{C}` while C is the last slide, since a slide added after C is in one
  and not the other; `*/o1/j` is not `bench/o1/j` while one bench is placed. A set's
  values count as written, deleted ones included. So adding or inserting a value never
  changes a decision: no value at an existing point, no drag's ops, no default rank, no
  merge's verdict, no lint (PT-3's `growing_an_axis_changes_no_decision`).
- **Unions of boxes are answers.** A query's result (§10), a winning region, a blast
  radius: unions, closed under intersection and difference, computed axis by axis
  without enumerating points (`iron_patch::region`). Answers are about the points that
  exist.
- **Selections by kind:**

  | Selection | Means |
  |---|---|
  | (absent) | every value |
  | `[a, b]`, `(a, b]`, `[a, ∞)`, … | an interval (ordered, time) |
  | `{a, b}` | these values |
  | `not {a, b}` | every value but these (grows as values are added) |
  | `*/o1/j` | instances whose path ends in these steps |
  | `*/j not */o2/j` | …less those ending in a longer pattern |
  | a class name | a named set of instances (§11) |

- **Pattern steps** are placement ids qualified by the definition that declares them,
  since placement ids are definition-local and two definitions may each have an `o1`.
  **A pattern is anchored where it is written:** its paths start at that scope, so a
  pattern written in a definition matches inside each placement of that definition and
  nowhere else.

## 5. The value at a point

### 5.1 Resolution

For a point x and one (node, field):
1. **Structure first** (P6). Whether a node exists at x is a field like any other
   (`present`), resolved as below, and a node exists where it is present and its parent
   exists. A patch that adds a node sets its presence; a base node is present unless a
   patch removes it. A nested template's `use.notation`, which replaces a template's
   children, is structure too, resolved with presence, so every target is found in
   the tree at x and not the original base (note §11.4).
2. **Then each field:** among the patches whose region contains x and that set the
   field, the strongest's entry (§6.1); else the base's value.
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
  point that has it, and an entry may name it. **An added node belongs to the scope
  that writes the patch**, not to the definition it is added into: a variant's added
  state or transition is the variant's, saved and deleted with it.
- **Removed nodes** are nodes a patch makes absent. What reads them where they are
  absent reads nothing, as a deleted node's reader does today.
- **Child order** is the base's: a patch does not reorder (P7). A patch that wants a
  different order sets a layout field.
- **Hidden nodes** exist: they are laid out where layout says so, readable by bindings,
  and not drawn. Absent nodes are none of these.
- **Structure may differ by point** (presence is a field), and evaluation at a point
  (PT-5) builds the structure of that point. A slice of *history* stays values only,
  as reactive-engine.md D10 leaves it: a ghost of a take that adds parts is PT-10's
  problem, not this algebra's.

### 5.3 Stale entries

An entry is **stale** when it does nothing anywhere it would win: its winning region
(§6.2) is non-empty, and at every point of it the entry's node is absent or has no such
field. Stale entries are kept, apply nothing, and are listed with their reason, `absent
node` or `no such field` (`Store::stale()`, replacing `stale_overrides`).
- **A removal is never stale:** an entry that makes its node absent is why the node is
  absent.
- **An entry absent at only some of the points where it wins is normal** (a part only
  take 3 adds), and is not listed.
- **"Not loaded" is not stale.** An entry whose node is in a chunk not yet loaded
  (PT-4b) is unknown until it loads, and a lint over a partly loaded scope says so.

## 6. Strength and the laws

### 6.1 Strength

Strength is **(layer, rank)**, compared layer first (P1, P15).

- **The layer is where a patch is written.** For any node, the scopes that may write it
  form a chain: the definition the node belongs to, each definition that places that
  one, outward, then the document, then the session. A patch written further out is
  stronger than every patch written further in, whatever their regions and whatever
  order they were made in: component-model.md §5.3's outermost-wins, by structure. An
  imported or `std:` definition's patches are in its layer and may not be written, and
  their ranks never compare with the document's.
- **The session is the outermost layer** (P8): the scratch layer, and above it a
  gesture's previews. Its patches are never saved or synced. Being a layer rather than
  an axis, a session patch takes any region, normally all of it, so a preview does not
  stop applying when `t` moves (note §11.3).
- **The rank orders patches within a layer.** It is an order key, like child order's,
  so a patch fits between two others without renumbering.
- **A new patch's rank comes from its region's axis keys** (below): just above every
  patch of its layer whose keys are no greater, below the next rank above those. So
  when patches are made, a narrower region is stronger, "this only" beats "from here
  on" beats "always", and a newer patch on an equal region beats the older.
- **The keys give the default and nothing else.** Once a patch is made its stored rank
  decides, and editing its region does not change it. Derived strength would: widening
  `{C}` to `[C, ∞)` would make an existing `[C, D]` the more specific, and C would
  change though it was in the patch throughout (PT-3 found this at seed 53, and in the
  example `apply_from_b_on_and_the_widening_flaw`). Stored rank keeps region edits
  local (law 3).
- **A rank against inclusion shows as dead entries.** An entry is dead where it wins
  nowhere: a stronger patch of its layer sets the field everywhere it applies
  (`World::dead`). The usual cause is a region strictly within another's that is the
  weaker, after a region edit or a reorder (`World::contradictions`), or an earlier
  edit that left no slot for a new patch below every more specific one, so it went
  above. The lint offers both fixes: raise the inner patch (reorder), or drop the dead
  entries.

**The axis keys** (P10). Each axis has a key on its selections, taken from the
selection as written, so it does not change as the axis grows; a selection strictly
within another (as written) always has the larger key. Larger is more specific:

| Kind | Key, compared in this order |
|---|---|
| Ordered, time | its lower end later (an open end just after a closed one at the same value); then its upper end earlier; then a finite set before an interval with the same ends (an interval holds values yet to be inserted); then fewer values in a set |
| Unordered | a finite set before a cofinite one (`not {…}`, or every value); then fewer values in a set, or more values excluded from a cofinite one |
| Instance | more steps fixed (a pattern's length; a class's shortest member); then a class before a pattern of that length; then fewer members in a class, or more excluded patterns that extend the pattern (an excluded pattern that does not extend it removes nothing, and does not count) |

A region's key is its axes' keys **in the declared axis order** (P11), compared
lexicographically.

**The declared axis order** (P11), first deciding first: **instance, picture, a
component's local state axes, clocks, then the document's sequence axes** (narrative,
slide, take). So an instance's override beats its definition's hover pose at
(this instance, hover), as it does today and as a CSS id beats `:hover`; and a hover
pose beats a slide's patch at (slide 3, hover). A document may reorder its own sequence
axes among themselves.

### 6.2 The laws, as they hold

1. **Identity.** A patch with no entries, or an empty region, changes no value.
2. **Disjoint patches commute.** Exchanging the ranks of two patches of one layer,
   adjacent in strength, that set no field in common changes no value.
3. **Locality.**
   - **Editing a field's entry** changes exactly the points of its **winning region**
     (its patch's region less the regions of stronger patches setting that field) where
     the value differs. This is an edit's blast radius, and it can be shown before the
     edit.
   - **Editing a presence or hiding entry** changes its winning region times the
     node's subtree: its descendants appear, vanish or hide with it, and readers bound
     to any of them follow, as readers of any changed value do.
   - **Editing a region** changes only the points it adds or removes.
   - **None covers values with history:** an edit on `t ∈ [2, 4]` changes an
     integrator's state at every later `t`, and a blast-radius display says so.
4. **Strength is stored; specificity gives the default** (§6.1). Where two patches of
   one layer were made with different keys, the more specific is the stronger until a
   region edit or reorder says otherwise, and the lint shows it when one does.
5. **Ties of specificity are flagged.** Two patches of one layer whose keys are equal,
   whose regions differ (as written) and meet, and that set a field in common were
   ordered by rank alone; the lint lists the pair (`World::ties`; `{a, b}` against
   `{b, c}` on one axis). Two patches on the same region are a deliberate order and are
   not listed. Where the keys decide, nothing is flagged: a slide patch under a hover
   pose is the order working.
6. **Stale entries are inert and kept** (§5.3), and axis values are kept as tombstones
   (§3.1).

Values do not depend on the order patches are stored in. These are properties in
`src/iron_patch/tests/laws.rs`, 19 of them over 10,000 random scripts each, checked
against a reference that enumerates every point and one written apart from the model;
PT-5 runs them against the store.

**What PT-3 does not cover.** The properties report a failing seed, not a minimal
case (no shrinking). Child order is not modelled (P7: patches do not reorder, so there
is nothing to check). There is no canonical form of a region, and none is needed: two
regions are the same when each is within the other as written. Not modelled at all:
values that are functions of the coordinates (a clip's), entries on a field a node
lacks, relative regions (§13), and cost.

## 7. Editing

### 7.1 The operations

| Operation | Does | Ops |
|---|---|---|
| **Edit** | changes an entry's value | set an entry |
| **Widen, narrow** | adds or removes points of a patch's region, on one axis | add or remove values or move an endpoint, per axis (PT-4b: they merge) |
| **Split** | moves some entries to a new patch of the same layer on another region; on the same region the new patch sits just above, and every value stays | add a patch, clear entries |
| **Merge** | joins two patches into the stronger | set entries, remove a patch |
| **Reorder** | changes a rank within its layer | set a rank |
| **Add, remove** | a patch | |

**Merge is refused** for a patch with itself; for a patch that may not be written; for
two layers; for regions that differ as written; and where a patch between the two in
strength meets them and sets a field the weaker sets and the stronger does not, since
one rank cannot then keep both results (note §11.2). The last refusal is
conservative: it does not look at whether that patch's value happens to agree, or
whether a fourth patch hides the overlap.

All are ops in transactions through `iron_document` (AGENTS.md), undone like any other;
each has an inverse (PT-3's `undo_restores_everything`).

### 7.2 The edit region and the drag

**Each view has an edit region** (the note's §5): "from here on" in a deck or takes,
"this state" in a machine, "the definition" inside a component; a modifier picks "this
only". The view's frame shows it ("Editing: B → end"). **A view also writes in a
layer:** a definition's when it edits inside one, the session's while it writes to
the scratch layer, else the document's. The drag's own preview is a session patch;
the rule below reads the values under it.

**A drag at x, from a view writing in layer L with edit region E (x in E), setting a
field to v** (P12):
- **The value at x is already v:** nothing is written.
- **The patch that wins at x is of a layer outside L:** refused. The view cannot
  override an outer scope, and says which one sets the value.
- **Nothing sets the field at x:** the weakest writable patch of L on exactly E (as
  written) takes it, made at the default rank if there is none.
- **The winner is of L, may be written, and its region is within E:** the winner takes
  it.
- **Otherwise** a patch of L on **E ∩ the winner's region** takes it: of those on
  exactly that region, the weakest that is stronger than the winner with no patch
  between them setting the field where it applies; else a new one, just above the
  winner, or at the default rank when the winner is of an inner layer.

So a drag shows what it set; dragging to the value shown writes nothing; a second drag
at x writes into the patch the first made, adds none, and ends as one drag would
(PutGet, GetPut, PutPut). It changes exactly the new entry's winning region where the
value differs, never a point outside E, and never a patch that may not be written (a
recorded move's, an imported or `std:` definition's). E is as written, so values the
axes gain later inside E are inside it. These are PT-3's `a_drag_obeys_the_lens_laws`
and its blast-radius property.

**A clip-driven field** (a value that is a function of `t`) is dragged as timeline.md
§12 says, by the visible auto-keyframe mode: on, a key at the playhead (or that key's
value, on a key); off, every key's value moved by the drag's delta. The guarantee
"nothing outside E changes" is for the key: a new key also changes the interpolated
values between its neighbours, and moving every key changes the clip wherever it
applies. The blast radius shown while dragging says so.

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
- **An arrow that jumps** (B → E, past C and D) is the diff between its ends, not the
  steps between run in turn: a patch that enters at C and leaves at D does not move.
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
- **The layer is not written:** it is the scope the `patch` element is in (§6.1).
- **`rank`** is the order key within the layer (P1). Markup without one takes §6.1's
  default when it is applied.
- **Entries** are `set` (a field: a constant, a bound value `{= …}`, or a clip),
  `add` and `remove` (presence), `hide` and `show`.
- **A machine** keeps its markup (machines.md §2), its channels and its transitions
  and tweens, but for poses: `state` nodes carry no channel values, and each pose is a
  `patch applies="vis = shown"` inside the definition setting channels, so a value
  several states share is one patch over them (`applies="vis in {shown, compare}"`).
  Template parts still bind to channels (M1).
- **An instance's overrides** are `patch applies="instance ~ */o1/j"` children of the
  definition or instance where they are written today (component-model.md §5.3's path
  targets become patterns, anchored there, §4).

## 10. Queries

A region is also a **selector**: a condition on a point. Queries use §9's grammar, and
answer in regions (unions of boxes), per node, without enumerating points.

| Question | Answer |
|---|---|
| Find: where is this field this value? | for each entry or base value that matches, its winning region ∩ the query |
| Why is this value what it is here? | the patches containing the point that set the field, strongest first (layer, then rank), the losers struck through |
| What would this edit change? | the entry's winning region (§6.2, law 3) |
| What changes between these two points? | the arrow (§8) |
| On which slides is anything blue? | roll-up: project the find's regions onto one axis |
| Lint | ties (law 5), dead entries (§6.1, which shows ranks against inclusion), stale entries with their reason (§5.3) |

- A search result can become a patch's region: "make all of those red" is one op per
  box of the answer.
- **The agent** reads patches with their regions instead of every point's values,
  explains a value by its patch stack, and states an edit's blast radius before making
  it.
- **Where symbolic answers stop:** a bound value is evaluated at the points asked
  about (slices, reactive-engine.md §10.5, which PT-5 takes over); values with history
  answer "not seekable"; "blue" matches theme tokens as well as literal colours; what
  is not loaded answers "not loaded" (PT-4b).

## 11. What each mechanism becomes

| Today | Becomes | Deleted when it lands |
|---|---|---|
| An instance's overrides (component-model.md §5.3), nested targets | patches on the instance axis, in the layer of the scope that writes them; a nested target's path is a suffix pattern (§4), and outermost-wins is the layers' order | `override` nodes, their application in expansion, `stale_overrides` (PT-7) |
| A machine's states and poses (machines.md M1, M3) | an unordered local axis of `state` nodes; channels stay, and each pose is a patch on the axis setting them, shared between states where wanted | the channel values on `state` (PT-6) |
| A machine's current state (M2) | the coordinate, set by its driver: the in port, which a binding, a clip key or a narrative override sets, as today (P9) | nothing |
| Transitions, `progress`, tweens (M4–M6, M8) | arrows; transitions and tweens stay nodes, so a class may add or remove them as structure; `progress` and interruption are the driver's | nothing; tweens refine the arrow's diff |
| M7, "one writer per leaf" | **kept** (machines.md §4.3): a channel's writer is its machine, and a clip keys the state, never a channel. Several patches setting one leaf are not several writers: strength picks one at each point | nothing |
| A lesson variant that `extends` (variants-and-imports.md V1) | a **class** (P14): a named set on the instance axis; `extends` chains are nested classes, so C's region covers D's when D extends C. A variant that adds a state or transition adds nodes that are the variant's (§5.2) | `extends` (PT-7) |
| A narrative override's extent (narrative note §6.5) | a region on the narrative axis: a hold is `[k, ∞)`, and holds are ranked by their start, so a later start wins without cutting the earlier region. Retiming a hold re-ranks it in the same edit | the extent kinds (PT-9) |
| A deck | an ordered document axis; slide patches default to `[k, ∞)` | |
| Takes of a loop | a sequence axis, driven by the loop's `n` (§3.1) | |
| A clip | a patch on its clock's time axis whose values are functions of `t`, with region `[k₀, ∞)` (a clip holds after its last key); a staggered target is its own patch | clips' separate resolution (PT-9) |
| The scratch layer, gesture previews | patches in the session layer, the outermost (P8) | the overlay's separate precedence (PT-5) |
| `ROOTED`'s list of facet kinds | holding a hidden composition still is the visibility gate on composition clocks (PT-1, done). `ROOTED`'s other role, keeping what a picture demands computed, stays until PT-10 | `ROOTED` (PT-10) |
| Value slices (reactive-engine.md §10.5) | evaluation at a point: an observer has a point on every axis, not only the clocks | the slice machinery (PT-5) |

**Two instance relations stay apart.** Several entities from one definition share
design and have their own state: the instance axis. One entity in several pictures
shares state (reactive-engine.md §8.4): a **picture axis**, after the instance axis in
the order, on which a region may set picture-side fields only (projection, style),
never the body's, so the body computes once.

**The boundary stays.** Whether patches from outside a definition may set a part's
fields is a policy per definition (today: any template node's leaf, component-model.md
§5.3); a definition may later declare which fields it exposes.

## 12. Phases

Each ends committed, recorded in the roadmap, with its exit met.

| Phase | Builds | Exit |
|---|---|---|
| **PT-5. Patches in `iron_document`** | Axes, patches and regions as document data; the ops of §7.1; resolution at a point (§5); layers, the stored rank as an order key (the model's `f64` is an artefact) and its default; the lints; evaluation at an observer's point (taking over reactive-engine.md §10.5's slices); a schema bump. No mechanism moves yet | Values agree with `iron_patch` on every script of a differential test (the crate's scripts, run through the store, its ranks mapped to order keys by order); §6.2's properties, and the axis-growth property (§4), hold over the store's random scripts; a patch edit at 700 meters is within 2× of a constant edit |
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
- **Whether ties are noise:** `ties()` lists equal keys on regions that meet; PT-8
  shows the lint and judges whether it helps.
- **Continuous time in the index:** intervals of reals with open ends are modelled
  (`iron_patch`); PT-4b says how they are stored.
- **Cost:** the model enumerates by design; PT-5 measures value-at-point and
  arrow size against the number of patches.

## 14. Decisions

| # | Decision |
|---|---|
| P1 | Strength is (layer, rank). The rank is a stored order key within a layer, given from the axis keys when a patch is made; a region edit does not change it; a rank against inclusion shows as dead entries |
| P2 | Axis values have stable ids, and regions name values by id |
| P3 | A deleted axis value stays in its place as a tombstone |
| P4 | Each clock is its own time axis; a time region names its clock |
| P5 | A patch's region is one box; unions are answers, not regions |
| P6 | Presence (and `use.notation`) is structure, resolved before the fields; hiding cascades and a hidden node exists |
| P7 | A patch does not reorder children |
| P8 | The session is the outermost layer (the scratch layer, previews above it), never saved; it is not an axis |
| P9 | Coordinates are set by drivers, never by patch entries; the axes' dependencies are acyclic |
| P10 | The axis keys of §6.1, from selections as written |
| P11 | The declared axis order: instance, picture, local states, clocks, sequences |
| P12 | The drag writes in the view's layer, into E ∩ the winner's region, by §7.2's rule, and is refused where an outer layer sets the value |
| P13 | One selector grammar for regions and queries |
| P14 | Variants become classes on the instance axis |
| P15 | Layers: where a patch is written; an outer layer is stronger than every patch of an inner one |
| P16 | Regions are compared as written, over values not yet added; queries answer over the points that exist |
