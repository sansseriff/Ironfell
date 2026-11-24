> **Historical (superseded 2026-11-23):** Do not build from this document. The universal
> N-axis patch algebra specified here was prototyped in `EN-3`–`EN-5c` (`05df865..85c4c45`,
> preserved on branch `archive/universal-patch-experiment`), added +11,800 net lines without
> deleting `Docs::copies` or shipping user-facing variation features, and was reverted in
> `cc9f380`. Read [`../notes/postmortem-universal-patch-algebra.md`](../notes/postmortem-universal-patch-algebra.md)
> for the post-mortem, and [`../specs/patches.md`](../specs/patches.md) for the grounded
> discrete-variation and node-inspectability plan that replaces it.

# Patches: one algebra for "the same thing, a bit different"

**Status:** Companion algebra spec to [`engine.md`](engine.md) (originally drafted 2026-11-21
and revised through council rounds 1–3; round-3 polish and user approval live in
[`../handoff-engine.md`](../handoff-engine.md) `EN-3`). Verified by `src/iron_patch`.

**Read first:** [`engine.md`](engine.md) §2, `src/iron_patch/src/lib.rs`, and `src/iron_patch/tests/laws.rs`.
For what is replaced: [`component-model.md`](component-model.md) §5.3,
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
| **Layer** | where a patch is written: a definition, a variant of it, one that places it, the document, the session | an instance's override is the document's |
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
| **Instance** | the placements of a definition, each a path of placement steps (outermost first) | a suffix pattern (`*/o1/j`), less some longer patterns | which meter, which oscilloscope's part |
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
the current point. **The axes' dependencies must be acyclic:** axis A depends on axis B
when A's driver reads a value, or a node's presence (a state value, a `use.notation`),
that some patch whose region names B sets. A patch on `vis = hover` that sets the
machine's in port, or decides whether `hover` exists, closes a cycle. The static axis-dependency check (over `Expr::deps`) refuses a cycle before derivation
(`run`); a dynamic cycle revealed only through a tracked read (`deref`) evaluates with
previous-pass coordinates and reports a problem rather than refusing after derivation
(reconciling P9 with `engine.md` E1). Built in `EN-6a`, since until then no driver reads
a patch.

## 4. Regions

- **A patch's region is one box** (P5): one selection per axis, the axes it does not
  name being whole. This is a decision, not a theorem: §6.1's keys are per axis, and
  per-axis keys cannot order unions (two unions can match on every axis's projection
  while one lies inside the other: note §11.1). Where a patch is wanted over a union,
  it is several patches.
- **Regions are compared as written** (P16). Whether one region is within another,
  equals it, or meets it, which strength, the drag, merge and the lints all ask, is
  decided from the selections as written, including values the axes do not have yet.
  `[C, ∞)` is not `{C}` while C is the last slide, since a slide added after C is in one
  and not the other; `*/o1/j` is not `bench/o1/j` while one bench is placed. A deleted
  value is still a value as written: after C is deleted, `{C}` and `[C, C]` are the same
  region, and neither is the empty one. So adding or inserting a value changes no
  value at an existing point, no drag's ops, no default rank, no merge's verdict, no
  tie and no inversion (PT-3's `growing_an_axis_changes_no_decision`, growing every
  axis kind). **Dead and stale entries are answers about the points that exist**
  (§10), and can change as an axis grows: an entry dead under a stronger `[B, D]` lives
  again at a slide appended after D.
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
  state or transition is the variant's (§11), saved and deleted with it.
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
  form a chain: the definition the node belongs to, its variants (an `extends` chain,
  innermost first), each definition that places it, outward, then the document, then
  the session. A patch written further out is
  stronger than every patch written further in, whatever their regions and whatever
  order they were made in: component-model.md §5.3's outermost-wins, by structure. An
  imported or `std:` definition's patches are in its layer and may not be written, and
  their ranks never compare with the document's.
- **The session is the outermost layer** (P8): the scratch layer, and above it a
  gesture's previews. Its patches are never saved or synced. Being a layer rather than
  an axis, a session patch takes any region, normally all of it, so a preview does not
  stop applying when `t` moves (note §11.3).
- **The rank orders patches within a layer.** It is an order key, like child order's,
  so a patch fits between two others without renumbering. **No two patches of a layer
  share a rank:** an op that would give one another's rank is refused, and the store
  mints keys that do not collide, across clients too (PT-4b). With equal ranks,
  "just above" one of them could land above the other, and a split would change a
  value.
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
- **A rank against inclusion shows as dead entries.** An entry is dead when, over the
  points that exist, stronger patches **of its own layer** set the field everywhere it
  applies (`World::dead`). An outer layer covering it is outermost-wins working, and is
  not listed. The lint offers both fixes, which are the layer's own: raise the inner
  patch (reorder), or drop the dead entries.
