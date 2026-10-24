# Packed path geometry

**Status:** approved 2026-10-22; phases 0–4 built 2026-10-23 (§11 has what was built
and where it differs from this text). Open questions settled on their
recommended answers (§10).

**Decision (user, 2026-10-22):** take a path's anchors out of the node tree. By
default a path's geometry is one packed value on the path. Its anchors become `point`
nodes only when something needs to address them one by one: point editing, a binding,
a constraint.

## 1. Why

Dropping a large SVG (`Coat_of_Arms_of_the_Kingdom_of_Prussia_1873-1918.svg`, 11 MB,
Wikimedia) into the web build aborts with `rust_oom` inside
`BTreeMap<NodeId, Facet>::clone`. Measured natively on 2026-10-22 with a throwaway test
around `iron_svg::import`, release build:

| | |
|---|---|
| Nodes the import creates | 684,104 |
| … `point` | 603,023 (88%) |
| … `contour` | 38,414 |
| … `path` / `group` / `ellipse` / `rect` | 28,083 / 13,476 / 818 / 290 |
| One `Document` clone | ~607 MB |
| Peak RSS inside `import()` alone, before the paste | 3.4 GB |
| `size_of::<Facet>()` / `Value` / `Slot` / `Node` | 616 / 48 / 56 / 72 bytes |

A point holds nine numbers (`transform2d` x, y, rot, sx, sy; `handles` ix, iy, ox, oy).
Stored as a node, it costs two 616-byte facets, a `Node`, an `OrderKey` string, and
entries in `children`, `by_type` and `by_facet`: more than 1.2 KB. The import holds
the document several times over: the `INode` tree, the scratch store, the transaction
kept in its history, the `Fragment` clone, then the paste ops, the main store's working
copy in `Store::run` and the paste transaction. That passes wasm32's 4 GB.

The renderer never sees this file. Raster and vector caching (`handoff-render.md` §4
item 4) don't help until the document fits.

Figma works the same way: a vector network's vertices are inside the vector layer, not
layers of their own. Iron made points nodes on purpose (`constraint-system.md` §13,
`select-tool.md` "Shapes and paths"), so that a point can be dragged through
`Store::begin_drag`, bound (the standard `fiber` component binds its ends to its port
empties), constrained, and simulated as a rope. All of that stays: those paths keep
their point nodes. What changes is the default.

## 2. The model

A `path` takes one of two forms. Every reader handles both; only editors need the
expanded one.

