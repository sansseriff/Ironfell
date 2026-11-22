# Structure in the graph: one reader of the document

**Status:** draft, revised 2026-11-22 after council round 1 (Opus 5/10, Sol 5/10; §9).
Not adopted. It changes the order of [`handoff-scaling.md`](../handoff-scaling.md) (the
`DS` track): DS-3b stops at part 2 (`08e0539`), and this spec's phases, DS-9 to DS-14,
replace the rest of DS-3b. DS-4a, DS-4b and DS-5 stay as planned there and do not wait
for these (§3.6). It replaces reactive-engine.md §7.2's table of what each op affects.

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

- the gate deciding again over every page (8 ms a frame, 2026-11-21) and its fix, a
  list of the ops that can change what it reads (`op::reshapes`);
- `regraph` (~200 lines), `Reactive::restructure`'s rederive and rescope, `Owners::affected`
  (~120), `sync_layout`, `sync_repeats`, `update_drawn` and the `prime_*` calls: rules, op
  by op, for what each edit could affect;
- the stamps and classifiers DS-3 added (`Docs::network_shaped`, `tokens_shaped`;
  `op::touches`, `rewires_network`, `retokens`) and the grouping reach
  (`solve::grouping_reach`): more of the same;
- cells kept whole because nothing could tell what part changed: the connections'
  signals and anchors' positions ("kept whole-document in 2b", never revisited).

Each rule is a guess that has to stay complete as features are added; the gate's was not.
Necessity and the gate's sweeps (handoff-scaling.md §3.1, §3.2) are a separate cause,
already designed there (§4.1, §4.3), and stay separate (§3.6).

## 2. The rule

**A kept result is a cell. A cell reads the document only through a recording reader,
which records every read, of structure and of facet contents as of leaves. A write
reports what it touched, and that marks stale exactly the cells that read it.**

- *Kept* means cached across transactions or frames: a cell's value, a binding's wiring,
  a list of cells to have.
- **One-off reads** may read the document directly: a command, export, the agent's view,
  a test, and the edit path itself (ops, expansion writing copies, the solver's
  write-back), which writes through the choke points (§3.3).
- **No hand rules:** no list of which ops affect which computation. The one mapping from
  writes to keys is at the document's mutation choke points (§3.3), knowing nothing of
  features.

## 3. Design

### 3.1 Keys

| Key | Recorded by | Fired by |
|---|---|---|
| `Node(n)` | `node`, `live`, `is_live`, `depth`, `is_ancestor`, `order_path`, a parent walk | create, tombstone, restore, remove, `set_parent`, `set_order` of `n` |
| `Children(n)` | `children` (authored and copies merged), `descendants` (each node walked) | a child placed or unplaced under `n`, in either document |
| `FacetSet(n)` | `facets_of` | a facet added to or removed from `n` |
| `Facet(n, kind)` | `facet`, `get`, `leaf`: presence **and content** | add, remove, or any field written (`facet_mut`) |
| `Relations(n)` | `relations_from`, `relations_to` (unfiltered, as the API is) | a relation with an end at `n` linked or unlinked |
| `Relation(id)` | `relation` | that relation linked or unlinked |
| `Rooted(owner)` | `rooted` | a relation of that owner linked or unlinked |
| `Type(t, scope)`, `With(kind, scope)` | `nodes_of_type`, `nodes_with`, scoped or not | membership of the type or facet index in that scope changed |
| `Roots` | `roots`, `last_root` | the root list changed |
| `Whole` | `nodes`, `live_nodes`, `relations`, `bound_slots`, `expanded_ids` | any structural write |
| `Bound(leaf)`, `Reads(leaf)` | the graph's own topology: whether a leaf is bound or claimed, what it reads (`Cx::graph()`'s uses, `transitive_deps`) | a cell made, dropped or rewired; a claim moved |

- **`Facet` is coarse on purpose:** a facet read through `Tree` depends on the whole
  facet, so any write to it fires the key. A hot field read that way (a transform read
  every frame of a drag) reruns its reader on every write; the cost test (§6) names it,
  and the fix is to read it as a leaf through `Cx::read`, which stays per leaf.
