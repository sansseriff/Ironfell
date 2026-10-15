# The select tool

**Status:** first cut done 2026-10-11 on `document-spine` (roadmap stage E, before the
pen tool). Code: `src/bevy_app/select.rs`. Exit checks: the last section of the run-iron
smoke script.

**Decision (user, 2026-10-11):** replicate Figma's selection, drag and transform state
machine and semantics. Konva's transformer was the prompt; Figma is the reference.

## What it does

### Selecting

Every pointer hit carries its **chain**: the nodes from the panel composition's top
level down to the hit node, through groups and embeds (`Hit2D::chain`,
`Hit3D::chain`, `PointerHits::embed_chain`). A click selects one node along the chain:
the outermost node whose container is not **open**.

- A container is open while something inside it is selected. A sibling of the
  selection is therefore one click away, as in Figma.
- **A top-level embed is always open.** It behaves like a Figma top-level frame (an
  artboard): its contents select directly, and its background selects the embed. Without
  this rule the viewer would be unusable, because the whole scene sits inside one
  top-level embed (`scene view`). This also keeps the click-through behaviour the user
  chose for the main layout in C1.
- A nested embed (a plane in the 3D view) and a group are closed: a click selects the
  embed or group as a whole.

| Input | Effect |
|---|---|
| Click | select the target, replacing the selection |
| Shift-click | add the target; Shift-click a selected node removes it |
| Cmd/Ctrl-click | deep select: the hit node itself |
| Double-click on a selected node | enter it: select the next node down the chain |
| Click a selected node in a multi-selection | that node alone (on release, so a drag still moves them all) |
| Press on empty canvas, or on an unselected top-level embed's background, then drag | marquee over that level (the panel's top level, or the embed's content); touching selects; Shift adds |
| Press inside the selection's box | drag the selection, even between its nodes |
| Esc, Shift+Enter | select the parent; at the top level, clear. Esc during a gesture cancels it |
| Enter | select the children (a group's, or an embed's content) |
| Tab, Shift+Tab | next or previous sibling |
| Cmd/Ctrl+A | all siblings of the selection, or the top level of the panel under the pointer |
| Arrows | nudge 1 px (Shift: 10 px), one transaction per press |
| Delete, Backspace | delete the selection, one transaction |

Hover shows what a click would select: an outline for a shape (the existing
highlight), a box for a group or embed. Cmd shows the deep target.

### The box

The selection's box sits in the panel's document space. For one node it is the node's
own box, oriented with the node. For several nodes it is the axis-aligned bounds of
their boxes, with each node outlined. Grips:

- **Corners** resize both axes; **edges** (invisible zones along the sides) resize one.
  Shift keeps the aspect ratio; Alt resizes about the centre. Dragging past the opposite
  side flips the node.
- **Just outside a corner** rotates about the box centre. Shift snaps the node's final
  rotation (one node) or the turn (several) to 15°.
- **Inside** moves the selection through `Store::begin_drag`, so constraint clusters and
  bound leaves behave as for any drag. Shift constrains the move to an axis.

Cursors follow the grip: CSS resize arrows along the grip's direction, and an SVG
rotation arrow for rotation, which the shell draws (`cssCursor` in `panel_manager.ts`).

## How it is built

- **Nothing the box shows is document state.** The box, the grips, the marquee and the
  hover box are drawn by `draw_select_overlay` into a second layer of each 2D panel's view
  (`VectorLayer::in_view`), so they never re-encode the document's layer.
- **Every gesture previews through the store and commits once.** Moves go through
  `begin_drag` and `drag_to`. Resize, rotation and nudges compute one affine `A` in panel
  document px and apply it to each affected node, called a *part*. A part's leaves come
  from decomposing `P⁻¹·A·P·L·B`, where:
  - `P` is panel ← the node's parent space;
  - `L` is the node's translate, rotate and scale;
  - `B` is the unit square onto the node's box.

  The decomposition drops shear, flips by negating `sy`, and keeps the rotation
  unwrapped against its start. It writes these leaves:

  | Node | Leaves written |
  |---|---|
  | rect, slider, 3D embed | `transform2d.x/y/rot/sy` and `size.w/h` (only `w` when a camera fixes the aspect) |
  | circle | its centre and `radius.r` |
  | 2D embed | `frame.*`: resizing changes its window and the content stays put. Without a `frame` component there is nothing to write, so its grips are disabled |
  | group | resized through its leaves; rotates as itself |

