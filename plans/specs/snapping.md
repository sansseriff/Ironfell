# Snapping and smart guides

**Status:** phases 1–2 built 2026-10-20 (the snap core; moving the selection with
guides); phase 3 mostly built the same day (resizing, path points, the pen); the shape
tools, 4 and 5 open. Work order: `handoff-product.md`, "Select and pen gaps".

**Decision (user, 2026-10-20):** Figma's object snapping in 2D compositions: edges to
edges, centres to edges, edges to centres, centres to centres, between a moved box and
nearby shapes.

## 1. What it does

While the selection is moved in a 2D composition, its box snaps to the boxes around it.
Each axis is solved on its own: the box's three vertical lines (left, centre, right) are
checked against every target's three, and the nearest within reach wins; then the same
for the horizontal lines. A red guide is drawn through every line that lines up after
the snap, with a cross at each aligned corner or centre.

| Rule | Value |
|---|---|
| Reach | 5 CSS px on screen (`SNAP_REACH`), whatever the zoom |
| Targets | The movers' siblings that are on screen, and the artboard (a 2D embed) each mover sits in |
| Not targets | The movers and what they contain; a group around a mover (its box follows the mover); anything whose position or size is bound to a mover (it moves with the drag); empties |
| Shift (axis lock) | Only the free axis snaps |
| Cmd or Ctrl, held | Snapping is off for as long as it is held |
| Alt-drag | The originals become targets while copies of them are dragged |
| A drag axis the store holds still (`DragMode::Axes`) | Does not snap |
| A drag through a constraint cluster | Does not snap at all (§4) |
| A rotated box | Its axis-aligned bounds snap, as in Figma |
| A path | Its tight bounds, not its control box |

Snapping only positions the selection: it writes nothing but the moved values, and the
commit is the same "move …" transaction. No constraint is created (CS U8, §4).

**Resizing** (Figma's): only the edges the grip moves snap, to the same targets'
edges and centres; with Alt both edges move about the centre and the nearer one snaps.
The feedback is the crosses alone, at the moved edge's corners and the target's, with
no line. A rotated box does not snap, nor a corner dragged with Shift (one axis follows
the other).

**Path points and the pen** (Figma's): a point dragged in point editing snaps level or
plumb with the path's other anchors, and so does a point the pen is about to place with
the path being drawn, with a line and crosses. The pen does not snap when its click
would close the path, add a point to a segment or remove one, and decides at the press,
so dragging out the new point's handles keeps it where it snapped.

## 2. How it is built

- **The core** is `iron_spatial::snap`, pure kurbo with no Bevy or store:
  `Targets::new(boxes)` sorts every box's features per axis; `Targets::snap(moving,
  tol, axes)` returns the offset and the guides. Tests: `src/iron_spatial/tests/snap.rs`.
- **The targets are frozen at the press.** `select::snap_session` builds them when a
  press becomes a move: siblings from the mover's chain (the panel's roots at the top
  level, an embed's composition, a group's children), culled to the visible part of the
  view with `SpatialScene::bounds`, then measured with `snap_box` (the node's frame's
  bounds; a path's tight bounds). Bindings are checked through
  `Store::reactive().transitive_deps` on the target's position and size leaves.
- **The snap is applied in `Manip::Drag::update`** (`manipulate.rs`), after Shift's
  axis lock and before `drag_to`: `v += snap.delta`. The tolerance is recomputed each
  frame from the view's zoom, since a gesture follows the view.
- **Resizing** builds the same targets (`Transform2::snap`, marks only). Each moved
  edge is a `snap::Line` that follows the pointer at a rate, `(e − anchor) / (u0 −
  anchor)` in the box's unit coordinates, so `Targets::snap_lines` returns a pointer
  correction (`manipulate::snap_resize`); `Targets::guides_at` marks only the moved
  edges.
- **Path points** use zero-size boxes at the path's other anchors as targets
  (`SnapSession` on the "move point" drag). **The pen** snaps the pointer in
  `pen_tool_system` and keeps the correction (`PenTool::offset`) apart from the press,
  so its hit tests and the handle drag use where the pointer really was.
- **A switch for tests:** `__iron.snapping(enabled)` (`document_snapping`, in both
  `web_ffi` and `engine_dispatch`) sets `SelectTool::snap_off` and returns the move's
  current guides. The smoke turns snapping off, since its drags aim at exact points;
  `.claude/skills/run-iron/snapping.txt` checks snapping itself. Phase 5's user setting
  will own this flag.
- **The guides** live on the drag (`SnapSession::guides`) and `draw_select_overlay`
  draws them in the selection's overlay layer, on the pixel grid, in Figma's red.

## 3. Phases

1. **Done 2026-10-20.** The snap core and its tests.
2. **Done 2026-10-20.** Moving the selection: targets, Shift, bypass, Alt-duplicate,
   the guides.
3. **Mostly done 2026-10-20.** Resizing (moved edges only, Alt symmetric, marks alone),
   path points and the pen (level and plumb with the path's anchors). Open: the shape
   tools (the press and the dragged corner), Shift at a corner (snap the leading axis),
   and pen points to other shapes.
4. **Open.** Equal spacing (snap to an existing gap between neighbours, or centred
   between two) with gap labels, then Alt-hover distance measurements. `label_outline`
   already draws overlay text.
5. **Open.** A per-user setting (objects on/off, pixel grid on/off, reach) in the
   composition settings menu, which needs a `web_ffi` export and an `engine_dispatch`
   arm; optional rounding to the pixel grid; moving the port snap onto the same core.

## 4. Known limits

- **Constraint clusters.** A solved drag moves the cluster's other nodes and may not
  land on the requested point, so a guide could show an alignment that never happened.
  Snapping is off for such drags until the achieved box is read back and compared.
- **Live content.** A target driven by a running producer or a clip moves while the
  targets stay frozen. Not excluded yet.
- **Rounding.** A snapped value is mapped into the node's parent space, so a nested node
  can commit values like `99.99999997`. Resizing has this noise with or without
  snapping (`h="167.00000000000003"` for a plain bottom-edge drag), from the affine
  decomposition in `Part::apply`.
- **Snaps creating constraints** (CS U8) is undecided. The guides' match data would let
  a later "snap to align" constraint reuse this.
