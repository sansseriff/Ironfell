# Architecture: An LLM-Editable Scene Document
 
A design for a visualization/animation system whose document is a stable-identity
node graph, edited concurrently by a human and an LLM, rendered by Bevy (WASM, in a
worker), and surfaced to the model through cheap, lazy, task-specific projections.
 
---
 
## 0. Core theses
 
Six ideas the rest of the document elaborates. If you keep only these, you keep most
of the value.
 
1. **Identity is separate from location.** Nodes have opaque IDs that survive
   reparenting, reordering, and animation. Paths are a *query language* over IDs, not
   identity. (The inode/path split.)
2. **Every mutation is a semantic operation.** No code path mutates the store
   directly. A move is `reparent`, not 1000 deletes plus 1000 inserts. The op log is
   simultaneously undo/redo, the Bevy sync channel, and the LLM invalidation source.
3. **Authored state is authoritative; everything else is derived.** Declared props
   and hierarchy are stored and edited. Resolved transforms, layout results, BVH,
   embeddings are recomputed and never edited. Writes only ever touch authored state.
4. **The model reads projections, not the document.** A projection is a typed view
   (tree / timeline / spatial / semantic) rendered at a fidelity budget, elided by
   default. Different questions get different views of the same node set, joined on
   NodeId.
5. **Laziness beats invalidation.** You cannot invalidate what was never read. Good
   default elision is a first-order win; precise hash-based invalidation is
   second-order. Build in that order.
6. **Read markup, write ops.** The model receives XML-ish views and emits declarative
   verb elements. Correspondence between the model's intent and the store is always
   *declared* (via IDs), never *inferred* (via diffing).
---
 
## 1. Layer map
 
```
L0  Identity & authored store      NodeId, components, tombstones, order keys
L1  Operations                     op vocabulary, transactions, undo/redo
L2  Derived state                  solvers, layout, BVH, shallow/deep hashes
L3  Projections                    tree / timeline / spatial / semantic renderers
L4  Context ledger                 what the model saw, at what fidelity, when
L5  LLM tool surface               read verbs, <edit> write surface, validation
L6  Semantic corpus                chunks, embeddings, NodeId-anchored links
L7  Bevy bridge                    op applier, ID map, spatial queries, intents
```
 
L0–L1 are the foundation and are painful to retrofit. L2 and L4 are optimizations
that should be driven by measurement. L6 is nearly independent — it needs L0 and
nothing else.
 
---
 
## 2. L0 — Identity and the authored store
 
### 2.1 NodeId
 
```ts
type NodeId = string;  // opaque; ULID or base62-encoded u64
```
 
Hard rules:
 
- **Never reused.** Deletion tombstones; it does not free the ID.
- **Never encodes position.** No paths, no indices, no parent prefix. A node that
  moves keeps its ID unchanged — this is the entire point.
- **Opaque to the model.** The LLM copies IDs from projections; it never constructs
  or predicts them.
ULIDs are a good default: sortable by creation time, collision-free without
coordination, and readable enough to debug. If token cost of IDs in projections
becomes material, mint short sequential IDs (`n1`, `n2`, …) instead — they are ~3
tokens instead of ~8, and the sortability is rarely needed.
 
### 2.2 Node record
 
```ts
interface Node {
  id: NodeId;
  type: string;         // "box" | "circle" | "group" | "clip" | ...
  parent: NodeId | null;
  orderKey: string;     // fractional index among siblings
  tombstoned: boolean;
  createdAt: number;
}
```
 
### 2.3 Components
 
Authored data lives in sparse per-type maps, not on the node:
 
```ts
type ComponentStore = Map<string, Map<NodeId, unknown>>;
// "transform"  -> { x, y, rot, scale }
// "fill"       -> { color, opacity }
// "timing"     -> { start, duration, easing }
// "semantic"   -> { links: SemanticLinkId[] }
```
 
This is component-shaped storage, **not an ECS**. There is no scheduler, no archetype
packing, no parallel query planner. You need indexed maps, a few derived indexes, and
the op log. Resist the urge to build a framework here — it is a few hundred lines and
should stay that way. Bevy is the ECS; this is a document store.
 
Only **authored** components live here. Nothing computed. (§4)
 
### 2.4 Tombstones
 
```ts
{ ...node, tombstoned: true, tombstonedAt: version }
```
 
Why not hard delete:
 
