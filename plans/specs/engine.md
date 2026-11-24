# The Iron Engine: Unified Architecture

**Status:** Adopted 2026-11-22; **revised 2026-11-23** after reverting the universal patch
algebra experiment (`EN-3`–`EN-5c`, see
[`../notes/postmortem-universal-patch-algebra.md`](../notes/postmortem-universal-patch-algebra.md)).
Consolidates the engine work into one cohesive, legible architecture.
**Work order and status:** [`../handoff-engine.md`](../handoff-engine.md) (keys `EN-n`).
**Companion specs:** [`patches.md`](patches.md) (discrete variation, semantic provenance,
and node inspectability), [`patch-storage.md`](patch-storage.md) (Stage 4 storage and
partial loading, draft), [`document-spine.md`](document-spine.md) (ops, transactions, ids,
canonical JSON).

---

## 1. Why one engine

`iron_document` evolved through three stacked eras:

1. **Imperative store** — `regraph`, `resolve_physics`, six pipeline entry points, and
   separate players for machines, clips, steps, queries, and presence.
2. **Reactive cells and owners** — a height-ordered cell graph, native cells, owner
   trees, shared templates, and a materialized second document (`Docs::copies`).
3. **Incremental scaling fast paths** — hand-written op classifiers, dirty stamps, and
   partial structural-key tracking.

Stacking these eras leaves ~20 hand-maintained invalidation rules, 5 separate variation
mechanisms (instance overrides, variants, machine poses, clips, gesture/scratch
overlays), and blurred state-vs-derived boundaries.

This architecture replaces those layers with **three orthogonal subsystems**, each with
one mechanism:

| Layer | Question it answers | Mechanisms |
|---|---|---|
| **1. Data, Views & Discrete Variation** | What authored content, lexical overrides, and discrete variations exist? | Authored `Document` + **Lexical Virtual Views** (`Tree`) + **3 tiny ephemeral/solver tables** + **Small Discrete Variation layer** (`Ordered`/`Unordered` axes, values & `hidden` only) |
| **2. Derivation, Execution & Inspectability** | What derived work depends on what, in what order does it run, and why is a node what it is? | **Recorded-read cells & makers** + **one write channel** + **one `settle(mode)` driver** + **first-class `inspect(node)` provenance** |
| **3. Demand, Time & Host** | What actually computes right now, and how does the host consume it? | **Local need counts** + **composition-graph clock gate** + **host-side scoped indexes** |

### What stays unchanged

- Every document mutation is an op in a transaction through `iron_document`, applied in
  place via `Document`'s rollback journal (`document-spine.md`).
- Deterministic path ids (`NodeId`), tombstones, fractional `OrderKey`s, and canonical
  JSON (`serial.rs`).
- The formula language (`expr.rs`), static dependencies (`Expr::deps`), and tracked
  reads (`deref`).
- Model/picture separation (`component-model.md`) and reactivity semantics (`reactivity.md`:
  `let.keep`, `on`, `@name`, traces, accumulators, queries, presence, error propagation).
- The public read boundary: `Store::tree()` (`Tree`) and `Store::take_changes()` (`Changes`).
- Breaking changes are welcome: schema bumps delete old paths with no migrations.

---

## 2. Layer 1: Data, Lexical Virtual Views, Ephemeral Tables, and Discrete Variation

### 2.1 Authored document only

`Document` holds authored nodes, facets, relations, flat persisted solver poses
(`Document::solved`, written via `Op::Solve`), and (in `EN-15`–`EN-16`) discrete variation
tables. Derived copies do not live in a second materialized `Document` (`Docs::copies` is
deleted in `EN-13`).

### 2.2 Four orthogonal boundaries (no universal `PatchTable`)

Lessons from the reverted `EN-3`–`EN-5c` experiment
([`../notes/postmortem-universal-patch-algebra.md`](../notes/postmortem-universal-patch-algebra.md)):
instance overrides, ephemeral previews, solver poses, continuous clips, and discrete
authored variations have fundamentally different lifetimes and structural legality. Keeping
them cleanly separated avoids N × M interaction bugs and keeps every node locally legible
to humans and LLMs ([`patches.md`](patches.md)):

