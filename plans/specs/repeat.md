# Repeat: one child per element, derived

**Proposed 2026-11-08.** Waits on the user; build after [`plot.md`](plot.md) phase 3
and [`machines.md`](machines.md) phase 1. The plot ships with fixed label slots until then.

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
