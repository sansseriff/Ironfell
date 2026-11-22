# Handoff: patches, regions and time

**Date:** 2026-11-20 · **Branch:** `master` · **Keys:** `PT-n` (plans/README.md,
"Work keys") · **Status:** the design is a note,
[`notes/patches-regions-and-time.md`](notes/patches-regions-and-time.md) ("the note"
below), **not adopted**. This file owns the order of work from that note to specs and
then to code. PT-1 can be built now: it is clock work already in an approved spec.
PT-2 to PT-4b are design. **Building from PT-5 on waits for the user to approve the
specs PT-4a and PT-4b write.** PT-1 started 2026-11-20.

## 0. Read first, in this order

1. This file.
2. The note, whole. Its §2.1 is the vocabulary (axis, value, point, region, patch),
   §2.2 the six laws, §4 where each existing mechanism lands, §9 what is open.
3. [`specs/reactive-engine.md`](specs/reactive-engine.md) §8.3 (what stays necessary),
   §10 (time, clocks, slices), §13 (partial loading), §17 (phases).
4. [`archive/handoff-reactive-structure.md`](archive/handoff-reactive-structure.md) §2 (the state of
   each engine phase) and §5 (its rules, which apply here too).
5. For PT-4b and PT-11: [`specs/scopes.md`](specs/scopes.md) §8, and iron-platform's
   `README.md` and `server/migrations/0001_init.sql`
   (`/Users/andrew/Documents/PROGRAM_LOCAL/iron-platform`).

**The change in one paragraph.** Iron has several mechanisms for "the same thing, a
bit different": instance overrides, lesson variants, machine states and poses, clips,
narrative extents, and (planned) slides and takes. The note replaces them with one
algebra. A **patch** is a set of overrides with a **region**, the set of points where
it applies. A value at a point is the base with every patch whose region contains the
point composed over it, weakest first. Six laws (a monoid; disjoint patches commute;
locality; specificity by region inclusion; a declared axis order for incomparable
overlaps; stale entries kept and inert) make it predictable. Time is an axis with a
driver: clocks per composition, gated by visibility. Search, provenance, storage and
loading all become arithmetic on regions.

## 1. What this takes over from the reactive-engine plan

The reactive-engine handoff ends with RE-5's layout, network and solver cells.
These items move here:

| From | Item | Here |
|---|---|---|
| RE-3.4 (deferred) | `now()`, `clock()`, clock providers (spec §10.1) | PT-1 |
| Spec §8.3 and `ROOTED` | what stays needed in a hidden composition | PT-1: the visibility gate |
| RE-5 | clips as cells | PT-9: clips become patches on the time axis |
| RE-6 | value slices (spec §10.5) | PT-5: evaluation at a point; a slice fixes a point on every axis, not only the clocks |
| RE-4.4 (deferred) | Unloaded owners, lazy slots, parking stateful owners | PT-11: loading by point |

RE-4.3 (Dormant by view rectangle) moved to [`handoff-render.md`](handoff-render.md)
§10 when the engine plan closed (2026-11-21). It is about view rectangles, not this.

## 2. Work order

| Key | Step | Kind | Gate before it | State |
|---|---|---|---|---|
| PT-1 | Composition clocks | Build | none (approved spec §10.1) | **Done 2026-11-20** (PT-1.1 to PT-1.3; PT-1.4 moved to PT-10, PT-1.5 to PT-9) |
| PT-2 | Council on the note | Design, read-only | none | **Done 2026-11-21**: Opus 6/10, Sol 6/10; findings in the note's §11 |
| PT-3 | The algebra as a crate, with property tests | Build (pure, no document) | none | **Done 2026-11-21**: `src/iron_patch`; stored strength, axis keys for its default (below) |
| PT-4a | `specs/patches.md`: the algebra spec | Design | PT-2 and PT-3 done | **Drafted 2026-11-21**; council round 1 (Opus 5.5/10, Sol 5/10) fixed in the model, and the spec revised; round 2 (Opus 6/10, Sol 6/10) fixed in both; round 3 (Opus 8/10, Sol 7/10) **not yet fixed**: paused 2026-11-21 for a performance report (below) |
| PT-4b | The storage-and-query spec | Design | PT-4a written | **Drafted 2026-11-21** (`specs/patch-storage.md`); round 1 (Opus 5/10, Sol 4.5/10) **not yet fixed**: paused (below) |
| PT-5 | Patches in `iron_document` | Build | **user approves PT-4a and PT-4b** | |
| PT-6 | Machines rebuilt on axes | Build | PT-5 | |
| PT-7 | Instances on the instance axis | Build | PT-5 | |
| PT-8 | Search and provenance | Build | PT-6 or PT-7 | |
| PT-9 | Decks, takes and the time axis | Build | PT-6 | |
| PT-10 | Clock operators, loops, ghosts, coverage | Build | PT-9 | |
| PT-11 | Storage and the server | Build | PT-5, and PT-4b's decisions | |

