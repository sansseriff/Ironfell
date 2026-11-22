# Structure in the graph: one rule for when, one loop for order

**Status:** adopted 2026-11-22, and revised twice that day after reviews of its direction.
DS-9 is built (§9). This spec replaces reactive-engine.md §7.2, its table of what each op
affects. It changes the order of [`handoff-scaling.md`](../handoff-scaling.md), the `DS`
track: DS-3b stops at part 2 (`08e0539`), and this spec's phases replace the rest of it.
The review records are in handoff-scaling.md §9.

## 1. Why the same kind of bug keeps coming back

The engine already has one good idea. A cell's value is a function of the leaves it
reads, and the graph records those reads. When a leaf changes, exactly the cells that
read it run again. Nobody writes a rule for which cells a value edit affects; the
records decide.

**Structure never got that treatment.** By structure we mean:
- which nodes exist;
- their parents and children;
- which facets they carry;
- which relations join them.

The store keeps many results derived from structure. Each has its own cache and its own
hand-written rule for when to refresh it. Every one of them is listed here, with where
it ends up:

| Kept result | Its refresh rule today | Becomes |
|---|---|---|
| Layout roots, pinned parts, glide specs | `sync_layout`'s candidate set, `layout_touched`, `prime_layout`, `prime_reorders` | makers (DS-11) |
| Constraint grouping and validation | `regraph`'s `regroup`, `in_reach`, `moved`; `prime_grouping`; `solve::grouping_reach`; `evaluate`'s Clusters rule (`graph.rs`, a changed tracked read re-primes `Clusters`) | a partition maker plus per-cluster cells (DS-12) |
| Cluster natives, `committed`, solve order (`next_to_solve`) | `sync_cluster_natives` after every grouping run; Kahn's sort in `clusters_with` | the partition maker; order by the graph's heights (DS-12) |
| The contexts library | `resolve_contexts`, a scan of every `context` node in each pipeline | a cell (DS-12, which reads `plug.mate`) |
| Drawn repeats and their natives | `update_drawn`'s op match, `ensure_drawn`, `repeat_natives`, `repeat_dirty` | makers (DS-15) |
| A repeat's settle | `REPEAT_INPUTS`, a hand list of the inputs to compare | the repeat native's recorded reads (DS-15) |
| Islands of the across/through network | `Docs::network_shaped`, `op::rewires_network`, `sync_islands` | makers (DS-13) |
| Connections' signals, anchors' positions and followers | `regraph`'s `rederive` op match, then whole-document recomputation in `restructure` | cells (DS-13) |
| `@name` wiring | `regraph`'s `names`, `under`, `rescope_all`; `Rescope` | wiring cells (DS-14) |
| Theme tokens | `Docs::tokens_shaped`, `op::retokens`, `token_leaves`, `theme_at`; `palette::publish` over every token | cells (DS-14) |
| Instance arguments' boundary checks | `regraph`'s `defs` → `naming` scan | wiring cells (DS-14) |
| Templates | `Tree::scope_stamp` per definition (`engine/template.rs`), read with no key | cells (DS-15) |
| Copies to re-expand | `Owners::affected` (120 lines), its `Which::All` fallback and the budget fallback that expands everything | expansion by recorded reads (DS-15) |
| Whether a transaction is structural at all | `after_transaction`'s op classifier | gone with `regraph` (DS-15) |

These stay outside the rule, and each has an owner:

| Kept result | Its rule today | Why it stays outside, and who owns it |
|---|---|---|
| Clip sampling, the animation end | `touches_clips`, `resample` after a structural transaction, `clips_end` | players' scheduling (PT-9); the cost that remains is listed in §6 |
| The host's change feed | `geom::Revisions::note_ops`, `classify` | the host is DS-8 |
| Graph heights | `compute_heights` | DS-3c |
| The gate's decision | `Clocks::gated`, `Docs::reshaped`, `op::reshapes` | DS-5 |
| Necessity, `ROOTED` | the sweep | DS-4a, DS-4b |