- **Locked leaves hold still** (`preview_target` refuses them, and they are logged at
  debug level). A grip is drawn grey and does not respond when no part could follow it.
- **Gestures without a box keep their own paths** (`interaction::drag_apply_system`):
  - meshes in 3D;
  - a plane in 3D, dragged by its 3D position;
  - content on a plane;
  - slider scrubbing;
  - anything in a 3D panel.

  They are started by `begin_legacy`.
- **A selection made elsewhere gets a reach.** The shell, Show in and the model surface
  can all set the selection without any record of how each node is reached in a panel.
  `find_reach` finds a 2D panel showing the node's composition, directly or through
  embeds.
- **The shell sends the modifiers and the tool keys** (`set_modifiers`, `tool_key`).
  Alt and Cmd never arrived as keys before. The worker now applies a press at the
  position the press happened.

## Fixed along the way

- **Undo of a delete failed when restored siblings bind to each other.** Deleting the
  pendulum group and undoing it was refused with "binding reads … was deleted". Binding
  and entity checks for a `Create` now run once the whole transaction has applied
  (`op::check_created`), so a subtree restores in any order. The regression test is
  `tests/spine.rs` `deleting_a_subtree_with_sibling_bindings_undoes`.

## Changed behaviour

- **The pendulum is a group.** A plain click on the pendulum selects and drags the whole
  group. The IK drags need Cmd (deep select), or a double-click into the group first. The
  smoke script holds Cmd for those steps.
- **The card plane in the 3D view is a nested embed.** Hover and click select the plane.
  Cmd reaches its content.

## The pen and paths (2026-10-12)

Stage E's exit: draw, close and edit a path; each click is one transaction; undo per
point. `bevy_app/pen.rs`, with the types in `iron_document`.