- IDs can never be recycled into a different node, so a stale reference in model
  context or a semantic link fails *loudly* instead of silently rebinding to
  something unrelated.
- Undo of a delete restores the same ID, so anything referencing it still works.
- The context ledger can answer "that node you saw is gone" rather than "unknown ID."
Garbage-collect tombstones only when they fall out of the undo horizon *and* no
semantic links or ledger entries reference them.
 
### 2.5 Sibling order
 
Use a fractional index (LexoRank / Figma-style string keys), not an integer array
position. Inserting between two siblings mints a key between theirs and touches
nothing else. Integer positions renumber every following sibling, which turns a
one-node insert into an N-node invalidation — the same failure that killed line
numbers as an edit address.
 
Rebalance lazily when key length exceeds a threshold; treat a rebalance as a system
op so consumers see it.
 
### 2.6 Indexes (derived, rebuilt on change)
 
| Index | Purpose |
|---|---|
| `children: Map<NodeId, NodeId[]>` | ordered child lists |
| `pathIndex: Map<string, NodeId>` | path → ID resolution for reads |
| `byType: Map<string, Set<NodeId>>` | type queries |
| `byComponent: Map<string, Set<NodeId>>` | "all nodes with timing" |
 
**Paths are queries, not identity.** `read("/scene/enemies/goblin_7")` is a
convenience for the model and for debugging; the response always carries the resolved
NodeId, and every write is by ID.
 
---
 
## 3. L1 — Operations
 
### 3.1 The discipline
 
> Every mutation to authored state goes through an op. No exceptions. UI event
> handlers construct ops; they never touch the store.
 
This single rule is what makes undo, Bevy sync, LLM invalidation, and any future
collaboration fall out for free. It is also the thing that is most miserable to
retrofit, because violations hide in dozens of event handlers. Enforce it
structurally: the store's mutating methods are private to the op applier.
 
### 3.2 Vocabulary
 
```ts
type Op =
  | { t: "create";    id: NodeId; type: string; parent: NodeId; order: string; props?: Props }
  | { t: "delete";    id: NodeId }
  | { t: "reparent";  id: NodeId; parent: NodeId; order: string }
  | { t: "reorder";   id: NodeId; order: string }
  | { t: "setProps";  id: NodeId; props: Props }
  | { t: "addComp";   id: NodeId; comp: string; data: unknown }
  | { t: "removeComp";id: NodeId; comp: string }
  | { t: "link";      from: NodeId; to: NodeId; rel: string; data?: unknown }
  | { t: "unlink";    from: NodeId; to: NodeId; rel: string };
```
 
`link`/`unlink` carry non-containment relations: temporal (`after`, `during`),
semantic (`references`), constraint (`alignsTo`). These are edges in the
multi-relational store (§7) and must be first-class ops, not props encoding a DSL.
 
### 3.3 Transaction envelope
 
```ts
interface Transaction {
  version: number;             // monotonic, post-apply
  label: string;               // semantic, human- and LLM-readable
  actor: "human" | "llm" | "system";
  ts: number;
  idempotencyKey?: string;
  ops: Op[];
  inverse: Op[];               // computed at apply time
}
```
 
**The label is a product surface.** `"align 12 objects to left edge"` is what the
model reads in an invalidation notice. `"setProps x14"` is not. Every UI interaction
that produces a transaction should name itself in the vocabulary a user would use.
 
### 3.4 Invertibility
 
Compute the inverse when applying, while the pre-state is in hand:
 
| Op | Inverse |
|---|---|
| `create` | `delete` |
| `delete` | `create` with captured props + children restore |
| `reparent` | `reparent` to previous parent/order |
| `setProps` | `setProps` with previous values (only for touched keys) |
| `addComp` | `removeComp` |
| `link` | `unlink` |
 
Undo = apply `inverse` reversed. Redo = re-apply `ops`. Because inverses are ops, they
flow through the identical pipeline — Bevy sync, invalidation, and validation all work
on undo with no special casing.
 
### 3.5 Coalescing
 
A drag emits one transaction on commit, not one per frame.
 
```
gesture start  → open a coalescing buffer, render optimistically
gesture move   → update buffer, no transaction
gesture commit → emit single transaction, labeled "move 3 objects"
gesture cancel → discard buffer, restore
```
 
Rule: coalesce ops of the same type on the same node within a gesture or within a
short time window (~500ms) for keyboard-driven property tweaks. Never coalesce across
structural ops.
 
