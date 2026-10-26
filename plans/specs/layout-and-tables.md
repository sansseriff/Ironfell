# Layout and tables

**Status:** Approved 2026-10-26. Phases 0–3 done 2026-10-26 (Taffy in `iron_layout`;
layout on groups and Shift+A; tables with merges, presets, the Table tool and
inspector; the `<table>` op, the compact view and spreadsheet paste), apart from style
inheritance (§4, deferred). §10 proposes affordances, how a container says what
gestures on its children mean, for the user to approve. Work order:
`handoff-product.md`, "Layout and tables".
**Scope:** A layout engine for 2D compositions (stacks and grids on groups), and a
static `table` built on it, with two looks (boxed and light), an inspector section and
a `<table>` verb for the model surface. Slides, a deck theme and port-fed tables are
named as follow-ups (§9), not specified here.

This spec came out of a conversation on 2026-10-26 about tools for slide decks. The
user's reference was a light engineering table: a muted monospace label column, numbers
right-aligned, hairlines between rows and no verticals, one column emphasised in bold
with its header in an accent colour, and a letter-spaced caption above.

---

## 0. Decisions at a glance

| Question | Decision | Why |
|---|---|---|
| Write or import a layout engine | **Import Taffy** (0.14, already in `Cargo.lock` through `bevy_ui`) | The Rust counterpart of Yoga, with CSS grid as well as flexbox; pure Rust, builds for wasm, used by `bevy_ui`, Zed and Dioxus. A table is a grid; slides will want stacks next |
| Where it lives | A new Bevy-free crate, **`src/iron_layout`**, the only crate that names Taffy's types; `iron_document` uses it as it uses `iron_text` | Bounds, picking and drawing must agree, so layout runs inside the store; the store stays free of a third-party style vocabulary |
| How Taffy reads Iron's tree | Taffy's low-level traits (`LayoutPartialTree`, `LayoutGridContainer`, `LayoutFlexboxContainer`, `CacheTree`) implemented over the document. Taffy's `NodeId` is a `u64`, as Iron's is | No mirror tree to keep in sync |
| What layout writes | **Nothing authored.** Positions and sizes are published into the graph by a **layout producer**, as `ProducerId::NETWORK` publishes the across/through solution | Derived, never saved, never in undo; bindings read them like any value; the store already locks producer-driven leaves |
| The vocabulary | A curated CSS subset: `stack_x`, `stack_y`, `grid`; gap, padding, align, justify; track sizes `auto`, fixed, `fr` | Models already know CSS's words; a subset keeps the inspector and the manifest small |
| A table | A **`table` node** whose children are `row` nodes, whose children are ordinary **`text` cells**; column widths are a `table` leaf | Editing, fonts, styled parts, `fmt` and the instance cache work on cells unchanged; HTML's `<tr>`/`<td>` shape is one models know, and deleting a cell disturbs only its row (§7, phase 2) |
| A table's lines and bands | **Drawn from the table, not stored**: a `rules` facet (outer, header, rows, columns), each with the `Stroke` fields, plus header and zebra fills, placed on Taffy's track positions | Adding a row never re-creates lines |
| Static or port-fed | **Static** in this spec. A cell may be bound to an expression; whole-column binding waits for a series value (§9) | The user's call, 2026-10-26 |
| Is a table a component | **No.** A table is a primitive; a component's template may contain one | Expansion is structural only (component-model §3): a component cannot vary its row count |
| Model creation | A **`<table>` verb** with pipe-separated rows, and a compact form in views | Thirty `<create>`s per table is slow and error-prone. Approved by the user 2026-10-26 |

---

## 1. Where the code is today

- **No layout.** Every node's position is its own `transform2d`. Groups take their
  bounds from their children (`geom::local_bounds`).
- **Text measures itself in the store.** `text::measure` / `measure_with`
  (`iron_document/src/text.rs`) lay out through `iron_text::engine()`, reading the
  *resolved* `size.w` for wrapping. The layout cache is keyed by (text, style, wrap
  width).
- **Derived values enter the graph two ways.** As derived *expressions*, added when
  `ReactiveGraph::rebuild` runs (`port::derived_signals`, `anchor::derived`). Or as a
  producer's *published values*: `Store::resolve_physics` solves the network after every
  change and publishes under `ProducerId::NETWORK`, and the Stats composition's
  producer overrides authored `text.content` the same way. Layout needs measured text,
  which no expression can call, so it is a producer.
