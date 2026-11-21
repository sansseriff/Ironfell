# Handoff: patches, regions and time

**Date:** 2026-11-20 · **Branch:** `master` · **Status:** the design is a note,
[`notes/patches-regions-and-time.md`](notes/patches-regions-and-time.md) ("the note"
below), **not adopted**. This file owns the order of work from that note to specs and
then to code. Step 1 can be built now: it is clock work already in an approved spec.
Steps 2 to 4 are design. **Building from step 5 on waits for the user to approve the
specs step 4 writes.** No step has started.

## 0. Read first, in this order

1. This file.
2. The note, whole. Its §2.1 is the vocabulary (axis, value, point, region, patch),
   §2.2 the six laws, §4 where each existing mechanism lands, §9 what is open.
3. [`specs/reactive-engine.md`](specs/reactive-engine.md) §8.3 (what stays necessary),
   §10 (time, clocks, slices), §13 (partial loading), §17 (phases).
4. [`handoff-reactive-structure.md`](handoff-reactive-structure.md) §2 (the state of
   each engine phase) and §5 (its rules, which apply here too).
5. For steps 4b and 11: [`specs/scopes.md`](specs/scopes.md) §8, and iron-platform's
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

The reactive-engine handoff ends with phase 5's layout, network and solver cells.
These items move here:

| From | Item | Here |
|---|---|---|
| Phase 3, step 4 (deferred) | `now()`, `clock()`, clock providers (spec §10.1) | Step 1 |
| Spec §8.3 and `ROOTED` | what stays needed in a hidden composition | Step 1: the visibility gate |
| Phase 5 | clips as cells | Step 9: clips become patches on the time axis |
| Phase 6 | value slices (spec §10.5) | Step 5: evaluation at a point; a slice fixes a point on every axis, not only the clocks |
| Phase 4, step 4 (deferred) | Unloaded owners, lazy slots, parking stateful owners | Step 11: loading by point |

Phase 4's step 3 (Dormant by view rectangle) stays with the engine plan. It is about
view rectangles, not this.

## 2. Work order

| # | Step | Kind | Gate before it | State |
|---|---|---|---|---|
| 1 | Composition clocks | Build | none (approved spec §10.1) | |
| 2 | Council on the note | Design, read-only | none | |
| 3 | The algebra as a crate, with property tests | Build (pure, no document) | none | |
| 4a | `specs/patches.md`: the algebra spec | Design | 2 and 3 done | |
| 4b | The storage-and-query spec | Design | 4a written | |
| 5 | Patches in `iron_document` | Build | **user approves 4a and 4b** | |
| 6 | Machines rebuilt on axes | Build | 5 | |
| 7 | Instances on the instance axis | Build | 5 | |
| 8 | Search and provenance | Build | 6 or 7 | |
| 9 | Decks, takes and the time axis | Build | 6 | |
| 10 | Clock operators, loops, ghosts, coverage | Build | 9 | |
| 11 | Storage and the server | Build | 5, and 4b's decisions | |

Step 1 is independent of everything after it, and fixes the cost this conversation
began with. Steps 1 and 2 to 4 can run in parallel. Steps 6 and 7 can be built in
either order. The reactive engine's phase 5 cells can be built before, between or
after any of these.

### Step 1: composition clocks

The deferred phase 3, step 4, amended with the gate (note §3.1, §3.2).

- **First, diagnose.** `Store::paused` (`src/iron_document/src/lib.rs`, ~2834)
  should already hold the Lorenz plot's accumulators in a hidden tab (demo.md). In
  the Prussia session it did not. Find out why, with a test, before building on it.
- **Then build:**
  - `now()` and `clock()`;
  - a `clock` facet that defines a composition's clock from its parent clock;
  - the **visibility gate**: a paused clock by default when nothing observes the
    composition and nothing running reads it, overridable per composition;
  - fixed-step steppers;
  - UI transitions on unscaled time.
- **Replace `ROOTED`'s facet-kind list** for stateful nodes: a paused clock does not
  advance them. Necessity, not `ROOTED`, then decides what is computed.
- **Amend the spec's §10.1 with the gate** in the same change. The user asked for
  "hidden tabs pause by default, overridable" (2026-11-20).
- **Exit:**
  - a hidden tab with the Lorenz plot costs no evaluations per frame;
  - on return, the plot resumes from where it was;
  - a meter on a shown page that reads a hidden page's laser keeps running;
  - the override keeps a hidden composition running;
  - `fiber.txt`, `pulses.txt` and `idle.txt` pass.

### Step 2: council on the note

Read-only, both members (AGENTS.md). Ask for:
- flaws in the laws;
- mechanisms that do not map (note §4);
- what step 3 must test;
- a score.

Fold the findings into the note, keeping it a note. The user reads the summary.

### Step 3: the algebra as a crate