| Concern | Mechanism |
|---|---|
| **Component instances & `extends` variants** | Lexical override maps (`outermost instance > inner instance > variant > definition`) + `use.notation` on lean virtual views (`EN-13`). No wildcard suffix selectors. |
| **Gesture & live-solve previews** | Two separate tiny ephemeral maps: `gesture_preview: BTreeMap<Leaf, Value>` (owned by `ActiveGesture`) and `live_solve_preview: BTreeMap<Leaf, Value>` (owned by `resolve_clusters_live`) (`EN-12`). |
| **Persisted constraint solver poses** | One flat saved table `Document::solved: BTreeMap<Leaf, Slot>` written by `Op::Solve` (`EN-12`); cleared when an authored edit moves that leaf or its last holding constraint is deleted; undo/redo/replay never re-solve. |
| **Continuous time & simulation** | Clock tree + `ProducerId` priority stack (`LAYOUT > CLIPS > MACHINES > NETWORK`) + isolated simulation history per placed instance. |
| **Discrete authored variation** *(machine states, slides, takes, narrative moves)* | Small (~350–500 line) discrete variation layer over finite **Ordered** and **Unordered** axes — **values & `hidden` only** (use `when` or `extends` for structural changes), with finite-set arrow diffs, provenance, and bubbling ops (`EN-15`, `EN-16`). |

### 2.3 Lean Lexical Virtual Views (`Tree`, `EN-13`)

Static component instances (`<place>` / `use`) and `extends` variants are answered through
`Tree` as **virtual views** over shared `Arc<Template>` plus lexical override maps, rather
than copying nodes into `Docs::copies`:

- **Read path & sparse resolved store:** Unpatched constant virtual nodes borrow directly
  from shared `Arc<Template>` with `0` heap allocations per read (`EN-3` spike verified
  15.8–22.4 ns / 0 allocs and 2.5% of materialized memory). A small sparse resolved table
  holds only overridden/rewritten virtual nodes; zero-alloc `NodeId::expanded_parts()`
  splits instance paths without allocating.
- **View deltas & keys:** Stage 1 diffs each view's record (template `Arc`, lexical
  overrides, notation, rewritten bindings/relations) and emits ordinary effective `Node`,
  `Children`, `Facet`, `Leaf`, `Relations`, and scoped `In`/`With` keys (plus `Changes`).
  Template compilation moves off `scope_stamp` onto recorded reads (`EN-12`).
- **Dynamic repeats and `when`:** `EN-13` introduces a minimal keyed `ItemTable`
  (`repeat view id → Vec<ItemRecord>` holding membership, order, value, index, and
  presence/exit state) so `Docs::copies` can be deleted completely in `EN-13`; item
  descendants use the same virtual template view, and `item.value`/`item.index` publish as
  producer signals. `EN-7` then migrates repeat count/input evaluation onto makers and
  deletes `regraph`.

### 2.4 Effective value precedence

For any live leaf, highest precedence first:

1. **`gesture_preview`** *(ephemeral, owned strictly by `ActiveGesture`, cleared on cancel/commit)*
2. **`live_solve_preview`** *(ephemeral, owned strictly by `resolve_clusters_live`)*
3. **Binding cell value** *(formulas always outrank producers, discrete overrides, and solver poses)*
4. **Layout producer (`ProducerId::LAYOUT`)**
5. **Clip producer (`ProducerId::CLIPS`)**
6. **Document discrete variation** *(active slide / narrative joint / take override — `EN-16`)*
7. **Component machine producer (`ProducerId::MACHINES`)** *(interpolates discrete state poses — `EN-15`)*
8. **Other runtime producers** *(network, items, state, selection, theme, steps, presence, queries, events, hosts — fixed `ProducerId` priority rank; waiting requests promote on release)*
9. **Persisted solver pose (`Document::solved`, `Op::Solve`)** *(flat saved map; never shadows a binding or producer)*
10. **Lexical instance / variant override** *(outermost `<place>` → inner `<place>` → `extends` variant)*
11. **Base authored / template constant** *(or facet default)*

### 2.5 First-class node inspectability (`EN-14`)

