# Structure in the graph: one rule for when, one loop for order

**Status:** adopted 2026-11-22, and revised three times that day after reviews of its direction.
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
| Cluster natives, `committed`, solve order (`next_to_solve`) | `sync_cluster_natives` after every grouping run; Kahn's sort in `clusters_with` | the partition maker; the solve in the loop's stage 4 (DS-12) |
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
| Copies the solver holds | `owners.hold`, `keep_solved`, `release` writing template values back, `apply_recorded` | holds as state (§3.4); a dropped hold marks its owner for expansion (DS-12) |
| Blocked bindings | retried across the graph when wiring changes (`graph.rs`, the blocked retry) | wiring keys (DS-14) |
| Attachment validation | `attachment::validate` over every attachment in each transaction | a validation cell per composition (DS-12) |
| Whether a transaction is structural at all | `after_transaction`'s op classifier | gone with `regraph` (DS-15) |

These stay outside the rule, and each has an owner:

| Kept result | Its rule today | Why it stays outside, and who owns it |
|---|---|---|
| Clip sampling, the animation end | `touches_clips`, `resample` after a structural transaction, `clips_end` | PT-9 replaces clips (patches on the time axis); its design must put them under §2's rule. Until then this rule survives (§6) |
| The host's change feed | `geom::Revisions::note_ops`, `classify` | the host is DS-8 |
| Graph heights | `compute_heights` | DS-3c |
| The gate's decision | `Clocks::gated`, `Docs::reshaped`, `op::reshapes` | DS-5 |
| Necessity, `ROOTED` | the sweep | DS-4a, DS-4b |

**The order of derived work** is decided by five loops of its own, each with a round limit:
- `pipeline_from`'s four rounds, which relax the whole graph past the limit;
- `current`'s four rounds;
- `resolve_physics`' `LAYOUT_ROUNDS`;
- `settle_repeats`' rounds (`REPEAT_CHANGES`, `feedback`, `restless`);
- `post_apply`'s solve loop (`next_to_solve`).

They are spread over `regraph`, `after_transaction`, `post_apply`, `apply_session` and
`settle_repeats`. §3.2 replaces them with one loop.

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
3. **Order.** One loop, with five stages and their declared inputs (§3.2). It replaces
   the five round-limited loops of today. A maker's reader can't read a value, by type.

A new feature adds facets, types and cells. It goes through the same reads and writes,
and it takes its place in the loop by its kind, so it needs no rule. **The door stays
open where this spec does not reach, and each opening has an owner:**
- clips (PT-9);
- the host's change feed (DS-8);
- the gate's stamp (DS-5).

Each owner's design must follow §2.

Today a new feature needs a new rule in `regraph`, and forgetting one ships a stale result. After this, a
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

### 3.2 The loop

Derived work that writes into the document can't be a cell, because a cell must not
write its own inputs. So the engine has **one loop**, and each stage of it is either a set
of cells or one of the writers that exist today:

| Stage | What runs | Reads | Writes |
|---|---|---|---|
| 0. Inputs | what enters from outside: the transaction's ops, a session write, the playhead, host publishes, the selection's and session state's publish, the theme, fonts, a layout preview, drivers and fingers | — | the document, and input leaves |
| 1. Expand | owners' copies of instances and definitions (`Owners`), outer owners first | structure; the solver's holds (state) | copies |
| 2. Make | **makers**: cells whose value is a set of cells to have | structure, authored constants and the contexts library; **no evaluated leaf**, by type | the graph's native set and wiring, as one batch |
| 3. Evaluate | value cells: bindings and natives (layouts, clusters' change tests, repeats, islands) | values, structure and wiring | values and claims |
| 4. Write back | `settle_repeats` (copies and item values from counts), and the solve (in a transaction: dirty clusters solved against their drivers, as `post_apply` does; in a gesture or on the playhead: previews, as `resolve_clusters_live` does) | values; state (poses, holds) | copies, item values, poses, holds |

- **Makers read no evaluated leaf.** They read only authored constants, through
  `Facet` keys, and a bound slot reads as its default. This is today's behaviour:
  `constraint.kind` and `enabled` count only as constants, and layout roots depend only
  on structure (`layout::structural_roots`).
- **Rule of order.** Within a round, a stage reads derived results only from earlier
  stages or earlier rounds. Any stage may read state (§3.4). The solver's holds are
  state, so stage 1 reading them is no backward edge.
- **Holds.** When a hold is dropped, the copy's owner is marked, and stage 1 expands it
  again without the hold. That deletes `release` as a writer of its own.
- **Quiescence.** The loop runs until nothing is marked. A writer that produces what is
  already kept writes nothing:
  - a settled repeat gives the same copies;
  - a converged cluster solved from its own pose doesn't move.

  So the loop stops when no stage has anything left to do.
- **Solve order.** In stage 4 the solve takes the next dirty cluster after each
  write-back, as `next_to_solve` does today. A cluster B that reads cluster A's variable
  through a binding is marked by that write and solved after it. Rings of clusters are
  reported by the per-cluster wiring check (DS-12), not discovered by the budget.
  Clusters don't claim their variables.
- **The budget.** One budget per transaction replaces the five loops' limits. When it
  runs out, the state reached is kept and a problem names what was still marked, as
  `restless` does today. This is for feedback the engine already detects: a repeat
  whose copies change what decides its count.
- **Reconciliation is one batch.** A maker's diff (natives to make and drop, outputs
  reserved, claims handed from a dropped native to its replacement) is applied by one
  `set_natives` call, as today's batched claim handoff does. A reservation is wiring;
  an active claim is a value of stage 3.