**Each refresh rule is a guess** about which edits can change its result, and each has
to stay complete as features are added. They don't stay complete. About half the bugs
the last code review found were in these rules:
- a nested layout root missed;
- a repeat's inputs list one entry short;
- `can_refuse`'s guess of which documents need a snapshot.

And a fix for another bug added a clip classifier that forgot transitions.

reactive-engine.md planned these results as "structure cells" (§7, §9.4). But it had
them told what changed by an op table (§7.2), and that table is where the rules came
from. This spec deletes the table.

## 2. The rule

**Every cached derivation is a cell: a function computed from scratch over a bounded
scope. It reads the document and the graph only through recorded reads. A write reports
the keys it touched, and the cells that read those keys run again. Nothing else decides
when a derivation is refreshed.**

- **Cached derivation** means a result kept across transactions or frames that is a
  function of the document and the graph. State that evolves in time is not a
  derivation, and it keeps its own explicit contract (§3.4): players, the solver's pose,
  islands' last solutions, presence, session state.
- **From scratch** means the function has no incremental logic of its own: given its
  scope, it computes the whole answer. A cell can still be fast inside (a layout
  reusing measured text), but no cell needs to know what changed.
- **Scope** is chosen by what computes together:
  - a layout root;
  - an expansion owner;
  - a cluster;
  - an island;
  - one top-level root (a composition, a definition, a timeline) for a maker.

  handoff-scaling.md §2 allows cost in proportion to the shown composition and to the
  cone of what changed, never to the rest of the document.
- **One-off reads** (commands, export, the agent's view, tests, the edit path) read
  directly.
- **No new rules.** A fix that would need an op classifier or a stamp must not add one.
  Until a result is a cell, it refreshes when any key in its scope is touched. The
  touched keys name their nodes, and the nodes name their roots, so this needs no
  classifier. It is the coarse form of the same rule.

### Why this closes the door rather than moving it

What can still be wrong after this is a short list, fixed by the data model and the
engine. Features don't change it:

1. **Reads.** A cell gets a `Cx` that offers only recorded reads:
   - the recording `Tree`;
   - leaf reads;
   - the graph's wiring reads (`is_bound`, `driven`, `transitive_deps`), each keyed.

   `Cx::graph()` is deleted, and side tables (`Docs::gone`, `Owners`, the store's
   clusters) aren't reachable from a cell. `Cx::previous` (a warm start) is allowed
   only in cells that declare `Native::history`. A `Tree` read with no key fails a debug
   assertion. That includes `scope_stamp`, which today has neither.
2. **Writes.** About ten mutation choke points in `Document` report keys: `insert_node`,
   `remove_node`, `restore_node`, `write_node`, `place`, `unplace`, `insert_facet`,
   `remove_facet`, `facet_mut`, and the relation and index writers. The graph reports
   its wiring keys where it changes a binding, a claim or a tracked read.
3. **Order.** One loop, with four stages and their declared inputs (§3.2). A stage that
   reads a later stage's output fails a type check or a debug assertion.

A new feature adds facets, types and cells. It goes through the same reads and writes,
and it takes its place in the loop by its kind, so it needs no rule. Today a new feature
needs a new rule in `regraph`, and forgetting one ships a stale result. After this, a
missed key is a bug in one of a dozen functions features don't touch, and the oracle (§4)
names it.

## 3. The mechanism

### 3.1 Keys: reads record, writes report

DS-9 built four keys (§9): `Node(n)`, `Children(n)`, `FacetSet(n)` and `Facet(n, kind)`.
Each phase adds the keys its cells read:

| Key | Recorded by | Reported by |
|---|---|---|
| `In(root, type)`, `With(root, kind)` | the scoped indexes: nodes of a type, or with a facet kind, under one top-level root | the index writers, for the node's root |
| `Relations(n)`, `Relation(id)` | `relations_from`, `relations_to`, `relation` | `insert_relation`, `remove_relation`, at both ends |
| `Roots` | `roots` | placing or unplacing a node with no parent |
| `Wiring(leaf)` | `Cx::is_bound`, `Cx::driven`, `Cx::transitive_deps` (each leaf walked) | the graph, where a leaf's binding, claim or tracked reads change |

The type and facet indexes become **per top-level root**. A node's root follows its
authored parent, or for a copy its owner's. So moves, undo and expansion keep membership
right, and moving a subtree reindexes that subtree, which is the cone of the move.

### 3.2 The loop: four stages

Derived work that writes into the document can't be a cell, because a cell must not
write its own inputs. So the engine has **one loop**, and each stage of it is either a
set of cells or one of the writers that already exist:

| Stage | What runs | Reads | Writes |
|---|---|---|---|
| 1. Expand | owners' copies of instances and definitions (`Owners`) | structure | copies (document writes, through the choke points) |
| 2. Make | **makers**: cells whose value is a set of cells to have (layout roots, drawn repeats, islands, the constraint partition, binding wiring) | structure and wiring only; **no values**, by type: a maker's reader has no leaf read | the graph's native set and wiring, as one batch |
| 3. Evaluate | value cells: bindings, natives (layouts, clusters, repeats, islands) | values, structure and wiring | values and claims |
| 4. Settle | value-to-structure writers: `settle_repeats` (copies from counts), and in a transaction the solver's write-back | values | copies and authored poses (document writes) |

A round runs stages 1 to 4 over what is marked. The touched keys are drained into marks
before each stage. Another round starts **only if stage 4 wrote** (its touched keys are
not empty).

**Wiring during evaluation.** Wiring can change in stage 3: a tracked read moves, or a
claim is made. That can't reach a maker in the same round, because makers read no
values and the partition reads no wiring that stage 3 changes (DS-12). The facts stage 3
changes are per-cluster:
- which leaves are free;
- the order the clusters solve in.

So they move into the per-cluster cells of stage 3, which read wiring through keyed `Cx`
reads.

**Reconciliation is one batch.** A maker's diff (natives to make and drop, outputs
reserved, claims handed from a dropped native to its replacement) is applied by one
`set_natives` call, as today's batched claim handoff does. A reservation is graph
wiring; an active claim is a value of stage 3.