One read-only `inspect(node)` query (backing both the UI Inspector and the agent's XML
`<look connections/>`) reads the fixed precedence stack above plus the Stage-1 reactive
graph's incoming/outgoing edges to answer, for any node:
- its tree path and semantic derivation (`extends` variant, `<place>` definition, active slide/take/state);
- property provenance (`why` each non-default leaf has its current value);
- everything driving it (bindings, clips, machines, constraints, layouts, discrete variations);
- everything it drives downstream.

---

## 3. Layer 2: Recorded-Read Graph and Execution Order

### 3.1 The one invalidation rule

**Every cached derivation is a cell or maker: a function computed from scratch over a
bounded scope. It reads the document, patches, and graph exclusively through recorded
reads. Every write reports the keys it touched through one channel, and the cells that
read those keys run again. Nothing else decides when a derivation refreshes.**

- **From scratch:** A cell has no incremental diffing logic of its own; given its scope,
  it computes the whole answer.
- **Bounded scope:** Chosen by what computes together — a layout root, a candidate
  constraint component, a network island, a node, or one top-level root (composition,
  definition, timeline) for a maker.
- **No hand rules:** No op classifiers (`op::rewires_network`, `op::retokens`,
  `after_transaction`'s structural match), no manual dirty sets (`layout_touched`,
  `repeat_dirty`, `regroup`, `Owners::affected`), and no hand-placed `drain_touched` calls.
- **One-off reads** (commands, export, agent views, edit planning) read `Tree` directly
  without recording.

### 3.2 Keys and one write channel

Every read through `Cx` or a recording `Tree` records a `Key`:

| Key | Recorded by | Reported by |
|---|---|---|
| `Node(n)` | node existence, type, parent, order | `insert_node`, `remove_node`, `restore_node`, `write_node`, `place`, `unplace` |
| `Children(n)` | child list of `n` | `place`, `unplace` |
| `FacetSet(n)` | facet presence / set on `n` | `insert_facet`, `remove_facet`, node create/delete/restore |
| `Facet(n, kind)` | facet contents (authored constants) | `insert_facet`, `remove_facet`, `facet_mut`, node create/delete/restore |
| `Leaf(leaf)` | leaf value reads | value writes, node create/delete/restore (every leaf held) |
| `In(root, ty)` | scoped type index under top-level `root` | index writers (old & new root on move) |
| `With(root, kind)` | scoped facet index under top-level `root` | index writers (old & new root on move) |
| `Relations(n)`, `Relation(id)` | relation lookups | `insert_relation`, `remove_relation` (both ends) |
| `Roots` | top-level roots list | placing/unplacing a root |
| `Wiring(leaf)` | `Cx::is_bound`, `Cx::driven`, `Cx::transitive_deps` | `Reactive` where bindings, claims, or tracked reads change |

**Enforcement:**
- `Cx::graph()` is deleted. Every cell reads only through `Cx`'s recorded methods and the
  recording `Tree`.
- Any unkeyed `Tree` read under a recording tree fails a `debug_assert!`.
- `Document` choke points and `Reactive` wiring mutations append to one touched-key log.
  The graph consumes the log itself at the start of evaluation.
- Structural fields (`constraint.kind`, `constraint.enabled`, `use.notation`) are marked
  in the schema: until made value-driven by their owning phase, binding one reports a
  problem on its leaf rather than silently reading the default.

### 3.3 One written execution order: `settle(mode)`

Every mutating entry point in `StoreCore` ends by calling one driver, `settle(mode)`,
with `mode ∈ { Transaction, Live, UndoRedo, Replay }`.

| Stage | What runs | Reads | Writes |
|---|---|---|---|
| **0. Inputs** | Transaction ops, session writes, playhead/clock advance, host publishes, selection/theme/fonts, drag previews, drivers & fingers | — | Document, discrete variations, input leaves |
| **1. Structure / Views** | Update view/owner bindings *(until `EN-13`: `Owners` expansion; after `EN-13`: lean lexical virtual views + `ItemTable`)* | Structure | Derived views / repeat items |
| **2. Make** | **Makers**: cells whose output is the set of native/binding cells for a bounded scope | Structure, authored constants, contexts library (**no evaluated leaves**, enforced by type) | Graph's native/binding cell set & wiring (reconciled in one batch with claim handoff) |
| **3. Evaluate** | Value cells: bindings and natives (layouts, cluster candidate/watcher cells, cluster dep order, network islands, repeat inputs) | Values, structure, wiring | Values, claims, extras |
| **4. Write-back** | Value-driven repeat item settling (`settle_repeats`); constraint cluster solves (ordered by derived cluster deps; `Transaction` writes to `Document::solved`/`Op::Set`, `Live` writes `live_solve_preview`, `UndoRedo`/`Replay` skip solving) | Evaluated values, state | Repeat items, solved values |