- **A deleted node** reports every leaf it held as changed, at the choke point. So a
  binding that read one of them runs again with no structural key.
- **Within stage 3**, wiring that changes (a tracked read moving, a claim made) reaches
  cells of stage 3 only. No maker reads it: the partition gives up `is_free` and the
  solve order, which become per-cluster cells (DS-12).

The stage order is written once, as this table. It is not a list of ops, and a cell's
stage is its kind.

### 3.3 Refusal

Today `Saved` clones the graph and about thirty store fields for every transaction in a
document holding a constraint. That costs in proportion to the whole document, and its
field list is another hand list. There are two candidates, and DS-7 decides between them
by measuring:
- **Persistent maps** (`im` or `imbl`) for the graph's and the store's tables. `Saved`
  stays as it is, and a clone becomes nearly free. Every field is covered by
  construction, at the price of slower map operations.
- **A journal of the graph's own mutations**: `set_cell`, `drop_cell`, `set_natives`,
  key → readers, reservations, claims, errors, pending queues and each cell's first
  value change. It is undone in reverse, as the document's journal is. It is opened
  before the first speculative write and must cover the store fields field by field.

Either way the oracle's refusal check (§4) makes the choice checked rather than trusted.
DS-7 is brought forward from handoff-scaling.md and can land any time after DS-10.
Whether a refused solve should stay refused (rather than, say, keep the last good pose
and report a problem) is product behaviour, and it is out of scope.

### 3.4 State is not a derivation

These kept results depend on their own history, and each declares it:

| State | Kept in | Read by |
|---|---|---|
| The solver's poses and holds, `committed` | the document's solved leaves, `Owners::solved` | stage 1 (holds), stage 4 |
| Islands' last solutions | the islands (`Native::history`, `previous()`) | stage 3 |
| A failed binding's last good value | the graph | stage 3 |
| Players: machines, steps, queries, presence, clips | the players | stage 0, woken by leaf changes |
| Session state | `session_state` | stage 0 |

Bookkeeping isn't state: repeat memos, wake sets and dirty flags can be rebuilt from
the document and the state, and the oracle rebuilds them (§4).

## 4. One oracle: the same step, with nothing cached

The differential suite compares two incremental stores, so a bug they share passes it.
DS-9's structure oracle reimplements layout's claim rules: it can pass stale output and
panic on a valid bound leaf, and each family would need its own.

**The oracle** (feature `fresh-oracle`, implying `differential`) replaces them. Before
every facade call, it clones the subject into a reference, keeping all state (§3.4) and
dropping every derived result and all bookkeeping:
- every cell and maker is marked;
- every owner is marked for expansion;
- the indexes are rebuilt from the node table;
- memos, wake sets and dirty flags are cleared.

Then the same call runs on both stores, and their snapshots must be equal:
- values and errors;
- dependency edges, forward and reverse;
- claims and reservations;
- native extras;
- copies and the document.

The reference computes everything. The subject computes only what its keys and marks
say, from the same history: the same poses, last solutions, last good values and
players. So a difference is a missed key, or a cell that isn't a function of what it
read. The oracle checks *when* each cell runs, not *what* it computes. It shares the
computing functions with the store, and none of the decisions about when to run them.