**Termination** follows from the stratification and a budget. Stages 1 to 3 converge in
one pass, because each reads only what earlier stages wrote. Rounds repeat only for
stage 4's writes. These are repeats whose copies change what decides their count, and
the engine already detects and reports that (`feedback`, `restless`). It runs a round
budget, as `settle_repeats` does, and reports a problem past it.

The stage order is written once, as this table. It is not a list of ops, and a cell's
stage is its kind.

### 3.3 Refusal

Today `Saved` clones the graph for every transaction in a document holding a constraint.
That costs in proportion to the whole graph. It is replaced by **a journal of the
graph's own mutations**:
- `set_cell`, `drop_cell` and `set_natives`;
- `key_readers` and reservations;
- each cell's first value change in the transaction.

The journal is undone in reverse, as the document's journal is (RE-2a). Its cost is what
the refused transaction changed. It doesn't depend on any other phase. It lands as
**DS-7**, brought forward from handoff-scaling.md, any time after DS-10.

**What else `Saved` restores** is store state, and the journal covers it field by field:
- `presence` and `presence_moving`;
- `item_memo`, `repeat_inputs`, `item_shown`;
- `owners`' `made` and `solved`;
- `committed`, `grouped_at`;
- `geometry`, `motion`, `session_state`, `pending`.

DS-7's design lists each one as either journaled, or derived again by its family's keys
once that family is converted.

Whether a refused solve should stay refused (and not, say, keep the last good pose and
report a problem) is product behaviour, and it's out of scope.

### 3.4 State is not a derivation

Five kinds of kept results depend on their own history:
- the solver's pose and `committed`;
- islands' last solutions;
- a failed binding's last good value;
- players (machines, steps, queries, presence, clips);
- session state.

None of them is a cell under §2, so each declares itself. A native with `history` reads
`previous()`, and the oracle checks it as a fixpoint (§4). Players are woken by leaf
changes and keep their own scheduling.