**Order rules:**
- **Earlier stages only:** Within a pass, a stage reads derived results only from earlier
  stages or previous passes.
- **No self-triggering write-backs:** A writer's direct writes to its own variables do not
  re-mark its own change test; feedback returning through another cell does.
- **Bounded multi-pass settling:** If Stage 4 writes keys that mark earlier-stage cells
  (e.g., a repeat count changing, or a cluster moving a layout that feeds a downstream
  cluster), `settle` loops up to its explicit per-writer budgets (per-repeat feedback
  detection; per-cluster resolve cap; cross-writer pass cap) and reports a problem on
  cycle/feedback rather than hanging.

### 3.4 No refusal after derivation

Once `run`'s pre-derivation validation passes (unknown nodes, scope, authored static
cycles, binding legality) and derived work begins, **nothing refuses a transaction**:
- An unsatisfiable constraint cluster keeps the pose it had when its solve began and
  reports a `"constraint"` problem derived from residuals (`solve::HOLDS`).
- Clusters in a binding-dependency cycle (Tarjan SCC) are left unsolved and report a
  problem; independent or downstream clusters still solve normally.
- Unreadable or non-finite constraint values are reported as problems and excluded from
  grouping rather than solved as `0`.
- An agent's `exact` edit is dry-run on `scratch()` before mutating the real store and
  refused cleanly if any request is unmet.

### 3.5 Explicit State vs. Derived boundary

State that evolves in time or across interactions is **not** a pure derivation and is
explicitly isolated:

| State (persisted in memory across calls) | Derived (pure function of Document + State) |
|---|---|
| Solved constraint poses on held operands | Cell values, errors, dependency edges, heights |
| Network islands' last warm-start solutions (`Native::history`) | Claims, reservations, waiting-request winner |
| Failed bindings' last good values | Maker outputs (layout roots, cluster partitions, islands, binding sets) |
| Glides and motion transitions in flight | Cluster dependency order & violated-constraint problems |
| Clocks (paused-from times), steppers (traces/accumulators), queries, presence, machine transition progress | Scoped type/facet/patch indexes |
| Session layer (scratch, drag previews, fingers, drivers, layout preview) & observers | Contexts library & table grids |

### 3.6 Derived families and their scopes

| Family | Scope | Makers / Cells |
|---|---|---|
| **Layout** (`EN-2a`) | Per layout root; `Pins` per composition | Composition maker over `With(root, Layout)` creates `LayoutNative`s and per-composition `PinsNative`; glide targets scoped per layout root |
| **Constraints & Contexts** (`EN-2b`) | Per candidate component / cluster; contexts per context name | Partition maker over `With(root, Constraint)` + candidate-mate relations → per-component cell filtering effective mates → per-cluster watcher + derived upstream-cluster deps; contexts library cell |
| **Network, Connections, Anchors** (`EN-2c`) | Per network island / connected component | Island maker over ports/connections/equations; anchor chains & derived port signals as cells |
| **Bindings, `@name`, Theme, Repeats** (`EN-7`) | Per node / per top-level root | Per-node binding maker; `@name` scope & boundary check cells; theme token cells; blocked retries via `Wiring` keys; repeat makers reading inputs through recorded reads |

---

## 4. Layer 3: Demand, Time, and Host Scaling

**Rule:** For work in a shown composition *C*, cost depends only on *C*, the cone of what
changed, and what is shown — never on hidden compositions, hidden entities, or unrelated
nodes elsewhere. (Excluded: load, save, export, whole-document queries, undo memory.)

### 4.1 Local need counts and scoped pulls (`EN-8`)

- **Exact need-count invariant:** Maintain `leaf_need: Map<Leaf, u32>`, counting distinct
  `(needed reader, channel)` demand edges targeting each leaf. A cell's `need` equals the
  sum of `leaf_need` over its outputs/reservations plus its separately owned hold tokens
  (`observe_all`, observers, Stage 4 document-writer holds, and scoped `pull`/`current`
  holds).