### 3.6 The undo stack
 
```ts
interface History {
  past: Transaction[];
  future: Transaction[];
  horizon: number;    // max depth
}
```
 
Notes:
 
- **Undo is a first-class actor.** Emit undo transactions with `actor: "system"` and
  label `"undo: <original label>"` so the model can tell an undo from a fresh edit.
- **LLM edits go on the same stack** as human edits. A user must be able to undo the
  model's work with the same keystroke. Do not build a parallel history.
- If you later add multiplayer, this becomes selective undo and gets much harder —
  but the op-based foundation is the prerequisite either way.
---
 
## 4. L2 — Derived state
 
### 4.1 The split
 
| Authored (stored, editable) | Derived (recomputed, read-only) |
|---|---|
| local transform | resolved world transform |
| layout constraints | resolved rect after layout pass |
| declared duration / easing | resolved absolute time ranges |
| hierarchy | BVH, spatial partition |
| text content | embeddings |
 
**Never cache derived state in model context by default.** Derived state has the
worst fan-out of anything in the system: reparenting a node changes nothing declared
about its subtree, but invalidates every resolved transform beneath it. If declared
and computed live in the same blob, every structural edit poisons everything.
 
Serve derived state on demand, scoped, and clearly marked as a snapshot at a given
version.
 
### 4.2 Solvers
 
- **Layout solver** — 1D constraint solve per axis, flexbox semantics.
- **Temporal solver** — the *same* solver on the time axis. Explicit durations are
  fixed basis; stretch-to-fill is `grow`; clamps are min/max; keyframe anchors are
  alignment constraints; `<seq>` and `<par>` are `direction: row` and `stack`.
- **Spatial index** — BVH over resolved transforms, rebuilt or refit per frame in
  Bevy.
Sharing the layout and temporal solver is not a metaphor; it is the same
implementation with different units. It also means the model can be given timeline
markup in flex vocabulary, which lands on a strong pretraining prior.
 
### 4.3 Hashing (build in Phase 4, not before)
 
```
shallow(n) = hash(type + authored props of n + ordered child ID list)
deep(n)    = hash(type + authored props + [deep(c) for c in children])
```
 
- `shallow` is the **invalidation unit**. Compute eagerly on write; it is cheap.
- `deep` is the **"do I need to look inside"** check. Compute lazily, memoized,
  invalidated up the ancestor chain on write.
**Critical reporting rule:** a deep-hash change on an ancestor is *not* the same as
that ancestor being dirty. Report them distinctly:
 
```
#group_a: contents changed (deep) — node itself unchanged
  └ #goblin_7 moved out → #group_b
```
 
Textbook Merkle discipline propagates dirtiness to the root, which would make every
edit look like it invalidated the whole document. Don't do that.
 
### 4.4 The 1000-entity case, resolved
 
Pulling one object out of a 1000-entity group dirties exactly three shallow hashes:
old parent's child list, new parent's child list, moved node's parent pointer. The
other 999 nodes are byte-identical and every cached fact about them remains valid.
 
And in practice it is even cheaper, because the model never saw 1000 entities — it saw
a summary (§6.3). See §8.4.
 
---
 
## 5. L7 — The Bevy boundary
 
Placed here because it constrains projection design.
 
### 5.1 Authority
 
**The authored store lives in TypeScript. Bevy is a derived consumer.**
 
Reasons, in order of how much they settle the question:
 
1. `bevy::Entity` is a generational index and is **recycled after despawn**. It cannot
   be your node identity. A `NodeId ↔ Entity` map is required regardless, so Bevy
   cannot be the identity authority even in principle.
2. Bevy holds predominantly *computed* state — the highest-churn side of the split.
   Authority belongs with the low-churn authored side.
3. Reflection-based serialization is slow and lossy; you do not want it on the hot
   path of every model read.
4. Bevy is pre-1.0. Your document format should not be hostage to its API churn.
This is the standard DCC architecture, not a compromise: Blender scene data vs
evaluated depsgraph, USD stage vs Hydra render index, Maya DAG vs DG.
 
### 5.2 The three channels
 
| Direction | Payload | Shape |
|---|---|---|
| TS → Worker | authored ops | transaction stream, batched per frame |
| Worker → TS | spatial answers | scoped query results keyed by NodeId |
| Worker → TS | user gestures | **intents**, not mutations |
 
