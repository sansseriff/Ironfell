# Structure in the graph: one rule for when, one written order

**Status:** adopted 2026-11-22, and revised five times that day after reviews of its direction.
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
| Layout roots, pinned parts, glide specs | `sync_layout`'s candidate set, `layout_touched`, `prime_layout`, `prime_reorders` | makers (DS-12) |
| Constraint grouping and validation | `regraph`'s `regroup`, `in_reach`, `moved`; `prime_grouping`; `solve::grouping_reach`; `evaluate`'s Clusters rule (`graph.rs`, a changed tracked read re-primes `Clusters`) | a partition maker plus per-cluster cells (DS-13) |
| Cluster natives, `committed`, solve order (`next_to_solve`) | `sync_cluster_natives` after every grouping run; Kahn's sort in `clusters_with` | the partition maker; the cluster order as a cell; the solve in stage 4 (DS-13) |
| The contexts library | `resolve_contexts`, a scan of every `context` node in each pipeline | a cell (DS-13, which reads `plug.mate`) |
| Drawn repeats and their natives | `update_drawn`'s op match, `ensure_drawn`, `repeat_natives`, `repeat_dirty` | makers (DS-16) |
| A repeat's settle | `REPEAT_INPUTS`, a hand list of the inputs to compare | the repeat native's recorded reads (DS-16) |
| Islands of the across/through network | `Docs::network_shaped`, `op::rewires_network`, `sync_islands` | makers (DS-14) |
| Connections' signals, anchors' positions and followers | `regraph`'s `rederive` op match, then whole-document recomputation in `restructure` | cells (DS-14) |
| `@name` wiring | `regraph`'s `names`, `under`, `rescope_all`; `Rescope` | wiring cells (DS-15) |
| Theme tokens | `Docs::tokens_shaped`, `op::retokens`, `token_leaves`, `theme_at`; `palette::publish` over every token | cells (DS-15) |
| Instance arguments' boundary checks | `regraph`'s `defs` → `naming` scan | wiring cells (DS-15) |
| Templates | `Tree::scope_stamp` per definition (`engine/template.rs`), read with no key | cells (DS-16) |
| Copies to re-expand | `Owners::affected` (120 lines), its `Which::All` fallback and the budget fallback that expands everything | expansion by recorded reads (DS-16) |
| Which copies the solver holds | `owners.hold`, from each grouping run | derived from the partition, compared by the oracle (DS-13) |
| Blocked bindings | retried across the graph when wiring changes (`graph.rs`, the blocked retry) | wiring keys (DS-15) |
| Attachment validation | `attachment::validate` over every attachment in each transaction | a check of the edit, before against after; its scan per composition (DS-13) |
| Layout's glide targets | a sweep of every glide target on each layout run (`glided`) | per layout root (DS-12) |
| The pins of parts no layout holds | `PinsNative`, over the whole document | a maker per composition (DS-12) |
| Whether a transaction is structural at all | `after_transaction`'s op classifier | gone with `regraph` (DS-16) |

These stay outside the rule, and each has an owner:

| Kept result | Its rule today | Why it stays outside, and who owns it |
|---|---|---|
| Clip sampling, the animation end | `touches_clips`, `resample` after a structural transaction, `clips_end` | PT-9 replaces clips (patches on the time axis); its design must put them under §2's rule. Until then this rule survives (§6) |
| The host's change feed | `geom::Revisions::note_ops`, `classify` | the host is DS-8 |
| Graph heights | `compute_heights` | DS-3c |
| The gate's decision | `Clocks::gated`, `Docs::reshaped`, `op::reshapes` | DS-5 |
| Necessity, `ROOTED` | the sweep; `pipeline_from`'s and `current`'s rounds, which exist because code outside the graph reads cells nothing needed | DS-4a, DS-4b (handoff-scaling.md §4.2) |
| The solver's write-backs | `keep_solved`, `release`, `apply_recorded`: recorded writes of solved values, so undo puts them back | writers of stage 4 (§3.2), kept; DS-13's design places each |
| Edit-path validation | `run`'s `undeclares` op match before `expand::check_context` | one-off reads of the edit; its op match is a guess like the others, replaced in DS-15 by checking the contexts a declaration's readers use |