- **Separate demand from scheduling edges (no false cycles, no stale promotions):**
  - If `L` is bound, demand and scheduling both target `binding(L)` (never a reserving
    native, preventing false cycles when a bound leaf inside a stack is read by layout).
  - If `L` is unbound, **demand** targets *every* native reserving `L` (both the active
    claimant and waiting producers, so waiting requests are never stale on promotion),
    while **scheduling/value edges** follow only the active claimant.
  - When a binding or reservation changes, `retarget(L, old, new)` transfers `leaf_need(L)`
    atomically.
  - `Cx::settled` reads add demand to the owning layout native (and `Reactive::settled`
    must not bypass the read recorder on its glide-target fast path); dynamic `Wiring`
    reads (e.g. `transitive_deps` over a sleeping `deref`) pull freshness before returning.
- **Eager makers & permanent Stage 4 writer holds:** Stage 2 makers remain eager (structure-only,
  so hidden makers cannot leave stale reservations that block moved nodes). Stage 4 writers
  that mutate `Document` (constraint clusters and their input cones) stay held permanently
  so saved content never depends on viewport observation. `LayoutNative`s are un-rooted in
  `EN-8` and held per shown composition.
- **Needed-only stale propagation:** Dependents are partitioned into needed vs. unneeded.
  Value/key changes push only to needed readers. On `0 → 1`, increments apply immediately,
  the waking set is re-heighted and evaluated unconditionally (history natives must be
  idempotent for equal `(inputs, clock)` so waking never steps them); `1 → 0` decrements
  batch to the end of the pass to avoid flapping.
- **No `exact()` / `relax()`:** Reading a need-0 cell outside a `current` scope becomes a
  `debug_assert!` (`pinned` and `escaped`-forever deleted). Store operations (`run`,
  `scratch()`, `edit::plan`, `post_apply`, `advance`, `engine_api.rs`) use `Store::pull(leaves)`
  or scoped `current(|s| …)` holds — always pulling/validating inputs **before** committing
  side effects (query seq bumps, stepper integration, action emits) and recording structural
  view reads as well as leaf reads. `query::is_read` counts needed subscribers/holds only.
  Only export and unscoped whole-document views call `exact_all()`.