```ts
// TS → Bevy
{ kind: "tx", version: 42, ops: [...] }
 
// TS → Bevy, request
{ kind: "query", id: 17, q: { pick: { x: 410, y: 220, t: 500 } } }
// Bevy → TS, response
{ kind: "queryResult", id: 17, hits: ["#a91f", "#b03c"] }
 
// Bevy → TS, gesture
{ kind: "intent", intent: { drag: { ids: ["#a91f"], dx: 12, dy: -4 }, phase: "commit" } }
```
 
### 5.3 Why gestures are intents
 
A drag in the viewport does **not** mutate Bevy's authoritative state, because Bevy
has no authoritative state. It emits an intent → main thread converts to a
transaction → transaction flows back to Bevy. During the drag, Bevy renders
optimistically for latency; on commit, one transaction.
 
This looks redundant and is not. It gives you: one op log, working undo of viewport
manipulations, and LLM visibility into direct-manipulation edits.
 
### 5.4 Bevy's query surface
 
```
pick(x, y, t)                → NodeId[]
raycast(origin, dir, t)      → hits with distance
inAABB(min, max, t)          → NodeId[]
nearest(id, k, t)            → NodeId[] with distances
worldTransform(ids, t)       → resolved transforms
visibleSet(camera, t)        → NodeId[]
```
 
All scoped, all returning small results keyed by NodeId. No bulk state dumps cross
the boundary.
 
### 5.5 What never touches Bevy
 
The semantic corpus (§10). Text chunking, embeddings, link management, and text
rendering are DOM/browser concerns. They need L0 and nothing else.
 
---
 
## 6. L3 — Projections
 
### 6.1 Definition
 
```ts
interface ProjectionQuery {
  relation: "tree" | "timeline" | "spatial" | "semantic";
  scope: NodeId | NodeId[] | Region;
  fidelity: "skeleton" | "summary" | "full";
  depth?: number;
  filter?: ComponentFilter;
}
```
 
A projection is a **query**, and the four relation types are genuinely different
algebras — which is why "hypergraph" is the wrong abstraction. It is permissive
enough to describe the system but discards the structure that makes it work.
 
| Relation | Structure | Authored? |
|---|---|---|
| tree (containment) | ordered tree, ≤1 parent, acyclic | **yes, writable** |
| timeline | ordered tree over intervals | **yes, writable** |
| spatial | derived partition, not a tree | no, read-only |
| semantic | ranked, approximate, points outside the doc | no, read-only |
 
The sharper description of the whole system: **one entity set, many typed relations,
projections are queries selecting a relation plus a fidelity budget.** A database with
multiple indexes; projections are views.
 
### 6.2 NodeId as join key
 
The same `#a91f` appears in every projection. The model pivots across views without
re-identifying anything:
 
```
pick(410, 220, t=500)   → #a91f
read timeline #a91f     → keyframes, parent interval, easing
read tree #a91f         → parent, siblings, declared props
read semantic #a91f     → linked notes, source references
```
 
Four cheap scoped reads answer "why is this blue ball animating across my screen at
t=500." Cross-projection fields degrade to bare ID stubs the model can follow if it
cares — that is the representation masking, made mechanical.
 
### 6.3 Fidelity and elision
 
Default is elided. Detail requires drill-down.
 
```xml
<!-- skeleton -->
<group id="#g1" name="enemies" count="1000" elided/>
 
<!-- summary -->
<group id="#g1" name="enemies" count="1000" bounds="0,0,800,600"
       schema="transform,health,faction" elided>
  <sample id="#e001" type="goblin" x="12" y="40"/>
  <sample id="#e002" type="goblin" x="50" y="44"/>
</group>
 
<!-- full -->
<group id="#g1" name="enemies">
  <goblin id="#e001" x="12" y="40" health="30" faction="red"/>
  ...
</group>
```
 
**Always mark elision explicitly.** Without the `elided` attribute the model cannot
distinguish "empty" from "not shown," and will confidently reason about children that
exist. This is nearly free and prevents an entire class of silent errors.
 
Elision strategies by node type: uniform collections → count + schema + samples +
aggregates; deep subtrees → depth-limit with `elided` markers; long text → head +
length; keyframe tracks → count + range + a few anchors.
 
### 6.4 Formats, per projection
 
XML's token edge over JSON is real but shape-dependent (attribute-heavy nodes favor
XML; deep nesting favors JSON; uniform records lose to both). So pick per projection
rather than unifying:
 
