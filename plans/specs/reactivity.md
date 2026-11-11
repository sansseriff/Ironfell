# Reactivity: one model for values, structure, state and events

**Proposed 2026-11-09. Phases 1 to 5 built the same day** (§13, where what was built differs from §3–§7 it says so); phases 6 and 7 wait on a need. Follows the council review of `e6c81de`
(repeat, let, components as markup; [`repeat.md`](repeat.md) §9) and a study of Svelte 5's
runtime source. It answers the council's problems and the user's six decisions, and
orders the work as phases. Nothing here is built.

The goal: UI and diagrams as expressive as most Svelte or Solid code, in a document that
stays saved, undoable, inspectable and agent-editable, with no general interpreter.

## 1. What was studied

**Svelte 5.57.2**, read in `node_modules/svelte/src/internal/client/`:

| Mechanism | Where | What it does |
|---|---|---|
| Sources and deriveds | `reactivity/sources.js`, `deriveds.js` | A write marks direct readers `DIRTY` and the rest `MAYBE_DIRTY` (`mark_reactions`); nothing recomputes until read |
| Pull with versions | `runtime.js` `is_dirty`, `update_reaction` | A `MAYBE_DIRTY` reader asks each dependency to update first, then compares write versions: glitch-free, with equality cutoff |
| Dynamic dependencies | `runtime.js` `update_dependencies` | Every run re-collects what was read and replaces the dependency list |
| Errors as values | `runtime.js` (`ERROR_VALUE`) | A derived that threw holds the error; reading it rethrows, so readers fail too instead of seeing a stale value |
| Owner tree | `reactivity/effects.js` (`branch`, `block`, `destroy_effect`) | Every piece of structure and state belongs to an effect; destroying a parent destroys what it owns |
| Keyed lists | `dom/blocks/each.js` (`items: Map<key, EachItem>`, `reconcile`, `create_item`) | One owner per key; each item has **its own sources** for value and index (`v`, `i`), written by the block, so an item re-runs only when its own element changes |
| Conditionals | `dom/blocks/if.js`, `branches.js` | A map from branch key to owner: the same mechanism as a list |
| Exit transitions | `effects.js` `pause_effect` | A removed branch is paused, plays its outros, then is destroyed; re-adding it resumes it |
| Async | `reactivity/async.js`, `batch.js`, `blocks/boundary.js` | Async deriveds, a pending state per boundary, stale results aborted |

**Solid** (1.x, from knowledge; its repository could not be fetched): the same graph with
explicit calls. `mapArray` (`<For>`) keys items by reference and gives each a root that is
disposed when the item leaves; `indexArray` (`<Index>`) keys by position and gives each
slot a signal for its item, which is what Iron's repeat does today. `createStore` tracks
each property of nested data; `createResource` exposes `loading`, `error` and the value.

**Prior art closer to Iron than either:** Jane Street's Incremental: a graph with static
heights, recomputed in height order with cutoff, plus one `bind` node for structure that
depends on a value, whose height is adjusted when it runs. Iron's `graph.rs` is already
this design without `bind`.

## 2. Two decisions about scope

**No compiler.** Svelte's and Solid's compilers turn JavaScript and templates into calls on
their runtimes (`derived(fn)`, `each(…)`, `if_block(…)`). Iron's document already is that
output: bindings name their inputs, structure is nodes. What transfers is the runtimes'
design, the nine mechanisms above, not the compilers.

**Dynamic dependency tracking: yes, for one construct.** The frameworks track at run time
because a JavaScript function is opaque. Iron's expressions are a closed tree, so
`Expr::deps` already lists a superset of what any run reads. That gives three things worth
keeping: a cycle is refused when the edit applies (`op.rs` `check_binding`), the wires UI
can draw dependencies without running anything, and an agent can read them in the view.

Static dependencies cannot express a read whose *target* is a value: "the x of the
selected node", "the label of the item this row refers to", a value from the nearest
provider. That is a real gap for UI state, and it is the only one. So:

- Static dependencies stay the rule.
- One new expression form, a **tracked read** (`deref(ref, facet.field)`, and context
  lookup, §6), records what it read during evaluation, as `update_dependencies` does.