- **Lines are paths.** No `line` type exists. A straight line is a `path` with a
  `contour` and two `point`s; `Stroke` carries width, colour, join, cap and `dash`.
- **No list value.** `Value` has records but no series, so a port cannot carry a column.
- **Model verbs that expand.** `<import-svg>` (`src-ui/model/edit.ts`) is parsed in TS
  and handed to one engine call, `document_import_svg_at`, which builds the ops in
  Rust. New engine calls need a `web_ffi` `document_<fn>` export and an
  `engine_dispatch` arm.

---

## 2. `iron_layout`

### 2.1 The crate

- `src/iron_layout`, workspace member, depends on `taffy` with
  `default-features = false` and features `std`, `flexbox`, `grid`,
  `detailed_layout_info` (track positions for the table's lines). It does not enable
  `taffy_tree`, `block_layout`, `float_layout` or `calc`.
- Its API takes a **read-only view of a tree** (a trait the store implements: children
  in sibling order, each node's layout style, and a measure callback for leaves) and
  returns, per laid-out node, a box: `x`, `y` relative to its container, `w`, `h`. For
  grids it also returns track positions (`rows: Vec<(start, size)>`, the same for
  columns).
- It owns the translation from Iron's style (§2.2) to Taffy's `Style` and the result
  cache (`CacheTree`), keyed by Iron `NodeId`.

### 2.2 The `layout` facet

On `group`, and implied on `table`:

| Field | Kind | Meaning |
|---|---|---|
| `mode` | Enum | `stack_x`, `stack_y` (the default when the facet is added), `grid`, or `none` (children place themselves) |
| `gap`, `row_gap` | Number | Space between a stack's items or a grid's columns; between a grid's rows |
| `pad_t`, `pad_r`, `pad_b`, `pad_l` | Number | Padding inside the container |
| `align` | Enum | Cross-axis alignment of items: `start`, `center`, `end`, `stretch`, `baseline` |
| `justify` | Enum | Main-axis distribution: `start`, `center`, `end`, `between` |
| `columns`, `rows` | Str | Grid tracks, space-separated: `auto`, a number (px), `2fr` |
| `sizing_w`, `sizing_h` | Enum | The container's own size on each axis: `hug` (fits its content) or `fixed` (`size.w`/`size.h`) |

A child of a laid-out container may carry `layout_item`: `w`/`h` as `auto` or
`fill`, and `align`; in a grid, `column`, `row` (1-based; 0 flows) and `span_x`,
`span_y`. A child with `layout_item.absolute = true` keeps
its own `transform2d`, as in Figma.

### 2.3 Running it

- **The layout producer** (`ProducerId::LAYOUT`) runs where `resolve_physics` runs:
  after every transaction, preview and publish, and from `refresh_text` when fonts
  arrive. It lays out only **dirty containers**: those whose structure, `layout`
  fields or children's measured content changed since the last run (from the
  transaction's ops, and from resolved-value changes to `text.*`, `font.*` and
  `size.*` on their children).
- It publishes each child's `transform2d.x`/`y` and, where layout decides them,
  `size.w`/`size.h`; and the container's own `size` when it hugs.
- **Text is measured at a width.** The measure callback calls a new
  `text::measure_at(store, id, width)`: Taffy asks for min-content and max-content
  widths, then the height at the width it chose. A cell's text wraps at the column's
  width (its `sizing` behaves as `auto_height` inside a layout).
- **Nested containers** lay out in one Taffy pass from the outermost dirty container,
  so a stack inside a grid cell sizes correctly.
- **Bounds and picking** read the published values like any resolved value, so
  `geom` and the spatial index need no special case. Changes are reported through
  `drain_changes` as for any producer.

### 2.4 Editing laid-out things

- A child's published `x`/`y`/`w`/`h` are locked, as any producer-driven leaf
  is (component-model §7), with the reason "placed by #group's layout". In the first
  version **the select tool does not move them**. A drag on one selects it; moving it
  means changing the container.
- Resizing the container (`sizing = fixed`) relays the children live, through the
  ordinary preview path.
- Not in this spec: drag to reorder within a stack or grid, and Figma's resize
  handles on a child that switch it to `fixed`. Both are later select-tool work.

---

## 3. Tables

### 3.1 The nodes

```text
table   #T   transform2d, table{columns, header, pad_x, pad_y, width, fills, preset}, rules{…}
  row
    text     "PI per side (µm)"
    text     "75"   …
  row
    text     "S (µm)"   …
```

- `table` (new type). Children: `Only(["row"])`. Facets: `transform2d` (required),
  `table` (required), `rules`, `fill` (its background), `size` (published by layout),
  `appearance`, `name`, `host`, `layout_item`. It is laid out as a grid whose columns
  are `table.columns` and whose rows are its `row`s.
- `table` facet: `columns` (one track per column: `auto`, a length, `1fr`), `header`
  (how many rows are the header), `pad_x`/`pad_y` (each cell's words inset from its
  edges), `width` (0 hugs the columns; more fixes it, shared by `fr` columns),
  `header_fill` and `zebra_fill` (transparent for none), `preset` (which look the
  cells were styled with).
- `row` (new type). Children: `Only(["text"])`, its cells left to right. No facets of
  its own yet (a height and a header flag are the obvious next ones). It has no
  placement and is never drawn or selected: the select tool looks through it.
- Cells are **ordinary `text` nodes**. They keep their own `font`, `fill` and
  `text.runs`; one can be bound (`text.content = {= fmt(#calc.loss, "dB", 2)}`).
  A cell's box is the whole cell, published by layout as `fixed` sizing; its words
  are inset by the table's padding (`text::cell_pad`), so a click anywhere in a cell
  lands on it and the cells tile the table.
- **Column settings live on the table and the cells**, not on column nodes:
  widths in `table.columns`, alignment and emphasis as the cells' own `font` leaves,
  which presets and the restyle edit write and the compact view reads back
  (`table::infer`). HTML's `<colgroup>` is optional for the same reason.

Inserting a row is one `Create` of the row and its cells; a column is one cell at the
end of each row and one more track. Deleting a cell disturbs only its own row: the
cells after it in that row close up, and other rows keep their columns.

### 3.2 Drawing the lines and bands

`rules` facet: `outer`, `header`, `rows`, `columns`, each a `*_width` (0 for none) and
a `*_color`. The `header` rule falls under the last header row, `rows` between the
others, `columns` between columns, `outer` around. `table::decor` works the bands
(background, header band, zebra rows) and the line segments out from the table's
`Grid` (its track positions, kept by the store from the last layout) and splits each
line around merged cells. `materialize` gives the table `Shape2d::Table { w, h,
decor }`; `render.rs` fills the bands and strokes the lines (butt caps) under the
cells. A table whose tracks move is reported changed, so its lines redraw.

### 3.3 The two looks

A preset is a set of values a table and its cells take when made or restyled. It is
not a live style:

| | `light` (the reference image) | `boxed` |
|---|---|---|
| Lines | header and rows 1 px, ink 12%; no outer, no columns | outer 1 px and header 1.5 px at 30%; rows and columns 1 px at 15% |
| Header | muted ink | a `header_fill` band; weight 600 |
| Label column | JetBrains Mono 14, muted, left | Inter, weight 500 |
| Body | Inter 15, right-aligned, tabular figures, vertically centred | the same |
| Emphasised column | bold; its header in the accent colour | bold; its header in the accent colour |
| Padding | 14 × 9 | 12 × 8 |

**Restyling** (`preset:light`, `preset:boxed`) rewrites the table's padding and fills,
replaces its `rules`, and rewrites every cell's `font` and `fill.color` from the
preset, keeping the label column, emphasised column and alignments it reads back from
the cells. A caption is the `<table>` op's: a stack group holding the caption (12 px,
weight 500, 12% letter spacing, capitals, muted) above the table.