| Projection | Format | Rationale |
|---|---|---|
| tree / layers | XML-ish | nesting *is* the data |
| timeline | XML structure w/ flex vocabulary; TSV keyframe rows | hierarchy nests, keys tabulate |
| spatial results | TSV / aligned columns | uniform records, 3–4× cheaper than markup |
| semantic hits | markdown list with scores | ranked, short, approximate |
| invalidation | terse custom lines | not a data format |
 
Two rules:
 
- **Flatten past ~4 levels of depth.** Indentation and closing tags stop paying;
  a flat `id parent type props` table reads fine and costs much less. Nest for
  navigation, flatten for bulk.
- **Stay inside the HTML/XML/JSON/TSV/markdown family.** A bespoke encoding throws
  away the pretraining prior, which is worth far more than any token savings.
### 6.5 Timeline in flex vocabulary
 
```xml
<seq id="#tl_main" dur="30s">
  <clip id="#intro" dur="2s"/>
  <clip id="#body" grow="1" min="5s"/>
  <par id="#finale" dur="3s">
    <clip id="#fade" target="#a91f" prop="opacity" from="1" to="0"/>
    <clip id="#spin" target="#b03c" prop="rot" from="0" to="360"/>
  </par>
</seq>
```
 
Explicit durations are fixed basis, `grow` is flex-grow, `min`/`max` are clamps,
`<seq>`/`<par>` are row/stack. "Smooth all animation after t=200" becomes edits to
easing attributes on interval nodes, not arithmetic over a thousand absolute
timestamps.
 
### 6.6 The manifest
 
Always present in the system prompt, ~200 tokens, generated from the same source as
the tool schemas:
 
```
PROJECTIONS
  tree      containment, layer order, declared props.  read/write.
            verbs: read_tree(scope, depth, fidelity)
  timeline  intervals, keyframes, temporal relations.  read/write.
            verbs: read_timeline(scope, t0, t1, fidelity)
  spatial   resolved positions at a time. DERIVED, read-only.
            verbs: pick(x,y,t) raycast() in_aabb() nearest()
  semantic  linked corpus. APPROXIMATE, ranked, read-only.
            verbs: search(query, k), links_for(id)
```
 
Without this, projection selection is inference; with it, it is lookup.
 
---
 
## 7. Relations
 
Non-containment edges are first-class, stored as typed relations:
 
```ts
interface Relation {
  from: NodeId; to: NodeId;
  rel: "after" | "during" | "alignsTo" | "references" | ...;
  data?: unknown;    // e.g. { offset: "10s" }
}
```
 
Rendered as elements, never as encoded property strings:
 
```xml
<after target="#ball.blink" ref="#spinner.spin" offset="10s"/>
```
 
not
 
```xml
<node blink="#spinner.spin+10s"/>
```
 
The element form is validatable, legible to the model, and does not require inventing
and parsing a DSL. **Anchor everything to NodeIds, never to paths or positions** — so
reparenting and animation never break a relation.
 
---
 
## 8. L4 — The context ledger
 
The one genuinely novel component. No prior system needed it: renderers recompute for
free, humans self-update by looking. An LLM has neither property.
 
### 8.1 State
 
```ts
interface LedgerEntry {
  nodeId: NodeId;
  projection: ProjectionKind;
  fidelity: Fidelity;
  versionAtRead: number;
  turnAtRead: number;
  turnsSinceQuery: number;
  cumulativeDeltaBytes: number;
}
```
 
The ledger records *what was rendered into context*, not what exists.
 
### 8.2 Invalidation on each turn
 
1. Diff current shallow hashes against `versionAtRead` for ledger entries only.
2. Discard changes that would not alter what was rendered **at that fidelity**.
3. Emit notices at a precision determined by decay (§8.3).
4. Compact when dirty fraction crosses threshold (§8.5).
Step 2 is where most of the savings live.
 
### 8.3 Decay
 
Precision decays; **the notice never disappears**. Silent decay is the one genuinely
dangerous failure mode in this design — it leaves the model confidently acting on a
picture it does not know is stale. Loud imprecision is safe; quiet precision-loss is
not.
 
```
recent   #a91f.duration 2s → 3s
older    timeline region t=180–260 changed (7 edits, human)
oldest   timeline projection STALE — re-read before acting
```
 
