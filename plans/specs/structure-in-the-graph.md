# Structure in the graph: one reader of the document

**Status:** adopted 2026-11-22 by the user, after council rounds 1 (Opus 5/10, Sol 5/10),
2 (Opus 6/10, Sol 6/10) and 3 (Opus 7/10, Sol 7/10); §10. **DS-9 built** (§11). It changes the order of
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

Line numbers below are as of `13c6500`; functions are named so they survive edits.

### 3.1 Keys

DS-9 has only the keys its slice reads. Each later phase adds the keys its cells need,
with its own oracle (§5).

| Key | Recorded by | Fired by |
|---|---|---|
| `Node(n)` | `node`, `live`, `is_live`, `depth`, `is_ancestor`, `order_path`, a parent walk | create, tombstone, restore, remove, `set_parent`, `set_order` of `n` |
| `Children(n)` | `children` (authored and copies merged), `descendants` (each node walked) | a child placed or unplaced under `n`, in either document |
| `FacetSet(n)` | `has(n, kind)` (new: presence only), `facets_of` | a facet added to or removed from `n`, including by `restore_node` |
| `Facet(n, kind)` | `facet`, `get`, `leaf`, and `facets_of` for each facet it yields: presence **and content** | add, remove, or any field written (`facet_mut`) |

- **Any other `Tree` method on a recording tree panics under test** (relations, type and
  facet indexes, roots, whole-document iterators): a read DS-9 has no key for is caught,
  not missed.
- **`Facet` is coarse on purpose:** a facet read through `Tree` depends on the whole
  facet. A presence check reads `has`, which records `FacetSet` only. DS-9 converts the
  slice's own presence reads to `has` (`layout::lays_out`, `geom::local_affine`, the
  `LayoutItem` checks). A hot field read through `Tree` reruns its reader on every write;
  the fix is a leaf read through `Cx::read`.