**How a path is stored.** A `path` node holds a `path` component (`closed`) and its
`point` children, in sibling order. Each point has a `transform2d` (its position in the
path's space) and `handles` (`ix iy ox oy`, relative to the point; zero is a corner).
The handles are leaves on the point, as decided in roadmap §7 item 4, so a handle can
be bound like any other value. The bridge builds the outline from the points
(`materialize::path_outline`): a line where both handles are zero, a cubic otherwise.
A point's change redraws its path.

**Picking.** A click hits a closed, filled path's inside, or any path within 5 px of
its line.

**The pen tool.** Figma's: P picks it, V returns to select, and the toolbar in each
composition tab bar shows which is active.
- A click places a corner. A click-drag places a smooth point: the out-handle follows
  the drag, the in-handle mirrors it, and Shift keeps the handle on an axis.
- Clicking the first point closes the path.
- Enter or Esc finishes the path open. Esc with nothing drawn returns to select.
- **One transaction per point:** "draw path" makes the path with its first point,
  then each "add point", then "close path". Undo takes back one.
- The line to the pointer and the handle being dragged are overlay drawing, never
  stored.
- A new path goes into the artboard under the press (a top-level embed's composition)
  or the view's own composition.

**Editing.** A lone selected path shows its points and handles.
- A point drags through `Store::begin_drag` like any node ("move point").
- A handle drags its leaves ("move handle"), mirrored unless Alt.
- Resizing a path moves its points and scales its handles (`Kind::Points`), so the
  stroke keeps its width.

## Shapes and paths (2026-10-14)

**Decision (user, 2026-10-14): Figma's model.** Rectangles and ellipses stay
typed shapes with parameters; every shape has a path outline; editing a
shape's points turns it into a `path`. Not Illustrator's (everything a path
with "live shape" metadata): Iron binds and constrains parameters (`radius`,
`size`), the model surface reads them, and points are child nodes, so a path
per rectangle would multiply the node count by five.

- **`circle` became `ellipse`** (schema 4): `radius.rx`, `radius.ry`, centred
  on its origin as the circle was, so constraints on joints are unchanged.
  Figma has no circle type either. A resize writes both radii (and the
  rotation and flip, like a rect), so stretching a circle makes an ellipse.
  Schema 3 files load: `circle` reads as `ellipse`, `r` fills both radii, and
  every binding or clip that read `radius.r` reads `radius.rx`
  (`serial::migrate_radius`). The model surface now sees `<ellipse rx ry>`.
- **Converting** (`iron_document::shape::to_path`) is a `Delete` and a
  restoring `Create` of the same id in one transaction, plus the points
  (four corners; four smooth points with κ = 0.5523 handles). Id, parent,
  order, transform, paint, name and every relation survive, so figures,
  constraints and the selection still hold; one undo restores the shape.
- **Entering it:** double-click a lone selected rect or ellipse, or press
  Enter. It converts at once rather than on the first point edit, as Figma
  does; the result is a selected path showing its points.
- **The guard (user's proposal):** converting is silent unless it cuts
  something: a binding on the shape's own size or radius, another node's
  binding that reads them, or a clip animating them. Then the engine sends a
  question (`send_convert_from_worker`) and the shell's dialog
  (`ConvertDialog.svelte`) lists the cuts; on "Convert to path" readers are
  baked to their current values and such clips are deleted. Undo restores all
  of it. `__iron.toPath(node, force)` drives the same call.

**The toolbar.** The tab-bar tool buttons (squeezed once tabs overflowed) are
gone. A Figma-style toolbar floats at the bottom of every 2D composition view
(`views/html/CompositionTools.svelte`, the composition notation's HTML
part): Move (V); Rectangle (R) and Ellipse (O) behind one button that
remembers the last used, with a menu; Pen (P).

**Shape tools** (`bevy_app/shape_tool.rs`): drag out a box; Shift for a
square or circle, Alt from the centre, a click makes 100 × 100. One
transaction ("draw rectangle", "draw ellipse"), placed like a pen path (the
artboard under the press, else the view's composition); then the shape is
selected and the tool returns to Move.

**Grips** are about 1.4× larger: corner squares 10 px (were 7), path points
8 px (6), handle ends radius 4 (3), with hit zones to match.

## Corners, arcs and shadows (2026-10-14)

Three optional components, each added by its first gesture or from the
inspector's **+ corners / + arc / + shadow** buttons (and removed by the ×
on its heading):

- **`corners`** on a `rect`: `tl tr br bl`, clamped to half the shorter side
  when drawn. A lone selected rect shows a dot inside each corner (when it
  spans 48 px on screen); dragging one along its diagonal sets all four,
  Alt one ("round corners").
- **`arc`** on an `ellipse`: `start`, `sweep` (radians, clockwise on screen,
  parameter angles) and `inner` (hole ratio): a pie, a ring segment or a
  ring. Handles: the sweep's end on the rim, the start once the arc is
  partial, and the inner ratio ("edit arc"). Shift snaps angles to 15°;
  a sweep closing within 1° is the whole ellipse. A full ring has two
  outlines, so it refuses to become a path.
- **`shadow`** on a `rect`, `ellipse` or `path`: `color dx dy blur`,
  Figma's defaults (black 25 %, 0, 4, 4). The offset is in the parent's
  space, so the shadow stays below a rotated shape; `blur` is twice the
  Gaussian's standard deviation, as in CSS.

**One geometry.** `iron_document::shape` (now with `kurbo`) builds every
shape's outline as anchors with handles: the renderer draws it, and a
conversion to a path writes the same anchors (a rounded rect becomes eight
points).

**A gesture may add a component.** `Store::handle` treats a leaf of a
component the node allows but lacks as `Direct`, previews read it from the
overlay, and `commit_gesture` adds the component (defaults plus the previewed
fields) before its `Set`s: one transaction, one undo. This also unblocks
resizing a 2D embed without a `frame` (not yet wired).

**What Vello offers** (vello_hybrid 0.2, read from source): no rounded-rect
primitive. `fill_rect` has a fast path for plain axis-aligned rects only,
and a rounded rect is a path. For shadows it has two routes, both used:
`fill_blurred_rounded_rect`, an analytic blurred rounded rect with no
offscreen pass, for a plain or evenly rounded rect's shadow
(`DrawCmd::BlurredRect`); and filter layers with `DropShadow` (blur, offset,
composite) for any other shape (`DrawCmd::PushShadow`), which costs an
offscreen pass per shadowed shape. Document rects now reach the backend as
`Shape::Rect`/`RoundedRect` rather than paths. The classic backend draws the
analytic shadow but has no filter layers, so it skips the others.

## Selection and hover geometry cache (2026-10-13)

**Done in the working tree (roadmap H):** selection boxes, individual frames,
grip eligibility, and hovered-container boxes are retained in
`SelectionGeometryCache`, shared by drawing and idle cursor/grip testing.
Detailed resize/rotate `Part`s are constructed only on a press. This removes
the repeated descendant walks when the pointer or selection rests.

The cache is keyed by selected reaches, authored version, bridge
`GeometryRevision`, and playhead time. Resolved previews and same-version loads
invalidate it; pending resolved changes bypass reuse. Drawing finalizes a cache
computed before transform propagation, since embed bounds can read Bevy globals.
Boxes remain in composition space, so current pan/zoom maps them at use time.
Closed panels retire their entries. Invalidation is globally conservative until
independent bounds have per-subtree revisions.

Measured with shipped wasm SIMD at 10k shapes: resting selection 119 → 2.7 ms,
hover 13.7 → 2.7 ms, equal to the 2.7 ms idle worker CPU p50. Cache rebuild samples
are zero during these resting intervals. [Tables and raw data](../rendering/render-benchmark.md#6-simd-baseline-and-selectionhover-cache-2026-10-13).

Verified with the final dev-opt build: complete `run-iron` smoke (including resize,
rotation/undo, group selection, marquee, point editing, bound drags, and linked
views), no page errors or panics, and inspected screenshots
`mid-drag-bound-follows`, `select-rotated`, and `selection-cache-view-mapping`.
`selection-cache.txt` passes all 11 assertions at both DPR 1 and DPR 2: resting
reuse, descendant edit, undo, pan/zoom reuse, same-version reload, and named
slider/sample addressing. The slider reaches approximately 0.5. Native
`iron_document` suite: 90 tests passed.

Picking, independent bounds and per-view culling were implemented next (below).
Independent overlay targets remain a follow-up. Changing an overlay in the shared view still re-encodes its
target; this cache only removes repeated geometry preparation.

## Not yet

- Alt-drag to duplicate. Copy, paste and Cmd+D are done (`iron_document::copy`,
  [clipboard-and-assets.md](clipboard-and-assets.md)); Alt-drag waits on gesture
  ownership, because it creates and removes nodes mid-gesture.
- Snapping and smart guides, and the distance and size labels. The size label needs text
  in the display list.
- Group and ungroup (Cmd+G).
- Reparenting by dragging into a group or frame.
- The scale tool (K), which scales strokes and content.
- Right-click "select layer".
- A transform box in 3D panels; gizmos are a separate question.
- Resizing a 2D embed that has no `frame`. It would need an `AddComp` in the same
  gesture; the current commit makes only `Set`s.
- The tool-context resource and the hit-test interface of roadmap §4 are still implicit:
  `ActiveTool`, `SelectTool`, `PenTool` and `PointerHits` play those roles.
- For the pen: adding points to an existing open path, inserting a point on a segment,
  deleting a point while keeping the path, and converting between corner and smooth.
- Converting a shape lazily on its first point edit instead of on entering.
- Several shadows per shape, inner shadows (`fill_blurred_rounded_rect`'s
  `invert`), spread, and layer blur.
- Back from a path to a shape (Figma cannot either).


## Change-driven and indexed picking (2026-10-13)

The base 2D hit list is retained while the physical pointer, panel/view, document,
resolved projection, propagated pose and spatial index are unchanged. The 3D pass
still runs and can replace base hits with hits through a plane, so cached 2D hits
are restored before that pass each frame. Stationary-pointer edits, undo,
same-version replacement, view pan and zoom invalidate correctly.

A canonical per-composition `understory_index` now supplies candidate leaves and
embeds. Exact local geometry and clip/occlusion checks are unchanged; candidates
are sorted in document painter order and each occurrence keeps its embed prefix.
`SpatialScene` is prepared independently of drawing, including hidden source
compositions. Geometry, painted and interaction bounds remain separate.

`picking.txt` checks invalidation and zero idle pick work. `spatial.txt` compares
ordered exact hit chains with the exhaustive reference mode, including repeated
and nested hidden-source embeds, transformed/clipped paths, negative/nonuniform
scale, history/deletion, and 2D hits through a 3D plane. `selection-cache.txt`
remains the cache/DPR regression gate. See `render-benchmark.md` §7 for results.

Marquee and selected-group frames still use the exact oriented `Geo` traversal;
sharing subtree summaries and finer dependency revisions is a later increment.
Outlines still contribute commands to the content target until retained overlay
targets are implemented.