**When to step down:** emit precise deltas only while `cumulativeDeltaBytes` stays
below the cost of re-reading that node at its rendered fidelity. Past that, the
notices have stopped paying for themselves — downgrade to a stale flag. This is
measurable and falls out of Phase 4 numbers rather than out of taste.
 
**What drives decay:** not raw token distance. Retrieval from deep context is decent;
the real problem is precise notices about regions the current task is not touching
competing for attention with notices that matter. So decay on
`turnsSinceQuery` plus a topic-shift trigger. A projection re-read every turn stays
precise regardless of depth; a projection abandoned three turns ago when the task
shifted decays fast.
 
### 8.4 Worked example
 
Human drags one entity out of a 1000-entity group.
 
- Ledger says `#g1` was rendered at `summary` fidelity.
- Summary content: count, bounds, schema, two samples.
- Change: `count: 1000 → 999`; bounds unchanged; samples unchanged.
- Notice emitted:
```
#g1 count 1000 → 999 (human moved #e447 → #g2)
```
 
Roughly 20 tokens. Nothing else invalidates. Had the group been rendered at `full`
fidelity, this would have been a ~40k-token invalidation problem — which is the
argument for laziness over cleverness.
 
### 8.5 Compaction
 
Accumulate notices at the tail (cheap, no KV invalidation). When cumulative notice
size or dirty fraction crosses ~30–50% of the projection's rendered size, emit a
fresh projection and drop the notice log.
 
Cost per edit ≈ `D/k + d` where `k` is edits between compactions.
 
**Never leave multiple full snapshots stacked.** Several plausible-looking documents
disagreeing with each other is worse than deltas. Either genuinely elide the old ones
or use a context-editing mechanism that supports removal.
 
### 8.6 Placement
 
Mutable content wants to be **late** in context (rewriting it only invalidates cache
from that point). Conversation also grows at the tail. For small documents, invert the
natural layout: render the canonical projection as a **trailing state block**,
refreshed each turn, after the conversation. You re-prefill `D` tokens per turn, keep
all conversation cache, and fragmentation goes to zero.
 
For a 2k-token projection against a 60k-token conversation, this beats everything
else in this document on total engineering cost. Only build the ledger when scenes
outgrow it.
 
### 8.7 Authority rule
 
State once in the system prompt:
 
> The most recent `<projection>` block is authoritative. Earlier blocks and change
> notices are historical. Nodes marked `elided` have contents you have not seen.
 
Cheap, and it means the model *locates* state rather than *reconstructing* it by
replaying deltas. Most fragmentation problems are ambiguity about which copy wins,
not inability to apply diffs.
 
---
 
## 9. L5 — The LLM interface
 
### 9.1 Read surface
 
```
read_tree(scope, depth?, fidelity?)
read_timeline(scope?, t0?, t1?, fidelity?)
pick(x, y, t) / raycast(...) / in_aabb(...) / nearest(id, k, t)
search(query, k) / links_for(id)
```
 
Every response carries `version` and, once L2 exists, `expect` hashes for the nodes
returned — so the model has what it needs for its next write without a second call.
 
### 9.2 Write surface: three verbs, one parser
 
**`<edit>` — compound ops. The default.**
 
```xml
<edit expect-version="41">
  <create id="$ball" type="circle" parent="#g2" color="blue" r="8"/>
  <set id="$ball" path="#route_7" easing="ease-out"/>
  <blink target="$ball" period="0.4s"/>
  <after target="$ball.blink" ref="#spinner.spin" offset="10s"/>
  <set id="#g2" layout="row" expect="7c1e"/>
  <reparent id="#b3" to="$ball" order="first"/>
</edit>
```
 
- One tool call carries N ops. Batching and markup-rewriting are **separable
  concerns**; a single call has never been limited to a single op.
- Tag name is the verb → parsing is one pass over children, no diffing.
- `$name` are batch-local aliases for nodes created in the same transaction,
  enabling forward references. The result returns the alias → NodeId map.
- Transactional: all-or-nothing, rollback via inverses.
**`<replace-subtree>` — bounded region rewrite with mandatory IDs.**
 
For the dense multi-aspect edits that make code editing pleasant — change fifteen
things about a region in one generation pass.
 
```xml
<replace-subtree id="#panel" expect="9f2a">
  <box id="#panel" layout="row" gap="8">
    <label id="#title" text="Status" color="blue"/>
    <icon id="$new_warn" glyph="alert"/>
  </box>
</replace-subtree>
```
 