- **Heights on the accepted needed DAG:** Replace `compute_heights` (including in `evaluate`
  and `raise`'s fallback) with local DAG maintenance on needed edges only: validate
  acyclicity against the post-removal edge set before committing an edge, recompute
  `1 + max(input heights)` in the affected reader cone (stopping where unchanged, preventing
  unbounded drift on edge reversals), and use a single queue-rekey path for both bindings
  and natives.

### 4.2 Composition-graph visibility gate and retiring `ROOTED` (`EN-9`)

- Each node's **owner** (composition, nearest pictured entity, or free) is cached and
  re-keyed on reparent/`Figures` edits.
- Cross-owner graph edges maintain reference counts on `(owner A, owner B)`.
- Running status is a small traversal over `(A, B)` from shown compositions, `run = always`
  compositions, and the free owner, emitting only the delta to `Clocks`.
- `ROOTED`'s 29-kind global list is retired: non-session `let`s and plain `args` are
  unrooted; repeats/conditionals are rooted only in running owners; clips and machines
  publish only for running owners; captures observe what they draw.

### 4.3 Host-side scaling (`EN-10`)

Every host pass in `document_bridge`, `iron_spatial`, and `bevy_app` starts from shown
compositions or `Changes`, never global scans:
- Per-composition indexes for embeds (by home and source), `empty` controls
  (`reachable_controls`), and 3D entities (`scene3d`, `planes`).
- Spatial `begin_changes` / `commit` only on scopes touched by `Changes`; incremental
  `rebuild_root` without global scans.
- Per-composition `paint_order` renumbered locally.
- Incremental timeline lane planning in `timeline_view`.

---

## 5. Layer 4: Storage and Partial Loading (`EN-11`)

Deferred until the in-memory engine (Stages 1–3) is settled. Specified in
[`patch-storage.md`](patch-storage.md) (to be revised before `EN-11` to cut premature
server-side SQL index tables):
- Content-addressed immutable chunks (base, axis, patch) under a Merkle manifest of
  owner `Summary`s carrying axis bounds.
- Views load only the chunks whose bounds contain the active point and prefetch arrows
  along axes; unloaded reads resolve as `Pending`.

---

## 6. Verification strategy

Every phase is verified with fast, targeted checks — no monolithic oracle that duplicates
the engine:

1. **Existing suites:** `iron_document` plain + `differential` + `differential_random`
   (including `IRON_OWNER_CHECK`), `iron_spatial`, `iron_svg`, `bun test`, and the
   run-iron smoke script.
2. **Per-family from-scratch checks + choke-point canaries (Stage 1 & `EN-7`):** Each
   converted family keeps a simple test-only from-scratch recomputation of *that family
   alone* compared against the incremental cell result, paired with a canary that
   silences each mutation choke point in turn and asserts the test fails.
3. **Virtual view & discrete variation tests (`EN-13`, `EN-15`–`EN-16`):** Virtual views
   checked against materialized scenes on all gallery pages and nested/sized/main fixtures;
   discrete variation checked directly on the worked product scenarios (slide decks, cyclic
   triangle machines, takes/ghosts, bubbling ops, and `inspect(node)` provenance).
4. **Scaling & need-count oracles (Stage 3):** The `work` choke-point + allocator scaling
   test (`work_does_not_grow_with_the_hidden_document`) across both axes (many hidden
   pages and one large hidden page), plus the need-count reachability oracle under
   partial observation (`observe_all(false)`).

---

## 7. Settled decisions

| # | Decision |
|---|---|
| E1 | **No refusal after derivation.** Failed constraint solves apply, keep their last pose, and report derived problems; `exact` edits dry-run on `scratch()`. |
| E2 | **Order-free producer conflicts.** Contested leaves are resolved by fixed `ProducerId` priority rank (**layout outranks clips**); waiting requests promote immediately on release. |
| E3 | **Structural fields.** Binding a structural field (`constraint.kind`, `constraint.enabled`, `use.notation`) reports a problem rather than silently reading default, until value-driven support lands (`EN-2b`, `EN-13`). |
| E4 | **One invalidation rule.** Every cached derivation is a bounded-scope cell/maker driven by recorded reads through one write channel; no new op classifiers or hand refresh rules. |
| E5 | **One driver.** Every mutating store entry point ends in `settle(mode)` with the four-stage order of §3.3; writers keep their own bounded loops and failure rules. |
| E6 | **Lean Virtual Views & delete `Docs::copies` before wiring/scaling.** Clean up flat `Op::Solve` + split previews (`EN-12`), then build lean lexical virtual views + `ItemTable` to delete `Docs::copies` (`EN-13`), finish `regraph` deletion (`EN-7`), and flatten scaling (`EN-8`–`EN-10`) before layering on discrete variations (`EN-14`–`EN-16`). |
| E7 | **Grounded discrete variation, not universal algebra.** Keep lexical instance overrides, ephemeral previews, flat `Document::solved`, and continuous clips separate from discrete variations (`patches.md`, post-mortem 2026-11-23). Discrete variations cover finite `Ordered`/`Unordered` axes (values & `hidden` only) with zero hidden CSS-style wildcard selectors. |
| E8 | **Pragmatic verification.** Verify recorded reads with locked-down `Cx`, per-family from-scratch checks, and choke-point canaries instead of building a whole-store `fresh-oracle` or dynamic `deref` cycle-set SCCs prematurely. |
| E9 | **Cohesion before scaling, scaling before new variation features.** Finish `EN-12`, `EN-13`, and `EN-7` so `Docs::copies` and `regraph` are gone, land Stage 3 scaling (`EN-8`–`EN-10`), then add node inspectability (`EN-14`) and discrete variations (`EN-15`–`EN-16`). |
| E10 | **No backwards compatibility.** Delete superseded code, rules, facets, and schema paths in the same change; bump `serial::SCHEMA` freely. |
| E11 | **Legibility, inspectability, and net-LOC discipline.** Every property and driver on a node must be locally inspectable via `inspect(node)` / `<look connections/>`. Any simplification phase that grows net LOC past +1,500 without deleting its target stops and checks in with the user. |