**The order of derived work** is nowhere written down. It is implied by the calls in
`regraph`, `after_transaction`, `post_apply`, `apply_session` and `settle_repeats`, and
by three loops that write results back, each with its own failure rule:
- `settle_repeats`' rounds: feedback is counted per repeat (`REPEAT_CHANGES`), and only
  the offending repeat is held;
- `post_apply`'s solve loop (`next_to_solve`): an unsatisfiable solve refuses the
  transaction;
- `resolve_physics`' rounds (`LAYOUT_ROUNDS`).

§3.2 writes the order down and checks it. Each of these three loops keeps its own
failure rule.

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
2. **Writes, through one channel.** About ten mutation choke points in `Document`
   report keys: `insert_node`, `remove_node`, `restore_node`, `write_node`, `place`,
   `unplace`, `insert_facet`, `remove_facet`, `facet_mut`, and the relation and index
   writers. A value write reports a `Leaf` key the same way, and a node that is made,
   deleted or restored reports every leaf it holds. The graph takes the log itself at
   the start of each evaluation, which deletes:
   - the `changed` lists built from ops (five sites in `lib.rs`);
   - the hand-placed `drain_touched` calls.

   The graph reports its wiring keys where it changes a binding, a claim or a tracked
   read. State is read only through inputs or leaves that report a key when they
   change. That includes glide targets, which `Cx::resolved` reads today with no key.
3. **Order, in one driver.** Every facade call ends in one function, `settle(mode)`,
   which runs the stages of §3.2 to rest. Its modes are:
   - transaction: drivers, exact, can refuse;
   - live: a gesture or the playhead;
   - undo and redo: recorded writes applied;
   - replay: no solve.

   Today six entry points run their own subsets of the stages, in their own orders:
   `after_transaction`, `apply_session`, `post_apply`, `pipeline_from`/`derive`,
   `relayout` and `advance`. A maker's reader can't read a value, by type, and each
   writer keeps its own loop and failure rule inside the driver.
4. **Fields that structure depends on can't be bound.** Some fields decide structure:
   `constraint.kind` and `enabled`, `use.notation`, `layout`'s presence. The schema marks
   them, and a binding on one is reported as an error. Today such a binding is silently
   read as its default.

A new feature adds facets, types and cells. It goes through the same reads and writes,
and it takes its place in the order by its kind, so it needs no rule. **The door stays
open where this spec does not reach, and each opening has an owner:**
- clips (PT-9);
- the host's change feed (DS-8);
- the gate's stamp and its side tables (`machines_full`, `watch_nodes`, `running_clips`,
  `animated`) (DS-5).

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

### 3.2 The order

Derived work that writes into the document can't be a cell, because a cell must not
write its own inputs. Those writers already exist. What is missing is a written order
that they and the cells keep. This table is that order:

