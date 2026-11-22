# Structure in the graph: one reader of the document

**Status:** draft, 2026-11-22, for council round 1. Not adopted. It changes the order of
[`handoff-scaling.md`](../handoff-scaling.md) (the `DS` track): DS-3b stops at part 2
(`08e0539`), and this spec's phases, keyed DS-9 to DS-14, come before DS-4.

## 1. Why

The graph records every **value** a cell reads, and a change reaches exactly the cells
that read it. It does not record what a cell reads of the document's **structure**:
which nodes exist, their types, parents and children, which facets they carry, which
relations join them. Every cell is handed the whole document (`Cx::tree()`), and the
store makes passes of its own over it.

So whatever reads structure has to be told by hand when to run again, and the only safe
default is to redo it over the whole document. That is the one cause behind:

- the visibility gate deciding again over every page, 8 ms a frame (2026-11-21), and its
  fix, a list of the ops that can change what it reads (`op::reshapes`);
- the necessity sweep after every change (`Store::necessity`, `Reactive::necessity`);
- `regraph` (~200 lines), `Reactive::restructure`'s rederive and rescope (most of its
  ~175), `Owners::affected` (~120), `sync_layout`, `sync_repeats`, `update_drawn` and
  the `prime_*` calls (~200): rules, op by op, for what each kind of edit could
  affect;
- the stamps and op classifiers DS-3 added (`Docs::reshaped`, `network_shaped`,
  `tokens_shaped`; `op::touches`, `rewires_network`, `retokens`) and the grouping reach
  (`solve::grouping_reach`): more of the same rules;
