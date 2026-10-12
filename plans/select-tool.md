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

## Not yet

- Alt-drag to duplicate. The store has no copy op yet; instantiation is the nearest.
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
  `SelectTool` and `PointerHits` play those roles. Extract them when the pen tool needs a
  second tool.