| Stage | What runs | Reads | Writes |
|---|---|---|---|
| 0. Inputs | what enters from outside: the transaction's ops, a session write, the playhead, host publishes, the selection's and session state's publish, the theme, fonts, a layout preview, drivers and fingers | — | the document, and input leaves |
| 1. Expand | owners' copies of instances and definitions (`Owners`), outer owners first; in undo, redo and replay, the recorded writes to copies after it | structure; the solved values in the document | copies |
| 2. Make | **makers**: cells whose value is a set of cells to have | structure, authored constants and the contexts library; **no evaluated leaf**, by type | the graph's native set and wiring, as one batch |
| 3. Evaluate | value cells: bindings and natives (layouts, clusters' change tests, the cluster order, repeats, islands) | values, structure and wiring | values and claims |
| 4. Write back | `settle_repeats` (copies and item values from counts); the solve (`post_apply` in a transaction, `resolve_clusters_live` in a gesture or on the playhead), with `keep_solved`, `release` and `apply_recorded` | values; state | copies, item values, solved values |

**The rules of the order:**
- **Makers read no evaluated leaf.** They read only authored constants, through `Facet`
  keys, and a bound slot reads as its default. This is today's behaviour:
  `constraint.kind` and `enabled` count only as constants, and layout roots depend only
  on facet presence (`layout::structural_roots`).
- **Earlier stages only.** Within a pass, a stage reads derived results only from
  earlier stages, or from the passes before. Any stage may read state (§3.4).
- **A writer's own direct writes don't mark it.** A cluster's change test reads its own
  variables, so without this rule its write-back would ask for a second solve. Today
  `pose()` clears `ran` to prevent that. A change that comes back through another cell
  does mark it.
- **Each writer keeps its own loop and failure rule:**
  - feedback is counted per repeat;
  - the solve takes a dirty cluster with no dirty cluster upstream of it;
  - an unsatisfiable solve refuses a transaction and keeps the last pose live;
  - after each solve write, its cone is evaluated before the next solve, as today.
- **Another pass** runs while a writer wrote something that marked a cell. Structure
  still depends on values across passes (counts make copies, copies feed makers).
  So the loop ends for two reasons:
  - each writer detects its own feedback and reports it;
  - feedback that runs through two writers (a solve that changes a count) is caught by
    a pass budget, which reports a problem on the writer that wrote in the last pass.
- **Cluster dependencies are derived.** Each cluster's cell names the clusters upstream
  of it, from the wiring of its inputs (`transitive_deps`, keyed), across compositions
  as well. The solve orders the dirty clusters by these edges. It reports a cycle among
  them and what lies upstream of them, which is the cone, not the document. This
  replaces Kahn's sort over every cluster.
- **Candidate components and effective clusters are separate.** The partition maker
  joins constraints and *candidate* mates by structure. A mate's frame can make it
  ineligible (its inverse can fail), so each candidate component's cell decides, in
  stage 3, which mates are effective. It then derives the clusters and holds from
  those. An ineligible mate joins nothing and holds nothing, as today.
- **Binding cells are made per node.** A per-node maker keyed on `FacetSet` and
  `Facet` decides which leaves hold binding cells and whether they track. Today that is
  `restructure`'s `look`, built from `regraph`'s `nodes`.
- **Reconciliation is one batch.** A maker's diff (natives to make and drop, outputs
  reserved, claims handed from a dropped native to its replacement) is applied by one
  `set_natives` call, as today's batched claim handoff does.
- **A deleted node** reports every leaf it held as changed, at the choke point. So a
  binding that read one of them runs again with no structural key.
- **Need:** "marked" means "marked and needed". What isn't needed stays marked until it
  is, which is DS-4a's and DS-4b's design, not this spec's.

The order is written once, as this table. It isn't a list of ops: a cell's stage is its
kind. No phase merges the writers' loops into one.

### 3.3 No refusal after derivation

**Decided by the user (2026-11-22): a failed solve is accepted and reported, not refused.**
Today `Saved` clones the graph and about thirty store fields for every transaction in a
document holding a constraint, so that a solve that fails can put everything back. That
snapshot is 47% of a constant edit in the scaled demo: 9.6 → 5.1 ms at +3 copies
(`document_scaling_time`, 2026-11-22).

With the decision, nothing refuses once a transaction's ops have applied and derived
work has begun:
- an unsatisfiable cluster keeps its last good pose and reports a problem, as a failed
  formula keeps its last good value;
- a cycle between clusters is reported as a problem;
- a constraint that can't be read is reported as a problem;
- `Saved`, `can_refuse` and the refusal branch go (DS-7, §10).

What `run` checks before deriving anything (unknown nodes, scope, binding checks) still
refuses. That needs only the document's journal. An agent's `exact` edit is checked
where it is planned, on the scratch store (§10).

### 3.4 State is not a derivation

These kept results depend on their own history, and each one is declared:

| State | Kept in |
|---|---|
| Solved values | the document's leaves on held copies and operands |
| Islands' last solutions | the islands (`Native::history`, `previous()`) |
| A failed binding's last good value | the graph |
| Live-solve previews (the next frame's warm start) | the overlay |
| Glides and motion in flight | the graph's glide state |
| Players: machines, steps, queries, presence, clips | the players |
| Clocks' paused-from times | the clocks |
| Session state | `session_state` |

**Inputs**, which the oracle also moves in:
- the overlay and previews, fingers and drivers;
- host publishes, the playhead, the theme and fonts;
- the layout preview, interaction states and the pinned machine state;
- the observers.

**Results that depend on evaluation order today become functions of the current state**
(DS-10):
- **Contested leaves:** the first producer to claim a leaf wins (`claim`). Producers get
  a fixed priority instead.
- **Cycles:** which binding in a cycle is blocked depends on which ran first. Every cell
  in the cycle reports the error instead.
- **Claim errors:** errors on leaves a producer released clear only on that producer's
  next publish. They are recomputed with the claims instead.

The fallback, if a change would alter visible behaviour beyond conflicts and cycles, is
to declare that result as state here.

**Derived, though it looks like state:**
- which copies are held (from the partition);
- `committed` together with the change test's run bit, which say whether a pose has
  been invalidated;
- repeat memos, wake sets, dirty flags, `restless` and `feedback`.

Each of these is derived again from the document and the state, and the oracle
compares them (§4).

## 4. One oracle for derived results

The differential suite compares two incremental stores, so a bug they share passes it.
DS-9's structure oracle reimplements layout's claim rules: it can pass stale output and
panic on a valid bound leaf, and each family would need its own.

**The oracle** (feature `fresh-oracle`, implying `differential`) replaces them. After
every facade call it builds a reference:
- the reference is **a fresh store, built from the document**;
- the state listed in §3.4 is **moved in from the subject, by a declared list**;
- every derived result is recomputed.

It is built from a list of what to keep, not a list of what to drop. So a cache someone
forgets is missing from the reference and fails loudly, instead of being shared with it
and hiding the bug. The reference and the subject must then agree in two ways:

1. **Derived results equal:**
   - values of current, needed cells, and their errors compared by kind;
   - dependency edges, forward and reverse;
   - claims and reservations;
   - native extras;
   - makers' outputs, hold membership, the cluster order and the indexes.

   These have no history, so a difference is a missed key or a cell that isn't a
   function of what it read.
2. **History stepped when it had to, and stepped right.** A history cell (an island, a
   cluster's solve) is compared differently. Under the oracle, each one records two
   things when it steps: the state it stepped from, and its inputs.
   - **Its input set** comes from the reference's own cell, not the subject's record. So
     an input the subject's cell missed is not a bug that both sides share.
   - **If the reference's inputs differ** from the recorded ones, the subject must have
     stepped since.
   - **If they are equal**, keeping or stepping are both allowed. An extra step is
     counted as wasted work, not failed.
   - **The step must reproduce the result.** One step from the recorded state, with the
     reference's current inputs, must give the subject's state. One step from the same
     warm start is deterministic. So a solve that ran on stale inputs fails here, and
     no idempotence is needed.

   History cells aren't idempotent: re-solving a converged cluster from its own pose
   changes low bits (measured: 173 of 200 cases, about 1.7e-11). That is why the check
   replays one step and doesn't test that another step changes nothing.

**Failure is checked apart:** a call that fails (now only in `run`'s checks, before
anything is derived) leaves the subject equal to its clone before the call, pending
notifications included. This is the harness's atomicity check, today only measured
(`IRON_ATOMICITY`, `intended::NOT_ATOMIC`). It becomes a failure in DS-10.

**Named exclusions:**
- a repeat held as `restless` or `depth_blocked`, compared for its problem only;
- a cluster that didn't converge and kept its last pose: its pose, problem and what
  reads it are compared; its failed step is not replayed;
- a cut by the global expansion budget (`NODE_BUDGET`), until DS-16 makes the budget
  per root.

**The canary:** a sweep silences each choke point in turn, and each silenced build must
fail the oracle on at least one test. DS-9's canary on `set_order` is the first.

"One oracle" means one for derived results. DS-4's necessity oracle and DS-5's gate
check stay, and they check other things.

**Rejected after a spike (2026-11-22): the document alone as the reference.** Comparing
with `StoreCore::with_document` of the authored document, solved as a host's first frame
does, failed 72 of 494 tests. Nearly all of the differences were state the document
doesn't hold, by design (§3.4). Hence the declared state list.

**Decision point (DS-10):** the oracle runs across the suite with only the named
exclusions, and the canary fails as it should. If not, the verification story is revised
before any family is converted.

## 5. Phases: one family at a time, each deleting its rules

Each phase converts one family of §1's first table completely: its makers, its cells,
its scope and its keys. It deletes that family's rules in the same change. Every phase
has the same exit:
- the oracle (§4) is silent across the suite with the rules gone;
- the family's canary fails as it should;
- **locality, tested and not inferred from keys:** the scaling test's rows for that family
  don't grow with hidden compositions. Counted work doesn't grow with unrelated units
  inside the active composition (a 1,000-item one), nor with joins and splits across
  compositions;
- the family's refresh rules and caches are deleted, as listed below.

Each phase reports its net lines, but they aren't a gate. DS-7, DS-10 and DS-11 may
add more than they delete.

Each phase gets a short design note, reviewed before it's built. The keys were
renumbered twice on 2026-11-22; reviews before then use DS-10 to DS-15 for earlier phases.

| Phase | Family | Deleted |
|---|---|---|
| **DS-7** | No refusal after derivation (§3.3, §10) | `Saved`, `can_refuse`, the refusal branch and its fallback rebuild, `TransactionInput::exact` |
| **DS-10** | The oracle (§4) and its canary sweep. Results that depend on order become functions of state (§3.4). Structural fields are marked in the schema | `check_keyed_layouts`, the grouping oracle, the `structure-oracle` feature |
| **DS-11** | The mechanism, completed. One write channel: leaf keys, and the graph taking the log. One driver, `settle(mode)`. `Cx` limited to recorded reads: `Cx::graph()` deleted, wiring reads keyed, `previous()` behind `history`, `scope_stamp` and `version()` keyed or asserted, absent lookups recorded, and old and new index membership reported on moves | the `changed` lists built from ops, the `drain_touched` call sites, the entry points' own stage sequences, `apply_session`'s structural classifier |
| **DS-12** | Layout: roots, pins (`PinsNative` per composition), pinned parts and glide specs as makers per composition, read through `With(root, layout)`; glide targets per layout root; claim handoffs in one batch; what reservations as wiring change in `is_free` | `sync_layout`, `layout_touched`, `layout_roots`, `pinned_by_use`, `prime_layout`, `prime_reorders`, `regraph`'s `shaped`, `glided`'s sweep |
| **DS-13** | Grouping (§3.2): the partition maker, the candidate components' cells, cluster dependencies, per-cluster roles and validation. The contexts library is a cell keyed per context name. Hold membership is derived | `prime_grouping`, `grouping_reach`, `Grouping::reach`, `regroup`, `in_reach`, `moved`, `evaluate`'s Clusters rule, `sync_cluster_natives`, Kahn's sort, `owners.held()`'s collection per transaction, `resolve_contexts`' scan |
| **DS-14** | The network, connections' signals, anchors' positions and followers. An island spanning roots is one cell, keyed on `Roots` for who makes it. A network `deref` is scoped to the unknowns it can name | `restructure`'s rederive, `chains`, `rest`, `network_shaped`, `rewires_network`, `sync_islands` |
| **DS-15** | Bindings and inputs: binding cells made per node, `@name` scope, theme tokens, boundary checks, blocked bindings retried by wiring keys, the selection's and session state's publish | `Rescope`, `restructure`'s `look`, `regraph`'s `names`/`under`/`defs`, `tokens_shaped`, `retokens`, `token_leaves`, `theme_at`, the blocked retry, `publish_selection`'s and `publish_state`'s scans, `run`'s `undeclares` match |
| **DS-16** | Repeats and expansion, or copies as views (§5's decision): drawn repeats as makers, repeat inputs as recorded reads, templates and owners by recorded reads, the expansion budget per top-level root | `update_drawn`, `ensure_drawn`, `repeat_natives`, `REPEAT_INPUTS`, `scope_stamp`, `Owners::affected` and its fallbacks, `regraph`, `after_transaction`'s classifier |

Attachment validation stays a check of the edit, before against after (one-off reads).
It is not a cached derivation, but its scan over every attachment goes per composition
in DS-13.

**The order.**
- **DS-7 now,** beside everything else: it is the one large speed gain, and it needs
  nothing else here.
- **DS-10 first** of the rest: the oracle has to exist before any rule is deleted.
- **DS-11 next:** it completes the mechanism and the driver, so no family is converted
  against a half-built one.
- **DS-12 then:** layout is the pilot. DS-9 left it half converted, and it holds the
  most maker code, so it is the cheapest full proof that deletion happens.
- **DS-13 to DS-16:** provisional until the decision on copies, below. DS-16 comes last,
  because it holds `regraph`'s last rules.

**Decision points.**
- **After DS-10:** the oracle runs with only the named exclusions, and every silenced
  choke point fails it. If not, the verification story is revised.
- **Before DS-11: copies as views.** A short design, with a prototype of stable ids,
  edits aimed at copies, and undo, set against written copies. The solver writes into
  copies, and that is the source of the holds, `keep_solved`, `release`, most of stage
  4's ordering, and where copies' keys come from. If views are cheaper, DS-11 to DS-16
  are designed for them.
- **After DS-12:** the layout family has no rule left, and its costs are within budget.
  If not, stop and rethink before DS-13.
- **In DS-13:** whether a constraint may span compositions. The public API accepts one
  today. Either refuse it, or let the partition span the roots its constraints join.
- **DS-13 exits on three tests:**
  - a binding retargeted with its value unchanged;
  - a cluster whose free leaves change when a claim moves;
  - an ineligible mate that joins nothing.

**Checkpoints: the council and the user.** Every phase has its design note reviewed by
the council before it is built, and its code reviewed after. These points also need the
user:

| When | Decision | Why the user decides |
|---|---|---|
| DS-7's design | **Decided 2026-11-22:** a failed solve is accepted and reported; no refusal after derivation | Product behaviour (§3.3) |
| DS-10's design | **Decided 2026-11-22:** a fixed priority of producers for a contested leaf; every cell in a cycle reports the error | The conflicts users see change |
| DS-10's design | **Decided 2026-11-22:** structural fields become bindable: `constraint.enabled` and `kind` evaluated with the mates (DS-13), `use.notation` value-driven like a `when` (DS-16). Until each lands, a binding on one is reported as a problem, not ignored | Documents that bind one today behave differently |
| Before DS-11 | Copies as views, or written copies | Architecture: it reshapes DS-11 to DS-16 |
| After DS-12 | Whether the layout pilot proved the approach: rules deleted, no slope, costs within budget | Continue, or stop and rethink |
| DS-13's design | Whether a constraint may span compositions | Product behaviour |
| Any time | When to resume the PT track (handoff-patches.md, paused before PT-5) | Priorities |

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
- **Refusal after derivation** goes in DS-7, and with it the snapshot that cost 47% of a constant edit.
- **Still in proportion to the document after DS-16,** and owned elsewhere:
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

- **Deleted** by DS-7 and DS-10 to DS-16: about 1,300 lines. These are:
  - the rule functions in §1's first table;
  - the `changed` lists and drain sites;
  - the entry points' own stage sequences;
  - the structure and grouping oracles;
  - `Saved`'s field list.

  The writers' loops stay.
- **Added:** about 900 to 1,200 lines:
  - the scoped indexes and keys, ~150;
  - the makers' batch, the driver and the order's checks, ~200;
  - makers adapted from functions that exist now (`structural_roots`, `drawn_repeats`,
    `island_natives`, the partition out of `clusters_with`), ~150;
  - per-cluster roles and order, ~100;
  - `Cx`'s keyed wiring reads, ~80;
  - the oracle, a fresh store with declared state and step replay, ~300;
  - DS-7: none; it deletes about 150.

So the total is roughly even with today's code, give or take 300 lines.

The end state won't be smaller than the engine before DS-2. That engine was smaller
because it redid everything over the whole document on every edit. The comparison that
matters is with getting the same performance by rules, which is today's code. Against
that:
- about twenty refresh mechanisms become one;
- the order of derived work is written down in one table and checked, where today it is
  implied by calls in five functions;
- verification has one oracle instead of one per family.

## 8. Risks

- **Stratification doesn't hold.** If a maker turns out to need values (DS-13's
  partition is the test), it moves to stage 3 as a per-unit cell, or the order's design
  is revised. The type check on makers' readers makes this visible, not silent.
- **Makers too coarse.** A maker that walks a whole composition instead of reading its
  scoped index costs O(composition) per structural edit. Each design names its maker's
  reads, and `cells_demo` measures a 1,000-item composition.
- **Keys too coarse.** `Facet` content and `In` or `With` reads rerun readers more
  often than a leaf would. That costs speed, never correctness, and the tests show it.
- **Escapes.** A cached derivation outside a cell escapes the rule. Each phase deletes
  its family's caches, and the oracle catches any stale one left.
- **The deeper alternative.** Copies as computed views, not written nodes, would remove
  stage 1 and most of stage 4. That is the real source of the order problem. It is
  decided before DS-11 (§5).

## 9. DS-9 as built (2026-11-22)

- **Keys and the recording tree** (`structure.rs`, `tree.rs`): `Node`, `Children`,
  `FacetSet` and `Facet`; `Tree::recording`; `Tree::has` for presence. Every other
  `Tree` read on a recording tree fails a debug assertion (`unkeyed`), except
  `scope_stamp` (DS-11).
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
  `restructure`'s leaf priming, both of which DS-12 deletes.
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

## 10. DS-7's design: no refusal after derivation (draft, 2026-11-22)

The user decided (§3.3): a failed solve is accepted and reported. So the store never has
to put derived state back, and the snapshot that does it goes.

**Measured on today's code** (`document_scaling_time`, native release, p50, with the
snapshot as now → with it skipped):

| Copies | Constant edit | Structural edit |
|---|---|---|
| +0 | 2.33 → 1.30 ms | 3.54 → 2.51 ms |
| +1 | 4.62 → 2.57 ms | 6.59 → 4.45 ms |
| +3 | 9.63 → 5.09 ms | 13.20 → 8.41 ms |

What remains of the slope is necessity and the gate (DS-4a, DS-4b, DS-5).

**What `post_apply` does instead of failing:**

| Today it refuses with | After DS-7 |
|---|---|
| `Unsatisfiable`: a cluster's solve didn't converge | The cluster keeps its pose as it was before the solve, with the transaction's own writes to its leaves kept. Its problem goes on the cluster's constraints (`problems()`, kind `"constraint"`), naming the residual and the likely culprits, as the error says today. Other clusters solve as usual |
| `Cycle`: clusters depend on each other through bindings (`regroup`) | The last good grouping stays, as `rebuild_clusters` already keeps it, and the cycle is a problem on each cluster in it |
| `BadConstraint`: a constraint can't be read (`validated`) | That constraint is left out of the grouping, and its problem goes on it. This also covers a binding on `kind` or `enabled` until DS-13 makes those bindable |
| `exact` not honoured | Moves to the edit's plan (below) |
| an `ApplyError` from `release` (an internal write that can't fail on a live copy) | An `expect` |

`post_apply` returns the summary, which keeps `unmet`, and no longer returns errors.

**The agent's exact edits.** `<edit exact="true">` is planned on a scratch store, verb by
verb (`edit::Planner::push`), before it is applied. After its last verb, the planner
looks at the scratch apply's `SolverSummary::unmet`. If any is non-empty, the plan fails
with today's message ("exact transaction not honoured: …"), and the real store is never
touched. `TransactionInput::exact` and the `exact` argument go. A plan's scratch store
is a copy, so the check costs what planning already costs.

**Deleted:**
- `Saved`, with its `of` and `restore` (about 90 lines);
- `can_refuse`;
- the refusal branch in `apply` and its fallback rebuild;
- `TransactionInput::exact`;
- `ErrorKind::Unsatisfiable`, and `Cycle` and `BadConstraint` where only `post_apply`
  makes them (`run`'s own checks keep theirs).

**Unchanged:**
- `run`'s checks, which refuse before anything is derived using only the document's
  journal (`abort`);
- the atomicity measurement for those (`IRON_ATOMICITY`).

**Tests.** About 23 assertions expect a refusal (`tests/constraints.rs`, `anchors.rs`,
`rollback.rs`, `atomicity.rs`, `reactive.rs`, `embeds.rs`, `spine.rs`). Each becomes:
- the edit applies;
- the cluster keeps its pose;
- the problem is reported;
- undo puts the edit back.

`rollback.rs`'s tests of putting derived state back after a refusal are deleted with the
refusal. Their subject is gone.

**Exit:**
- the suite, the differential suite with `IRON_OWNER_CHECK`, and the structure oracle
  pass;
- an unsatisfiable edit applies and reports;
- an exact edit that constraints would move is refused at planning;
- `document_scaling_time`'s constant edit at +3 is at most 5.5 ms;
- the smoke and `single-tab.txt` pass.

## 11. DS-10's design: the oracle (draft, 2026-11-22)

**1. Order-free results first** (§3.4, decided by the user). Without these, the oracle
would need exclusions.
- **Contested leaves.** In `Reactive::claim`, today, a leaf already driven by another
  producer refuses the newcomer. Instead, **the lower `ProducerId` wins**, whichever ran
  first. The order is the ids' order: clips, network, layout, machines, theme, items,
  state, selection, steps, queries, presence, events, then hosts in registration order.
  A clip therefore wins over a layout on a laid-out child, as it usually does today
  because clips are sampled first. The loser gets today's error, and the loser's error
  is recomputed whenever either one claims. So an error on a released leaf no longer
  waits for its producer's next publish.
- **Cycles.** Today a read that would close a cycle blocks the reading cell only (spec
  §6.3), so which binding is blamed depends on order. Instead, the walk that finds the
  cycle (`graph.rs`, the deref and native read checks) returns its path. Every cell on
  it is failed with the cycle error and keeps its last good value. A test for each
  order: the same cells fail whichever is edited first.
- **Structural fields.** The schema marks `constraint.kind` and `enabled` and
  `use.notation` as structural. A binding on one is reported as a problem on its leaf,
  until DS-13 and DS-16 make them work. It is no longer read silently as its default.

**2. The reference store.** Under the feature `fresh-oracle` (implying `differential`),
after every facade call that isn't mid-gesture, `Store::agree` builds a reference:
1. It calls `StoreCore::with_document` on the subject's authored document.
2. It moves in the subject's **declared state** through one function,
   `StoreCore::state()` → `State`, applied with `StoreCore::with_state`. `State` is a
   struct with one field per row of §3.4's state and input tables:
   - the solved values on held copies (`Owners::solved`, with their slots);
   - the islands' last solutions;
   - the last good values of failed cells;
   - the players (`machine::Player`, `Steps`, `Queries`, presence);
   - the clocks;
   - `session_state`;
   - the overlay, fingers and drivers;
   - host publishes;
   - time, theme, motion and fonts' generation;
   - the layout preview;
   - the observers.

   A field added to `StoreCore` must be classed as state (in `State`) or as derived (not
   in it). A test lists `StoreCore`'s fields and fails on one in neither list.
3. It lets the reference settle as a host's first frame does (`pipeline_from(true)`).

Then it compares the snapshots' derived parts:
- values of needed cells, and errors by kind;
- signals;
- problems;
- the view;
- clusters and port anchors;
- the contexts library and table grids;
- the dependency edges, forward and reverse.

**3. History.** Islands and cluster solves are history cells (`Native::history`). Under
the feature, each step records:
- the state it started from;
- the values of the inputs it read;
- what it produced.

After a call, for each history cell, the reference's own input set is read at its
current values:
- if these differ from the recorded ones, the subject must have stepped it in this call;
- the last recorded step is replayed (`solve::solve_cluster` from the recorded pose,
  or the island's solve from its recorded start), with the recorded inputs, and must
  give what the subject produced;
- the reference takes the subject's history state, so the two then compare equal.

**4. Failure.** `IRON_ATOMICITY`'s comparison becomes a panic under the feature, for
calls that fail. Its known cases (`intended::NOT_ATOMIC`) are each fixed or named as
exclusions here.

**5. The canary sweep.** `structure::silence_for_canary` takes any choke point by name.
A test, run under the feature, builds once per choke point with it silenced, runs the
suite's structural tests, and fails if a silenced choke point passes them all.

**Deleted:**
- `check_keyed_layouts`;
- `check_grouping`;
- the `structure-oracle` feature.

**Exit:**
- the suite passes under `fresh-oracle` with `IRON_OWNER_CHECK`, with only §4's named
  exclusions;
- every choke point's canary fails;
- the cycle and contested-leaf tests pass in either order;
- the time for the oracle's suite is reported, and if it is over 15 minutes, the heavy
  tests run the oracle every tenth call.

**Order of work:** order-free results, then the `State` split and the reference, then
history, then the canary sweep. Each is a commit with its tests.