- **History cells.** A history cell (an island, the solver) gives the same result from
  the same previous state and the same inputs. So it needs no exclusion, unless it is
  nondeterministic, and then that is a bug.
- **Refusal:** a call that fails leaves the subject equal to its clone before the call.
  This is the harness's atomicity check, today only measured (`IRON_ATOMICITY`,
  `intended::NOT_ATOMIC`), made a failure for refused transactions.
- **Named exclusions:**
  - a repeat marked `restless` or `depth_blocked`, which by definition isn't settled;
  - a cluster that didn't converge and kept its last pose.

  Both are compared for their problems only.
- **The canary:** a build with one choke point silenced must fail the oracle on a named
  test. DS-9's canary on `set_order` moves to it, and each phase adds one for its
  family.

**Rejected after a spike (2026-11-22): a store built afresh from the document.**
Comparing with `StoreCore::with_document` of the authored document, solved as a host's
first frame does, failed 72 of 494 tests. Nearly all of the differences were state the
document doesn't hold, by design (§3.4). Loading isn't the definition of correct for a
store with history; the same step from the same state is.

**Decision point (DS-10):** the oracle runs across the suite with only the named
exclusions, and the canary fails as it should. If not, the verification story is
revised before any family is converted.

## 5. Phases: one family at a time, each deleting its rules

Each phase converts one family of §1's first table completely: its makers, its cells,
its scope and its keys. It deletes that family's rules in the same change. Every phase
has the same exit:
- the oracle (§4) is silent across the suite with the rules gone;
- the family's canary fails as it should;
- the scaling test's rows for that family don't grow, and neither does counted work in
  a 1,000-item active composition;
- the family's refresh rules and caches are deleted, as listed below.

Each phase reports its net lines, but they aren't a gate. DS-10 and DS-7 add more than
they delete.

Each phase gets a short design note, reviewed before it's built. The keys were
renumbered on 2026-11-22; reviews before then use DS-10 to DS-14 for the old phases.

| Phase | Family | Deleted |
|---|---|---|
| **DS-10** | The oracle (§4) and its canary; `Cx` limited to recorded reads (`Cx::graph()` deleted, wiring reads keyed, `previous()` behind `history`, `scope_stamp` asserted) | `check_keyed_layouts`, the grouping oracle, the `structure-oracle` feature |
| **DS-11** | Layout: roots, pins, pinned parts and glide specs as makers per composition, read through `With(root, layout)`; claim handoffs in one batch | `sync_layout`, `layout_touched`, `layout_roots`, `pinned_by_use`, `prime_layout`, `prime_reorders`, `regraph`'s `shaped` |
| **DS-12** | Grouping split. The partition is a maker per composition: constraints, relations, which anchors mate; no values. Roles, frames, validation, and the check that clusters form no ring through bindings, are per-cluster cells. The solve joins the loop's stage 4 (§3.2). Holds are state, and a dropped hold marks its owner. The contexts library is a cell, and attachment validation is a cell per composition | `prime_grouping`, `grouping_reach`, `Grouping::reach`, `regroup`, `in_reach`, `moved`, `evaluate`'s Clusters rule, `sync_cluster_natives`, Kahn's sort, `release`, `owners.held()`'s collection per transaction, `resolve_contexts`' scan, `attachment::validate`'s scan |
| **DS-7** | Refusal by persistent maps or the graph's journal (§3.3), any time after DS-10 | `Saved`'s clone, `can_refuse` |
| **DS-13** | The network, connections' signals, anchors' positions and followers. An island spanning roots is one cell, made by the maker of the lowest root it touches. A network `deref` is scoped to the unknowns it can name | `restructure`'s rederive, `chains`, `rest`, `network_shaped`, `rewires_network`, `sync_islands` |
| **DS-14** | Binding wiring and the inputs: `@name` scope, theme tokens, boundary checks, blocked bindings retried by wiring keys, the selection's and session state's publish | `Rescope`, `regraph`'s `names`/`under`/`defs`, `tokens_shaped`, `retokens`, `token_leaves`, `theme_at`, the blocked retry over the graph, `publish_selection`'s and `publish_state`'s scans |
| **DS-15** | The loop whole, with repeats and expansion. Drawn repeats are makers, repeat inputs are recorded reads, templates and owners go by recorded reads, and the expansion budget is per top-level root, so a cut doesn't depend on other compositions | `update_drawn`, `ensure_drawn`, `repeat_natives`, `REPEAT_INPUTS`, `scope_stamp`, `Owners::affected` and its fallbacks, `pipeline_from`'s and `current`'s rounds, `regraph`, `after_transaction`'s classifier |

