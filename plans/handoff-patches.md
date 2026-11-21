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
| PT-4a | `specs/patches.md`: the algebra spec | Design | PT-2 and PT-3 done | **Drafted 2026-11-21**; council next, then the user |
| PT-4b | The storage-and-query spec | Design | PT-4a written | |
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
  - **Cost:** the gate scans every hidden composition each frame, not only those
    with an accumulator. On the demo with one page shown, `advance` is 0.72 ms p50
    in release, against 0.68 ms before. How much of the 0.7 ms is the gate is not
    measured; caching its result is open.
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