- Evaluation becomes pull within the existing height order: if a tracked read reaches a
  dirty leaf that has not run yet, that leaf runs first; a leaf found already running is a
  cycle and evaluates to an error (§4.1). `graph.rs`'s header already names its single
  resolver as the hook for this.

Conditional branches keep both branches as dependencies. The cost is a re-evaluation that
the equality cutoff then stops, never a wrong value. Branch pruning becomes necessary only
for calls with a cost (§8), which are lazy on their own.

Code-bodied deriveds stay out, as the user decided: they need an interpreter.

## 3. The model

Five concepts. Each exists in part today.

| Concept | Is | Svelte / Solid | Today |
|---|---|---|---|
| **Value** | A leaf: a constant, a binding, or published by a producer | `$state`, `$derived`; signal, memo | Built. `let` names one |
| **Scope** | An owner of derived structure and state: an instance, a repeat item, a branch | Branch effect; owner/root | Two separate cases in `expand.rs` (`want_use`, `want_items`) |
| **State** | A `let` holding a constant, written by actions, per scope | `$state` in a component | `let` built; writes and tiers not |
| **Action** | A declared write, run when an event fires | Event handler | `affords.activate` (toggle, step, choose) only |
| **Producer** | Something outside the formula language that publishes values over time | `$effect` + external store; resource | Built: clips, layout, machines, network, theme, hosts |

### 3.1 Scopes: one structural mechanism

Today a component instance and a repeat are expanded by different code in a fixed order,
which is why a `use` inside a repeat's template is never expanded, relations are not
copied for items, and settling is a whole-document pass.

One recursive function replaces both: **expand a template into a scope**. A scope is
`(owner path, key)`. Its copies' ids are the owner's path, the key, then the template
node (as now). Expanding a template node that is itself a `use` or a repeat opens child
scopes. So repeats of components, components holding repeats, and nested repeats all
work by construction, and every scope is disposed with its parent.

Each **repeat item is a real node**: a derived group `#g/~key` that holds the item's
copies and carries `item.value` and `item.index` as its own leaves, published by the
structure pass. Copies read *their item's* leaves. This is Svelte's `create_item`: an
item re-evaluates only when its own element changes, where today every copy reads all of
`each` (`at(each, k)`). The item node is also what is selected, moved by a layout, and
given state.

A **conditional** is a scope with at most one branch: `when="{= cond}"` on a group, the
same mechanism with keys `true` and `false`, as Svelte's `if` is a map of branches.

### 3.2 Keys

`repeat.key` is an expression over the item (`{= item.id}`). Defaults, in order: a node
reference is its own key; a record with an `id` field uses it; otherwise the value when
all values are distinct (tick labels slide when a range changes); otherwise the index.
`count` repeats use the index.

The key is the id's path step (`#g/~B12/7`, `#g/~450/7`), interned like field names, so
everything attached to an id (selection, overrides, state, a running transition) follows
the item when the list is reordered or grows at the front. Duplicate keys are an error
on the repeat; the duplicates fall back to index keys so the picture still draws.

### 3.3 When structure settles

Structure settles in the same pass as values, not at the next commit:

1. Evaluate dirty values (height order, pull for tracked reads).
2. For each scope owner whose structural input changed (`each`, `count`, `key`, `when`),
   reconcile *that owner only*: create, delete and reorder item scopes, publish item
   leaves.
3. Evaluate the bindings of new copies. Repeat from 1 while anything changed.

This runs after transactions, after a producer publishes, during gesture previews and
when the clock moves. So a list fed by live data grows without an edit, and a slider that
drives a count adds copies while it is dragged. The opacity guard that hides stale copies
goes away.

Derived structure made outside a transaction is not history. The store queues the ops and
the bridge drains them in `sync_document`, the path the generated timeline composition
already uses (`timeline_view::sync_themed`).

**Bounds.** A repeat's `each` may not read its own copies (refused when the edit applies:
copies' ids start with the repeat's path). Nesting deeper than 16 scopes is reported as a
problem on the node and still expands; 128 is a hard stop. One total budget of derived
nodes per document (default 50,000) replaces per-repeat caps multiplying. Reaching any
limit is reported (§4.3), never silent.