- **`Cx::read` and `Cx::constant` stay per leaf.** `read_with` and `beneath` read through
  an unrecorded tree and record only `Node(n)` and `FacetSet(n)`, so a leaf that comes into
  being (`Create`, `AddFacet`, undo's `restore_node`) reaches a reader that read it absent.

### 3.2 The recording reader

`Tree` gains an optional recorder. **In DS-9 only `NativeKey::Layout` cells whose layout is
not pinned get a recording tree**, and keys act only on them; every other cell (`Pins`,
pinned layouts, `Clusters`, `Cluster`, `Validate`, islands, repeats) runs as today.

- **Always recorded.** Values steer control flow (a layout mode, a `deref` target), so
  every run records, into a reused buffer, sorted and deduplicated. key → readers is
  updated only when the list differs from the cell's last run (an exact comparison).
- **Dropped with the cell:** `set_natives` removes a dropped or replaced native's
  key → readers entries, as it does its `dependents`; `Reactive::rebuild` clears them.

### 3.3 Writes: the touched-key log

The rollback journal is not the feed: it holds before-images, records nothing outside an
open transaction (`Document::rec`), and is dropped at commit; session ops, loading and
settling copies write outside it. Instead:

- **The mutation choke points** of `Document` (`insert_node`, `restore_node`, `tombstone`,
  `remove_node`, `set_parent`, `set_order`, `insert_facet`, `remove_facet`, `facet_mut`)
  push §3.1's keys into an always-on **touched-key log**, in both documents, apart from the
  undo journal. `facet_mut` fires `Facet(n, kind)` only. `restore_node` fires `FacetSet`
  and `Facet` for every facet it removes or puts back. A delete fires `Node` for every node
  whose liveness changes.
- **A drain only marks:** it puts the readers of the drained keys into the graph's
  `pending`, as `prime_natives` does, and evaluates nothing itself.
- **Drained where the graph is primed today, and only there:** in `regraph`, after
  `sync_layout` and `sync_repeats`, where `prime_layout` is called now; and just before the
  non-structural `invalidate` calls (in `after_transaction`'s else branch, the session
  path, and `apply_recorded`/`release`). So outputs are reconciled before any keyed
  native runs, and nothing is drained inside `run`. `settle_repeats`, `apply_recorded`,
  `release`, undo, redo and session ops all reach one of these. A session reorder, whose
  changed-leaf list is empty, still evaluates once after its drain.
- **Empty at `begin`:** the log is asserted empty at `Docs::begin` and cleared by
  `Docs::rollback`, so a refused or failed transaction's keys never reach the graph, and
  nothing about the log needs journaling. `scratch()` clones it empty. Load clears it and
  builds every cell fresh.

**Refusal.** `Saved` is taken after `run` and restores the graph, key → readers with it.
Its cost with recorded keys is reported, not a gate. Two refusal paths are covered by
tests: a refusal after a key-driven layout run, and a solver refusal through implied
mates with no `constraint` node, where `can_refuse` is false today and the fallback
rebuilds from the attempted edit's natives; DS-9 makes `can_refuse` count plugged anchors
too.

### 3.4 The slice

DS-9 puts **existing, unpinned layout natives** on keys alone:

- `regraph`'s call to `prime_layout` primes only `Pins` and the pinned layouts on the walk;
  unpinned ones rerun because a key they recorded fired. `prime_reorders`, which only calls
  `prime_layout`, likewise keeps only the pinned part.
- **Kept on their path:** `layout_touched` and `sync_layout`, which decide which roots
  exist and reserve their outputs (maker work, DS-12); `Pins` and pinned layouts, whose
  `constrained` scans every `use`.

### 3.5 Verification

The differential suite compares two incremental stores that keep copies differently
(`differential/mod.rs`, `call`). It is not a rebuild from scratch, and a missed key in
shared code passes it. So DS-9 adds, under a feature of its own (`structure-oracle`), once
per operation:

- **A reference for the slice:** after the facade's comparison, for every
  `layout::structural_roots(tree)`, `layout::lay_out(&StoreCore, root)` (the store
  implements `Reads` with `Cx`'s `layout_base`) is compared with the kept native's claims
  (as glide targets, not eased values), errors and table grids. It re-derives which roots
  exist as well, with no scratch store.
- **A canary:** a test build with one choke point silenced (`set_order`, then
  `insert_facet`) must fail the reference on two named tests: a sibling reorder in a stack
  with glides off, and a `layout_item` facet added. If it does not, the oracle is not
  checking.
- **Cost, native release and wasm:** the median per-frame time (`advance` plus the
  pipeline) of a drag inside an observed 800-node stack, shown to rerun its layout, and the
  median `preview` call on that stack, each within 1.3× of `13c6500`; `single-tab.txt` drag
  `advance` p50 under 0.1 ms in wasm (0.04 today).
- **No slope:** the scaling test at both axes.

**DS-9's exit:** the reference silent over the suite; the canary failing both tests; the
cost within budget; unpinned layouts primed by keys alone.

## 4. DS-9's size

| | Lines |
|---|---|
| Added: four keys, the recording `Tree`, key → readers, `has` | ~250–350 |
| Added: the touched-key log, its drains, the `begin` assertion | ~100–150 |
| Added: the reference and the canary (feature-gated) | ~80–150 |
| Removed: unpinned priming from `prime_layout` and `prime_reorders` | ~20 |

DS-9 adds a mechanism and deletes almost nothing; the deletions come in DS-10 to DS-14.
**The whole plan, from the code at `08e0539`:** about 900–1,100 lines removed and
1,300–2,000 added before tests, so a net of roughly **+400 to +1,100**. What that buys is
one mechanism in place of many rules: today's are spread over a dozen functions in four
files and must each be kept complete as features change; afterwards a new kind of kept
result needs no rule, and a missed dependency fails the oracle instead of showing a stale
result.

## 5. Later phases, and what each design must answer

Each is designed in a short addendum, reviewed at its group (§8), then built. Each deletes
a rule only in the phase that replaces it, after that phase's oracle has been silent for it.

| Key | Builds | Must answer (council rounds 1 and 2) |
|---|---|---|
| DS-10 | Grouping and validation as recorded cells, with the keys they read (relations, types, `Wired` for the graph's topology) and a shadow: each cell a rule marks and the keys do not is rerun, a difference being a key gap; `prime_grouping` and the grouping reach deleted | Whether `Wired` is fine enough: a `transitive_deps` walk must hear of a rewired intermediate leaf, unbound ones included |
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
- **The journal** stays the undo mechanism. The touched-key log is empty at every
  `begin` and cleared by rollback (§3.3).
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
  and compare (an estimate); updating key → readers costs more, and the comparison skips
  it on value-only reruns. If over budget: smaller keys, or hot fields read as leaves.
- **Coarse keys.** `Facet` content, `Whole` and unscoped type reads rerun readers more
  than a leaf would: visible and named by the tests, never stale.
- **Escapes.** A kept result outside `Cx` escapes the rule. Each phase lists the kept sets
  in the store it touches as a cell, a one-off, or one of DS-4/5's, as it puts cells on keys;
  each phase's oracle reruns what its rules mark.
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

**Round 3 (2026-11-22): Opus 7/10, Sol 7/10, on DS-9 alone.** Both: draining before
`sync_layout` would run old layout natives against new children (an output assertion),
so a drain only marks, where the graph is primed today; `prime_layout` is also the only
structural trigger for `Pins` and pinned layouts, which keep it; the oracle's reference
must not share the invalidation it checks; the canary must silence a choke point, not a
key kind. Opus: keys act only on unpinned layout cells in DS-9; the reference is
`layout::lay_out` on the store; an empty log at `begin` instead of a journaled position;
the shadow moves to DS-10; the key table shrinks to the four the slice uses; no hash.
Sol: refusal through implied mates with no constraint node; a session reorder still
evaluates; cost budgets named. All adopted (§3, §4).

## 11. DS-9 as built (2026-11-22)

- **Keys and the recording tree** (`structure.rs`, `tree.rs`): `Node`, `Children`,
  `FacetSet`, `Facet`; `Tree::recording`; `Tree::has` for presence. Every other `Tree` read
  on a recording tree fails a debug assertion (`unkeyed`).
- **The touched-key log** (`document.rs`): noted in `write_node`, `insert_node`,
  `remove_node`, `restore_node`, `place`, `unplace`, `insert_facet`, `remove_facet`,
  `facet_mut`. On only in a store's two documents (`Docs::log_touched`, at load);
  asserted empty at `Docs::begin`, cleared by `Document::rollback`.
- **The graph** (`graph.rs`): `Native::keyed` (unpinned `LayoutNative` only); `Cx::tree`
  hands a keyed native a recording tree, and `Cx::read`/`constant` note `Node` and
  `FacetSet` for the leaf's node; `run_native` keeps a run's keys sorted and deduplicated
  and updates `key_readers` only when they differ; `set_natives` drops a native's keys;
  `prime_keys` marks only. `Reactive::rebuild` keeps `key_readers`, unlike §3.2 said:
  natives survive a rebuild, and their keys with them.
- **Drains** (`Store::drain_touched`): in `regraph` after `prime_layout`; before the
  non-structural `invalidate` in `after_transaction`, the session path (which now
  evaluates once when a reorder set no leaf) and each cluster's write-back in
  `post_apply`; at the end of load.
- **The slice:** `prime_layout` primes `Pins` and pinned layouts only.
- **The oracle** (feature `structure-oracle`, implying `differential`):
  `StoreCore::check_keyed_layouts`, after every facade call, lays out every structural
  root from the store and compares each current, keyed native's claims and grids;
  skipped while a glide runs. **The canary** silences `set_order`
  (`silence_for_canary`) and must fail it (`layout::canary_a_silenced_reorder_fails_the_oracle`).
  A reorder is the only write that reaches layouts by keys alone in DS-9: a create,
  delete, move or facet change also reaches them through `sync_layout` replacing the
  native or `restructure`'s leaf priming, so a canary on `insert_facet` or `facet_mut`
  cannot fail (tried; both passed). With `prime_keys` disabled, two layout tests fail
  (the reorder tests), which is what the keys alone carry today.
- **Refusal:** `can_refuse` counts live anchors, so a mate implied by a connection
  refuses through `Saved` (test: `anchors::plugging_two_rigid_ports_mates_their_parts`'
  end); a keyed layout run in a refused transaction is put back
  (`rollback::a_keyed_layout_run_in_a_refused_transaction_is_put_back`).
- **Cost** (native release, `cells_demo`, p50 per change, before → after): a 1,000-item
  stack, edit one 3.08 → 3.34 ms, preview one 3.09 → 3.34 ms, reorder one 3.28 → 3.54 ms
  (+8%, within 1.3×); 1,000 containers read by one edit 47.4 → 50.0 ms; small cases within
  noise.

- **Checks:** plain (488, fonts 37), differential with `IRON_OWNER_CHECK` (492), the oracle
  build (492, fonts 38 with the canary), the scaling test, smoke, regression, and
  `single-tab.txt` in wasm (drag `advance` p50 0.03 ms, under the 0.1 ms budget; playing
  0.09 ms) pass.