## 4. One oracle: the loop is at a fixpoint

The differential suite compares two incremental stores, so a bug they share passes it.
DS-9's structure oracle reimplements layout's claim rules: it can pass stale output and
panic on a valid bound leaf, and each family would need its own. It is replaced by one
oracle (feature `fresh-oracle`, implying `differential`), run after every facade call.
The oracle checks three things:

0. **Indexes match the raw records.** The scoped indexes, rebuilt from the node table,
   equal the kept ones. So a stale index can't fool the other two checks.
1. **Makers are fresh.** Every maker is run from scratch on a clone over the same
   document and wiring, and must give its kept output: the native set with each spec,
   the wiring, and the partition. Makers read no values (§3.2), so there's no history
   to account for.
2. **The loop is at a fixpoint.** On a clone that carries the store's state (publishes,
   players, last good values, poses, last solutions), every value cell is marked stale
   and stages 3 and 4 run once. Two things must hold:
   - Values, errors, dependency edges, claims and native extras equal the kept ones.
   - Stage 4 writes nothing. A converged cluster solved again from its own pose with
     unchanged inputs doesn't move, and a settled repeat settles to the same copies.

   History cells are checked this way too, not left out.

The oracle checks *when* each cell runs, not *what* it computes. It shares the computing
functions with the store, and it shares none of the decisions about when to run them.

**Rejected after a spike (2026-11-22): a store built afresh from the document.**
Comparing with `StoreCore::with_document` of the authored document, solved as a host's
first frame does, failed 72 of 494 tests. Nearly all of the differences were state the
document does not hold, by design (§3.4). Loading isn't the definition of correct for a
store with history; a fixpoint is.

- **The canary:** a build with one choke point silenced must fail the oracle on a named
  test. DS-9's canary on `set_order` moves to it, and each phase adds one for its family.
- **Decision point (DS-10):**
  - all three checks run across the suite, with no exclusions beyond the gesture,
    preview and glide states, which aren't at rest;
  - the canary fails as it should.

  If not, the verification story is revised before any family is converted.

## 5. Phases: one family at a time, each deleting its rules

Each phase converts one family of §1's first table completely: its makers, its cells,
its scope and its keys. It deletes that family's rules in the same change. Every phase
has the same exit:
- the oracle (§4) is silent across the suite with the rules gone;
- the family's canary fails as it should;
- the scaling test's rows for that family don't grow, and neither does counted work in
  a 1,000-item active composition;
- the phase's net line count is negative.

Each phase gets a short design note, reviewed before it's built. The keys were
renumbered on 2026-11-22; reviews before then use DS-10 to DS-14 for the old phases.

| Phase | Family | Deleted |
|---|---|---|
| **DS-10** | The oracle (§4) and its canary; `Cx` limited to recorded reads (`Cx::graph()` deleted, wiring reads keyed, `previous()` behind `history`, `scope_stamp` asserted) | `check_keyed_layouts`, the grouping oracle, the `structure-oracle` feature |
| **DS-11** | Layout: roots, pins, pinned parts and glide specs as makers per composition, read through `With(root, layout)`; claim handoffs in one batch | `sync_layout`, `layout_touched`, `layout_roots`, `pinned_by_use`, `prime_layout`, `prime_reorders`, `regraph`'s `shaped` |
| **DS-12** | Grouping split: the partition is a maker per composition (constraints, relations, which anchors mate; no values). Roles, frames, validation and solve order are per-cluster cells. The contexts library is a cell | `prime_grouping`, `grouping_reach`, `Grouping::reach`, `regroup`, `in_reach`, `moved`, `evaluate`'s Clusters rule, `sync_cluster_natives`, Kahn's sort, `next_to_solve`'s list, `resolve_contexts`' scan |
| **DS-7** | Refusal by the graph's journal (§3.3), any time after DS-10 | `Saved`'s graph clone, `can_refuse` |
| **DS-13** | The network, connections' signals, anchors' positions and followers; an island spanning roots is one cell | `restructure`'s rederive, `chains`, `rest`, `network_shaped`, `rewires_network`, `sync_islands` |
| **DS-14** | Binding wiring: `@name` scope, theme tokens, boundary checks | `Rescope`, `regraph`'s `names`/`under`/`defs`, `tokens_shaped`, `retokens`, `token_leaves`, `theme_at` |
| **DS-15** | Repeats and expansion: drawn repeats as makers, repeat inputs as recorded reads, templates and owners by recorded reads | `update_drawn`, `ensure_drawn`, `repeat_natives`, `REPEAT_INPUTS`, `scope_stamp`, `Owners::affected` and its fallbacks, then `regraph` and `after_transaction`'s classifier |