- **Inversions are listed beside them** (`World::contradictions`): a region strictly
  within another's of its layer, as written, that is the weaker, whether or not they
  share a field. They come from a region edit or a reorder, or from an earlier edit
  that left no slot for a new patch below every more specific one, so it went above.

**The axis keys** (P10). Each axis has a key on its selections, taken from the
selection as written, so it does not change as the axis grows; a selection strictly
within another (as written) always has the larger key. Larger is more specific:

| Kind | Key, compared in this order |
|---|---|
| Ordered, time | its lower end later (an open end just after a closed one at the same value); then its upper end earlier; then a finite set before an interval with the same ends (an interval holds values yet to be inserted); then fewer values in a set |
| Unordered | a finite set before a cofinite one (`not {…}`, or every value); then fewer values in a set, or more values excluded from a cofinite one |
| Instance | more steps fixed (the pattern's length); then its exclusions, reduced to those that extend the pattern and that extend no other exclusion, counted per length, shortest first, more being more specific. One `*/b/j` excluded removes more than `*/a/b/j` and `*/c/b/j` together, and counts first |

A region's key is its axes' keys **in the declared axis order** (P11), compared
lexicographically.

**The declared axis order** (P11), first deciding first: **instance, a component's
local state axes, clocks, then the sequence axes** (narrative, slide, take, and a
component's loop iteration). It decides only within a layer, where neither region
contains the other: the document's patch for instance `o1` beats the document's own
pose for `hover` at (`o1`, hover), as a CSS id beats `:hover`. A document may reorder
its own sequence axes among themselves.

**Across layers the layer decides, at every state.** An instance's override, written
in the document, beats its definition's hover pose. So does a slide's patch written in
the document: a deck that sets a part's leaf on slide 3 replaces that part's hover
feedback there too, since the pose sets a channel and the slide's patch the leaf the
channel drove (M7). To keep the feedback, the slide's patch leaves `hover` out of its
region, or the document writes its own hover pose. (PT-3:
`an_outer_layer_beats_the_definition_s_pose_at_every_state`.)

### 6.2 The laws, as they hold

1. **Identity.** A patch with no entries, or an empty region, changes no value.
2. **Disjoint patches commute.** Exchanging the ranks of two patches of one layer,
   adjacent in strength, that set no field in common changes no value.
3. **Locality.**
   - **Editing a field's entry** changes exactly the points of its **winning region**
     (its patch's region less the regions of stronger patches setting that field) where
     the value differs. This is an edit's blast radius, and it can be shown before the
     edit.
   - **Editing a presence or hiding entry** changes only points of its winning region,
     and there only the node, its subtree and what reads them: descendants appear,
     vanish or hide with it unless already absent or hidden on their own, and readers
     bound to them follow, as readers of any changed value do.
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
   not listed. Where the keys or the layers decide, nothing is flagged.
6. **Stale entries are inert and kept** (§5.3), and axis values are kept as tombstones
   (§3.1).

Values do not depend on the order patches are stored in. These are properties in
`src/iron_patch/tests/laws.rs` over 10,000 random scripts each, checked against a
reference that enumerates every point and one written apart from the model; 16 check
the rules taken, and 3 show the rules not taken failing (the laws as written cycle;
derived strength and widening by moving entries are not local). PT-5 runs them
against the store.

**What PT-3 does not cover.** The properties report a failing seed, not a minimal
case (no shrinking). Child order is not modelled (P7: patches do not reorder, so there
is nothing to check). There is no canonical form of a region, and none is needed: two
regions are the same when each is within the other as written. Layers are one integer
per patch, which is how one node's chain orders them. Not modelled at all: values that
are functions of the coordinates (a clip's), entries on a field a node lacks, pattern
anchoring and qualified steps, `use.notation`, relative regions (§13), continuous
intervals (the ordered axes are discrete, with open and closed ends), and cost.

## 7. Editing

### 7.1 The operations

| Operation | Does | Ops |
|---|---|---|
| **Edit** | changes an entry's value | set an entry |
| **Widen, narrow** | adds or removes points of a patch's region, on one axis | add or remove values or move an endpoint, per axis (PT-4b: they merge) |
| **Split** | moves some entries to a new patch of the same layer on another region; on the same region the new patch sits just above, and every value stays. Refused for a patch that may not be written | add a patch, clear entries |
| **Merge** | joins two patches into the stronger | set entries, remove a patch |
| **Reorder** | changes a rank within its layer; refused for a rank another patch of the layer has | set a rank |
| **Add, remove** | a patch | |

**Merge is refused** for a patch with itself or one that does not exist; for a patch
that may not be written; for two layers; for regions that differ as written; and where a patch between the two in
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

**A drag at x, from a view writing in layer L with edit region E, setting a field of
a node to v** (P12). It is refused where x is outside E or the node is absent at x.
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

So a drag shows what it set (PutGet), and dragging to the value shown writes nothing
(GetPut). A second drag at x writes into the patch the first made and adds none, and
when its value differs from the one before the first, the two end as one drag would
(PutPut); dragging back to the original value leaves the first drag's patch, holding
that value. A gesture previews in the session layer and commits one drag, with its
final value, so its samples are not drags. A drag changes exactly the new entry's
winning region where the value differs, never a point outside E, and never a patch that
may not be written (a recorded move's, an imported or `std:` definition's). E is as
written, so values the axes gain later inside E are inside it. These are PT-3's
`a_drag_obeys_the_lens_laws`, from views in every layer, and its blast-radius property.

**A clip-driven field** (a value that is a function of `t`) is dragged by timeline.md
§12.2's table, whole, carried into PT-9: on a key, that key's value in either mode;
auto-keyframe on, a key at the playhead; off, every key moved by the drag's delta, or
before the clip starts, the authored value. The guarantee "nothing outside E changes"
is for the key: a key also changes the interpolated values between its neighbours,
and the held tail after the last; moving every key changes the clip wherever it
applies.

## 8. Arrows

An arrow x → y is determined by its two ends: the patches whose region holds y and not
x **enter**, those holding x and not y **leave**, and **what moves is the diff of the
values resolved at x and at y**. That diff can include a patch that neither enters nor
leaves: a clip whose value differs at the two times.
- On an ordered axis built from "from here on" patches alone, the arrow B → C is
  exactly the patches starting at C.
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
  `axis not in {a, b}`, `instance ~ pattern [not pattern …]`. **Values are saved by
  id:** a name is shorthand in what is typed, resolved once when it is read, so a
  value added later under the same name, or a rename, changes no stored region. The
  same grammar is the query language's (§10).
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
| Lint | ties (law 5) and inversions, as written; dead and stale entries (with their reason), over the points that exist (§5.3, §6.1) |

- **The agent** reads patches with their regions instead of every point's values, and
  explains a value by its patch stack.
- **Where symbolic answers stop:** a bound value is evaluated at the points asked
  about (slices, reactive-engine.md §10.5, which PT-5 takes over); values with history
  answer "not seekable"; what is not loaded answers "not loaded" (PT-4b).
- **For PT-8 to design, not this spec:** turning a search result into regions to edit,
  showing what an edit shadows, matching theme tokens in a find.

## 11. What each mechanism becomes

| Today | Becomes | Deleted when it lands |
|---|---|---|
| An instance's overrides (component-model.md §5.3), nested targets | patches on the instance axis, in the layer of the scope that writes them; a nested target's path is a suffix pattern (§4), and outermost-wins is the layers' order | `override` nodes, their application in expansion, `stale_overrides` (`EN-5`) |
| A machine's states and poses (machines.md M1, M3) | an unordered local axis of `state` nodes; channels stay, and each pose is a patch on the axis setting them, shared between states where wanted | the channel values on `state` (`EN-6a`) |
| A machine's current state (M2) | the coordinate, set by its driver: the in port, which a binding, a clip key or a narrative override sets, as today (P9) | nothing |
| Transitions, `progress`, tweens (M4–M6, M8) | arrows; transitions and tweens stay nodes, so a variant may add or remove them as structure; `progress` and interruption are the driver's | nothing; tweens refine the arrow's diff |
| M7, "one writer per leaf" | **kept** (machines.md §4.3): a channel's writer is its machine, and a clip keys the state, never a channel. Several patches setting one leaf are not several writers: strength picks one at each point | nothing |
| A lesson variant that `extends` (variants-and-imports.md V1) | **a layer** (P14): a variant sits between the definition it extends and what places it, and its patches are anchored in it, so they apply in each placement of the variant. An `extends` chain is nested layers. An override on a variant's instance beats the variant, and the variant beats its base, by structure; placing the variant again changes no region. A variant that adds a state or transition adds nodes that are the variant's (§5.2) | V1's own resolution of `extends` (`EN-5`) |
| A narrative override's extent (narrative note §6.5) | a region on the narrative axis: a hold is `[k, ∞)`, and holds are ranked by their start, so a later start wins without cutting the earlier region. Retiming a hold, or moving the joint it starts at, re-ranks the holds it passes in the same edit | the extent kinds (`EN-6b`) |
| A deck | an ordered document axis; slide patches default to `[k, ∞)` | |
| Takes of a loop | a sequence axis, driven by the loop's `n` (§3.1) | |
| A clip | a patch on its clock's time axis whose values are functions of `t`, with region `[k₀, ∞)` (a clip holds after its last key); a staggered target is its own patch | clips' separate resolution (`EN-6b`) |
| The scratch layer, gesture previews | patches in the session layer, the outermost (P8; constants only) | the overlay's separate precedence, when it moves onto the session layer (`EN-5`) |
| Solver poses on instance copies | entries in a dedicated **solve layer** between the document layer and session (P17) | `Owners::solved`, `keep_solved`, `release`, `apply_recorded` (`EN-5`) |
| `ROOTED`'s list of facet kinds | holding a hidden composition still is the visibility gate on composition clocks (done); remaining `ROOTED` demand retired after need counts and delta clock gating | `ROOTED` (`EN-9`) |
| Value slices | evaluation at a point: an observer has a point on every axis, not only the clocks (`EN-4`) | nothing |

**The boundary stays.** Whether patches from outside a definition may set a part's
fields is a policy per definition (today: any template node's leaf, component-model.md
§5.3).

## 12. Phases

Work order and exits live in [`../handoff-engine.md`](../handoff-engine.md) under the
unified `EN-n` keys (`EN-3` design & spike, `EN-4` patches & point evaluation in
`iron_document`, `EN-5` instances as virtual views + minimal `ItemTable`, `EN-6a`
machines on axes, `EN-6b` clips & ordered axes, `EN-11` chunked storage). Product UX
follow-ups (search/provenance UI, clock operators/ghosts) follow in `handoff-product.md`.

## 13. Open

- **Relative regions:** "each instance in its own hover state" is a union over
  instances of (that instance, its state = hover): a diagonal no box writes. Deferred
  past `EN-6b`; until designed, a pose patch lives in the definition (local state axis
  alone) or per instance as `(instance) × (state)`.
- **Per-point picture axis:** body leaves/history key on entity (at its point) and
  picture leaves key on `use`, so two pictures of one entity already disagree on
  appearance without duplicating body computation (P20). A separate picture axis is
  deferred past `EN-6b`.
- **Whether ties are noise:** `ties()` lists equal keys on regions that meet; product UX
  shows the lint and judges whether it helps.
- **Continuous time:** `src/iron_patch`'s ordered axes are discrete; continuous real
  intervals are implemented and verified in `EN-6b` (or `EN-4` if `Time` is exposed
  early) and stored per `patch-storage.md` (`EN-11`).
- **Optional facet synthesis:** `EN-3` settles explicit facet-presence entries so adding
  an optional allowed facet on an instance coexists cleanly with §5.3's inert-entry rule.
- **Release policy on constraint deletion:** user decision in `EN-3` — whether a solved
  instance part clears its solve-layer entry on constraint deletion (current behaviour)
  or retains its solved pose like authored nodes.

## 14. Decisions

| # | Decision |
|---|---|
| P1 | Strength is (layer, rank). The rank is a stored order key, distinct within a layer, given from the axis keys when a patch is made; a region edit does not change it; a rank against inclusion shows as dead entries |
| P2 | Axis values have stable ids, and regions name values by id |
| P3 | A deleted axis value stays in its place as a tombstone |
| P4 | Each clock is its own time axis; a time region names its clock |
| P5 | A patch's region is one box (a decision: the keys are per axis); unions are answers, not regions |
| P6 | Presence (and `use.notation`) is structure, resolved before the fields; hiding cascades and a hidden node exists |
| P7 | A patch does not reorder children |
| P8 | The session is the outermost layer (the scratch layer, previews above it; constants only), never saved; it is not an axis |
| P9 | Coordinates are set by drivers, never by patch entries; static axis cycles are refused before derivation, dynamic `deref` cycles evaluate with previous-pass coordinates and report a problem |
| P10 | The axis keys of §6.1, from selections as written |
| P11 | The declared axis order, deciding within a layer: instance, local states, clocks, sequences |
| P12 | The drag writes in the view's layer, into E ∩ the winner's region, by §7.2's rule, and is refused where an outer layer sets the value |
| P13 | One selector grammar for regions and queries |
| P14 | A variant is a layer between its definition and what places it; there are no classes |
| P15 | Layers: where a patch is written; an outer layer is stronger than every patch of an inner one |
| P16 | Regions are compared as written, over values not yet added, deleted ones included; queries, dead and stale answer over the points that exist |
| P17 | Solver poses on instance parts live in a dedicated system-owned **solve layer** between the document layer and session (exact instance path, saved, below bindings and producers in `engine.md` §2.4, cleared on authored edit or release) |
| P18 | A node's instance coordinate is the enclosing view's path (not the view it places), so presence patches on a `use` are never self-referential |
| P19 | Presence, `use.notation`, and bound entries vary directly on the instance axis and layers; dynamic state/sequence presence resolves in Stage 1 from pass-start coordinates (forbidden on time axes; use `hide`); dynamic bound entries are forbidden or compiled as fixed mux cells |
| P20 | Body leaves and history key on entity; picture leaves key on `use` (no separate picture axis required in `EN-3`–`EN-6b`) |
