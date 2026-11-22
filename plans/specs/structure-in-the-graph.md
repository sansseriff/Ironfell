# Structure in the graph: one rule for every kept result

**Status:** adopted 2026-11-22; revised 2026-11-22 after a review of its direction. DS-9 is
built (§9). This spec replaces reactive-engine.md §7.2, its table of what each op affects,
and it changes the order of [`handoff-scaling.md`](../handoff-scaling.md) (the `DS`
track). DS-3b stops at part 2 (`08e0539`), and this spec's phases replace the rest of it.
The review records are in handoff-scaling.md §9.

## 1. Why the same kind of bug keeps coming back

The engine has one good idea already. A cell's value is a function of the leaves it
read, and the graph records those reads. When a leaf changes, exactly the cells that
read it run again. Nobody writes a rule for which cells a value edit affects; it falls
out of the records.

**Structure never got that treatment.** "Structure" here means which nodes exist, their
parents and children, which facets they carry, and which relations join them. The store
keeps about twenty results derived from structure, each with its own cache in
`StoreCore` and its own hand-written rule for when to refresh it:

| Kept result | Its refresh rule today |
|---|---|
| Layout roots, pinned parts, glide specs | `sync_layout`'s candidate set, `layout_touched`, `prime_layout`, `prime_reorders` |
| Constraint grouping and validation | `regraph`'s `regroup`, `in_reach`, `moved`; `prime_grouping`; `solve::grouping_reach`; `evaluate`'s Clusters rule |
| Cluster natives | `sync_cluster_natives`, after every grouping run |
| Drawn repeats and their natives | `update_drawn`'s op match, `ensure_drawn`, `repeat_natives`, `repeat_dirty` |
| A repeat's settle | `REPEAT_INPUTS`, a hand list of the inputs to compare |
| Islands of the across/through network | `Docs::network_shaped`, `op::rewires_network`, `sync_islands` |
| Connections' signals, anchors' positions and followers | `regraph`'s `rederive` op match, then whole-document recomputation in `restructure` |
| `@name` wiring | `regraph`'s `names`, `under`, `rescope_all`; `Rescope` |
| Theme tokens | `Docs::tokens_shaped`, `op::retokens`, `token_leaves`, `theme_at` |
| Instance arguments' boundary checks | `regraph`'s `defs` → `naming` scan |
| Copies to re-expand | `Owners::affected` (120 lines) |
| Animation end | `clips_end`, cleared by hand |
| Whether a transaction is structural at all | `after_transaction`'s op classifier |

Each rule is a guess about which edits can change its result, and each has to stay
complete as features are added. They don't. About half the bugs the last code review
found were in these rules: a nested layout root missed, a repeat's inputs list one
short, `can_refuse`'s guess of which documents need a snapshot. And the fix for another
added a clip classifier that forgot transitions. Each fix was another, better
guess. That's the band-aid loop the user asked about.

reactive-engine.md planned these results as "structure cells" (§7, §9.4), but it had
them told what changed by an op table (§7.2). That table is where the rules came from.
This spec deletes the table.

## 2. The rule

**Every kept result is a cell: a function of the document, computed from scratch over a
bounded scope, which reads the document only through a recording reader. A write reports
the keys it touched, and the cells that read those keys run again. Nothing else decides
when a kept result is refreshed.**

The reads that structure cells record and the leaf reads that value cells already record
are the same idea at two grains. A leaf read is a fine key; a `Children(n)` read is a
coarse one.

- **Kept** means cached across transactions or frames.
- **From scratch** means the function has no incremental logic of its own: given its
  scope, it computes the whole answer. The engine's only incremental machinery is when
  to call it (keys) and how much it covers (scope). Any cell may still be fast inside
  (a layout reusing its measured text), but no cell needs to know what changed.
- **Scope** is one top-level root (a composition, a definition, a timeline) or a unit
  inside it, such as one layout root. handoff-scaling.md §2 allows exactly this: work
  in a shown composition may cost in proportion to that composition and to the cone of
  what changed, never to the rest of the document.