Correspondence rules, **declared not inferred**:
 
| Markup | Meaning |
|---|---|
| ID present, node exists | same node, props updated |
| no ID (or `$alias`) | new node |
| node was present, now absent | deleted |
| order in markup | sibling order |
 
Bounds: ≤ ~40 nodes, no `elided` descendants inside the target, all pre-existing
descendants must carry IDs. Past that, re-emitted unchanged content costs more than
targeted ops and mis-transcription risk climbs.
 
**`<create-subtree>` — new content.** No `expect` needed; nothing to reconcile.
 
### 9.3 The line that actually matters
 
Not "parser vs no parser" — you have a parser either way. The line is **declared
versus inferred correspondence**.
 
Recovering intent by comparing two document states is what turns a reparent into 1000
deletes and 1000 inserts. Whole-file rewrite works for code because text *is* the
artifact and there is no identity to lose. Your DOM has identity, which makes blind
rewriting lossy — but also makes ID-declared rewriting *safer* than code editing, since
correspondence is verified rather than guessed.
 
There is no whole-document rewrite verb, ever.
 
### 9.4 Validation pipeline
 
```
1. XML parse           → structural errors
2. Zod per verb        → type / attr errors
3. Semantic validation → store-dependent errors
4. Transactional apply → rollback on failure
```
 
**1. Parse.** `fast-xml-parser` or `htmlparser2` in forgiving mode. If it can
self-close unclosed tags rather than throwing, take that — a re-ask costs a full
generation.
 
**2. Zod, discriminated union on tag name.**
 
- *Coerce liberally.* `10s`, `10`, `"10s"`, `0:10` should all parse. Same for colors
  and for IDs with or without `#`. Rejecting a semantically clear op on formatting is
  pure waste.
- *Reject unknown attributes; do not strip.* Silent stripping means the model believes
  it set something it did not, surfacing later as inexplicable behavior instead of an
  immediate, fixable error.
**3. Semantic validation** — needs the store, so Zod cannot do it:
 
- referenced IDs exist and are not tombstoned
- reparent would not create a cycle *(commonly forgotten; corrupts the tree permanently)*
- `$aliases` defined before use
- target type legal for the verb
- `expect` hashes match current
- relation type valid for the node types involved
**4. Validate the whole batch before applying any of it.** Ops within a batch are
interdependent, so a half-applied batch diverges the model's picture from reality in a
way that is worse than clean rejection. Cheapest reliable implementation: apply inside
a transaction, collect failures, roll back if any.
 
### 9.5 Error design
 
Every rejection costs a full round-trip. Error text is a token-budget line item and
should be designed as an artifact.
 
```xml
<edit-failed applied="none" version="43">
  <error line="4" verb="after">
    ref "#spinner.spin" — node #spinner exists, no relation "spin".
    available: rotate, fade
  </error>
  <error line="5" verb="set">
    expect "7c1e" ≠ current "b204". #g2 was edited by human since your last read.
    re-read before retrying.
  </error>
</edit-failed>
```
 
- **Report all failures at once.** One round-trip instead of five.
- **Include the nearest valid alternative.** `available: rotate, fade` usually turns a
  retry into a fix.
- **State that nothing was applied**, so the model does not guess whether to undo.
- Type this as a result value, not a thrown exception — partial-failure paths are
  exactly where exceptions get swallowed.
### 9.6 Concurrency
 
Compare-and-swap on `expect` hashes. Rejection returns current hash plus a re-read
hint. This is what demotes context fragmentation from a *correctness* bug to a
*latency* cost — the model can hold a stale picture and still cannot silently clobber
the human's work.
 
Add **idempotency keys** on transactions. Retries after ambiguous failures (worker
timeout, dropped `postMessage`) will happen, and double-applying is worse than not
applying.
 
### 9.7 Generate docs from schemas
 
Zod → JSON Schema is mechanical. The verb list in the manifest, the tool schemas, and
the validator must come from one source. Drift between documented and enforced surface
is a silent ongoing tax.
 
### 9.8 Send the human's current view
 
Both parties see projections. Serialize the human's:
 
```xml
<user-view panel="timeline" t="200" selection="#a91f,#b03c"
           viewport="0,0,1200,800"/>
```
 
