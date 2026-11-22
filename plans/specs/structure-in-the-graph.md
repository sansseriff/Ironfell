# Structure in the graph: one reader of the document

**Status:** draft, revised 2026-11-22 after council rounds 1 (Opus 5/10, Sol 5/10) and 2
(Opus 6/10, Sol 6/10); §10. Not adopted. It changes the order of
[`handoff-scaling.md`](../handoff-scaling.md) (the `DS` track): DS-3b stops at part 2
(`08e0539`), and this spec's phases, DS-9 to DS-14, replace the rest of DS-3b. DS-4a,
DS-4b and DS-5 stay as planned there and do not wait for these (§6). It replaces
reactive-engine.md §7.2's table of what each op affects.

This spec designs the mechanism and its first slice, DS-9, in full. Each later phase gets
a short design of its own, reviewed before it is built; §5 lists what each must answer.

## 1. Why

The graph records the leaves a cell reads through `Cx::read`, and a change reaches the
cells that read it. Nothing records what kept code reads of the document through `Tree`:
which nodes exist, their types, parents and children, which facets they carry and what
those facets hold (`get(..).field.constant()`, 233 such reads in `iron_document`), which
relations join them. Cells are handed the whole document (`Cx::tree()`), bindings are
wired against it (`palette::provided`, `types::declared`), and the store makes passes of
its own.

So whatever reads the document outside `Cx::read` has to be told by hand when to run
again, and the safe default is to redo it over the whole document. That one cause is
behind:

- the gate deciding again over every page (8 ms a frame, 2026-11-21) and its fix, a list
  of the ops that can change what it reads (`op::reshapes`);
- `regraph` (~200 lines), `Reactive::restructure`'s rederive and rescope,
  `Owners::affected` (~120), `sync_layout`, `sync_repeats`, `update_drawn` and the
  `prime_*` calls: rules, op by op, for what each edit could affect;
- the stamps and classifiers DS-3 added (`Docs::network_shaped`, `tokens_shaped`;
  `op::touches`, `rewires_network`, `retokens`) and the grouping reach
  (`solve::grouping_reach`): more of the same;
- the connections' signals and anchors' positions, recomputed whole on every rederive
  ("kept whole-document in 2b", never revisited).

Each rule is a guess that has to stay complete as features are added; the gate's was not.
Necessity and the gate's sweeps (handoff-scaling.md §3.1, §3.2) are a separate cause,
already designed there (§4.1, §4.3), and stay separate (§6).

## 2. The rule

**A kept result is a cell. A cell reads the document only through a recording reader,
which records every read, of structure and of facet contents as of leaves. A write
reports what it touched, and that marks stale exactly the cells that read it.**

- *Kept* means cached across transactions or frames: a cell's value, a binding's wiring,
  a list of cells to have.
- **One-off reads** may read the document directly: a command, export, the agent's view,
  a test, and the edit path itself (ops, expansion writing copies, the solver's
  write-back), which writes through the choke points (§3.3).
- **No rules in the store:** no list of which ops affect which computation. The one
  mapping from writes to keys is at the document's mutation choke points (§3.3), knowing
  nothing of features. The host's op rules (`geom::Revisions::note_ops`, `note_changes`)
  are DS-8's, out of this spec.

## 3. The mechanism (DS-9)

### 3.1 Keys

| Key | Recorded by | Fired by |
|---|---|---|
| `Node(n)` | `node`, `live`, `is_live`, `depth`, `is_ancestor`, `order_path`, a parent walk | create, tombstone, restore, remove, `set_parent`, `set_order` of `n` |
| `Children(n)` | `children` (authored and copies merged), `descendants` (each node walked) | a child placed or unplaced under `n`, in either document |
| `FacetSet(n)` | `has(n, kind)` (new: presence only), `facets_of` | a facet added to or removed from `n`, including by `restore_node` |
| `Facet(n, kind)` | `facet`, `get`, `leaf`, and `facets_of` for each facet it yields: presence **and content** | add, remove, or any field written (`facet_mut`) |
| `Relations(n)` | `relations_from`, `relations_to` (unfiltered, as the API is) | a relation with an end at `n` linked or unlinked |
| `Relation(id)` | `relation` | that relation linked or unlinked |
| `Rooted(owner)` | `rooted` | a relation of that owner linked or unlinked |
| `Type(t)`, `With(kind)` | `nodes_of_type`, `nodes_with` | the type or facet index's membership changed |
| `Roots` | `roots`, `last_root` | the root list changed |
| `Whole` | `nodes`, `live_nodes`, `relations`, `bound_slots`, `expanded_ids` | any structural write, and any slot made, replaced or cleared as an expression |
| `Wired` | the graph's topology as natives read it (`is_bound`, `transitive_deps`; `Cx::graph()`'s uses) | `Reactive::wired` changing |