### 3.4 Document copies or host instances

Both exist, for different jobs:

- **A repeat** makes nodes: each copy can be selected, bound to, labelled and animated.
  Right for tens to a few thousand things with identity.
- **A host instancer** (the gas demo's 160 Bevy particles, `document_bridge/gas.rs`)
  makes engine entities with no nodes, driven by document leaves and publishing back as a
  producer. Right for swarms where no member matters by itself.
- **Path data** (`circles`, `segments`) is one node for any number of marks.

## 4. Fixes to the reactive core

### 4.1 Errors propagate

A binding that fails keeps its previous value today (`graph.rs:425`), and its readers
compute from it as if it were current. Instead a failed leaf holds its error, as Svelte's
`ERROR_VALUE` does: reading it fails the reader with "depends on #x, which failed: …".
The renderer keeps the last drawn value, and the view and the inspector mark the leaf
`stale` with the error. A scientific display must not show a number computed from a
failed input as if it were good.

### 4.2 Presentation state flows through lets

Hover, pressed and selected may drive only how a node looks (`op.rs` `check_binding`).
The check looks at the binding's direct target, so hover → `let` → fill is refused while
hover → fill is allowed. The rule becomes transitive: a leaf whose dependencies include
interaction state is *presentation-derived*; such a value may be read by look facets and
by other lets and channels, and never by a leaf that is saved content. `transitive_deps`
already computes what the check needs.

### 4.3 Problems are data

`expansion_problems` is a list of strings, replaced on each expansion. Problems become
records on a node (kind, message), published like any runtime value: depth over 16, the
node budget reached, duplicate keys, a truncated repeat, a failed binding. The layers
panel and the inspector show them; the agent's view prints them. This is where "depth
beyond 16 should introduce an error in the UI" lands.

## 5. State and actions

### 5.1 State is a `let`

A `let` holding a constant is a state cell; where it is declared decides whose it is,
which is already how expansion works:

| Declared in | One per | Example |
|---|---|---|
| A definition's body | instance (entity), shared by its pictures | a device's on/off |
| A definition's template | picture (`use`) | an accordion's open flag |
| A repeat's template | item | a row's "expanded" |
| A composition | document | the current tab |

`let.keep` says how long it lives:

- `document` (default): saved and undoable. A write to a derived copy's `let.value` is
  stored as an override on the instance, the mechanism that already stores any edit made
  inside an instance (`expand::translate`), so the definition's value is the initial
  value and each instance keeps its own.
- `session`: not saved, not in undo, kept by the store per id for as long as its scope
  lives, reset to the initial value when the scope is disposed. For hover-like UI state.

The other kinds of state are not lets and already have homes: interaction state (the
host publishes it), a machine's current state (an argument), values over time and
results of calls (producers, §8).

### 5.2 Actions

Until now every write came from a person's gesture or an agent's edit, both already
transactions. An action is the first time the document itself declares a write, which is
why its place in history has to be decided now. It is the general form of
`affords.activate`:

```xml
<on event="click" set="$open" to="{= !$open}"/>
<on event="click"><set leaf="$count" to="{= $count + 1}"/><set leaf="$open" to="true"/></on>
```

- **Triggers:** pointer and key events on the parent node (click, press, release, enter,
  leave, key), a machine entering a state, a leaf crossing a condition (`when="{= … }"`,
  fires on false → true).
- **Verbs**, a fixed list: `set` (a leaf, to an expression read *before* any write in
  this action), `step`, `toggle`, `emit` (an event to the host or the narrative),
  `call` (start a query, §8). No loops, no reads after writes: an action is a parallel
  assignment, so it is total and its result does not depend on order.
- **History:** one firing is one transaction, one undo step, labelled by the action. Sets
  on `session` lets are session ops and leave no history.
- An action whose writes re-trigger it within the same pass is cut after a fixed number
  of rounds and reported.

This is Svelte's event handler as data. `$effect` (run arbitrary code when anything
read changes) is deliberately not borrowed: derived values cover what effects are
misused for, and the rest is producers.

