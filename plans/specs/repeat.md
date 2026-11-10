# Repeat: one child per element, derived

**Proposed 2026-11-08. Phase 1 built 2026-11-09** (§9), with `let` (§7) and node markup
(§8), so that `std:plot` is defined by markup alone. Where §2–§4 and §9 differ, §9 says
what was built. **Reworked the same day** by reactivity.md phases 1–2 (§9, second
entry): item nodes, live settling. Phase 2 (value keys, enter and exit) is open and
now belongs to reactivity.md phases 3 and 7.

## 1. Why

Some things must be separate nodes and their count depends on data: tick labels (a path
cannot hold text), a brightness strip whose cells each have their own opacity (a path
has one fill), wavelet rings fading by ring, bars a reader drags one at a time, a card
per table row. A template's structure is fixed, so today the choices are a fixed number
of hidden slots, or the agent writing every copy out by hand (the 2026-11-07 log: 61
rects, ten ellipses). Drawing-only repeats (tick marks, grid lines, a single-colour
scatter) don't need this: one path with generated SVG path data does them (plot.md §2).

## 2. The node

```xml
<repeat each="{= ticks(x_min, x_max, n)}" as="t" key="value">
  <text x="{= scale(t, x_min, x_max, 0, w)}" content="{= fmt(t) + ' nm'}" …/>
</repeat>

<repeat count="10" as="i">
  <ellipse rx="{= (i + phase) * spacing}" appearance.opacity="{= 1 - i / 10}" …/>
</repeat>
```