- **`Facet` is coarse on purpose:** a facet read through `Tree` depends on the whole
  facet. A presence check reads `has`, which records `FacetSet` only, so a transform
  `Set` does not rerun a reader that asked only whether a transform is there (DS-10
  converts grouping's `facet(..).is_some()` reads, `solve.rs:275`, `constraint.rs:147`,
  `plug.rs:287`). A hot field read through `Tree` reruns its reader on every write; the
  cost test (§3.5) names it, and the fix is a leaf read through `Cx::read`.
- **`Cx::read` and `Cx::constant` stay per leaf.** `read_with` and `beneath`
  (`graph.rs:2676`, `418`) read through an unrecorded tree, and record only `Node(n)` and
  `FacetSet(n)`, so a leaf that comes into being (`Create`, `AddFacet`, undo's
  `restore_node`) reaches a reader that read it absent.
- **Scopes are deferred.** Every type and facet key is document-wide in DS-9; no DS-9 or
  DS-10 reader needs less. Per-composition indexes come with the first maker that needs
  them (DS-12).
- **`Whole`** is allowed only to cells on a named list; in tests a recording reader panics
  on it otherwise.

### 3.2 The recording reader

`Tree` gains an optional recorder; about 45 methods record. `Cx::tree()` hands a native a
recording `Tree`, so the code natives already run (layout, `constraint::all`,
`plug::implied_mates`, the network) records its reads unchanged.