### 3.4 Merged cells

As in HTML, PowerPoint and Google Slides, a merged region is one cell spanning several
columns or rows:

- A cell carries `layout_item.span_x`/`span_y` (default 1 × 1); the region's other
  positions have **no node**. Row by row, each cell takes the first column at or
  after the previous cell's end that no cell above covers (HTML's table-forming
  algorithm, `table::place`); a span runs no further than the last row. A cell with
  no room left in its row goes past the last column, where it shows as an extra
  column rather than shifting its neighbours.
- Lines are not drawn through a merged region.
- **Merge** (inspector, with cells selected) takes the rectangle around them, which
  must contain only whole cells; the top-left cell keeps the others' non-empty words
  on new lines (PowerPoint's behaviour), the rest are deleted. **Split** sets the span
  back to 1 × 1 and adds empty cells where it stood, in their rows. One undo each.
- In the `<table>` op and the compact view a merged position is written `<` (merged
  into the cell on its left) or `^` (into the one above); a merge that is not a
  rectangle is refused.

---

## 4. Style inheritance (deferred)

The plan was for a cell's `font` and `fill` to become optional, resolving from table →
column → row → cell, so a preset writes a few leaves rather than every cell's. It is
deferred: what it was for is covered another way. Restyling is one edit and one undo
step (a 5 × 6 table's restyle is about 370 ops, all in one transaction), and a model
reads a table's look as the preset plus `label-col`, `emphasis`, `align` and per-cell
`<style>` differences (§5.2), not as thirty fonts. Revisit when a deck theme (§9)
needs cells to follow a shared style live, or when restyles show up in timings.

---

## 5. Creating tables

### 5.1 The canvas and inspector

- A **Table** tool sits in the shape tools' menu. A click places a 3 × 3 `light`
  table (`Column 1…3` over two rows of `Cell`) and selects it.
- The inspector's **Table section** shows for a table or any of its cells: + and −
  Row (at the bottom), + and − Column (at the right), Light and Boxed (restyle), and,
  for cells, Merge (two or more selected) and Split. Each is one undo step. The
  table's `table` and `rules` leaves show in the generic "Table style" and "Lines"
  sections.
- **Selecting.** A click on a table selects it; a double-click goes through its row
  to the cell; Shift-click adds cells, from any row; Esc goes back up past the row
  to the table; Enter on a table lists its cells. Editing a cell's words is the text
  tool's, as for any text.
- **Paste.** Tab-separated text (a spreadsheet's copy) pasted with a table or a cell
  selected fills the table from that cell, over what is there, growing it to fit
  (`table::paste_ops`); with no table selected it makes a new `light` table centred
  in the view (`table::fragment`), through the ordinary paste path.

### 5.2 The model surface

**The op** (alone in its `<edit>`, like `<import-svg>`):

```xml
<table to="#slide3" x="80" y="400" preset="light" header="1" label-col="true"
       emphasis="3" caption="Same cable, other polyimide thickness">
PI per side (µm) | 75   | 100  | 125  | 150  | 200
S (µm)           | 85   | 113  | 140  | 163  | 198
</table>
```

- Cells are split by `|`, trimmed; `\|`, `\<`, `\^` and `\n` are a bar, `<`, `^` and
  a line break inside a cell. A cell `{= expr}` is bound. `<` and `^` merge (§3.4).
- `emphasis` is 1-based, as the view prints it; `align` lists alignments for the
  first columns (the rest are right-aligned, the label column left); `header` counts
  header rows; `caption` adds the caption stack.
- `<table id="#T">rows</table>` **replaces the words**: when the rows keep the
  table's shape (rows, columns, merges), only cells whose content changed are set, so
  every cell keeps its id, style and selection; otherwise the rows are made again in
  the table's current look. `preset=` or `header=` there restyle it in the same
  history entry.
- `edit.ts` parses the op and calls `DocumentApi.table(spec)` (the engine call
  `table`, `engine_api::document_table`), which builds it with `table::create_ops` or
  `table::replace_ops`.

**The view.** Every view a model reads prints a table in the op's form
(`table::compact`), with its size, its cells' ids in the same shape, and what differs
from the preset:

```xml
<table id="#3w" name="table" x="60" y="40" w="468" h="182" preset="light" header="1"
       columns="auto auto auto auto auto auto" label-col="true" emphasis="3">
  PI per side (µm) | 75   | 100  | 125  | 150                   | 200
  Loss 2 GHz (dB)  | 1.10 | 0.89 | 0.79 | {= fmt(0.5, "dB", 2)} | 0.74
  W / 1000 lines   | 0.40 | 0.40 | 0.41 | 0.42\n0.43            | <
  <cells>#40 #41 #42 #43 #44 #45 / … / #4Q #4R #4S #4T #4U</cells>
  <style cell="#4G" color="#cc1a1aff"/>
</table>
```

`expand` (with the table's id) shows its rows and cells in full instead. The op
ignores `<cells>` and `<style>` lines, so a model may send back what it read. The
manifest has a TABLES section.

---

## 6. Interaction with the rest of Iron

- **SVG export** does not yet draw a table's lines and bands (its cells export as
  text); import does not recognise tables.
- **The render cache.** A table with constant cells is static content; a bound cell
  is dynamic, alone. A relayout that moves cells is a geometry change for them.
- **Components.** A table may sit in a component's template; its cells may read the
  definition's ports (scope rule 1).
- **Spatial index and picking.** A table is a scope like a group (its bounds are its
  cells' union, which tile it); its cells are the leaves picked.
- **Copy and paste** copy a table as its subtree.

---

## 7. Phases and exit criteria

**Phase 0: spike (timeboxed).** Add `iron_layout` with Taffy, implement the traits over
a toy tree, lay out a 3 × 3 grid of measured text and a stack, and build the shipped
wasm.
*Exit:* the grid's track positions match a hand calculation in a native test; the
shipped wasm's size increase is measured against the baseline (brotli) and recorded
here; any API mismatch with §2 is written into §2 before phase 1.

*Done 2026-10-26.* `src/iron_layout` (Taffy 0.14: `std`, `flexbox`, `grid`,
`content_size`, `detailed_layout_info`) implements Taffy's partial-tree traits over a
flat `Tree` of the caller's nodes, keyed by their ids; it returns each node's box and
each grid's track positions. Native tests (`iron_layout/tests/layout.rs`): a 3 × 3
grid of measured text sized to its widest cells with padding and gaps, a merged cell,
a fixed column wrapping text, a stack, `Fill` along a stack. Taffy matched every hand
calculation (one test's expected width was wrong, not Taffy). An `auto` track takes no
leftover space (the grid's content is justified to the start), so a table's auto
column is as wide as its widest cell. Size, measured with `build-wasm.sh webgpu` on
HEAD (2489333) and on the change, back to back with a shared target: 20,107,665 →
20,298,546 bytes raw (+191 KB) and 4,487,913 → 4,539,524 brotli-11 (+52 KB, 1.1%).
That is Taffy, `iron_layout` and the store's layout code together.

**Phase 1: layout on groups.** The `layout` and `layout_item` facets, the producer,
dirty tracking, `measure_at`, locking, the inspector's layout fields; schema bump.
*Exit:* a `stack_y` group of three texts reflows when the middle one's content grows,
in one transaction, with no authored change to the others; undo restores it; save
writes no layout output; dragging a laid-out child refuses with the reason. Native
tests in `tests/layout.rs`; a smoke section.

*Done 2026-10-26.* `iron_document::layout` builds one `iron_layout::Tree` per
outermost laid-out group and publishes under `ProducerId::LAYOUT` (2; host producers
now start at 3), run by `Store::resolve_physics` after the network solve, so after
every transaction, preview and publish, on load, and from `refresh_text` when a font
arrives. Text is measured with `iron_text` as `text::measure` does; a text whose width
the layout chose is published as `auto_height` at that width, so it wraps there and
aligns within its cell. Views mark a laid-out leaf `x.by="#group"`, and the manifest
has a LAYOUT section. **Add auto layout** (Shift+A, `layout::add_ops`; the
`layout` tool key) gives a selected group a layout, or wraps sibling shapes in a new
`stack` group, the direction the one they spread along, the gap their mean gap, the
order the one they sit in, so nothing moves. The inspector offers Auto layout on
groups and "In layout" on a node a layout places, with the generic field editor.
Tests: `iron_document/tests/layout.rs` (5) and the run-iron `layout.txt` script; the
full smoke passes apart from lines known to vary and one render-inspector line in a
section being reworked in the working tree (not touched by layout).
Deviations from §2:
- **Fields.** `gap` (a stack's spacing, a grid's columns) and `row_gap`, not
  `gap_x`/`gap_y`; `sizing_w`/`sizing_h`, not one `sizing`; `layout_item.w`/`h` are
  `auto` or `fill` (a fixed size is the node's own).
- **No dirty tracking yet.** Every laid-out group is laid out again after every
  change, with an early return when nothing has a layout and no republish when
  nothing moved; text layouts are cached by `iron_text`, so a stack of a few dozen
  nodes costs little. Dirty containers (§2.3) wait for a measurement that needs them.
- **No schema bump.** The two facets are additive; older files load unchanged.
- **A laid-out group's box** is still its children's union for selection and
  picking: its padding is not part of its bounds, and it draws nothing. Tables
  (phase 2) draw their own box.
- **Shift+A also reaches Bevy**, where Shift and A run a 3D view's camera left;
  the engine acts on `layout` only with the pointer over a 2D view.
- **Dragging a laid-out child** moves its group when the group is what is
  selected (Figma's behaviour); a child reached deeper is refused with the
  layout's reason, as `handle` locks every published leaf.
- Not yet: a designed Auto layout section (direction buttons, padding pairs, a
  gap field on the canvas), drag-to-reorder, and resize handles that switch a
  child to `fill`.
- The inspector's "Auto layout" and "In layout" add buttons only appeared once
  `layout` and `layout_item` joined `INSPECTOR_OPTIONAL` (`engine_api.rs`), fixed in
  phase 2.

**Phase 2: tables.** `table`, `row`, the `rules` drawing, the two presets, the Table
tool and the inspector's Table section without paste.
*Exit:* the reference image rebuilt by hand in the `light` preset, and the same data
in `boxed`, screenshotted; adding a row and a column is one undo step each; a cell
bound to a slider updates and relays live; a header cell spanning two columns and a
label spanning two rows lay out and draw their rules around the merge; merge and split
are one undo step each.

*Done 2026-10-26.* `iron_document::table` (placement, `Grid`, `decor`, presets,
`create_ops`, `edit_ops`), the `table` and `row` types and the `table` and `rules`
facets; layout lays a table out as a grid of its cells in explicit areas and keeps its
`Grid` (`Store::table_grid`); `Shape2d::Table` draws it. The reference table and a
boxed table with a two-column header and a two-row label were rebuilt and
screenshotted; native tests in `tests/table.rs`. Deviations:
- **Cells were first the table's direct children**, rows and columns implied by
  their order, so that a double-click would reach a cell. That made a cell's row
  and column depend on every cell before it: deleting one shifted the rest of the
  table, silently. Explicit rows contain that (a test checks it), match HTML,
  PowerPoint and Slides, and give rows a home for their own settings; the select
  tool now looks through rows instead (`see_through` in `select.rs`; §10 generalises
  it).
- **No `column` nodes.** Widths are `table.columns`; alignment and emphasis are the
  cells' own leaves, read back by `table::infer` (§3.1).
- **Cell padding is a text inset**, not a layout margin, so cells tile the table and
  are hit anywhere in them (the spatial index has no "box with children" kind).
- **Merges use `layout_item`'s spans**, not a `cell` facet.
- **Restyle edits** (`preset:light`/`preset:boxed`) were added so the preset buttons
  do something; §3.3.

**Phase 3: the model surface and inheritance.** The `<table>` op, the compact view,
§4's cascade, and TSV paste.
*Exit:* the op above produces the reference table in one transaction; the view prints
it back in the same form; switching presets on a 5 × 6 table writes only the table's,
columns' and header's `cell_style`; pasting a 6 × 5 block from a spreadsheet fills it.

*Done 2026-10-26*, except the cascade (§4, deferred). The `<table>` op makes a table
(with a caption stack) or replaces one's words, keeping ids when the shape holds;
every view prints tables in its form (`table::compact`); tab-separated paste fills a
selected table from the selected cell or makes a new one. Tests: `tests/table.rs` (11)
and the run-iron `tables.txt` script; the full smoke passes apart from lines known to
vary and the render-inspector line in the section being reworked in the working tree.
Not yet: Tab between cells while editing, a designed Table inspector (column widths,
padding and lines as their own controls), SVG export of the lines, drag to make a
fixed-width table, row heights.

---

## 8. Decisions taken and open questions

Decided by the user, 2026-10-26:

- **Alignment as Google Slides and PowerPoint do it.** Both offer per-cell horizontal
  alignment (left, center, right) and vertical alignment (top, middle, bottom), and
  neither aligns a table column on the decimal point. Iron does the same: a column's
  `align` and `valign` with a per-cell override through the cell's own `font.align` /
  `font.valign`. Numbers stay right-aligned with tabular figures, which lines up
  equal-precision columns such as the reference.
- **Merged cells are in.** See §3.4.
- **Lines on plain grid groups: dropped.** `rules` stays a table facet.

Open:

1. **Laid-out children in 3D.** Not considered; 2D compositions only.

---

## 9. Follow-ups (not in this spec)

- **Slides.** A slide is, for now, a group with a rect `group_clip` and a `slide`
  facet (order, title) on a 2D composition. A **Slides view** lists them with
  thumbnails (embeds and view textures already render compositions), reorders them,
  adds one from a layout, and presents by stepping the camera from slide to slide.
  Stacks from this spec are what a title-and-bullets slide is made of.
- **A deck theme.** A theme node in the document holding named leaves (accent, ink,
  muted, the fonts for title, body, label and number, the default table preset) that
  presets and text bind to. It belongs in the document, not in Settings: it is
  content, undoable and editable by the model.
- **Port-fed tables.** A series value type (overlapping ECM phase 5's sampled
  signals), `column.values` bound to one, and rows generated from its length. A
  standard "data table" component with series `in` ports follows from that.

---

## 10. Affordances: how a container says what gestures on its children mean (proposal)

**Status:** Proposed 2026-10-26, for the user to approve; nothing built. Written at the
end of phase 3, at the user's request, after tables needed the select tool to treat
one node type (`row`) differently. It may move to its own spec when approved.

### 10.1 What layout and tables showed

Two kinds of interaction customisation came up, and only one is general today.

- **What a gesture writes is data.** When layout took a child's position over, no
  interaction code changed: the position became a published leaf, `handle.rs` locks
  every published leaf, and the drag was refused with layout's reason. Binding
  inversion, overrides, solver drivers and locks all work this way: the gesture → op
  mapping is decided by the document (vision 02 §8.2, §9.2), not by node types.
- **What a gesture targets is code.** Which node a press selects, what a double-click
  enters, what Esc climbs to, what Enter lists, what counts as a container, what a drag
  on a node *means*: `select.rs` decides these with about 51 node-type checks, in about
  ten hand-kept lists (`"group" | "use" | "empty" | "embed"`, with variations). Tables
  added `"table"` to four lists and a new rule, `see_through` for rows. Each new kind
  of content adds branches here.

The second kind has a pattern: **the role a node plays in a gesture is decided by its
container, not by its own type.**

| Container | What its children are, to a gesture |
|---|---|
| A free composition or group | Points: a drag moves them (vision 02 §9.3's "point in 2D") |
| A stack (layout) | A sequence: Figma's drag reorders (today: locked) |
| A table | Cells: the rows are looked through; a drag would select a range of cells (a region) |
| A component instance | Closed: only its ports are hot (component-model §4.3) |
| A path being edited | Points again, of its contours |

So the general statement: **every gesture is resolved in an interaction context, the
nearest enclosing container that declares one, and the context assigns each of its
children an *affordance*.** The free composition is the default context.

### 10.2 The model

An affordance answers four questions about a node, for one container:

| Question | Values | Today's code it replaces |
|---|---|---|
| **select**: what a press on it targets | `target` (selectable, enterable if it has children), `through` (never a target; presses go to its children: a row), `closed` (a target that is never entered; its ports are hot: an instance), `none` | `target_index`, `see_through`, the Enter and Esc paths, the container lists |
| **drag**: which manipulation a drag is | one of vision 02 §9.3's archetypes: `point`, `track` (along a path), `ring` (an angle about a centre), `sequence` (reorder among siblings), `region` (a set by area), `none` | the per-type drag branches; laid-out children's lock |
| **writes**: where the manipulation lands | for built-ins, the node's own leaves (`transform2d` for `point`, sibling order for `sequence`); in general, assignments from the archetype's value to leaves (§10.4) | `preview_writes`, `shape_grip_writes` choosing leaves by type |
| **edit**: what entering it in place means | `text` (the text tool), `points` (path editing), `none` | the double-click branches for paths, rects and text |

It sits between the layers that exist. The pointer state machine (`select.rs`) asks
the **resolver** which node a press targets and which archetype a drag is; the
archetype's **manipulator** (`manipulate.rs`) turns the pointer into a value each
frame; the store realises the writes (`handle.rs`: direct, inverted, translated to an
override, or locked with a reason), previews them and commits one transaction. Undo,
gesture ownership (infinite-canvas §4), locks and the model surface see nothing new,
because the writes are ordinary leaf edits. That is what keeps interaction code from
growing: a new kind of content adds an affordance record, and at most one new
archetype; it adds no branches to the state machine.

The vision's rule still decides what is allowed: **an affordance's writes must land on
leaves** (§9.2). A write to a derived leaf goes through the store's translation, which
inverts it when it can and refuses it with a reason when it cannot, so an affordance
cannot smuggle in a non-injective manipulation; those keep vision 02 §8.3's route
(model-mediated repair as a reviewable diff).

### 10.3 Where affordances come from

1. **Built in, per node type.** `TypeSpec` (the registry) gains a default affordance
   for its children and for itself: a composition and a group give children `point`;
   a `table` gives its rows `through` and its cells `target` + `region` + `text`; a
   `use` is `closed`. These are the ten lists of today, written once as data.
2. **From a facet that makes a container a context.** A group with `layout` gives its
   items `sequence` instead of `point`.
3. **Authored in the document** (§10.4): a container holds `affordance` children that
   override the built-ins for some of its children.

Resolution walks the hit's chain from the outside in; at each level the nearest
declaring context gives that level's affordance. Cmd (deep select) still reaches the
hit node itself, whatever the contexts say: an escape hatch that keeps every node
reachable.

### 10.4 Affordances as document data

A node type `affordance`, a child of any container (a group, a composition, a table, a
definition's template), with one facet:

| Field | Meaning |
|---|---|
| `of` | Which children it applies to: `all`, or a name or type to match |
| `select`, `drag`, `edit` | As in §10.2; `default` keeps the built-in |
| `along` | The node an archetype runs along or about: the path of a `track`, the centre of a `ring` (a Ref) |
| `writes` | Assignments from the archetype's value to leaves, in the expression language: `#m.args.in = (angle + pi) / pi * #m.args.range` |

The archetype supplies named values to `writes`: `p` (the pointer in the container's
space) for `point`, `t` (0–1 along the path) for `track`, `angle` for `ring`, `index`
for `sequence`. Because an affordance is a node, it is undoable, saved, shown in views
and authored by a model with `<create type="affordance" …>`; because `writes` are
expressions over leaves, they are checked by the same scope rules and read the same
way as bindings. This is vision 02 §9.3's "making the binding layer data", applied to
gestures on content rather than to editor panels, and it is how a document defines
its own interaction dynamics.

### 10.5 The demo

One composition, `affordances`, whose dynamics are all declared in the document:

- **A dial you turn.** A dial meter whose needle is dragged *around* its pivot. Its
  container declares `drag="ring" along="#pivot" writes="#m.args.in = …"`. Linear
  inversion cannot do this: the needle's position is not linear in the input; its
  angle is, and only a ring archetype produces an angle.
- **A bead on a wire.** A circle constrained to a curved path, dragged along it:
  `drag="track" along="#wire" writes="#bead.args.t = t"`, the bead's position bound
  to the point at `t` on the wire. Needs two expression functions, the point at `t` on
  a path and the nearest `t` to a point.
- **A slide sorter.** A stack of slide thumbnails (embeds) whose drag reorders them,
  the built-in `sequence` of a laid-out group, with the presentation order following
  sibling order.
- **A model that rewrites the dynamics.** The demo's last step asks the model to
  change one affordance ("make the dial snap to whole milliwatts", a `writes` edit),
  and the next drag behaves differently with no code change.

### 10.6 Phases

**A. The resolver.** `iron_document::interaction` (Bevy-free, tested natively):
`Affordance`, the built-in records on `TypeSpec`, layout's and table's overrides, and
`resolve(doc, chain, selection, deep)`. `select.rs`'s type lists and `see_through`
become calls to it. *Exit:* behaviour unchanged (the full smoke, `layout.txt`,
`tables.txt`, `snapping.txt`); `select.rs` has no node-type lists left for selection.

**B. Two archetypes.** `sequence` (stacks reorder by drag, replacing the lock; table
rows reorder by a row handle) and `ring`. *Exit:* dragging a stack's item reorders it
in one undo step; a ring drag on the dial meter sets its input.

**C. Authored affordances and the demo.** The `affordance` type, `writes` evaluated
through the store's edit translation, views and the manifest, the `track` archetype
and its two path functions, the `affordances` composition. *Exit:* §10.5 runs, and a
model edit to an affordance changes the next drag.

**D. Later.** `region` (a range of table cells), drop semantics (what dropping into a
container means: reparent, insert at an index, paste into cells), and 3D.

A is the "small refactor" of the earlier discussion, but built as the data structure
B and C read, so the refactor is done once.

### 10.7 Open questions

1. **Precedence between nested contexts.** Nearest wins in this proposal; should a
   component be able to declare its own affordances that its user's container cannot
   override (a sealed control)?
2. **Modifiers.** Cmd is the deep-select escape hatch; Alt duplicates. May an
   affordance claim a modifier (Shift to snap a ring to steps), or are modifiers the
   archetype's?
3. **What a model is told.** Should views print a node's effective affordance (an
   `affords="sequence"` attribute), or only the `affordance` nodes authored?
4. **Constraints.** A `point` drag on a solver variable already runs through the
   solver (constraint-system.md's interaction-time roles). Are a cluster's roles an
   affordance (the context is the cluster), or stay as they are below the write?