- **A scope** is a composition, or the document. `nodes_of_type` and `nodes_with` gain
  scoped forms served by per-composition indexes. An unscoped read records the
  document-wide key: allowed, and the scaling test names it.
- **`Whole`** is allowed only to cells on a named list; in tests a recording reader
  panics on it otherwise.
- **`Cx::graph()`** goes. What natives read of the graph's topology becomes tracked
  accessors (`Cx::bound`, `Cx::driven`, `Cx::reads`), recording `Bound` and `Reads` keys.
  Overlay, previews and claims keep their existing input paths.

### 3.2 The recording reader

`Tree` gains an optional recorder. `Cx::tree()` hands a cell a recording `Tree`, so the
code natives already run (`constraint::all`, `plug::implied_mates`, layout, the network)
records its reads unchanged. About 45 `Tree` methods record.

- **Bindings too.** A binding's **wiring** (its deps, its `@name` context from
  `palette::provided`, the port types `types::declared` reads) is a kept result: it is
  made through a recording reader, and its keys rewire it when they fire. Evaluation's
  structural reads (`deref` into items, `types::declared` at evaluation) record into the
  binding's keys. `Rescope`, `token_readers`, `token_readers_by` and `regraph`'s
  definition rule go (DS-11).
- **Always recorded.** Values steer control flow (a `deref` target, a constraint's
  `kind`, an anchor's `port`, a layout mode), so every run records. The keys go into a
  reused buffer, sorted and hashed; key → readers changes only when the hash differs from
  the cell's last run, so a value-only rerun pays the recording but not the index update.

### 3.3 Writes: a touched-key log

The rollback journal is not the feed: it holds before-images, records nothing outside an
open transaction (`rec`, `document.rs:672`), and is dropped at commit; session ops,
loading and settling copies write outside it. Instead:

- **The mutation choke points** of `Document` (`insert_node`, `restore_node`, `tombstone`,
  `remove_node`, `set_parent`, `set_order`, `insert_facet`, `remove_facet`, `facet_mut`,
  `insert_relation`, `remove_relation`, and the type, facet and relation-end indexes)
  push the keys of §3.1's last column into an always-on **touched-key log**, in both
  documents, apart from the undo journal. `facet_mut` fires `Facet(n, kind)` only; index
  keys come from index changes only, so a transform `Set` fires no `With` key.
- **A move** fires `Node(n)` and both child lists. When it changes the moved subtree's
  composition, the subtree's scoped index entries move (the cost of the subtree) and both
  scopes' keys fire.
- **A delete** fires `Node` for every node whose liveness changes (a tombstone keeps its
  facets and relations, so liveness is the key that tells). `Docs::gone` stays as what
  readers of a gone node read.
- **Drained** at each settlement boundary: after a transaction's ops, each expansion
  round, the solver's write-back, a release, session ops, load. The store hands the keys
  to the graph, which marks their readers stale.
- **Refusal:** today the `Saved` snapshot restores the graph, so the log since `begin` is
  dropped with it. When DS-7 replaces the snapshot, rollback fires the keys it undoes.

### 3.4 Cells that make cells

What cells exist is a kept result: one per layout root, repeat, island, cluster.
Today each has a sync function fed by a rule. Under the rule each is a **maker**: a cell
that reads scoped keys and keeps a child cell per member, made, kept or dropped by
difference.

- A maker makes cells **during a pass**: their heights by `raise`, cycles checked and
  disposal done as it runs. So DS-3c (heights by `raise`, the fixpoint as an oracle)
  comes first.
- A kept child whose inputs changed has its output reservations reconciled (a layout
  root whose children changed keeps its identity, not its outputs).
- A needed child keeps its maker needed (the child reads its maker's membership), and a
  pull into a hidden page runs its makers first.

### 3.5 Per-unit cells

- **Connections and anchors:** a cell per connection for its derived signal, and per
  anchor for its position and following (reactive-engine.md §9.1), made by makers;
  `restructure`'s rederive goes (DS-13). The scaling test's reparent row names them.
- **Grouping keeps its unit for now.** It becomes one cell with recorded reads, rerun
  only when what it read changes, with the grouping reach and its priming deleted (DS-10).
  Splitting it, per composition as reactive-engine.md §9.4 proposed, is not designed:
  grouping unions shared operands across compositions and checks solve order across
  clusters (`solve.rs:228`), and §9.4 leaves the composition boundary unproved. A later
  design, if the scaling test still names it.
- **Clips stay with PT-9.** DS-2's running-clip path stands; `resample` after structure
  stays whole until then.

### 3.6 Necessity and the gate

handoff-scaling.md §4.1 (necessity as counts) and §4.3 (the gate as a traversal) stand,
as DS-4a, DS-4b and DS-5, and do not wait for these phases. **Structural keys are neither
necessity edges nor gate edges:** reading another page's structure (an embed's metadata)
does not make its cells needed or wake it; only leaf reads count, as §4.1 and §4.3 say.
The gate's stamp (`Docs::reshaped`, `op::reshapes`) goes with DS-5, not before.

## 4. Size

Rough, from the code at `08e0539`. Tests not counted; this adds about 600 to 900 lines of
them (§6).

| Removed | Lines |
|---|---|
| `regraph`'s rules and the `prime_*` calls | ~180 |
| `restructure`'s rederive and rescope; `token_readers_by` | ~130 |
| `sync_layout`, `sync_repeats`, `update_drawn`, `sync_islands`, `sync_cluster_natives` | ~230 |
| The classifiers and stamps (`op::touches`, `rewires_network`, `retokens`, `network_shaped`, `tokens_shaped`) | ~80 |
| `grouping_reach`, `settle_repeats`' own tracking | ~130 |
| `Owners::affected`, `Document::touch`'s scope stamps (DS-14) | ~170 |
| **Total** | **~900–1,100** |

| Added | Lines |
|---|---|
| Keys, the recording `Tree` (~45 methods), hashing, key → readers | ~450–600 |
| The touched-key log at the choke points, scoped indexes, subtree reindexing on a move | ~250–350 |
| Binding wiring as a kept result; `Cx`'s topology accessors | ~200–300 |
| The maker: cells made mid-pass, reservations, maker necessity | ~250–350 |
| Connection and anchor cells; expansion's recorded reads (DS-14) | ~250–350 |
| **Total** | **~1,400–1,950** |

**Net: about 300 to 850 lines more.** What it buys is not fewer lines but one mechanism
in place of many rules: today's are spread over a dozen functions in four files and must
each be kept complete as features change; afterwards a new kind of kept result needs no
rule at all, and a missed dependency is a test failure (§6), not a stale screen.

## 5. Phases

Each phase is committed alone, passes the differential suite (with `IRON_OWNER_CHECK`),
the scaling test and the oracle (§6), and retires its `SCALING` pairs. A rule is deleted
in the phase that replaces it, never before.

| Key | Builds | Exit |
|---|---|---|
| DS-9 | Keys, the recording `Tree`, the touched-key log, key → readers, the oracle (§6); on beside the rules. One vertical slice: layout's rule (`prime_layout`, `layout_touched`) deleted, layout natives invalidated by keys alone | Oracle silent over the suite; the slice's rule gone; recording's cost measured (§6) within budget |
| DS-10 | The priming rules deleted: network and token stamps and classifiers, `prime_grouping`, the grouping reach, `prime_reorders`; grouping one recorded cell | Differential, scaling and oracle pass with them gone |
| DS-11 | Binding wiring recorded; `Cx::graph()` replaced; rescope, `token_readers_by` and the definition rule deleted | `let made`, `theme` lose their rescope counters |
| DS-3c | Heights by `raise` along the cone (handoff-scaling.md), before makers | As planned |
| DS-12 | The maker; the sync functions deleted | Structural edit and reparent lose their sync counters |
| DS-13 | Connection and anchor cells; rederive deleted | Reparent and connect lose their counters |
| DS-14 | Expansion records what it read (templates, definitions, ancestors); `Owners::affected`, `by_def`'s rule and `Document::touch`'s scope stamps deleted | Owners' oracle passes with them gone |

DS-4a, DS-4b and DS-5 may land at any point after DS-9; DS-6 to DS-8 follow as planned.
**Final exit:** handoff-scaling.md §6's: every operation flat at both axes, store and
host.

## 6. Verification

- **The oracle (DS-9 on, under `differential`):** after every settlement boundary, every
  cell *not* marked stale is rerun on a fresh recording reader, and must give the same
  output, the same errors and the same keys; every maker's children must equal what a
  rebuild makes. This checks recorded reads against the code, not against the rules
  being deleted, and catches reads that escape the recorder (store state, the graph's
  topology). The rules are kept as a diagnostic only (they over-fire).
- **The rebuild from scratch:** the differential suite, as today.
- **No slope:** the scaling test at both axes, plus an **active** large page: a drag
  inside an observed layout of 800 nodes, so recording's cost on a large cell is
  measured, not only hidden growth.
- **Recording's cost:** per cell run and per frame, native and wasm, counted as key
  visits, index updates and retained keys. Budget: `single-tab.txt` drag `advance` p50
  under 0.1 ms in wasm (0.04 today), and the active large-page drag within 1.3× of today.

## 7. Risks

- **Recording's cost.** A layout over 500 nodes makes 2.5k–5k reads, ~50–150 µs to sort
  and hash; updating key → readers would cost far more, which the hash check skips on
  value-only reruns (estimates; DS-9 measures). If over budget: smaller keys, a cell
  recording once per pass, or reading hot fields as leaves (§3.1).
- **Coarse keys.** `Facet` content and unscoped type reads rerun readers more than a leaf
  would; visible and named by the tests, never stale.
- **Escapes.** A kept result outside `Cx` (store state) escapes the rule; the oracle
  reruns clean cells, and DS-9 lists every kept set in the store as a cell, a one-off, or
  one of DS-4/5's.
- **Scope.** The engine's core again. Grouping's unit, clips and the host are out (§3.5,
  DS-8).

## 8. Council

Reviews by group, both members (Opus 5.5 and Sol 6.2, high), read-only:

1. **This spec**, before any code (rounds 1, 2).
2. **DS-9 and DS-10:** whether recorded reads are complete is the riskiest claim and the
   cheapest to test here. The code of DS-2 and DS-3 that stays (indexes, event-driven
   players) is reviewed with it.
3. **DS-11, DS-3c and DS-12:** bindings, heights, makers.
4. **DS-13 and DS-14,** with DS-4 and DS-5 if they have landed.
5. **The rest,** with DS-6 to DS-8.

## 9. Council round 1 (2026-11-22): Opus 5/10, Sol 5/10

Both: the diagnosis holds, the rule is right. Both found, and this revision adopts:

- facet contents read through `Tree` escaped both kinds of key (§3.1: `Facet` is
  content);
- the rollback journal is no change feed: before-images, transactions only, dropped at
  commit (§3.3: the touched-key log);
- bindings' wiring and evaluation, and natives' reads of the graph, ran outside `Cx`
  (§3.1, §3.2);
- recording only on structural change is unsound, since values steer reads (§3.2: always
  record, hash and skip);
- rules as the oracle over-fire and cannot find escapes (§6: clean cells rerun equal);
- delete each rule with its replacement; decouple necessity and the gate (§3.6, §5);
- defer clip cells and grouping's split (§3.5); the line estimate was low (§4).

Opus also: makers need heights by `raise` first and necessity of their own (§3.4);
structural keys are not gate edges (§3.6); the key table's gaps (§3.1). Sol also:
measure an active large page (§6); a layout root's reservations when its children change
(§3.4); a vertical slice that retires a real rule (DS-9).