- **Always recorded.** Values steer control flow (a `deref` target, a constraint's `kind`,
  an anchor's `port`, a layout mode), so every run records. The keys go into a reused
  buffer, sorted and hashed; key → readers changes only when the hash differs from the
  cell's last run. Correctness rests on exact key-set equality; the hash is only a fast
  path.

### 3.3 Writes: the touched-key log

The rollback journal is not the feed: it holds before-images, records nothing outside an
open transaction (`rec`, `document.rs:672`), and is dropped at commit; session ops,
loading and settling copies write outside it. Instead:

- **The mutation choke points** of `Document` (`insert_node`, `restore_node`, `tombstone`,
  `remove_node`, `set_parent`, `set_order`, `insert_facet`, `remove_facet`, `facet_mut`,
  `insert_relation`, `remove_relation`, and the type, facet and relation-end indexes) push
  §3.1's keys into an always-on **touched-key log**, in both documents, apart from the
  undo journal. `facet_mut` fires `Facet(n, kind)` only; `With` comes from the facet
  index, so a `Set` fires no `With`. `restore_node` fires `FacetSet` and `Facet` for every
  facet it removes or puts back.
- **A delete** fires `Node` for every node whose liveness changes (a tombstone keeps its
  facets and relations). `Docs::gone` stays as what readers of a gone node read.
- **The log's position is journaled:** `Docs::begin` records it and `Docs::rollback`
  truncates to it (`tree.rs:98`, `110`), so `run`'s error path and a refusal both leave
  nothing undelivered from the undone writes. `scratch()` clones it empty, as the journal.

**Drained**, at these points only, each before the first evaluation that can see the
writes:

1. `after_transaction`'s start, before the overlay is cleared (`lib.rs:4193`): the
   transaction's ops and expansion, once `run` has succeeded. Never inside `run`, whose
   checks can still roll the document back without the graph.
2. Each `settle_repeats` round, before its publish (`lib.rs:2477`).
3. After each cluster's write-back in `post_apply`, before the next cluster reads.
4. After `apply_recorded` and `release` (undo, redo and replay's copy writes).
5. After session ops commit.
6. Load: a clean baseline. The log is cleared and every cell is built fresh.

A drain hands the keys to the graph, which marks their readers stale and evaluates them in
the same pass as `invalidate` today.

**Refusal.** `Saved` is taken after `run` (`lib.rs:4323`). Keys drained after it reach
cells whose state `Saved` restores, and the journal's truncation drops any not yet
drained, so a refused transaction leaves no keys behind. `Saved` clones key → readers with
the rest of `Reactive`; DS-9 measures it. Journaling the graph instead of cloning it is
DS-7's.

### 3.4 The slice

DS-9 puts **existing, unpinned layout natives** on keys alone:

- `prime_layout`'s structural priming and `prime_reorders`' call to it are deleted
  (`lib.rs:4009`, `4044`); a layout native reruns because a key it recorded fired.
- **Kept on their path:** `layout_touched` and `sync_layout`, which decide which roots
  exist (maker work, DS-12); `Pins`, whose `constrained` scans every `use`
  (`layout.rs:542`) and would record document-wide keys.
- Everything else keeps its rules. The mechanism runs beside them, in shadow (§3.5).

### 3.5 Verification

The differential suite compares two incremental stores that keep copies differently
(`differential/mod.rs:137`). It is not a rebuild from scratch, and a missed key in shared
code passes it. So DS-9 adds, under a feature of its own (`structure-oracle`), once per
operation:

- **A reference for the slice:** every layout native rebuilt from the current tree and
  run on a scratch `Reactive` with glides off, its claims (placements), errors and extra
  (table grids) compared with the kept native's.
- **Keys predict, rules act:** while a rule still acts, each cell it marks and the keys do
  not is rerun on a scratch `Reactive` and compared the same way; a difference is a key
  gap, reported with the op. This finds gaps before each later phase deletes its rule.
- **A canary:** a test build that drops one kind of key makes the reference comparison
  fail. If it does not, the oracle is not checking.
- **Cost:** key visits, index updates and retained keys per cell run and per frame,
  native and wasm. Budget: `single-tab.txt` drag `advance` p50 under 0.1 ms in wasm (0.04
  today); a drag inside an observed 800-node stack, proven to rerun its layout, and the
  same stack's preview, each within 1.3× of today; the `Saved` clone with recorded keys.
- **No slope:** the scaling test at both axes. A row a phase claims to retire is checked
  by trace (`trace_one_count`), since another scan can keep a row alive.

**DS-9's exit:** the reference and the shadow silent over the suite; the canary fails as
it should; the cost within budget; `prime_layout`'s priming gone. DS-9's commit maps every
`SCALING` row tagged DS-3b to the phase that retires it.

## 4. DS-9's size

| | Lines |
|---|---|
| Added: keys, the recording `Tree`, hashing, key → readers | ~350–450 |
| Added: the touched-key log, its journaled position, the drains | ~150–200 |
| Added: the oracle, reference and canary (feature-gated) | ~200–300 |
| Removed: `prime_layout`'s structural priming, `prime_reorders`' call | ~30 |

DS-9 adds a mechanism and deletes almost nothing; the deletions come in DS-10 to DS-14.
**The whole plan, from the code at `08e0539`:** about 900–1,100 lines removed and
1,300–2,000 added before tests, so a net of roughly **+400 to +1,100**. What that buys is
one mechanism in place of many rules: today's are spread over a dozen functions in four
files and must each be kept complete as features change; afterwards a new kind of kept
result needs no rule, and a missed dependency fails the oracle instead of showing a stale
result.

## 5. Later phases, and what each design must answer

Each is designed in a short addendum, reviewed at its group (§8), then built. Each deletes
a rule only in the phase that replaces it, after the shadow (§3.5) has been silent for it.

| Key | Builds | Must answer (council rounds 1 and 2) |
|---|---|---|
| DS-10 | Grouping and validation as recorded cells; `prime_grouping` and the grouping reach deleted | Whether `Wired` is fine enough: a `transitive_deps` walk must hear of a rewired intermediate leaf, unbound ones included |
| DS-11 | Bindings: wiring as a kept result; the theme publish and the network as cells; rescope, `token_readers_by`, the definition rule, and the network and token stamps deleted | Who makes and drops binding cells (a drained `Facet` diffed against its bound slots, `facet_mut` hands out `&mut`); `@name` keyed by a name → providers index, not `provided`'s walk to the roots, which would record `Roots`; the `tokens` map passed in from outside the recorder; wiring and evaluation subscribed apart, wiring committed through the edge, cycle and height path |
| DS-3c | Heights by `raise` along the cone (handoff-scaling.md) | As planned there |
| DS-12 | The maker; `layout_touched`, `sync_layout`, `sync_repeats`, `update_drawn`, `sync_islands`, `sync_cluster_natives` deleted; per-composition indexes | Cells made mid-pass; a maker pulled when a child or reservation does not yet exist, its necessity and height, kept out of the gate; reservations reconciled through the batched claim handoff (`graph.rs:2502`); a shown cell reading the copies of a repeat in a paused page |
| DS-13 | The connections' signals and anchors' positions as one recorded cell; `restructure`'s rederive deleted | Split per connection only if the scaling test still names it |
| DS-14 | Expansion records what it read; `Owners::affected` and `by_def`'s rule deleted | Template cache hits must carry their dependencies before `Document::touch`'s scope stamps go |

Out: splitting grouping per composition (`solve.rs:228` unions operands across
compositions; reactive-engine.md §9.4 leaves the boundary unproved); clip cells (PT-9;
DS-2's running-clip path stands).

## 6. Necessity and the gate

handoff-scaling.md §4.1 (necessity as counts) and §4.3 (the gate as a traversal) stand as
DS-4a, DS-4b and DS-5, and may land any time after DS-9. **Structural keys are neither
necessity edges nor gate edges:** reading another page's structure does not make its
cells needed or wake it; only leaf reads count, as §4.1 and §4.3 say. The gate's stamp
(`Docs::reshaped`, `op::reshapes`) goes with DS-5.

## 7. Undo, redo and refusal

Transactions apply in place (RE-2a, `d9ec4ea`): `Document`'s mutators write a journal that
`abort` undoes in reverse, checked under `differential` against a shadow copy taken at
`begin`. A refusal in `post_apply` also restores the graph and store state from `Saved`,
cloned per transaction when the document holds a constraint (`Saved::of`). Undo and redo
apply a recorded transaction's inverse or ops through `run`.

- **Undo and redo** are edits: their writes go through the choke points and fire keys like
  any other, drained at §3.3's points 1 and 4.
- **The journal** stays the undo mechanism. The log's position is journaled with it
  (§3.3).
- **`Saved`** clones key → readers with the graph. DS-9 measures it; if it matters, DS-7
  (refusal by journal and re-derivation, no snapshot) comes forward, journaling the
  graph's own changes as the document's are.

## 8. Council

Reviews by group, both members (Opus 5.5 and Sol 6.2, high), read-only:

1. **This spec**, before any code (rounds 1 to 3).
2. **DS-9 built:** whether recorded reads are complete is the riskiest claim and the
   cheapest to test here. The code of DS-2 and DS-3 that stays (indexes, event-driven
   players) is reviewed with it.
3. **The DS-10 and DS-11 designs, then their code.**
4. **DS-3c and DS-12,** design then code.
5. **DS-13 and DS-14,** with DS-4 and DS-5 if they have landed; the rest with DS-6 to
   DS-8.

## 9. Risks

- **Recording's cost.** A layout over 500 nodes makes 2.5k–5k reads, ~50–150 µs to sort
  and hash (an estimate); updating key → readers costs more, and the hash check skips it
  on value-only reruns. If over budget: smaller keys, or hot fields read as leaves.
- **Coarse keys.** `Facet` content, `Whole` and unscoped type reads rerun readers more
  than a leaf would: visible and named by the tests, never stale.
- **Escapes.** A kept result outside `Cx` escapes the rule. DS-9 lists every kept set in
  the store as a cell, a one-off, or one of DS-4/5's, and the shadow reruns what the rules
  mark.
- **Scope.** The engine's core again. Grouping's split, clips and the host are out.

## 10. Council

**Round 1 (2026-11-22): Opus 5/10, Sol 5/10.** Both: the diagnosis holds, the rule is
right. Adopted: facet contents are a key; a touched-key log, not the rollback journal;
binding wiring and the graph's topology recorded; always record, hash and skip; the
oracle reruns cells rather than trusting the rules; each rule deleted with its
replacement; necessity and the gate decoupled; clips and grouping's split deferred; the
size estimate was low.

**Round 2 (2026-11-22): Opus 6/10, Sol 6/10.** Both: DS-9's slice could not delete
`layout_touched`, which is maker work; the oracle could pass with a key missing (the
differential suite is two incremental stores, not a rebuild); drains inside `run` would
evaluate on a document that can still roll back without the graph; scoped indexes belong
with makers. Adopted: DS-9 narrowed to existing, unpinned layout natives (§3.4); an
independent reference, a shadow and a canary (§3.5); drains only at §3.3's points, the
log's position journaled; `has` for presence and `Cx::read` per leaf (§3.1); `Bound` and
`Reads` folded into `Wired`; per-unit connection and anchor cells cut (DS-13); later
phases' open questions listed for their own designs (§5). Fixed in code at once: `Saved`
now restores the layout roots, pinned parts and glide nodes DS-3b part 2 added
(`tests/rollback.rs`, a layout made in a refused transaction).