**The order.**
- **DS-10 first:** the oracle has to exist before any rule is deleted, and limiting
  `Cx` makes "every read is recorded" true.
- **DS-11 next:** DS-9 left layout half converted, and layout holds the most maker
  code, so it's the cheapest full proof that deletion happens.
- **DS-12 then:** it fixes the open bug, a binding retargeted onto a cluster's variable
  that isn't regrouped.
- **DS-13 to DS-15:** in the order of what the scaling test names.

**Decision points and named tests.**
- **After DS-10:** the oracle runs, or the verification story is revised.
- **After DS-11:** the layout family has no rule left, its lines are net negative, and
  its costs are within budget. If not, stop and rethink before DS-12.
- **DS-12 exits on two tests:**
  - a binding retargeted with its value unchanged;
  - a cluster whose free leaves change when a claim moves.

  Every cluster lying in one composition (reactive-engine.md assumption A11) becomes a
  checked rule, reported as a problem. If the suite or the gallery holds a
  cross-composition cluster, the partition's scope is redesigned. Solve order crosses
  compositions through bindings; the graph's heights carry it.
- **DS-7 exits on refusal** after an expansion, after a claim change and after a
  maker's batch.

**Planned in handoff-scaling.md and unchanged:** DS-3c (heights by `raise`), DS-4a,
DS-4b, DS-5, DS-6 and DS-8, as well as DS-7's other half (plans as aborted
transactions). Structural keys are neither necessity edges nor gate edges.

## 6. Performance

- **Value edits, drags and playback** cost what they do today. A keyed native that
  reruns also pays for recording: +8% on a 1,000-item stack (§9), with key changes
  applied as a difference.
- **A structural edit** costs:
  - the cells that read its keys;
  - the makers whose indexed reads changed, each in proportion to its family in its
    composition (the layouts, repeats or constraints there), not to the number of nodes;
  - the reruns that follow.

  None of this depends on other compositions.
- **Read across compositions, by right:** the theme's tokens and the contexts library,
  whose readers are the cone of an edit to them; `Roots`, when a top-level root is made
  or removed; and wiring across compositions, which is its readers' cone.
- **Refusal** costs what the refused transaction changed, from DS-7.
- **Still in proportion to the document after DS-15,** and owned elsewhere:
  - necessity, `ROOTED` and per-frame player scans (DS-4a, DS-4b, DS-5);
  - clip sampling after a structural transaction, and the animation end's scan after an
    edit (PT-9: clips as per-clip cells);
  - the selection's publish (`publish_selection`);
  - `scratch()`'s clone for plans (DS-7's other half);
  - the host (DS-8).
- **Budgets per phase:**
  - the scaling test at both axes;
  - `cells_demo`'s rows within 1.3× of the phase's base;
  - `single-tab.txt`'s drag `advance` p50 under 0.1 ms in wasm.

## 7. Size

- **Deleted** by DS-10 to DS-15 and DS-7: about 1,100 lines. These are the rule
  functions in §1's first table, the structure and grouping oracles, and `Saved`'s
  clone.