- cells kept whole because nothing could tell what part of them changed: the
  constraint grouping, for the whole document (reactive-engine.md §9.4 asks for one per
  composition), and the connections' signals and anchors' positions ("kept
  whole-document in 2b", never revisited).

Each rule is a guess that has to stay complete as features are added. One that misses a
case leaves a stale result; the gate's did.

## 2. The rule

**A kept result is a cell, and a cell reads the document only through the graph, which
records every read, of structure as of values. An edit marks stale exactly the cells
that read what it touched.**

- *Kept* means cached across transactions or frames: a cell's value, a set the store
  keeps (what is drawn, what is paused, what is needed), a list of cells to have.
- A **one-off read** can read the document directly, since nothing is kept: a command,
  export, the agent's whole view, a test. So can the edit path itself (ops, expansion
  writing copies, the solver's write-back), which writes through the document's choke
  points.
- **No hand rules:** no list anywhere of which ops affect which computation. The only
  mapping from edits to what they touch is the document's own, §3.2, written once for
  its few primitives and knowing nothing of features.

The cost of an edit is then what it touched plus the cells that read that, and the cost
of a frame is the cells whose inputs moved. A cell that reads something large is visible
as one cell with one large read, counted by the scaling test (`DS-1`), not as a pass
hidden in the store.

## 3. Design

### 3.1 Structural keys and the recording tree

A **structural key** names one thing a read can depend on:

| Key | A read that records it |
|---|---|
| `Node(n)` | whether `n` exists and is live, its type, its parent and order (`node`, `live`, `is_live`, a parent walk) |
| `Children(n)` | `n`'s child list (`children`; `descendants` records it for each node it walks) |
| `Facet(n, kind)` | whether `n` carries `kind` (`facet`, `get`, `facets_of` records each kind present and absent) |
| `From(n, rel)`, `To(n, rel)` | the relations of kind `rel` from or to `n` (`relations_from`, `relations_to`) |
| `Rooted(owner)` | an owner's relations (`rooted`) |
| `Type(t, scope)`, `With(kind, scope)` | the nodes of type `t`, or with facet `kind`, in `scope` |
| `Roots` | the root list |

A **scope** is a composition, or the whole document. `nodes_of_type` and `nodes_with`
gain scoped forms (`in(composition)`), served by the type and facet indexes kept per
composition. An unscoped read records the whole-document key, which every create,
delete or move of that type touches: allowed, and what the scaling test catches.

`Tree` gains an optional recorder. `Cx::tree()` hands a cell a recording `Tree`, so the
code cells already run (`constraint::all`, `plug::implied_mates`, layout, the network,
expression evaluation's `deref` and `@name` lookups) records its structure reads with no
change of its own. Values keep their path (`Cx::read`, `Cx::constant`).

### 3.2 The write side: the journal says what an edit touched

The document already journals every primitive change for rollback (`document.rs`,
`Rec`: a node, a facet, a relation, a type or facet index entry, a relation end, a child
list). Each record names the keys it touches:

| Record | Keys |
|---|---|
| `Node(n, _)` | `Node(n)`, and `Type(t, scope)` for its type and composition |
| `Facet(kind, n, _)` | `Facet(n, kind)`, `With(kind, scope)` |
| `Relation(id, _)`, `BySource`, `ByTarget` | `From(a, rel)`, `To(b, rel)`, `Rooted(owner)` |
| `Placed`, `Unplaced`, `ChildrenKey` | `Children(parent)` (`Roots` for the root list) |
| `ByType`, `ByFacet` | `Type(..)`, `With(..)` |

A leaf's value is not structure: a `Set` touches its leaf, as today. A move touches the
moved node's `Node` key and both child lists; a cell that walked through it recorded
`Node` keys along its parent walk, so a cell beneath a moved wrapper hears of it with
no subtree rule. The copies document has the same journal, so expansion's writes and
the solver's write-back reach readers like any edit.

At the end of each step that applies ops (an op, a round of expansion, a rollback), the
store hands the graph the touched keys. The graph keeps **key → readers**, as it keeps
leaf → dependents, and marks those readers stale.

### 3.3 Cells that make cells

What cells should exist is itself a kept result: one per layout root, per repeat, per
island, per cluster, per connection, per anchor, per clip. Today each has a sync
function fed by a rule (`sync_layout`, `sync_repeats`, `update_drawn`, `sync_islands`,
`sync_cluster_natives`). Under the rule each is a **maker**: a cell that reads a scoped
key (the layout containers of a composition, its repeats, its constraints) and keeps one
child cell per member, made and dropped by difference (reactive-engine.md §6, Incremental's
structure cells). One maker type serves every kind.

### 3.4 The cells reactive-engine.md asked for

With reads recorded, the whole-document cells split as that spec planned:

- **Grouping per composition** (reactive-engine.md §9.4): reads its composition's
  constraints, their operands and the anchors that imply mates; makes a cluster cell
  per cluster. Validation alongside it, per composition. `Clusters` and `Validate` as
  whole-document cells go, with the grouping reach.
- **A cell per connection** for its derived signal, and **per anchor** for its position
  and following (§9.1). `restructure`'s rederive goes.
- **A cell per clip** (§9.1; PT-9 holds clip *providers*, unaffected): reads the clock
  and its clip's keys and target; `resample`'s whole-document sample goes.

### 3.5 Necessity and the gate (DS-4, DS-5) on recorded edges

`handoff-scaling.md` §4.1 (necessity as counts) and §4.3 (the gate as a traversal) stand
as adopted, with one change: the edges they count include structural reads. The store
side indexes §4.3 names (embeds by home and source, each node's owner) become what the
embed and owner cells record. Nothing in either is a pass over the document.

## 4. What goes, what comes

Rough, from the code as of `08e0539`. Tests are not counted; this adds roughly 400 to
600 lines of them (§6).

| Removed | Lines |
|---|---|
| `regraph`'s rules (what remains hands touched keys to the graph and expansion) | ~160 |
| `restructure`'s rederive and rescope | ~115 |
| `sync_layout`, `sync_repeats`, `update_drawn`, `sync_islands`, `sync_cluster_natives`, `prime_*` | ~250 |
| The op classifiers and stamps (`op::reshapes`, `touches`, `rewires_network`, `retokens`; `Docs::reshaped`, `network_shaped`, `tokens_shaped`), the gate's cache key | ~120 |
| `grouping_reach`, the whole-document `Clusters` and `Validate` | ~80 |
| `Store::paused`'s walks, the necessity sweeps, `need_what_reads_reach` | ~180 |
| `settle_repeats`' own tracking (`drawn`, `repeat_dirty`) | ~80 |
| `Owners::affected` (DS-14) | ~120 |
| `resample`, `resample_running`, the running-clip bookkeeping | ~60 |
| **Total** | **~1,150–1,400** |

| Added | Lines |
|---|---|
| Keys, the recording `Tree`, key → readers, marking stale | ~250–350 |
| The journal's keys (§3.2) and scoped type and facet indexes | ~100–150 |
| The maker cell (§3.3) | ~100–150 |
| Grouping and validation per composition, connection, anchor and clip cells | ~250–350 |
| Necessity as counts, the gate's traversal (DS-4, DS-5, as planned) | ~250–300 |
| **Total** | **~950–1,300** |

**Net:** about 200 to 450 lines fewer in the engine. Against the old plan's end state,
where DS-4 and DS-5 add the same ~250–300 and every rule stays, about 1,000 fewer. The
measure that matters more: today's rules are spread over a dozen functions in four
files and must each be kept complete; afterwards there is one mapping (§3.2), in one
place, over the document's dozen kinds of structural record.

## 5. Phases

Each phase is committed alone, passes the differential suite (with `IRON_OWNER_CHECK`)
and the scaling test, and retires its `SCALING` pairs.

| Key | Builds | Exit |
|---|---|---|
| DS-9 | Keys, the recording `Tree` in `Cx`, the journal's keys, key → readers. On beside the hand rules; under `differential`, every cell a rule primes must also be stale by its recorded reads (the rules become the oracle) | The oracle never fires; recording's cost measured: a drag's `advance` stays under 0.1 ms natively |
| DS-10 | The hand rules deleted: the stamps, classifiers, grouping reach and `prime_*`; `regraph` and `restructure` reduced to handing on keys | Differential and scaling pass with them gone |
| DS-11 | The maker cell; the sync functions deleted; grouping and validation per composition | Adding a constraint costs its composition |
| DS-12 | Connection, anchor and clip cells; rederive and `resample` deleted | Reparent and connect lose their counters |
| DS-13 | Necessity as counts and the gate as a traversal, on recorded edges (was DS-4a, DS-4b, DS-5; `handoff-scaling.md` §4.1–4.3) | As those phases' exits; every operation loses its necessity and gate counters |
| DS-14 | Expansion: an owner's expansion records what it read; `Owners::affected` deleted | Owners' oracle passes with it gone |

DS-3c (heights by `raise`), DS-6 (captures, roots), DS-7 (refusal by journal) and DS-8
(the host side) follow as planned. **Final exit:** `handoff-scaling.md` §6's: every
operation flat at both axes, store and host.

## 6. Verification

- **Recorded reads are complete:** DS-9's oracle (every rule-primed cell is also stale by
  keys); from DS-10 on, the differential suite's rebuild from scratch. A property test
  over random documents and op sequences: after each, every cell is re-run on a fresh
  recorder, and any key it now reads that it did not record before the op, and that the
  op touched, is a failure.
- **No slope:** the scaling test, unchanged, at both axes.
- **Cost of recording:** the native drag and playing timings, and `single-tab.txt`.

## 7. Risks

- **Recording's cost.** A layout over 500 nodes records thousands of keys per run.
  Keys are small and recorded into a reused buffer, sorted once; DS-9 measures before
  anything depends on it. If it is too slow, a cell records structure only on its first
  run and after a structural key fired, since a value change cannot change which
  structure it reads, and verification checks that assumption.
- **Coarse reads.** A cell that reads a whole-document key reruns on any such change.
  The rule makes that visible, not impossible; the scaling test names it.
- **Reads outside `Cx`.** Store code that keeps a result but reads `Tree::of(&self.docs)`
  directly escapes the rule. Each kept set in the store is listed in DS-9 and either
  becomes a cell or is shown to be one-off.
- **Scope.** The engine's core again. Anything that is not a kept result reading
  structure is out.

## 8. Council

Reviews by group, both members (Opus 5.5 and Sol 6.2, high), read-only:

1. **This spec**, before any code: the rule, the keys, the write side, the phases.
2. **DS-9 and DS-10**, the mechanism and the first deletions: whether recorded reads
   are complete is the riskiest claim, and the cheapest point to find out. The code of DS-2
   and DS-3, not yet reviewed, is reviewed with it where it stays (its indexes and
   event-driven players).
3. **DS-11 and DS-12**, the cells.
4. **DS-13 and DS-14**, necessity, the gate and expansion.
5. **The rest** with DS-6 to DS-8, as `handoff-scaling.md` has it.