PT-1 is independent of everything after it, and fixes the cost this conversation
began with. PT-1 and PT-2 to PT-4b can run in parallel. PT-6 and PT-7 can be built in
either order. RE-5's cells can be built before, between or after any of these.

### PT-1: composition clocks

RE-3.4, deferred there, amended with the gate (note §3.1, §3.2).

Tasks, in order; each is committed alone.

- **PT-1.1, diagnose.** `Store::paused` should already hold the Lorenz page's
  accumulators while the page is hidden (demo.md), and in the Prussia session it did
  not. **Done 2026-11-20.**
  - **Cause:** `paused` counted any reader outside every composition as running. A
    placed instance's entity is outside every composition, and it reads its
    arguments. So any page with a placed component that reads the page's state was
    "read by something running", even when that component is pictured on the page
    alone. Lorenz has two such plots.
  - **Fix:** a model node runs with the compositions that picture it. It also runs
    when something running reads it, or when nothing pictures it (the timeline,
    loose roots).
  - **Tests:** `reactivity::a_hidden_page_s_own_placed_plot_does_not_keep_it_running`;
    `demo::tests::a_hidden_lorenz_page_holds_still` over the real demo document.
- **PT-1.2, composition clocks and the gate.**
  - A composition's time is presentation time less the time it spent paused.
  - Every stepper in it reads that time: accumulators, traces, machines' own
    transitions, presence, glides and a query's debounce.
  - The gate (today's `paused` rule) pauses a hidden composition holding any of
    these, not only accumulators. A machine paused mid-transition therefore resumes
    mid-transition, and a pulse in flight stays in flight.
  - Amend the spec's §10.1 with the gate in the same change. The user asked for
    "hidden tabs pause by default, overridable" (2026-11-20).
  - **Done 2026-11-20.** What was built:
    - `clocks.rs` keeps each clock's pause. A clock belongs to a composition, or to an
      entity some composition pictures. `Store::gate`, run once a frame from
      `advance`, sets them.
    - The steppers read their node's clock: traces and accumulators (`step.rs`),
      machines' own transitions (`machine.rs`), and presence.
    - Layout glides and a query's debounce stay on presentation time.
    - A shown `on_demand` composition still pauses only if it holds an accumulator,
      as before.
  - **Tests:**
    - `plot::a_hidden_plot_s_transition_holds_part_way` (against a store stepped
      through the page's own time);
    - `reactivity::a_hidden_page_s_trace_keeps_its_change_in_flight`;
    - `reactivity::an_accumulator_in_a_hidden_composition_holds_still`, now through
      observers.
  - **Cost, as built:** the gate scanned every hidden composition and its readers
    each frame. The 0.72 ms measured then had every page observed, so nothing was
    hidden. With one composition shown, as a user works, it was **8.1 ms a frame**
    natively (~20 ms in wasm), at rest and while dragging (found 2026-11-21 from the
    user's `__iron.timings`: `frame` p50 22 ms in the demo, about 1 ms of it in
    timed phases).
  - **Fixed 2026-11-21:** the gate keeps its decision (`Clocks::gated`) until the
    document (`Docs::edited`, stamped by every op and rollback), the wiring
    (`Reactive::wired`, stamped when an edge or a native changes, tracked reads
    included) or the compositions observed change. A drag in a new composition in
    the demo: `advance` 8.1 → 0.1 ms p50 (`demo::tests::drag_in_a_new_composition_time`,
    ignored, release). Test: `reactivity::a_preview_that_starts_reading_a_hidden_accumulator_runs_it`
    (fails without the tracked-read stamp). `advance` is now a phase in
    `__iron.timings`.
- **PT-1.3, the override:** a composition setting that keeps it running while hidden,
  in the composition settings menu. **Done 2026-11-20**, with PT-1.2:
  - The existing **Run** setting carries it. `run = always`, which already meant
    "render every frame", now also keeps the clock running while hidden. The menu's
    hint says so, and free-running.md §2 is amended.
  - Test: `reactivity::an_always_running_page_keeps_running_while_hidden`.
  - **§8.8's property test now runs with its two pages set to Always**
    (`observing_part_of_the_time_changes_nothing_once_all_is_observed`). Its random
    script holds presence, the fiber's traces and the plot's machine, which the gate
    now pauses: a deliberate exception to §8.8. With the gate overridden, the test
    is about necessity alone, which is what §8.8 states.
- **PT-1.4, fixed-step steppers: moved to PT-10** (2026-11-20). Accumulators would
  step at a fixed length, several steps a frame if needed, so a result does not
  depend on the frame rate.
  - **Why it moved:** each extra step is a graph pass, and a pass re-evaluates the
    pictures that read the accumulators, not only their `next` formulas. The Lorenz
    page would compute its two 400-point plots twice a frame to show one.
  - **What it needs first:** a pass that evaluates only the accumulators' own cells
    between steps.
  - **Where it pays:** replays, takes and checkpoints, where results must not depend
    on the frame rate.
  - Gallery pages' `min(dt, 0.03)` clamps go with it.
- **PT-1.5, `now()` and `clock()` in formulas: moved to PT-9** (2026-11-20).
  - **The conflict:** spec §10.1 defines `clock()` as the nearest *scripted* clock,
    the playhead by default. The note makes each composition's clock the default
    and a scripted one a placement (note §3.1).
  - **PT-9 decides what "scripted" means.** The name, what a model node reads, and
    how a shared template reads its host composition's time all depend on that.
  - **Until then,** an accumulator summing `dt` is the way to read time in a
    formula, as the Lorenz page's `clock` does. It is on its composition's clock.
- **PT-1's exit (below) is met without PT-1.4 and PT-1.5. PT-1 is done
  (2026-11-20).**
- **Moved to PT-10:**
  - UI transitions on unscaled time: nothing scales time until rate operators exist.
  - Replacing `ROOTED`'s facet-kind list: once a paused clock holds a composition's
    state still, what `ROOTED` keeps needed does not change, so it costs nothing.
- **Exit:**
  - a hidden tab with the Lorenz plot costs no evaluations per frame;
  - on return, the plot resumes from where it was;
  - a meter on a shown page that reads a hidden page's laser keeps running;
  - the override keeps a hidden composition running;
  - `fiber.txt`, `pulses.txt` and `idle.txt` pass.

### PT-2: council on the note

Read-only, both members (AGENTS.md). Ask for:
- flaws in the laws;
- mechanisms that do not map (note §4);
- what PT-3 must test;
- a score.

Fold the findings into the note, keeping it a note. The user reads the summary.

### PT-3: the algebra as a crate

A pure crate, `src/iron_patch`, with no dependency on `iron_document`. It holds:
- axes, both ordered (intervals) and unordered (sets);
- regions as unions of boxes;
- patches and their composition at a point;
- the winning region;
- the edit operations (note §2.3).

Property tests over random scripts, as the store's tests already are, check each of
the six laws, then the editing rule ("a drag always changes what you see", note §5).
**What it must test** is the note's §11.5 (PT-2's council): a brute-force reference that
enumerates every point, thirteen properties, and the worked examples there, which add
to the three below. It tries the repairs in §11.1 against each other: derived or
stored strength, per-axis keys, one box per patch.

PT-3 settles two open items: **law 5's axis order** and **structure in patches**
(note §9). It is kept, not thrown away: PT-5 builds on it.

**Exit:**
- every law is a property test, green over 10,000 scripts;
- three worked examples are written in the crate and their values checked:
  - the plot's `vis` machine;
  - a three-slide deck where B, C and D build on A;
  - a two-level instance nesting with outermost-wins.

**As done (2026-11-21).** `src/iron_patch`, no dependencies, about 900 lines, its tests
in 5 s (`[profile.dev.package.iron_patch]` builds it at opt-level 2):
- `axis`: ordered, unordered and path axes; values by stable id, a deleted value kept
  as a tombstone in its place; points.
- `region`: per-axis selections (intervals with open and closed ends, sets, cofinite
  sets, suffix patterns), a patch's region as one product, unions as query answers;
  intersection, difference, inclusion, all symbolic.
- `patch`: patches with a stored rank, the base as a fixed tree, presence and
  hiding as fields, and three strength rules: `AsWritten` (laws 4 and 5 as the note
  writes them), `AxisKeys` (derived from the regions) and `Stored`.
- `world`: the value at a point, winning regions, the stale listing, the inclusion
  lint, a new patch's default rank.
- `edit`: edit, region edits, widening by moving entries, split, merge (refused
  where it would change a value), reorder and the drag, as ops with inverses.

`tests/laws.rs`: 17 properties against enumerating every point, 10,000 scripts each.
`tests/examples.rs`: 10 worked examples, the three above and seven from the note's
§11.5. **What it settled:**
- **Law 5.** The laws as written cycle (found at seed 1554). Per-axis keys (an ordered
  axis: later first value, then earlier last; a path axis: trailing steps every
  selected instance shares; then fewer values), compared in the declared axis order,
  then rank, are a strict total order that extends inclusion over 10,000 scripts.
  The axis order decides an instance override against a definition's hover pose
  (instances first: the instance wins).
- **Strength is stored.** Derived strength changes points a region edit keeps (seed 53:
  narrowing a patch from everywhere to `[1, 4)` changed slide 1), and so does widening
  by moving entries, under either rule. A stored rank, given from the axis keys when
  a patch is made, keeps region edits local over 10,000 scripts, and agrees with
  inclusion until a region edit; `World::contradictions` is the lint for after.
- **The drag** (note §11.2) obeys PutGet, GetPut and PutPut, changes nothing outside
  its edit region, and never writes a read-only patch, under both total rules.
- **Structure:** presence is a field resolved before the rest; hiding cascades; an
  entry on an absent node is inert, and listed as stale only when absent wherever it
  wins. Axis values are tombstoned, so an interval keeps its place.
- **Merge** is refused where a patch between the two in strength sets what the weaker
  sets (Sol's counterexample, as a test).
- **For PT-4a to decide:** a class whose members all end in the same step ties with
  that step's pattern on the keys, and the rank decides (the newer); whether that is a
  lint.

**Not modelled, and why:** continuous axes (intervals with open ends are built, so
real positions change no rule); values that are functions of the coordinates (a
clip's); patches that set coordinates (not fields here, so refused by construction);
relative regions such as "each instance in its own state" (note §11.4, open); the cost
counts of §11.5 item 12 (the crate enumerates by design; PT-5's index is where cost is
measured).

### PT-4a: the algebra spec

`specs/patches.md`. It covers:
- the terms and laws, as PT-3 found them;
- axes and their kinds;
- regions as selectors;
- the editing rule and the edit operations;
- markup for patches and regions;
- the query language (note §4.2);
- what each existing mechanism becomes, and what is deleted (note §8);
- phases with exits.

Council it, then give it to the user.

**Council round 1 on the draft (2026-11-21): Opus 5.5/10, Sol 5/10.** The central
finding (Opus): the model compared regions over the axis values that exist now, so
`[C, ∞)` equalled `{C}` while C was the last slide, `*/o1/j` equalled `bench/o1/j`
while one bench was placed, and drags, merges and default ranks changed when an axis
grew. **Fixed in the model** (`src/iron_patch`, 19 properties and 13 examples, all
green):
- Regions are compared **as written**, over values yet to come: `Sel::within`,
  `void`, `meets`, `Product::within`, `same`. Queries keep the extensional `subset`,
  `minus` and `points`. A set's values count as written, deleted ones too.
- Keys come from the selection, not its members (`axis_key`): intervals by their
  cuts (open and closed ends compare, so continuous time needs nothing more); a
  finite set before an interval or a cofinite set; on the instance axis more steps
  fixed, a class before a pattern of its length, and only exclusions that remove
  something counted.
- **Strength is (layer, rank).** `Patch::layer` is where a patch is written, outer
  stronger: outermost-wins by structure, whatever order patches were made in, and an
  import's ranks never interleave with a document's. Default ranks are within a
  layer; where earlier edits leave no consistent slot, above wins.
- The drag (`edit::drag`, now with the view's layer): refused where an outer layer
  sets the value; reuses only the weakest qualifying patch, and only with no patch
  between it and the winner setting the field there; region checks as written.
- Merge refuses a patch with itself, a read-only patch and two layers.
- Lints: `dead` (entries that win nowhere; a rank against inclusion shows here),
  `ties` (equal keys in one layer, different regions that meet, a shared field),
  `stale` (a removal is never stale), `contradictions`.
- New properties: `growing_an_axis_changes_no_decision` (a slide appended and one
  inserted: values at old points, the drag's ops, default ranks, every merge and the
  lints unchanged); `values_agree_with_a_reference_written_apart`; the drag's blast
  radius equals its entry's winning region where the value differs; PutPut applies
  the second drag and equals one drag. New examples: outermost-wins in either order,
  one placement against the hover pose, the tie flag, a removal not stale and an
  entry returning with its node.

**Left for PT-4a** (the spec's text, then a second council round, then the user).
**Done 2026-11-21:** all eleven written into the spec (P15, P16 added; §6, §7.2 and
§11 rewritten); patch-storage.md's index gained the layer:
1. §6.1 and law 5: the stored (layer, rank) is strength; the keys only give a new
   patch's rank. Ties as `ties()` defines them. P15: layers.
2. §4: regions compared as written; P10's key table as `axis_key` has it.
3. §6.1's lint: contradictions are dead entries; offer drop as well as reorder.
4. §7.2: the drag as `edit::drag` now runs it, with the layer; a clip drag writes a key
   (the outside-E guarantee is for the key; interpolation changes between its
   neighbours), and with autokey off follows timeline.md (keys move by the delta).
5. §7.1: merge's refusals, stated as conservative.
6. §5.3: a removal is never stale; staleness names its reason (absent node, no such
   field); "not loaded" is not stale (PT-4b).
7. Law 3: entry locality is for fields; a presence or hiding edit's radius is its
   winning region times the subtree, and bound readers follow.
8. One place for take axes (sequences, wherever declared); the session as the
   strongest layer (scratch below preview), not an axis; the picture axis after
   instances.
9. §11: channels survive (poses set channels, tweens unchanged); M7 kept (clips key
   the state); M2's state set by its driver (clip keys and narrative overrides set the
   argument); transitions and tweens are nodes a class adds or removes; a variant's
   added nodes belong to the scope writing the patch; pattern steps are qualified by
   their definition and anchored where written; `use.notation` is resolved with
   structure, before fields; `ROOTED`'s demand role stays until PT-10; a narrative
   hold retimed is re-ranked; an arrow that jumps is the endpoint diff; structure per
   point is supported, history slices stay values only (D10, for PT-10's ghosts).
10. PT-5's exit gains the axis-growth properties, and ranks are order keys, not `f64`.
11. What PT-3 does not cover, said plainly: shrinking, child order, a canonical form
    (not needed: "same" is mutual inclusion as written).

**Council round 2 (2026-11-21): Opus 6/10, Sol 6/10.** Fixed in the model (16
examples, 19 properties, all green) and in the spec:
- **Variants are layers, not classes** (Opus): a class beat an instance's override on
  the variant's own instance, and a class of paths grew as placements were added
  (round 1's flaw again). A variant sits between its definition and what places it,
  its patches anchored in it. Classes, and path sets, are gone.
- **The layer decides across layers, at every state** (Opus): a document slide patch
  beats a definition's hover pose. The spec said otherwise; §6.1 now says it plainly,
  and the axis order decides only within a layer
  (`an_outer_layer_beats_the_definition_s_pose_at_every_state`).
- **Written equality keeps tombstones** (Sol): `{C}` after C's deletion was void, so
  `[C, C]` = `{C}` = `{}` but `[C, C]` ≠ `{}`. A set of tombstones is no longer void,
  and an intersection with a set keeps its written members.
- **The instance key** (Sol): counting exclusions put `*/j not {*/a/b/j, */c/b/j}`
  above the narrower `*/j not */b/j`. Reduced exclusions are now counted per length,
  shortest first (`one_short_exclusion_is_narrower_than_two_long_ones`; the generator
  now makes up to three exclusions).
- **Ranks are distinct in a layer** (both): equal ranks let a split change a value.
  `reorder` refuses a taken rank; `split` refuses a read-only patch.
- **`dead` counts its own layer only** (Opus); dead and stale are about existing
  points, and the growth guarantee says so (both).
- **PutPut** holds when the second value differs from the original; dragging back
  leaves the first patch (Sol). The lens property now drags from every layer, and the
  growth property grows every axis kind.
- Text: driver cycles defined through regions (PT-6 builds the check); arrows by their
  endpoints' values; PT-5's read contract (one point per composition, other observers
  values only); the picture axis moved to §13; markup saves ids; PT-8's UI items cut
  from the spec.

**Paused 2026-11-21** at the user's request, to look into dragging in a new composition
being slower in the demo than in an empty scope. Resume here: fix round 3's and PT-4b's
round 1 findings (below), then stop for the user's approval.

**Council round 3 (2026-11-21): Opus 8/10, Sol 7/10.** Round 2's fixes hold. Left:
1. **Rank minting** (both): rank keys carry the minting client's number, as node ids
   do (scopes.md D2); paste and import re-mint, keeping relative order; the taken-rank
   refusal is enforced at the op (`Op::Add`, `Op::Rank`, load, replay, undo), not only
   in `reorder`; a swap of two ranks is one transaction.
2. **The layer chain with variants at every level** (Opus): at each level outward,
   the definition then its variants, innermost first; a placement of V counts as one
   of D when anchoring D's patches.
3. **What a point read resolves** (Sol): the active chain and local coordinates per
   occurrence; presence and dependencies at the point asked; a sibling-variant fixture
   and a read at a point where a child is absent from the live one.
4. **Lint and locality contracts** (Sol): find and the blast radius intersect the
   winning region with the target's presence and field; `dead` needs a non-empty
   region; void boxes get one default key.
5. Can wait: re-ranking on a slide move (PT-9); whether the drag reads under the whole
   session layer or only its preview (PT-7); tombstones never collected in PT-5; P11's
   tiebreak among several local axes; the doc comment of
   `one_placement_still_overrides_the_hover_pose`; narrative retiming as a region edit
   and reorder together (PT-9).

**PT-4b council round 1 (2026-11-21): Opus 5/10, Sol 4.5/10.** Left:
1. **A missing patch chunk is a silent wrong value** (both): bounds go stale when an
   axis value moves; dropped remote ops can widen a patch onto the shown point; loading
   needs to repeat after structure resolves and to cover each node's layer chain. The
   invariant: a client holds every patch chunk whose bounds may hold a point it shows.
   Keep a versioned tail for unloaded content (Sol).
2. **The SQL cannot answer §10** (both): instance exclusions, time ranges, ordered
   sets, missing-axis rows (an anti-join), layers (the room computes strength; SQL only
   narrows), `btree_gist`.
3. **Does Postgres earn its place** (Opus): the room holds the whole `Store`; cut §5.1
   to the chunk catalogue unless a query only Postgres can answer is named.
4. **Region sync is incomplete** (both): rank minting; a set-selection op for a form
   change; cofinite widening; crossed endpoints refused; tombstone collection at
   compaction only.
5. Chunk ownership by writing scope, never splitting a patch; content-defined chunk
   boundaries; bounds as the longest common suffix; chunk GC by reachability; cut the
   Bloom filter and "a placed instance's own nodes".

### PT-4b: the storage-and-query spec

This revises, in one change:
- scopes.md §8: loading by point; chunked, content-addressed snapshots;
- reactive-engine.md §13: the loading unit, and `Summary` with axes and regions;
- iron-platform's README and schema: index tables maintained by the room, region ops
  that merge (a set CRDT per axis).

It decides **the browser cache**: whether the planned OPFS cache and outbox become a
relational store (SQLite on OPFS) holding the same index as the server. It also says
what is still unsolved about ops on unloaded parts (note §4.3). Council it, then give
it to the user.

### PT-5 to PT-11, in outline

Each gets a section here, with tasks and an exit, once its spec is approved.

- **PT-5. Patches in `iron_document`.**
  - Axes, patches and regions as document data, with ops for the edit operations; a
    schema bump.
  - Evaluation at a point, which takes over RE-6's value slices: an observer has a
    point.
  - The derived index.
  - Values checked against the PT-3 crate on every script.
- **PT-6. Machines rebuilt** on an unordered axis plus a transition graph.
  - `machines.md` and its code are replaced and deleted, with no migration.
  - `std:plot`'s `vis` is the first user.
  - The state editor becomes the axis editor (note §6).
- **PT-7. Instances on the instance axis.** component-model.md §5.3's overrides become
  patches, and outermost-wins comes from laws 4 and 5. This step has the largest blast
  radius; the differential harness from the engine plan is the model.
- **PT-8. Search and provenance.**
  - The query in markup for the agent.
  - "Why is this blue": the winning patch and its region.
  - Provenance chips, the blast radius, and lint for stale and shadowed entries.
- **PT-9. Decks, takes and the time axis.**
  - Ordered axes; slides; takes as an iteration axis.
  - The timeline as the region editor for any ordered axis.
  - Clips as patches on the time axis: RE-5's clip cells.
  - Narrative positions as points (narrative note §6.5).
  - One source leads per axis.
  - The free-running to scripted UX.
  - `now()` and `clock()` in formulas (PT-1.5, moved here).
- **PT-10. Clock operators** (rate, offset, loop with `n`, ping-pong, gate, hold, remap),
  then ghosts, coverage shading and checkpoints.
  Also from PT-1: UI transitions on unscaled time; `ROOTED`'s facet-kind list
  replaced; fixed-step accumulators (PT-1.4).
- **PT-11. Storage and the server.**
  - Chunked snapshots.
  - Loading a view's point; prefetching the arrow.
  - Parking and lazy slots, from RE-4.4.
  - iron-platform's index tables and mergeable region ops.

## 3. Open items and the step that settles each

| Open (note §9) | Settled in |
|---|---|
| The axis order for law 5 | PT-3, written into PT-4a |
| Structure in patches (added and hidden nodes, stale rules, cost) | PT-3, written into PT-4a |
| Events across clocks; timed transitions not counting paused time | PT-1 |
| Moving a system from free-running to scripted time | PT-1 (the mechanism), PT-9 (the UX) |
| One source leads per axis | PT-9 |
| Recorded moves as patches | PT-9 |
| What persists per point; checkpoints for takes | PT-10 |
| Ops on unloaded parts | PT-4b, then PT-11 |
| The browser cache's shape | PT-4b |

## 4. Rules

- The reactive-engine handoff's §5 rules apply, and so do its reasons to stop and
  report.
- **Build only from specs.** Until PT-4a and PT-4b are approved, the note is not a
  source to build from (plans/README.md). PT-1 and PT-3 are the exceptions: PT-1 is in
  an approved spec, and PT-3 is a pure crate the specs are written from.
- **Use the note's words:** axis, value, point, region, patch, arrow. "Applies to" is
  the interface's word for a region.
- **Breaking changes are welcome.** Old machines, overrides and variants are deleted
  when their replacement lands: a schema bump, and no migrations (AGENTS.md).
- **Fight scope creep.** The note's UX ideas (§6) are not part of any step unless the
  step names them.
- **Also stop and report** when a law fails in PT-3 and cannot be repaired without
  changing a law, or when a mechanism in the note's §4 does not map.
- **Each step ends** committed, pushed, recorded in `roadmap.md`'s Status table and in
  §2 here.