- **One-off reads** (commands, export, the agent's view, tests, the edit path itself)
  read the document directly.
- **No new rules, ever.** A fix that would need a new op classifier or stamp must not
  add one. Until its result is a cell, it clears unconditionally, as `clips_end` now
  does after every transaction.

### Why this closes the door rather than moving it

The places that can be wrong after this are fixed by the data model, not by the
features built on it:

1. **The reads:** `Tree`'s read methods. Each records a key. A read with no key fails a
   debug assertion on a recording tree, so a new kind of read is caught the first time
   a test runs it.
2. **The writes:** about ten mutation choke points in `Document` (`insert_node`,
   `remove_node`, `restore_node`, `write_node`, `place`, `unplace`, `insert_facet`,
   `remove_facet`, `facet_mut`, the relation and index writers). Each reports the keys
   of what it changed.

A new feature adds facets, types and cells. It goes through the same reads and writes,
so it needs no rule. Today a new feature needs a new rule in `regraph` and whoever
forgets one ships a stale result. After this, a missed key is a bug in one of a dozen
functions that don't change with features, and the oracle (§4) names it.

**What stays outside the rule, on purpose:**
- **Players.** Machines, steps, queries, presence, clips and physics islands are state
  that runs in time. They're event-driven since DS-2, and are woken by leaf changes
  (`invalidate` wakes them).
- **The gate and necessity.** These are DS-4a, DS-4b and DS-5 in handoff-scaling.md, a
  separate cause that is already designed.
- **The host's revisions** (DS-8).

## 3. The mechanism

### 3.1 Keys: reads record, writes report

Built in DS-9 (§9): `Node(n)`, `Children(n)`, `FacetSet(n)` and `Facet(n, kind)`. Each
phase adds the keys its cells read. The writes for each are at the choke point that
changes what it names:

| Key | Recorded by | Reported by |
|---|---|---|
| `In(root, type)`, `With(root, kind)` | the scoped indexes: the nodes of a type, or with a facet kind, under one top-level root | the index writers, for the node's root |
| `Relations(n)`, `Relation(id)` | `relations_from`, `relations_to`, `relation` | `insert_relation`, `remove_relation`, at both ends |
| `Roots` | `roots` | placing or unplacing a node with no parent |
| `Wiring(leaf)` | `Cx::is_bound`, `Cx::driven`, `Cx::transitive_deps` (each leaf walked) | the graph, where a leaf's binding, claim or tracked reads change |

The type and facet indexes are whole-document today (`nodes_of_type`, `nodes_with`).
They become **per top-level root**, so a maker reads only its own composition's
repeats, layouts or constraints. Moving a subtree to another root reindexes the
subtree, which is the cone of that move.

### 3.2 Two strata of cells

**Value cells** are today's bindings and natives (layouts, clusters' change tests,
repeat natives, islands). They read leaves, and now structure too.

**Makers** are structure cells whose value is a set of cells to have: "the layout roots
in this composition", "the drawn repeats in this composition", "the clusters in this
composition". A maker is a from-scratch function over its scope, read through the
recording tree, with keys like any cell. The store reconciles makers in one generic
loop, which replaces `sync_layout`, `sync_repeats`, `update_drawn`, `sync_islands` and
`sync_cluster_natives`:

1. **Drain** the touched keys: mark the cells that read them, makers and value cells
   alike.
2. **Run the marked makers** in a fixed order (binding wiring, then derived wiring, then
   makers of natives). Diff each one's wanted set against what it made last time, and
   hand the graph one batch of natives to make and drop (`set_natives`, not run).
3. **Drain again,** for what a maker's natives wrote while being made (their
   reservations). Repeat from 2 until no maker is marked. This terminates because makers
   write no structure.
4. **Evaluate** the value cells.