**The order.**
- **DS-10 first:** the oracle has to exist before any rule is deleted, and limiting
  `Cx` makes "every read is recorded" true.
- **DS-11 next:** DS-9 left layout half converted, and layout holds the most maker
  code, so it's the cheapest full proof that deletion happens.
- **DS-12 then:** it fixes the open bug, a binding retargeted onto a cluster's variable
  that isn't regrouped.
- **DS-13 to DS-15:** in the order of what the scaling test names. DS-15 comes last,
  because the loop can be whole only once every family is in it.

**Decision points and named tests.**
- **After DS-10:** the oracle runs, or the verification story is revised.
- **After DS-11:** the layout family has no rule left, its lines are net negative, and
  its costs are within budget. If not, stop and rethink before DS-12.
- **DS-12 exits on two tests:**
  - a binding retargeted with its value unchanged;
  - a cluster whose free leaves change when a claim moves.

  Every cluster lying in one composition (reactive-engine.md assumption A11) becomes a
  checked rule, reported as a problem. The public API accepts a constraint spanning
  two compositions today, so DS-12's design decides whether to refuse that or to make
  the partition span the roots its constraints join. Solve order crosses compositions
  through bindings, and stage 4's solve-the-next-dirty-cluster carries it.
- **DS-7 exits on refusal** after an expansion, after a claim change and after a
  maker's batch.
- **Before DS-15:** a short design of copies as computed views, not written nodes (§8),
  set against converting `Owners`. Copies as views would remove stage 1 and most of
  stage 4. If they are cheaper than DS-15, DS-15 becomes them.

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
    edit (PT-9);
  - `scratch()`'s clone for plans (DS-7's other half);
  - the host (DS-8).
- **Where the speed comes from.** This track mostly makes the scaling that DS-1 to
  DS-3b reached by rules correct by construction, keeps it scoped and deletes the
  rules. Its one large speed gain is DS-7: `Saved`'s clone was about 650 samples per
  transaction in a constrained document. The largest remaining whole-document costs per
  frame and per edit are necessity and the gate (handoff-scaling.md §3.1, §3.2). They
  have their own cause and design (DS-4a, DS-4b, DS-5), and they run alongside this
  track: performance needs both.
- **Budgets per phase:**
  - the scaling test at both axes;
  - `cells_demo`'s rows within 1.3× of the phase's base;
  - `single-tab.txt`'s drag `advance` p50 under 0.1 ms in wasm.

## 7. Size

- **Deleted** by DS-10 to DS-15 and DS-7: about 1,300 lines. These are the rule
  functions in §1's first table, the five round loops, `release`, the structure and
  grouping oracles, and `Saved`'s clone.
- **Added:** about 900 to 1,200 lines:
  - the scoped indexes and keys, ~150;
  - the loop and the makers' batch, ~200;
  - makers adapted from functions that exist now (`structural_roots`, `drawn_repeats`,
    `island_natives`, the partition out of `clusters_with`), ~150;
  - per-cluster roles and order, ~100;
  - `Cx`'s keyed wiring reads, ~80;
  - the oracle, which clones with derived results dropped, ~200;
  - DS-7: ~100 with persistent maps, ~250 with a journal.

So the total is roughly even with today's code, give or take 300 lines.

The end state won't be smaller than the engine before DS-2. That engine was smaller
because it redid everything over the whole document on every edit. The comparison that
matters is with getting the same performance by rules, which is today's code. Against
that:
- about twenty refresh mechanisms become one;
- the order of derived work is written down in one table, where today five loops spread
  over five functions decide it;
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
  its family's caches, and the oracle catches any stale one left.
- **Persistent maps too slow** (DS-7). If they are, DS-7 uses the journal, and the
  oracle's refusal check keeps its field list honest.
- **The deeper alternative.** Copies as computed views, not written nodes, would remove
  stage 1 and most of stage 4. That is the real source of the order problem. It is
  weighed before DS-15 (§5).

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