## 6. Context

A group may `provide` named values; `ctx.name` in an expression reads the nearest
provider above the reader. It resolves when the graph is built for authored nodes and as
a tracked read inside copies. Theme tokens (`@ink`) are the existing special case. Build
when ports become a burden, not before.

## 7. Collections

Three value kinds, the second new:

- **Series** (built): numbers with one unit, packed. The fast path for data.
- **List** (new): an ordered run of any values: numbers, text, node references, records.
  A series is accepted wherever a list is.
- **Record** (built, `Value::Record`): named fields, read as `r.field`.

Floor functions: `list(…)`, `len`, `at`, `map`, `filter`, `sort_by`, `zip`, `enumerate`,
`unique`, `record(name: value, …)`, `get(r, 'field')`, `children(#node)` (a list of
references, for structure that follows the document). `repeat.each` takes any of them.

Tables stay what they are, a node type for laid-out cells. Later a table can publish its
columns as lists, and "a card per row" is a repeat over them: built from the primitive,
not a second collection.

Not borrowed: deep proxy stores. A list or record is a value; changing one element makes
a new value, and per-item leaves (§3.1) give the fine-grained updates proxies exist for.

## 8. Async and time

**Query.** A `query` node names a Model call and its arguments and exposes `status`
(`idle`, `pending`, `ready`, `error`), `value` and `error` as leaves a producer
publishes. It runs only while something drawn reads it, debounces on commit, keeps the
last good value while pending (shown stale, as §4.1), and drops a response that arrives
after a newer request. Solid's `createResource` without Suspense: the pending state is a
leaf the picture binds to, not an implicit boundary. Details in `model-interop.md`.

**Accumulators.** A value that depends on its own past (a counter of events, a running
mean, a history ring, an integration step) cannot be a formula: formulas are pure. Each
is a producer node with declared inputs, stepped by the clock or by an event, publishing
its value, resettable, and frozen into the document on request (as the gas demo's
pressure is). Counters that should be undoable are actions on a `let` instead.

## 9. Not borrowed, and why

| Idea | Why not |
|---|---|
| Dynamic tracking for every read | Loses apply-time cycle refusal and static wires; §2 keeps it for the one case that needs it |
| Code-bodied deriveds, `$effect` | An interpreter; unbounded; not inspectable |
| Deep proxy stores | Hidden identity and mutation; §7's values plus item leaves cover the use |
| Suspense boundaries | Implicit control flow; a `status` leaf is explicit and bindable |
| Laziness everywhere | Eager evaluation is fine while formulas are cheap; only queries are lazy. Measure first |

## 10. How each council finding is answered

| Finding | Answer |
|---|---|
| `each` takes numbers only | Lists (§7) |
| Structure settles only on commit; a publish never settles; the opacity guard | Settle with values, per owner (§3.3) |
| Identity by position | Keys in ids (§3.2) |
| No component inside a repeat; relations not copied | One scope expansion (§3.1) |
| Limits fail silently | Problems as data; depth 16 reported, 128 blocked; a total node budget (§3.3, §4.3) |
| A let cannot carry hover state | Transitive presentation check (§4.2) |
| Errors keep stale values | Errors propagate (§4.1) |
| Every copy depends on all of `each` | Item leaves (§3.1) |
| Whole-document settle and rebuild | Per-owner reconcile; graph updated for the copies changed (§3.3) |
| Enter and exit | Phase 7: a removed scope is paused while its `disappear` plays, as `pause_effect` |

## 11. Phases