A maker never runs during evaluation. So no cell is made mid-pass, which was the open
question of the old DS-12. Copies that values decide (a repeat's count) are still made
by `settle_repeats`' rounds after evaluation. Their writes go through the choke points
like any other, and the next round starts again at 1.

The order in step 2 is one fixed list of maker kinds, written once. It is not a list of
ops.

### 3.3 Refusal by the journal, not a snapshot

Today `Saved` clones the graph for every transaction in a document holding a constraint,
which costs in proportion to the whole graph. After this, every structure result is a
function rerun by keys. So a refused transaction can be undone the way it was done:

- the document journal rolls the document back through the choke points;
- the keys those undo writes touch mark the cells that ran on the refused state;
- those cells run again on the restored document.

The few cells whose result depends on history get a value journal: a cell's previous
value is saved the first time it changes in a transaction. These are the solver's
previous pose and an island's last solution. This is handoff-scaling.md §8 decision 5
("refusal restores the document journal and re-derives the cone"), and it lands as
phase DS-7, brought forward from handoff-scaling.md.

## 4. One oracle: nothing kept is stale

The differential suite compares two incremental stores, so a bug they share passes it.
DS-9's structure oracle reimplements layout's claim rules, and the review found that it
can both pass stale output and panic on a valid bound leaf. Each new family would need
its own such oracle. One generic oracle replaces it (feature `fresh-oracle`, implying
`differential`). It runs after every facade call and makes two checks:

1. **Structure is fresh.** The kept structure is compared with the same functions run
   from scratch over the same document. "Kept structure" means:
   - every maker's set of natives, with their specs;
   - every leaf's wiring: its expression, after `@name` and token resolution, and what
     it reads;
   - the derived signals and anchors;
   - the clusters.

   Structure is a pure function of the document, which is this spec's premise, so this
   comparison has no history to account for.
2. **Every current cell is current.** A clone of the graph has every cell marked stale
   and evaluated, and the values must equal the kept ones. The clone carries the
   store's state over: publishes, players, a failed binding's last good value, the
   solver's pose and the islands' last solutions. Only cells whose result depends on
   their own previous result are left out (islands, the solver's clusters); each one
   says so through `Native::history`.

Together these mean that nothing kept is stale. The oracle checks *when* each cell
runs, not *what* it computes. It shares the computing functions with the store, and it
shares none of the decisions about when to run them.

**Rejected after a spike (2026-11-22): a store built afresh from the document.**
Comparing with `StoreCore::with_document` of the authored document, solved like a
host's first frame, failed 72 of 494 tests. Nearly all of the differences are state
the document does not hold, by design:
- a failed binding keeps its last good value;
- copies the solver placed;
- machines, traces, accumulators and queries in flight;
- network solutions;
- instance session state.

Loading is not the definition of correct for a store with history.

- **The canary:** a build with one choke point silenced must fail the oracle on a named
  test. DS-9's canary on `set_order` moves to it, and each phase adds one for its
  family.
- **Decision point (DS-10):** both checks run across the suite with only the declared
  history cells left out, and the canary fails. If not, the verification story is
  revised before any family is converted.

## 5. Phases: one family at a time, each deleting its rules

Each phase converts one family of kept results (§1's table) completely: its makers, its
cells, its scope and its keys. Each deletes that family's rules in the same change. Its
exit is the same each time:
- the oracle (§4) is silent across the suite with the rules gone;
- the family's canary fails;
- the scaling test's rows for that family don't grow;
- the phase's net line count is negative.

Each gets a short design note reviewed before it's built. The keys were renumbered on
2026-11-22: DS-10 to DS-14 in the reviews before then name the old phases.

| Phase | Family | Deleted |
|---|---|---|
| **DS-10** | The oracle (§4) and its canary | `check_keyed_layouts`, the grouping oracle, the `structure-oracle` feature |
| **DS-11** | Layout: roots, pins, pinned parts, glide specs per composition, read through `With(root, layout)` | `sync_layout`, `layout_touched`, `layout_roots`, `pinned_by_use`, `prime_layout`, `prime_reorders`, `regraph`'s `shaped` |
| **DS-12** | Grouping and validation per composition, with `Wiring` keys; cluster natives from the maker | `prime_grouping`, `grouping_reach`, `Grouping::reach`, `regroup`, `in_reach`, `moved`, `evaluate`'s Clusters rule, `sync_cluster_natives` |
| **DS-7** | Refusal by the journal (§3.3) | `Saved`'s graph clone, `can_refuse` |
| **DS-13** | The network, connections' signals, anchors' positions and followers, per composition | `restructure`'s rederive, `chains`, `rest`, `network_shaped`, `rewires_network`, `sync_islands` |
| **DS-14** | Binding wiring: `@name` scope, theme tokens, boundary checks | `Rescope`, `regraph`'s `names`/`under`/`defs`, `tokens_shaped`, `retokens`, `token_leaves`, `theme_at` |
| **DS-15** | Repeats and expansion: drawn repeats per composition, repeat inputs as recorded reads, copies' owners | `update_drawn`, `ensure_drawn`, `repeat_natives`, `REPEAT_INPUTS`, `Owners::affected`, then `regraph` and `after_transaction`'s classifier entirely |

**The order.** DS-11 goes first because DS-9 left layout half converted and it holds the
most maker code, so it's the cheapest full proof that deletion happens. DS-12 fixes the
open bug: a binding retargeted onto a cluster's variable is not regrouped. DS-7 follows
DS-12 because the grouping is what forces snapshots. DS-13 to DS-15 can move in order of what the
scaling test names.

**Decision points.**
- **After DS-10:** the oracle runs, or the verification story is revised.
- **After DS-11:** the layout family has no rule left, its lines are net negative, and its
  costs are within budget. If not, stop and rethink before DS-12.
- **In DS-12:** every cluster lying in one composition (reactive-engine.md assumption A11)
  becomes a checked rule, reported as a problem. If the suite or the gallery holds a
  cross-composition cluster, DS-12's scope is redesigned.

**DS-3c (heights by `raise`), DS-4a, DS-4b, DS-5, DS-6 and DS-8** stay as planned in
handoff-scaling.md, as does DS-7's other half (plans as aborted transactions). Structural keys are neither necessity edges nor gate edges.

## 6. Performance

- **Value edits, drags and playback** cost what they do today. A keyed native that
  reruns also pays for recording: +8% on a 1,000-item stack (§9), with key changes now
  applied as a diff.
- **A structural edit** costs:
  - the cells that read its keys;
  - the makers whose indexed reads changed, each costing the size of its family in its
    composition (the layouts, repeats or constraints there), not the number of nodes;
  - their reruns.

  None of this depends on other compositions.
- **What is read across compositions, and why that is allowed:**
  - The theme's tokens. Their readers are the cone of a theme edit.
  - The contexts library. It is document-level by definition, and its readers are the
    cone.
  - `Roots`. It's read when a top-level root is made or removed, for O(roots).
  - Cross-composition wiring. A binding's reads are its cone.
- **Refusal** costs the cone of the refused transaction, from DS-7. Until then a document
  with constraints pays for `Saved`'s clone, a known whole-document cost that DS-7 removes.
- **Budgets per phase:** the scaling test at both axes, `cells_demo`'s rows within 1.3×
  of the phase's base, and `single-tab.txt`'s drag `advance` p50 under 0.1 ms in wasm.

## 7. Size

- **Deleted by DS-10 to DS-15:** about 1,000 lines. The rule functions in §1's table and the
  structure and grouping oracles.
- **Added:** about 700–900 lines:
  - the scoped indexes and their keys, ~150;
  - the maker loop, ~120;
  - family makers adapted from functions that exist now (`structural_roots`,
    `drawn_repeats`, `island_natives`, `clusters_with`), ~150;
  - graph-side wiring keys, ~80;
  - the oracle, ~120;
  - the journal refusal, ~150, net of `Saved`.

From today's code that's roughly break-even to 300 lines smaller.

It doesn't make the engine smaller than before DS-2. That engine was smaller because it
redid everything over the whole document on every edit. The comparison that matters is
against getting the same performance with rules, which is today's code. Against that,
the count of refresh mechanisms goes from about twenty to one.

## 8. Risks

- **History cells hide staleness.** A cell excluded from check 2 because its result
  depends on its previous one (islands, the solver's clusters) is checked for structure
  only. Each such cell is named in the code, and the list is reviewed with each phase.
- **Makers too coarse.** A maker reading a whole composition's tree, instead of its
  scoped index, would cost O(composition) per structural edit. Each design names its
  maker's reads, and `cells_demo` measures a 1,000-item composition.
- **Keys too coarse.** `Facet` content and `In`/`With` reads rerun readers more than a
  leaf would. That costs speed, never correctness, and the tests show it.
- **Escapes.** A kept result outside a cell escapes the rule. Each phase deletes its
  family's caches from `StoreCore`, and the oracle catches any stale one left.

## 9. DS-9 as built (2026-11-22)

- **Keys and the recording tree** (`structure.rs`, `tree.rs`): `Node`, `Children`,
  `FacetSet`, `Facet`. `Tree::recording` and `Tree::has` (presence). On a recording
  tree, every other `Tree` read fails a debug assertion (`unkeyed`).
- **The touched-key log** (`document.rs`): noted in `write_node`, `insert_node`,
  `remove_node`, `restore_node`, `place`, `unplace`, `insert_facet`, `remove_facet` and
  `facet_mut`. It's on only in a store's two documents, asserted empty at `Docs::begin`
  and cleared by `Document::rollback`.
- **The graph** (`graph.rs`):
  - `Native::keyed`, for unpinned `LayoutNative` only;
  - `Cx::tree` is recording for a keyed native;
  - `Cx::read` and `Cx::constant` note `Node` and `FacetSet` once per node read in a row;
  - `run_native` sorts a run's keys, and applies only their difference to `key_readers`
    (2026-11-22, after review);
  - `set_natives` drops a native's keys;
  - `prime_keys` marks only.
- **Drains** (`Store::drain_touched`): in `regraph` after `prime_layout`; before the
  non-structural `invalidate` in `after_transaction`, the session path and each
  cluster's write-back in `post_apply`; at the end of load.
- **The structure oracle** (`structure-oracle`, to be replaced in DS-10) and its canary on
  `set_order`. A reorder is the only write that reaches layouts by keys alone in DS-9.
  Creates, deletes, moves and facet changes still also reach them through `sync_layout`
  and `restructure`'s leaf priming, which DS-11 deletes.
- **Cost** (native release, `cells_demo`, p50, before → after): 1,000-item stack edit
  3.08 → 3.34 ms, preview 3.09 → 3.34, reorder 3.28 → 3.54 (+8%); 1,000 containers read
  by one edit 47.4 → 50.0 ms. Drag `advance` in wasm: 0.03 ms p50.
- **Fixed after review (2026-11-22):**
  - the animation end is read again after every transaction, not by a clip classifier
    (`machines::playback_stops_after_a_lengthened_transition`);
  - a running clip withdraws only leaves no other clip drives
    (`time::a_running_clip_leaves_a_paused_clip_s_value_alone`).