| Form | Geometry lives in | Children | Used for |
|---|---|---|---|
| **Packed** (new; the default) | `path.d`, one value | none | Imported paths, static art, anything not being point-edited |
| **Expanded** (today's form) | `contour` and `point` nodes | `contour`s, each with `point`s | A path being point-edited; a path whose points are bound, constrained, named or simulated |

**Invariant:** a path has a non-empty `path.d` or `contour` children, never both. A
path with neither is empty, as today. `op::check_created` and the facet checks refuse
both together.

"On demand" means the node form is made on demand. The anchors themselves are always
in memory, packed. Nothing is fetched lazily from disk.

### 2.1 The value

A new value kind, `Geometry`:

```rust
/// A path's outlines, packed: what its `contour`/`point` children say when it
/// has none (plans/specs/packed-paths.md).
pub struct Geometry {
    pub contours: Vec<PackedContour>,
    // Derived, built on first read: the stroked and filled outlines and the
    // tight bounds. Never serialised or compared.
    cache: OnceLock<Outlines>,
}
pub struct PackedContour { pub closed: bool, pub anchors: Vec<Anchor> }   // shape::Anchor

pub enum Value { …, Geometry(Arc<Geometry>) }
```

- **`Arc`**: cloning the document, an op, an inverse or a transaction shares the
  anchors instead of copying them. `Value` stays 48 bytes.
- **Immutable.** An edit replaces the whole value with `Op::Set` on `path.d`. Its
  inverse is the old `Arc`, so undo is cheap whatever the path's size.
- **Equality** compares `contours` (with `Arc::ptr_eq` as a fast path), never the cache.
- **Constant only.** The leaf can't be `Bound`: no expression produces geometry, and
  `Slot::Bound` on a `Geometry` leaf is refused like any kind mismatch. `Animated`
  stays reserved. Path morphing may want it one day.
- **The cache** keeps the `BezPath`s that `contour::outline` and `fill_outline` build
  today, plus the bounds. Built once per value, it saves `materialize` and `geom`
  rebuilding a path's outline every time they read it.

### 2.2 The facet field

`path.d` is a new field of the existing `path` facet (`PathShape`), next to
`fill_rule`, with an empty `Geometry` as its default. Like SVG's `d`, it sits next to
the fill rule. The name keeps the model surface readable to someone who knows SVG.

### 2.3 In the file

The value is written as SVG path data in **Iron's own convention**, the one
`iron_svg::import::parse_d` already reads exactly:

- `M` for each contour's first anchor. Each segment is `L` when both facing handles
  are zero, otherwise `C` with absolute control points.
- A closed contour ends in `Z`. Its closing segment is written out as a `C` back to the
  first anchor only when it is curved. A closing line is just the `Z`. This is the
  rule that makes "a closing line to a point on the start is a real point" hold
  (`Contours::flush`, `iron: true`).
- Numbers use Rust's shortest round-trip `f64` formatting, so the file reproduces the
  anchors bit for bit. Not `round4`, which suits foreign SVG but not Iron's own data.

```json
"path": { "fill_rule": "nonzero", "d": "M0 0 C10 0 20 5 20 10 L0 10 Z" }
```

**One gap:** a contour of fewer than two anchors has no path data (`Contours::flush`
drops it). A packed path never holds one. A path with such a contour (a pen path
after its first click) stays expanded (§4).

**Schema 8.** A new value kind is something older builds can't read. Nothing needs
migrating: every schema 7 path is an expanded path and stays valid. Load doesn't
pack old files. They're small, and packing a path changes its point ids.

### 2.4 In the model surface

`view::render_tree` prints a packed path's geometry as a summary, for example
`d="38 contours, 2,140 anchors"`, unless the query asks for geometry, in which case it
prints the data in full. A 50 KB attribute would drown the rest of the view (open
question 3). An expanded path prints as today.

## 3. Reading: one API for both forms

`iron_document::contour` gets one entry point for readers that need only the shape:

```rust
/// `path`'s outlines whichever form it takes: its `path.d` when packed, built
/// from its `contour`/`point` children when expanded.
pub fn geometry(store: &Store, path: NodeId) -> Arc<Geometry>;
```

For a packed path this returns the stored `Arc` (no allocation). For an expanded path
it builds a `Geometry` from resolved values, as `contours` does now.
`contour::contours` keeps returning point ids and is for editors only. On a packed
path it returns no contours, and an editor that needs ids expands the path first (§4).

| Reader | Today | Then |
|---|---|---|
| `materialize.rs` (`Shape2d::Path`, `path_outline`) | `contours` → `outline` / `fill_outline` | `geometry`, cached outlines |
| `geom.rs` hit and bounds (`"path"` arm) | `outline(contours)` bounding box | the cached bounds |
| `geom::classify` | a point's or contour's leaf is a `Local` change of its path | also: `path.d` is a `Local` change of the path itself |
| `gesture.rs` watched facets | `Handles`, `Contour` | also `PathShape` (a resize previews `path.d`) |
| `iron_svg` export (`d`, `open_filled`, embed points at line 668) | `contours` | `geometry` |
| `view.rs` | `contour` and `point` as generic child elements | the summary of §2.4 |
| `shape.rs` (`to_path`, `outline`) | `contours` | see §4: conversion produces an expanded path |
| Snapping (path tight bounds, other paths' points) | `contours` / outline | `geometry` |
| `select.rs` `Geo::points` / `runs` (PathEdit), `pen.rs` | point ids | unchanged: they run on expanded paths only |
| Copy, paste, duplicate | `contour`/`point` come with their owner | a packed path is one node; its geometry is a facet like any other |

## 4. Expanding and packing

Two store-level functions in `contour.rs` return ops, like `shape::to_path`:

```rust
/// Give a packed `path` point nodes: `path.d` set empty, and a `contour` per
/// packed contour with a `point` per anchor, ids from `next_ids`.
pub fn expand_points(store: &Store, path: NodeId) -> Result<Vec<Op>, String>;

/// The reverse, if nothing would be lost: `Err` naming the reason otherwise.
pub fn pack_points(store: &Store, path: NodeId) -> Result<Vec<Op>, String>;
```

`pack_points` refuses when it would cut something: a point or contour with a bound or
animated slot, a `name`, a non-default `rot`/`sx`/`sy`, or a relation to or from it.
It also refuses for a contour of fewer than two points, or a path under a gesture or
in a running simulation. Packing deletes the point nodes, and deleted nodes stay as
tombstones in the store and the file (`document.rs`, `serial.rs`). That is a reason not
to pack routinely (open question 1).

**When a path expands:**

- **Entering PathEdit** (double-click or Enter on a path): a packed path expands at
  once, in one transaction labelled "edit points". The selection and the PathEdit
  state then hold the new point ids. This copies the precedent `select-tool.md` set
  for shapes: a rect or ellipse converts to a path when you enter it, not on the first
  point edit, and one undo restores it. Undo here returns the path to packed and
  leaves PathEdit.
- **Making a point-level binding or constraint** expands first, in the same
  transaction as the link.
- **Born expanded:** a path drawn with the pen (its first point is a one-point
  contour), a rect or ellipse converted by `shape::to_path`, and the standard
  components that bind points (`fiber`). These are paths about to be edited, or
  already addressed point by point.

**When a path packs:** never automatically in this spec (open question 1). An
expanded path stays expanded after PathEdit ends, as a converted rect stays a path.

**Size limit:** expanding a path of N anchors creates N + contours nodes, each costing
about as much as today's points. Entering one 50k-anchor path is fine. A future
"select all, edit points" over the whole coat of arms is not, and is out of scope.

## 5. Editing a packed path without expanding it

Operations on the whole path don't need point ids. They write `path.d`:

| Operation | Today (expanded) | Packed |
|---|---|---|
| Resize (`Kind::Points`: points move, handles scale) | a `Set` per point leaf | one `Set` of `path.d`, transformed; previewed the same way |
| Move, rotate (the path's `transform2d`) | unchanged | unchanged |
| Flip, the fill rule, paint, stroke | unchanged | unchanged |
| SVG export | contours | `geometry`, written by the same `path_d` |

Only resize changes. It's the one path-wide operation that writes points instead of
the transform (so the stroke keeps its width).

## 6. Import

`iron_svg::import` emits packed paths. Rect and ellipse recognition is unchanged.
`contour_node` and `point_node` go, and a path's `INode` carries a `path.d` built
from `contours` instead.

- Iron's own SVG (`data-iron-type`) reads its `d` with `parse_d` as now. The anchors go
  into the packed value, exact.
- `Imported::nodes` counts nodes, which no longer includes points. The paste guard
  and any size warning should count anchors separately (`Imported::anchors`).
- **Fewer copies.** `fragment()` hands over its scratch store's document by value
  (a `Store::into_document`) instead of `store.document().clone()`. With `Arc`
  geometry the remaining copies are cheap, but this one is free to remove.

Two other things would help this file. Both are out of scope here and listed in §9:
`<use>` as linked instances (usvg expands 1,178 `<use>`s of 811 targets; the source's
8,722 `<path>`s become 28,083), and the shared-facet size (§7).

## 7. Memory after

An estimate for the Prussia file, not a measurement. Phase 3 measures it.

| | Today | Packed |
|---|---|---|
| Nodes | 684k | ~42.7k (13.5k groups, 28.1k paths, 1.1k shapes) |
| Anchors | 603k nodes, >700 MB | ~603k × 48 B ≈ 29 MB, shared by every clone |
| One `Document` clone | ~607 MB | ~100–150 MB (dominated by 616-byte facets on paths and groups) |

What's left is mostly `Facet` being as big as its largest variant: a path's five or so
facets cost about 3 KB. Boxing the variants above some size, or storing each facet
kind's map as its own concrete type, would roughly halve that again. It's
independent of this spec and phase 4 here. The per-transaction clone in `Store::run`
(about 100 MB per edit after this spec) is the next memory and latency cost. Its own
comment names the fix: apply with rollback through the inverses instead of on a copy.

## 8. Phases

Each phase ends with the store tests (`cargo test -p iron_document -p iron_svg --target
aarch64-apple-darwin`), the smoke script, and its notes in `roadmap.md`'s Status table.

**Phase 0: a permanent measurement.** An `#[ignore]` test in `iron_svg/tests`,
`big_import`, that imports the file named by `BIG_SVG` and prints node counts by type,
anchors, one-clone size and peak RSS. It's the probe of §1, kept. Record the
before-numbers in `rendering/render-benchmark.md`.

**Phase 1: the value and the readers.** `Value::Geometry`, `ValueKind::Geometry`,
JSON as §2.3, `path.d`, the invariant, schema 8, `contour::geometry` and the cached
outlines. Move every reader in §3's table to `geometry`. Tests:
- the `d` round-trip is bit-exact, including closing lines vs. closing curves, open
  contours and holes;
- a packed path and its expanded twin render, hit-test, bound and export identically;
- a file with packed paths saves and loads, and schema 7 files load unchanged;
- `Bound` on `path.d` is refused;
- undo and redo of a `Set` on `path.d`.

**Phase 2: expanding.** `expand_points` and `pack_points` with their refusals; PathEdit
expands a packed path on entry ("edit points", one undo); point-level links expand
first; resize writes `path.d` on packed paths. Run the select and pen scripts and add
a driver step that enters a packed path, moves a point and undoes twice.

**Phase 3: import.** Packed paths from `iron_svg::import`, `Imported::anchors`, the
by-value fragment. Exit: the Prussia file drops into the web build without aborting,
and phase 0's test shows a document under 200 MB and an import peak under 1 GB. Then
note the drawing cost (28k paths) in `render-benchmark.md` for the render track.

**Phase 4 (optional): facet size.** Measure, then box the large `Facet` variants or
split the facet maps by kind. Exit: phase 0's clone size roughly halves, and nothing
else changes.

## 9. Not in this spec

- **Done 2026-10-24** as references ([`instance-cache.md`](instance-cache.md) phase D):
  Prussia imports as 5,924 paths, 534 definitions and 1,832 references.
  **`<use>` as linked instances.** Import a repeated `<use>` target as a definition
  and its uses as linked instances (component-model phase 2). That would cut this
  file's paths by about 3×. Its own spec, after this one.
- **Drawing it fast.** After phase 3 the file is 28k paths and 603k anchors on
  screen. Zoomed-out encode is the measured weak spot, and baking a static subtree
  (`handoff-render.md` §4 item 4) suits a big import. Render track.
- **The per-transaction document clone** in `Store::run` (§7).
- **Tombstone collection**, which would make packing an edited path back free.

## 10. Open questions for the user

1. **Packing back.** After PathEdit, should a path return to packed? (a) Never
   automatically, with an explicit "Simplify path storage" command later.
   **Recommended:** no extra undo step and no tombstone churn, and a path someone
   edits is usually one they'll edit again. (b) On leaving PathEdit, when
   `pack_points` allows. This adds an undo step each time and leaves tombstones.
2. **Entering edit as an undo step.** §4 expands in its own "edit points"
   transaction, as entering a rect does today. The alternative is to fold the
   expansion into the first real edit. That's invisible in history, but the first
   edit would then have to change ids under a held gesture. Is the precedent right
   here?
3. **The model surface.** A summary of `path.d` by default, with full data on request
   (§2.4), or always the full data?
4. **Pen paths.** Born expanded (§4). Should the pen pack a path when it's finished
   (Enter or Esc), if nothing refuses? That would keep hand-drawn art packed too.

## 11. Done (2026-10-23)

Phases 0–3 as §8, with these differences from the text above:

- **The file form is numbers, not SVG path data** (§2.3 is superseded). Path data
  can't round-trip a handle exactly: absolute control points re-derive it as
  `(at + inn) - at`, which loses bits (0.1 + 0.2 − 0.1 ≠ 0.2). Path data also has
  nowhere to put an open contour's end handles. `path.d` is written
  `{"geo": [[closed, [x, y, ix, iy, ox, oy, …]], …]}`, exact through serde_json's
  shortest round-trip floats. This also closes §2.3's gap: a contour of fewer than two
  anchors, and an open contour's end handles, can be packed.
- **The model view always prints the summary** (`d="1 contour, 2 anchors"`, empty `d`
  left out). §2.4's full data on request was not added: a query flag would touch every
  `ViewQuery` construction, and the anchors are in the file form.
- **`pack_points` is not built.** Nothing packs automatically (question 1: never), so it
  waits for the explicit command. `contour::expand_points` is built.
- **Questions 2 and 4:** entering PathEdit (double-click or Enter) is its own "edit
  points" transaction; pen paths are born expanded and stay so.
- **Readers** take resolved values: `contour::geometry` reads the resolved `path.d`, so a
  resize preview of a packed path redraws. `contour::packed` reads the authored form and
  decides which form a path is in. The outlines and bounds are cached on the value.
- **The invariant** is a new `ErrorKind::MixedPath`, checked once a transaction has
  applied. A binding on `path.d` is `LeafError::ConstantOnly`.
- **Resize** of a packed path previews `path.d` whole (`Part::packed_d`), skipping an
  unchanged value as `preview_writes` does for numbers.
- **Import** reports `Imported::anchors` (also in `__iron.importSvg`'s reply), and the
  fragment takes its scratch store's document by value (`Store::into_document`).

**Phase 4:** every `Facet` variant is boxed inside the `facets!` macro, so a facet
costs its own fields rather than `font`'s eleven slots (616 bytes). No call site
changed: matches auto-deref, and construction goes through `wrap()`. The facet maps
were not split by kind; boxing alone did better than §7's estimate.

Measured (`rendering/render-benchmark.md` §14): the Prussia file is 42,667 nodes (was
684,104), one clone 40 MB in 9 ms (607 MB before packing, 169 MB before boxing),
import peak 250 MB (was 3.4 GB). It imports
in the web build. Tests: `iron_document/src/geometry.rs` (exact file form, outline
rules), `iron_document/tests/packed_paths.rs` (twin reads, save/load, view summary,
no binding, the invariant, expansion and undo, `path.d` undo), and the updated
`iron_svg` tests. Smoke: the Figma import's line reads `d="1 contour, 2 anchors"`,
and the import is 4 ops (was 7).