| Phase | Builds | Exit |
|---|---|---|
| 1 | Core fixes: errors propagate, transitive presentation check, problems as data, depth 16 / 128 | A failed input marks its readers stale in the view; hover → let → fill works; a 17-deep nest shows a problem in the layers panel |
| 2 | Scopes: one expansion for instances, repeats and conditionals; item nodes with their own leaves; settle with values through a derived-op queue; the opacity guard removed | A repeat of `std:meter` draws; dragging a slider bound to `count` adds copies during the drag; a host publish grows a list with no edit; changing one element re-evaluates one item |
| 3 | Lists and records on the floor; `repeat.key`; keys in ids; `when` | A legend over `list('a','b','c')`; inserting at the front keeps each item's selection and state; plot tick labels keyed by value |
| 4 | State and actions: `let.keep`, writes through overrides, `on` with its verbs | An accordion component built in markup: click toggles, each instance separate, undo steps back one click; a session-only hover highlight |
| 5 | Tracked reads: `deref`, pull evaluation, run-time cycle error | "Inspector" text bound to the selected node's width updates on selection change and on resize |
| 6 | Query node; accumulator producers; `provide` / `ctx` | A plot of a Model's output with a pending state; a running-mean readout of a live signal |
| 7 | Enter and exit through machines; reorder by key | Tick labels fade in and out on a range change; dragging a row reorders its source list |

Phases 1 and 2 are the council's concerns and should come first; 3 and 4 are what make
UI state expressive; 5 to 7 can each wait on a need.

## 12. Open

1. Whether `session` lets should survive a conditional removing and re-adding their
   scope (Svelte resets; a "keep alive" flag is the alternative).
2. Whether an item node's layout facets come from the repeat or from the template's
   single top node.
3. The node budget's default, and whether it is per document or per composition.

## 13. Done

**2026-11-09: phases 1 and 2.**

Phase 1, the core fixes:

- **Errors propagate** (§4.1, `graph.rs`): a leaf whose input failed fails too, with
  "depends on #n.leaf, which failed: …" naming the root cause; it recovers when the
  input does. A read from outside the graph still returns the last good value.
- **Presentation state through lets** (§4.2, `op.rs`, `Document::reads_interaction`):
  the look-only rule is transitive. A `let` may hold an interaction-derived value;
  a content leaf that reads it, at any distance, is refused.
- **Problems are data** (§4.3): `expand::Problem {node, kind, message}` from expansion
  (depth, node budget, a repeat cut at its max) plus binding errors, read with
  `Store::problems()`. The model's view prints `problem="…"` on the authored node
  (rolled up from derived nodes); the layers panel shows a warning marker with the
  message as its tooltip.
- **Depth:** nesting past 16 is reported as a problem and still expands; 128 stops it
  (`DEPTH_REPORTED`, `DEPTH_MAX`); 50,000 derived nodes is the budget.

Phase 2, scopes:

- **One expansion** (`expand.rs`, `want_scopes`): a worklist expands instances, pictures
  and repeats until nothing is left, so a component inside a repeat's template and a
  repeat inside a component both work. Content inside a template is never expanded in
  place, only in its copies.
- **Item nodes:** each item of a repeat is a derived group `#g/~k` with an `item` facet
  (`value`, `index`) the store publishes (producer `ITEMS`); copies are `#g/~k/local`
  and read their own item's leaves. Changing one element re-evaluates that item's
  readers only (2 evaluations in `tests/reactivity.rs`).