Cheap, and it makes "smooth this down" resolvable without a clarifying question. Half
of normal deictic reference is unresolvable without it. This is where projection design
pays off in UX rather than tokens: the model and the human look at the same view of the
same nodes, so "this" means the same thing to both.
 
---
 
## 10. L6 — Semantic layer
 
Another index over the same ID space, with edges pointing into an external corpus.
 
```ts
interface SemanticLink {
  id: string;
  node: NodeId;          // anchored to ID, never to path or position
  chunkId: string;
  relevance: number;
  kind: "explains" | "cites" | "derivedFrom";
}
```
 
- Chunk the corpus, embed, store vectors client-side (or in a small service).
- **Anchor to NodeIds** so reparenting and animation never break a link.
- **Mark results as approximate** in projection output. Structural reads are ground
  truth; semantic reads are ranked retrieval, and the model should treat them
  differently.
- Entirely browser/DOM — never touches Bevy. Text editing and reading are far better
  served by the DOM than by a pre-1.0 game engine's text stack.
Depends on L0 only, so it can be built on a parallel track from Phase 1 onward.
 
---
 
## 11. Build order
 
### Phase 1 — L0 + L1
Store, ops, transactions, invertibility, undo/redo, coalescing, tombstones, fractional
order keys. Wire the **real UI** through the op log.
 
*Exit criteria:* a mouse drag, a multi-select align, and a reparent each produce a
single well-labeled transaction, and undo/redo is correct for all three. Do not
proceed until this holds — undo correctness is the proof that your op vocabulary
matches real interactions.
 
### Phase 2 — L7 write path
Transaction stream drives Bevy. `NodeId ↔ Entity` map. Mirror authored hierarchy into
Bevy `Parent`/`Children`. Intent emission for viewport gestures.
 
*Exit criteria:* replaying the op log from empty reconstructs the rendered scene
exactly. If the log can drive the renderer, it can drive any consumer.
 
### Phase 3 — L3 + L5, crude
Projections with elided defaults, the manifest, `<edit>` and `<create-subtree>`, Zod +
semantic validation, CAS on version only, and **blanket invalidation** — "the document
may have changed; re-read before editing."
 
No hashing. No ledger. This is shippable and it works; it is what current code agents
do.
 
### Phase 4 — Measure, then optimize
Instrument: tokens per turn, tokens spent on re-reads, how often blanket invalidation
forced a redundant re-read, distribution of projection sizes.
 
*Only now* build L2 shallow/deep hashing and the L4 ledger, aimed at what the numbers
actually show. If the trailing-state-block approach (§8.6) is holding up, skip the
ledger entirely.
 
### Phase 5 — L7 read path
Spatial queries from Bevy: `pick`, `raycast`, `in_aabb`, `nearest`, `worldTransform`.
Spatial projection rendering.
 
### Phase 6 — Temporal solver
Shared 1D constraint solver on the time axis. Timeline projection in flex vocabulary.
`<replace-subtree>` — by now you know whether the model needs it.
 
### Phase 7 — L6 semantic
Or in parallel from Phase 1, since it only needs L0. If text linking is core to the
product, start it as a separate track immediately.
 
---
 
## 12. Retrofit-cost ranking
 
Get these right at the start; they are expensive to add later:
 
1. **Op discipline** — every mutation through the log. Violations hide in dozens of
   event handlers and are miserable to hunt down.
2. **Stable IDs + tombstones + fractional order keys** — changing identity semantics
   later invalidates every persisted document and every semantic link.
3. **Authored/derived separation** — mixing them means untangling later under
   pressure.
4. **Declared-not-inferred correspondence** — a diff-based write path is hard to
   remove once tools depend on it.
Safe to defer:
 
- hashing and the ledger (measure first)
- `<replace-subtree>`
- multiple projection types beyond tree
- decay policy tuning
- moving the store to its own worker (keep it pure and serializable and this stays
  mechanical)
---
 
## 13. Open questions
 
- **Decay thresholds** are guesses until Phase 4 data exists. Instrument early.
- **Elision policy per node type** is the highest-leverage tuning surface and is
  domain-specific; expect to iterate.
- **Selective undo** if multiplayer arrives — the op foundation is necessary but not
  sufficient.
- **Temporal solver convergence** on cyclic or over-constrained relation graphs needs
  a cycle detector and a defined failure mode.
- **Whether short sequential IDs beat ULIDs** on net, once ID token cost in large
  projections is measurable.
 