A pure crate, `src/iron_patch`, with no dependency on `iron_document`. It holds:
- axes, both ordered (intervals) and unordered (sets);
- regions as unions of boxes;
- patches and their composition at a point;
- the winning region;
- the edit operations (note §2.3).

Property tests over random scripts, as the store's tests already are, check each of
the six laws, then the editing rule ("a drag always changes what you see", note §5).
This step settles two open items: **law 5's axis order** and **structure in patches**
(note §9). It is kept, not thrown away: step 5 builds on it.

**Exit:**
- every law is a property test, green over 10,000 scripts;
- three worked examples are written in the crate and their values checked:
  - the plot's `vis` machine;
  - a three-slide deck where B, C and D build on A;
  - a two-level instance nesting with outermost-wins.

### Step 4a: the algebra spec

`specs/patches.md`. It covers:
- the terms and laws, as step 3 found them;
- axes and their kinds;
- regions as selectors;
- the editing rule and the edit operations;
- markup for patches and regions;
- the query language (note §4.2);
- what each existing mechanism becomes, and what is deleted (note §8);
- phases with exits.

Council it, then give it to the user.

### Step 4b: the storage-and-query spec

This revises, in one change:
- scopes.md §8: loading by point; chunked, content-addressed snapshots;
- reactive-engine.md §13: the loading unit, and `Summary` with axes and regions;
- iron-platform's README and schema: index tables maintained by the room, region ops
  that merge (a set CRDT per axis).

It decides **the browser cache**: whether the planned OPFS cache and outbox become a
relational store (SQLite on OPFS) holding the same index as the server. It also says
what is still unsolved about ops on unloaded parts (note §4.3). Council it, then give
it to the user.

### Steps 5 to 11, in outline

Each step gets a section here, with tasks and an exit, once its spec is approved.

- **5. Patches in `iron_document`.**
  - Axes, patches and regions as document data, with ops for the edit operations; a
    schema bump.
  - Evaluation at a point, which takes over phase 6's value slices: an observer has a
    point.
  - The derived index.
  - Values checked against the step 3 crate on every script.
- **6. Machines rebuilt** on an unordered axis plus a transition graph.
  - `machines.md` and its code are replaced and deleted, with no migration.
  - `std:plot`'s `vis` is the first user.
  - The state editor becomes the axis editor (note §6).
- **7. Instances on the instance axis.** component-model.md §5.3's overrides become
  patches, and outermost-wins comes from laws 4 and 5. This step has the largest blast
  radius; the differential harness from the engine plan is the model.
- **8. Search and provenance.**
  - The query in markup for the agent.
  - "Why is this blue": the winning patch and its region.
  - Provenance chips, the blast radius, and lint for stale and shadowed entries.
- **9. Decks, takes and the time axis.**
  - Ordered axes; slides; takes as an iteration axis.
  - The timeline as the region editor for any ordered axis.
  - Clips as patches on the time axis: the engine plan's phase 5 clip cells.
  - Narrative positions as points (narrative note §6.5).
  - One source leads per axis.
  - The free-running to scripted UX.
- **10. Clock operators** (rate, offset, loop with `n`, ping-pong, gate, hold, remap),
  then ghosts, coverage shading and checkpoints.
- **11. Storage and the server.**
  - Chunked snapshots.
  - Loading a view's point; prefetching the arrow.
  - Parking and lazy slots, from phase 4 step 4.
  - iron-platform's index tables and mergeable region ops.

## 3. Open items and the step that settles each

| Open (note §9) | Settled in |
|---|---|
| The axis order for law 5 | 3, written into 4a |
| Structure in patches (added and hidden nodes, stale rules, cost) | 3, written into 4a |
| Events across clocks; timed transitions not counting paused time | 1 |
| Moving a system from free-running to scripted time | 1 (the mechanism), 9 (the UX) |
| One source leads per axis | 9 |
| Recorded moves as patches | 9 |
| What persists per point; checkpoints for takes | 10 |
| Ops on unloaded parts | 4b, then 11 |
| The browser cache's shape | 4b |

## 4. Rules

- The reactive-engine handoff's §5 rules apply, and so do its reasons to stop and
  report.
- **Build only from specs.** Until 4a and 4b are approved, the note is not a source to
  build from (plans/README.md). Steps 1 and 3 are the exceptions: step 1 is in an
  approved spec, and step 3 is a pure crate the specs are written from.
- **Use the note's words:** axis, value, point, region, patch, arrow. "Applies to" is
  the interface's word for a region.
- **Breaking changes are welcome.** Old machines, overrides and variants are deleted
  when their replacement lands: a schema bump, and no migrations (AGENTS.md).
- **Fight scope creep.** The note's UX ideas (§6) are not part of any step unless the
  step names them.
- **Also stop and report** when a law fails in step 3 and cannot be repaired without
  changing a law, or when a mechanism in the note's §4 does not map.
- **Each step ends** committed, pushed, recorded in `roadmap.md`'s Status table and in
  §2 here.