- **Structure settles with values** (§3.3, `Store::settle_repeats`): whenever values
  resolve (a transaction, a gesture preview, a producer's publish, a clock tick), each
  drawn repeat's count is read; where it differs, copies are made or removed and the
  ops queue for the host (`take_derived_ops`), which the bridge drains each frame. A
  drag of a slider bound to `count` adds copies during the drag, with no transaction.
  The opacity guard is gone.
- **A component per item:** an entity and a `use` with a figures link in a template
  give each item its own instance (`tests/reactivity.rs`, a repeat of `std:meter`).
- **Edits to a copy are refused** with the template to edit instead.

**Differs from the proposal:** `when` (conditional scopes) is left to phase 3 with
keys, so phase 2's "conditionals" is not built. A count change re-expands the whole
document and rebuilds the graph, rather than the one owner; fine at the sizes tested,
and the place to look if a large document with a live repeat is slow.

**Checked:** `tests/reactivity.rs` (6), `tests/repeat.rs`, `tests/plot.rs`, the whole
`iron_document` suite; `bun test` (100); the fast smoke. In the app: a repeat whose
count reads the level slider goes from 4 to 8 dots mid-drag at the same document
version, the release commits one "set slider", undo returns to 4; a placed `std:plot`
draws its tick labels; a repeat asking for 5 with `max` 2 shows the marker in Layers.

**2026-11-09, later: phases 3, 4 and 5.**

Phase 3, lists, keys and `when`:

- **Lists and records** (§7, `list.rs`, `Value::List`): `list(…)`, `record(name: value, …)`,
  `get`, `len`, `at`, `first`, `last`, `map`, `filter`, `sort_by`, `zip`, `enumerate`,
  `unique`, `concat`. A series is taken wherever a list is; `map` and `filter` over a
  series give a series back. A bare `#id` in a formula is the node itself, a reference
  value (`list(#3, #4)`), and follows into copies as leaf references do. Not built:
  `children(#node)`.
- **Keys in ids** (§3.2, `id.rs`): an item's path step is its key as text, interned
  (`#g/~beta/7`, `#g/~3/7`). `repeat.key` is a function of the item (`{= r => r.id}`).
  Without one: a list of node references is keyed by node, of records by their `id`,
  of words by word when all differ, anything else by index. Two items with one key is
  a problem on the repeat, and it falls back to index keys.
- **`when`** (§3.1): a `when` facet (`is`) on a group. Its children are a template with
  one derived copy, `#g/~on/…`, while the condition is true. A repeat and a
  conditional are the same code path (`expand::is_item_owner`).
- **A copy's values can differ per item:** a constant set on a copy is stored as an
  `override` on the repeat, targeting `~key/local`, as for an instance's part. It
  follows the key, survives the item leaving and returning, and is saved.

Phase 4, state and actions:

- **`let.keep`** (§5.1): `document` (default) or `session`. A write to a derived `let`
  is an override on its instance or repeat, so each picture and each item has its own.
  Session cells are held by the store, published under `ProducerId::STATE`, never
  saved, never in undo, and dropped when their node goes (open question 1: they
  reset, as Svelte's do).
- **`on` nodes** (§5.2): `on.event` is `click`, `press`, `release`, `enter` or `leave`;
  it listens on its parent, or on `on.node`. An event on a node reaches the nearest
  listener at or above it. A write is a `write` facet (`target`, `prop`, default
  `let.value`, `to`, or `verb` `toggle` or `step` with `by`), on the `on` itself or on
  `set` children. All writes read the values from before the action; one firing is
  one transaction named by the `on`. `Store::fire`, `fire_at`, `handlers`,
  `hover_listeners`.
- **The host** (`bevy_app/select.rs`): a press on something that listens runs the
  action on release instead of selecting it (Cmd-click selects); the pointer coming
  and going fires `enter` and `leave`.

Phase 5, tracked reads:

- **`deref(ref, 'facet.field', fallback?)`** (§2): the graph records what a binding
  holding one read at each evaluation and keeps those as its dependencies until the
  next. A dependency found late in a pass is evaluated first and the binding again. A
  read that reaches its own binding is an error on that leaf ("a cycle"), not a hang.
- **`selection` facet**: a `let` or group that carries it has `selection.node`,
  `.nodes` and `.count` published by the store, so a binding can read the selected
  node.

Phase 6, first part (2026-11-10): context, traces, accumulators.

- **Context** (§6): `@name` is the context read, and colour tokens are its root
  providers rather than a special case. `let.provide` is `no`, `yes` or `default`;
  `palette::provided` finds the nearest providing `let` of that name among the
  children of the reader's ancestors, then the document's token, then the nearest
  `default` let (a component's own fallback). Resolved per reader when the graph is
  built (a rename, a reparent or a change of `provide` rebuilds), so a component reads
  it with no port, and a group's own `ink` shadows the theme's for what is under it.
  A definition may read `@name`: it is the one sanctioned read past its boundary,
  under three rules (2026-11-10, `expand::check_scope`, `ErrorKind::OutOfScope`):
  1. **Template only.** Context depends on where a picture is placed, and one instance
     may have several, so inside a definition only a template may read `@name`: never
     an out port or a model-level `let`. Placement changes how a component looks, not
     what it computes; what changes behaviour is an input port.
  2. **Declared, with a default.** The template must hold a `let` of that name that
     provides (`provide="default"` is the fallback) above the reader. Reading an
     undeclared name is refused when written, and so is taking a declaration away
     (a rename, a delete, a reparent, `provide="no"`) while something reads it.
  3. **Shown.** `Store::context_reads(figure)` lists what a picture reads and the node
     providing each, or that its own default serves; the inspector's component section
     shows it under "From context".
  Colour tokens are the theme and are exempt from 1 and 2.
- **Stepped producers** (§8, `step.rs`, `ProducerId::STEPS`, `Store::advance_steps`):
  presentation time only, never saved, never undone, **not seekable** (decided
  2026-11-10: a wire is watched in the immediate mode, and scrubbing leaves it be).
  - A **`trace`** node records when the value it follows (`trace.of`) changed, for
    `trace.delay` seconds: change events with times, not per-frame samples, so a
    one-frame pulse is not missed. Published: `out` (the value that long ago), `dash`
    (where it was on along a path of length 1, as a `stroke.dash`), `ages`, `values`.
  - An **`accumulate`** node: `next` is a formula of its own `value` and `dt`. The
    store publishes the step's `dt`, reads `next`, and publishes it as `value`;
    `reset` returns it to `initial`. A step is at most 0.1 s.
  - Both may be children of a definition, copied to each instance as channels are
    (a signal's delay is the model's, not one picture's).
- **`std:fiber`** has a `delay_s` input: `out` is the input that long ago, and a
  `light` path over the curve shows it travelling. `fiber.txt` (run-iron) clicks a
  button and watches the pulse leave, arrive and its tail follow.

Phase 6, second part (2026-11-10): the query node.

- **`query`** (§8, `query.rs`, `ProducerId::QUERIES`): `model`, `input` (usually a
  `record(…)`), `debounce` (seconds, default 0.2) and `initial`; published `status`
  (`idle`, `pending`, `ready`, `error`), `value` (`initial` until the first answer),
  `error`, `stale` (the value answers older input). The store never calls out:
  `Store::advance_queries(now)` answers the requests that became due and
  `resolve_query(node, seq, result)` takes an answer, dropping one a newer request
  overtook. Asked only while a binding reads one of its outputs; the first request
  is immediate, later ones wait for the input to rest; a failure keeps the last good
  value. May be a child of a definition, per instance, as a trace is.
- **The host**: `motion::advance_queries` sends `EngineEvent::Queries` and the
  `query_result` command takes the answer. The web host's `ModelLoader`
  (`src-ui/runtime/model_loader.ts`, in the worker) treats `model` as a URL: POST the
  input as JSON, the response's JSON is the value (`query::from_plain`: an array of
  numbers is a series, an object a record). A newer request aborts the one in
  flight; 60 s is a timeout. This is the smallest tier 1 of `model-interop.md`, over
  HTTP rather than its WebSocket protocol, with no `describe`, progress or cancel
  message to the Model.
- `query.txt` (run-iron) plots a 1.5 s Model's answer through `std:plot` and reads
  pending, ready, pending-stale and ready again.

Phase 7, first part (2026-11-10): enter and exit.

- **Presence**: `repeat.enter_ms` and `repeat.exit_ms` (`when.enter_ms`,
  `when.exit_ms`). A key that joins has `presence` rise 0 to 1; a key no longer
  wanted is kept, `leaving`, with the element and index it had, while `presence`
  falls, and is then removed; a key wanted again while leaving turns back from where
  it is (machines.md M8's rule). The template reads `$g.repeat.presence` and
  `.leaving` as it reads `item`: expansion maps them to the item's own leaves
  (`item.presence`, `item.leaving`, `ProducerId::PRESENCE`). What it looks like is a
  binding (opacity, an offset through `ease`), so there is no second animation
  vocabulary. `Store::advance_presence(now)` on presentation time; what is there
  when an owner first settles is simply there.
- Leaving items come after the wanted ones in order. They are still pickable.

Phase 7, second part (2026-11-10): a parent's machine over its children's
(machines.md, "Parents over children"). `machine.at` drives a machine to a position
along its states, `machine.running` says a machine is moving, and a parent's tweened
channels walk each nested part through the part's own transition, handing back at rest.

Continuous events and the zoomable plot (2026-11-10):

- **`on.event` `drag` and `wheel`**, with what the event carries published on the
  `on` node (`ProducerId::EVENTS`): `dx`, `dy` (document px since the last event), `x`,
  `y` (the pointer in the listening node's space), `delta` (the wheel, px).
  `Store::pointer_action` **previews** the writes, so each event reads what the last
  left and the document is untouched until the host commits: a drag on release, a
  burst of wheel once it has rested 0.3 s, each one undo step named by the `on`.
- **The host** (`bevy_app/select.rs`): a press on something that listens for `drag`
  becomes `Gesture::Action` when it travels; `navigate_views` gives the wheel to a
  listener under the pointer before the view (Ctrl/Cmd+wheel still zooms the view).
- **`std:plot` pans and zooms**: three state lets on the instance (`zoom`, `centre x`,
  `centre y`, as fractions of the base range) from which `x_lo` … `y_hi` are computed;
  a `plot area` rect with a `drag` action (pan both axes) and a `wheel` action (zoom
  about the pointer). No machine is involved: state, two actions, and bindings. The
  margins still move the plot. `plot-view.txt` (run-iron).
- **`std:laser` pulses**: `pulse_hz` and `duty`, timed by an `accumulate` phase.
  `pulses.txt` (run-iron) runs it through a 2 s `std:fiber` into `std:meter`: three
  pulses on the curve at once, and the meter reading 12 mW and 0 in turn.

**Differs from the proposal:**

- Numbers are keyed by index unless `repeat.key` is given (§3.2 said by value when
  distinct). Keyed by value, a range that slides remakes structure on every change,
  and with index keys only the values move. For the same reason `std:plot`'s tick
  labels keep index keys until phase 7 gives value keys something to show (labels
  that slide and fade); `tests/reactivity.rs` shows ticks keyed by value.
- Actions have no key events, no machine-state or condition triggers, and no `call`
  (phase 6). `on.emit` is queued in the store (`take_emitted`); no host reads it yet.
- A template's own bindings are still evaluated once (reading item 0, or as if a
  `when` held), so a false conditional costs its template's bindings, not its copies'.
- `deref` inside a lambda's body does not resolve (the body is closed before it runs).
- Tracked reads are opaque to render analysis: a part that uses one is drawn live.
- `ctx.name` is written `@name`, and there is no `provide` node: a `let` provides.
  A definition declares what it reads with a default `let` in its template (above).
- An accumulator at rest costs nothing: the step length is republished only while a
  formula moves its value (`Steps::restless`). Before this fix (2026-11-10) a placed
  `std:laser` redrew its composition every frame, because frame times jitter and each
  new `dt` counted as a change.
- A trace shows on or off along the path, not a level: analog light (brightness
  following power along the fiber) needs a gradient the trace does not yet give.
- A query has no native host yet (the Tauri shell does not answer `Queries`), no
  progress, and the Model's reply carries no units: numbers arrive plain.
- Enter and exit are a `presence` number, not a `vis` machine on the item (repeat.md
  §3 said machines): a template that wants a designed appear binds its component's
  `vis` to `presence > 0.5` or `!leaving`. Linear in time; shape it with `ease`.
- The plot has no reset of its view yet (undo steps back), no zoom of one axis alone,
  and its tick labels are still index-keyed. There are no key events.
- Still whole-document: any change to a repeat's items or a conditional re-expands
  the document and rebuilds the graph.

**Checked:** `tests/reactivity.rs` (14: the floor's lists and records; a legend over a
list whose items keep node, state and selection when a word is added in front; ticks
keyed by value; a conditional; an accordion component in markup, per instance, one
undo per click, session hover; parallel writes; a readout of the selected node's
width following selection and resize; a tracked cycle), the whole `iron_document`
suite, `bun test` (115). In the app, `actions.txt` (run-iron skill): an accordion made
through the model's edit surface, hovered, clicked twice and undone with the real
pointer.