- `repeat` is a 2D node type (a group's facets plus a `repeat` facet: `each`, a series,
  or `count`, a number; `as`, the item's name; `key`, `index` or `value`).
- Its authored children are the **template for one item**. Inside them, the name in
  `as` (and `i`, the index, always) are readable in bindings, scoped like a lambda's
  parameter (plot.md §1.4): locals, never dependencies.
- What it draws are **derived copies**, one per element, with ids `#rep/<key>/<local>`,
  never saved, like a linked instance's parts (component-model.md §6). The view shows
  the repeat and its template, with `count="N"` and copies elided, as it elides an
  instance's picture; `<look expand="#rep"/>` shows them.
- A cap (1,000 copies by default, settable up to 10,000); past it the repeat draws the
  first N and the view says so.

## 3. Identity: by index or by value

When `each` changes, which old copy is which new one?

- **`key="index"`**: copy 3 is the third element. Labels change text in place. The
  default for `count`.
- **`key="value"`**: copies match elements of equal value, React's `key`, D3's data
  join. When an axis range grows, the "500" label slides to its new place; "750"
  **enters**; a label that falls off **exits**. The default for `each`.

Enter and exit tie to machines: an item whose template's definition (or the repeat
itself) has a `vis` machine plays `appear` on entering and `disappear` before it is
removed (D3's enter/update/exit). An exiting copy is kept, derived and unpickable, until
its transition ends.

Overrides on a copy (restyle one label) are keyed the same way: by index, or by value,
and kept as stale when their key disappears.

## 4. The hard part

The document's structure has so far never depended on resolved values: the graph
evaluates bindings over a fixed tree, and only `timeline_view.rs` generates nodes, from
code. A repeat makes the set of nodes a function of a value. The proposal:

1. A repeat's copies are produced by the expansion pass (expand.rs), which already turns
   documents into derived nodes, keyed by a derived-structure revision.
2. After the graph settles, any repeat whose `each` changed in length or keys marks
   structure dirty; expansion re-runs for that repeat only; the graph rebuilds the
   copies' bindings and evaluates them. At most one re-expansion per repeat per frame,
   so a repeat whose `each` reads its own copies cannot loop: that is a cycle, refused
   statically like any other.
3. Value-only changes (same keys) re-evaluate bindings without touching structure, the
   common case for a slider dragging a curve.

## 5. Open questions

1. Nested repeats (a grid of cells: rows × columns) with `i`, `j`? Proposed: allowed,
   with each level's `as` name in scope.
2. `each` over a table's rows (one card per row) waits on tables publishing columns as
   series (layout-and-tables.md's data-table follow-up).
3. Can a repeat appear outside templates, directly in a composition? Proposed: yes; it is
   as useful to the agent there.

## 6. Phases

| Phase | Builds | Exit |
|---|---|---|
| 1 | `count` and `each` with index keys; expansion; view elision | 61 strip cells from one repeat; dragging a slider that changes `count` adds and removes copies; undo restores |
| 2 | value keys, enter and exit through machines | a plot's tick labels slide on a range change; new ones fade in |
| 3 | `std:plot` replaces its label slots with repeats | |

## 7. Let: named values

A `let` node holds one value a binding computes, for other bindings to read as
`#n.let.value`: a series, a number, a string, a function (`v => scale(v, lo, hi, 0, w)`).
It is never drawn. It is the floor's *derive* verb as a node (vision 02 §6.2), and what
replaces a builder's habit of pasting one expression into many bindings.

- In a **definition's template**, each picture gets its own copy (it is template
  content), so a let may read the picture's box (`#T.size.w`).
- In a **definition's body**, each instance gets its own copy, like a channel: values
  the outputs share (`std:plot`'s every-series `ys_all`) without becoming ports.
- Anywhere else (a composition, a group) it is just a node.

## 8. Node markup

`markup.rs` reads nodes written as XML: the element is the node type, attributes are
its leaves (`facet.field`, or a bare field: the first of the type's facets, in registry
order, that has it), children are child nodes. A value is a constant of the leaf's kind,
a colour token (`@ink`), or a binding `{= expr}`. `id="x"` names a node for the rest of
the markup: `$x` is its id in a binding or a reference; a bare `$x` in a binding reads
the value a `let`, `port_spec` or `channel` holds. An input declared `choice(…)` holds an
enum. This is the model's `<create>` vocabulary, nested.

The standard components are written in it (`src/iron_document/src/std/*.xml`) and read
when first placed; `standard.rs` keeps only the list of entries and the file for each
key. Nothing in Rust knows what a standard component draws.

## 9. Done

**2026-11-09: phase 1, `let`, node markup, `std:plot` as markup.**

- **The repeat is a facet on a group**, not a node type: `repeat.each`, `repeat.count`,
  `repeat.max` (1,000; at most 10,000), and the read-only `repeat.item` and
  `repeat.index`. Everything that handles groups (transforms, clipping, layers, the
  select tool) handles a repeat.
- **No free names.** Instead of `as="t"`, the template reads `#g.repeat.item` and
  `#g.repeat.index`, ordinary leaves, so a saved template parses and evaluates like any
  binding (the template reads item 0, so it has no errors). Expansion rewrites those
  references in each copy: the item becomes `at(each, k)` (or `k` without a series),
  the index the literal `k` (`Expr::replace_refs`).
- **Copies** are derived by `expand.rs`, ids `repeat path / ~k / template node`
  (`NodeId::item`, shown `#4/~3/7`), never saved, never in undo. Template references
  inside a copy follow to that copy's nodes. A repeat inside a definition's template
  expands in each picture's copy, not in the library. Repeats in copies expand in turn.
- **When the count changes** (§4): after every transaction, undo, redo and load, the
  store reads each drawn repeat's live count and, where it differs from its copies,
  re-expands (`Store::settle_repeats`); the ops join that transaction's derived
  structure, so hosts see them as any expansion. Between transactions (a drag, a clip
  playing) the number of copies stays; a copy past the live count is hidden (its top
  nodes' opacity is guarded by `k < count`), and missing ones arrive at the commit.
- **Templates are not drawn or hit**: `expand::in_repeat_template`, used by the bridge's
  materialiser, the spatial index and the Library tab's baked previews.
- **Views** show the repeat with `copies="N"` and its template; copies are elided.
- **`std:plot` is `std/plot.xml`**: inputs and outputs are ports, shared values lets,
  marks paths with generated path data, the vis machine nodes, and each axis's labels
  a repeat over its ticks, one text per tick and no fixed slots. The Rust `plot()`
  builder and `TICK_SLOTS` are gone; the other five standard components are markup too.
- **Tests:** `tests/repeat.rs` (count, undo and redo, each over a series, save and
  load, a resize adding labels in the same transaction); `tests/plot.rs` unchanged but
  for reading labels from the copies; `markup.rs`'s unit test.

**2026-11-09, later: scopes** ([reactivity.md §13](reactivity.md#13-done)), replacing
parts of the entry above:

- Each item is a derived group `#g/~k` holding `item.value` and `item.index`, which the
  store publishes; copies are `#g/~k/local` and read those leaves (not a rewritten
  `at(each, k)`).
- The count follows values live: a drag, a clip or a producer's publish adds and
  removes copies without a transaction. No copy is hidden by an opacity guard.
- A component (an entity and its `use`) inside a template is repeated, one instance per
  item. Repeats inside a template expand only in its copies.
- An edit aimed at a copy is refused, naming the template. A repeat cut at its `max`
  is a problem on the repeat, shown in the view and the layers panel.

**Not yet:** value keys and enter/exit; overrides on a copy; layout groups ignoring a repeat's
template; a list-of-series input, so the plot's three series could be one repeat.