- **Added:** about 900 to 1,200 lines:
  - the scoped indexes and keys, ~150;
  - the loop and the makers' batch, ~150;
  - makers adapted from functions that exist now (`structural_roots`, `drawn_repeats`,
    `island_natives`, the partition out of `clusters_with`), ~150;
  - per-cluster roles and order, ~100;
  - `Cx`'s keyed wiring reads, ~80;
  - the oracle with state carried into its clone, ~200;
  - the graph's journal, ~200.

So the total is roughly even with today's code, give or take 300 lines.

The end state won't be smaller than the engine before DS-2. That engine was smaller
because it redid everything over the whole document on every edit. The comparison that
matters is with getting the same performance by rules, which is today's code. Against
that:
- about twenty refresh mechanisms become one;
- the order of derived work is written down in one table, where today it is spread over
  `regraph`, `after_transaction`, `post_apply` and `settle_repeats`;
- verification has one oracle instead of one per family.

## 8. Risks

- **Stratification doesn't hold.** If a maker turns out to need values (DS-12's
  partition is the test), it moves to stage 3 as a per-unit cell, or the loop's design
  is revised. The type check on makers' readers makes this visible, not silent.
- **Makers too coarse.** A maker that walks a whole composition instead of reading its
  scoped index costs O(composition) per structural edit. Each design names its maker's
  reads, and `cells_demo` measures a 1,000-item composition.
- **Keys too coarse.** `Facet` content and `In` or `With` reads rerun readers more
  often than a leaf would. That costs speed, never correctness, and the tests show it.
- **Escapes.** A cached derivation outside a cell escapes the rule. Each phase deletes
  its family's caches, and the oracle's fixpoint catches any stale one left.
- **The deeper alternative.** Copies as computed views, not written nodes, would remove
  stage 1 and most of stage 4. That is the real source of the order problem, and it is
  too large for this track.

## 9. DS-9 as built (2026-11-22)

- **Keys and the recording tree** (`structure.rs`, `tree.rs`): `Node`, `Children`,
  `FacetSet` and `Facet`; `Tree::recording`; `Tree::has` for presence. Every other
  `Tree` read on a recording tree fails a debug assertion (`unkeyed`), except
  `scope_stamp` (DS-10).
- **The touched-key log** (`document.rs`): noted in `write_node`, `insert_node`,
  `remove_node`, `restore_node`, `place`, `unplace`, `insert_facet`, `remove_facet` and
  `facet_mut`. It is on only in a store's two documents. It is asserted empty at
  `Docs::begin` and cleared by `Document::rollback`.
- **The graph** (`graph.rs`):
  - `Native::keyed`, which only an unpinned `LayoutNative` sets;
  - `Cx::tree`, which records for a keyed native;
  - `Cx::read` and `Cx::constant`, which note `Node` and `FacetSet` once per node read
    in a row;
  - `run_native`, which sorts a run's keys and applies only their difference to
    `key_readers`;
  - `set_natives`, which drops a native's keys;
  - `prime_keys`, which only marks.
- **Drains** (`Store::drain_touched`): in `regraph` after `prime_layout`; before the
  non-structural `invalidate` in `after_transaction`, the session path and each
  cluster's write-back in `post_apply`; and at the end of load.
- **The structure oracle** (`structure-oracle`, replaced in DS-10) and its canary on
  `set_order`. A reorder is the only write that reaches layouts by keys alone in DS-9.
  Creates, deletes, moves and facet changes also reach them through `sync_layout` and
  `restructure`'s leaf priming, both of which DS-11 deletes.
- **Cost** (native release, `cells_demo`, p50, before → after):
  - 1,000-item stack: edit 3.08 → 3.34 ms, preview 3.09 → 3.34 ms, reorder 3.28 → 3.54
    ms (+8%);
  - 1,000 containers read by one edit: 47.4 → 50.0 ms;
  - drag `advance` in wasm: 0.03 ms p50.
- **Fixed after review (2026-11-22):**
  - the animation end is read again after every transaction
    (`machines::playback_stops_after_a_lengthened_transition`);
  - a running clip withdraws only the leaves no other clip drives
    (`time::a_running_clip_leaves_a_paused_clip_s_value_alone`).
