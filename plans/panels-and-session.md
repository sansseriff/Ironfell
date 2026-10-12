# Panels, inspectors and session state

**Status:** design note, 2026-10-12. Built: the dock presenter and views, pan and zoom,
and the session tier with the selection (see "Built so far" at the end). Not yet built:
the workspace root, the inspector, and selection in undo. It came out of a conversation
after the select tool (plans/select-tool.md) and records the direction agreed there. It
revises one earlier verdict, in document-spine.md §4; see "What this changes" below.

## The question

The left panel is a placeholder. The bottom panel is a timeline by default. The plan is
control and parameter panels on the left and right, and Blender-like layout: any panel can
be any type, and some panels follow context, such as the parameters of the selected object.
The project blurs the line between UI and visual content, so the question is what a panel
is in those terms, and how a panel follows the selection.

## Three tiers of state

Every piece of state belongs to exactly one tier:

| Tier | Holds | Saved | Undo | Shared with collaborators |
|---|---|---|---|---|
| **Document** | content: compositions, entities, definitions, bindings, clips | yes | yes | yes |
| **Workspace** | panel layout, which panel shows what, pans and zooms, pinned inspectors | yes, beside the document, as Blender saves its screens in the .blend | no | no, per user |
| **Session** | selection, hover, playhead, the generated content of inspectors | no | selection, later (below) | no |

All three live in the store and speak the same language: nodes, components, relations,
transactions and bindings. The tiers differ only in the rules a root imposes. One
`Document` holds a `session` root and a `workspace` root beside the content roots. Their
rules:

- `serial::save` writes the workspace to its own section and never writes the session.
- **Neither advances the document's version** that views and collaborators track. Each
  has its own counter.
- **Validation keeps references one-way.** Session and workspace nodes may refer to
  content: relations and `Ref` values, read by bindings. Content may never refer to them.
  A saved file is therefore complete without them.
- **The model surface sees the session**, read-only by default: "what is selected" is a
  question a model can ask.

Keeping one `Document` means bindings, the reactive graph, views and ops work across
tiers with no second store and no cross-store references. The playhead, already runtime
state held by the store, is the precedent; it would move into the session root.

## Selection is relations

A selection is a node with a `selects` relation to each selected node. These are relations,
not children: a child is owned and placed by its parent, and a selected square stays where
it is in its composition.

- **Each viewer has its own `selection` node** in the session (one today, one per
  collaborator later). Its relations keep their order, so a Link still knows which figure
  was picked first.
- **Selecting is a transaction** of `Link` and `Unlink` ops on that node, tagged as
  session.
- **The select tool's reach is session data.** A reach is how a node was reached in a
  panel: the panel and the chain through groups and embeds (`select.rs`). It becomes
  relation data on `selects`. Today it lives in the `SelectTool` resource.
- **Continuous gestures preview.** A marquee changes the selection every frame, so it
  previews as a drag does and commits once on release.

### Selection in undo (later)

Figma puts selection into undo so that losing a large selection is recoverable. When
added, it works this way:

- **An edit records the selection.** A document transaction stores the selection before
  and after it, and undoing the edit restores the earlier selection.
- **Selection changes merge.** Consecutive session-only transactions combine into one
  undo step: five clicks between edits are one undo, and a 500-object marquee lost to a
  misclick is one undo away.
- **Redo mirrors undo.**

Until then, session transactions skip the undo stack. Nothing about the representation
changes when undo is added, which is why the representation comes first.

## A panel is a view

**A panel is a window onto a composition:** a composition, a projection (a 2D pan and zoom,
or a camera) and a rectangle. Panel "types" are not kinds of panel code but kinds of
composition:

| Panel | Shows |
|---|---|
| viewer | a content composition (`main` by default) |
| inspector | the session's inspector composition for one target |
| outliner | a composition generated from the model or composition view (the markup `__iron.view` already produces), drawn as rows |
| timeline | the timeline composition (clips over a time axis), as today |
| signal / data | a time-series composition, once ports exist (stage F) |

Blender's "any panel can be any type" is then a workspace setting: which composition a panel
rectangle shows. Splitting, resizing and swapping are workspace transactions, and the Rust
layout solver (taffy, document-spine §4) owns every rectangle, as it already owns hosted
nodes'.

## Views and presenters

"Panel" blurs two things that are kept apart:

- **A view is the meaning:** a source (a composition, the timeline root, the document,
  the selection), a notation (how it is drawn), and later a target (a pinned node, the
  selection, a point on screen).
- **A presenter is the container:** a docked tab or split (dockview), and later a
  popover, a context menu, a tooltip, or an embed in another composition.

A notation decides which part of a view the canvas draws (a Rust panel kind:
`composition`, `timeline`) and which part is HTML (a registered Svelte component). A view
can be **all canvas** (a composition viewer), **all HTML** (the document markup today),
or **hybrid** (the inspector: canvas sliders, HTML text fields). In a hybrid, the HTML
part lies over the canvas part and takes the pointer only where it draws. An existing
Svelte panel joins the system by being registered as a notation's HTML part. A
runtime-compiled Svelte panel (an LLM's, say) would be one more notation whose source is
a document node holding the code, mounted in a sandboxed iframe.

Ephemeral presenters (menus, tooltips) belong to the session tier. One physical rule
places them: there is one canvas under all HTML, so anything that floats over HTML
chrome must itself be HTML. Inside a canvas view it may be drawn by the canvas.

**Style follows meaning, not container.** Selected, hovered, "a figure of entity X", "a
port of kind heat" are facts in the document and the session. Highlight and colour rules
key on those facts, as cross-probe highlights already do through `figures`, so a node
looks the same in a tab, a tooltip, an embed or a plane in 3D.

## An inspector is a bound composition

An inspector is a composition in the session whose content is generated from its target's
components, one control per leaf, and whose controls are **bound** to the target's leaves.

- **Width is a worked example.** The inspector's width control is a `slider` (or number
  field) whose `value` is bound to `target.size.w`. Dragging it previews
  `slider.value` through its handle, and linear binding inversion (roadmap stage A) writes
  `size.w` on the square. One transaction, the same path a bar bound to a slider already
  uses.
- **Locked and derived values show as such.** A leaf with no handle (bound to two free
  leaves, driven by a clip, solved by a cluster) gets a read-only control with the
  `handle()` reason, which answers "why can't I change this" in place.
- **The target is a relation.** An inspector node `targets` either the session's selection
  (it follows) or a fixed node (it is **pinned**, as Blender's pin is). Pinning replaces one
  relation, and the pin is workspace state.
- **The content is regenerated on target change** by a bridge system, as transactions on
  the session. It is derived, so it is never saved. The authored part is the *template*
  per component kind: which control, with what range and units. It lives in the registry
  (code) at first and can become definitions in the library later (ECM §4), so a domain
  object (a laser) can bring its own inspector layout.
- **Hosting is per control.** Text and number fields are DOM-hosted nodes (spine §4's
  `host` component); sliders, colour wells and curves draw on the canvas. The inspector
  composition does not care which.
- **Several selected nodes** get controls for the leaves they share. A differing value
  shows as mixed, and an edit writes to all of them, as one transaction.

### What this changes

document-spine §4 classed the property inspector and the layers tree as **DOM chrome**,
outside the document. This note makes them compositions in the session, drawn by the same
reconcilers. It keeps their text inputs in the DOM through hosting. The spine anticipated
this: "chrome can become document content later without a store change." The tests in
that section still decide *hosting* per control. What changes is that a panel's content
is document-shaped (nodes, bindings), so the "objective rules linking state, visual and
control" apply to the editor's own UI too.

Menus, dialogs and the merge question stay Svelte chrome. They are transient and not
bound to leaves.

## Order of work

1. **Session root, selection as relations.** Move `SelectionState` and the select tool's
   reaches into the session; the shell and the model read them from there. No undo yet.
2. **A right-hand inspector** for the selection, with controls for `transform2d`, `size`,
   `radius`, `fill`, `stroke`, `slider` and `params`. Build it alongside the pen tool, as
   stage F's laser → fiber → meter demo needs parameter controls.
3. **The outliner** on the left, from the model and composition views. It replaces the
   current Document panel.
4. **Workspace root:** panel layout, splits and types, pins, saved beside the document.
5. **Selection in undo**, with merging and restore-on-undo.

## Open questions

- **Where the workspace is saved:** in the document file, as Blender does, or beside it
  per user. Collaboration argues for beside it.
- **How the inspector's per-component templates become definitions**, and whether a
  definition's inspector is part of its "notation" (ECM §5).
- **Whether an inspector of several nodes edits through each node's own handle**, or
  refuses when any one is locked.

## Built so far

**2026-10-12, the dock presenter.** `src-ui/views/` holds:

- `views.ts`: `ViewSpec` (source, notation) and the notation registry.
- `workspace.ts`: dockview-core 8.5.1, its stylesheet vendored as `dockview.css`, the
  default layout, and the saved layout (browser storage, `iron.workspace.v1`;
  `?layout=default` or **Reset layout** forgets it).
- `ViewPanel.svelte`: one view in a tab. It renders the canvas part only while the tab is
  visible, so a hidden tab posts no rectangle and Rust draws nothing for it.
- `AddViewMenu.svelte`: "+" in every tab bar opens any composition (`document_compositions`)
  or a fixed view.
- `html/`: the old Controls card split into the Document (markup), Model (conversation)
  and Selection views.

The demo's pendulum and level slider moved into compositions of their own (`pendulum`,
`controls`). `main` embeds them where they sat, so the Main tab reads as before. The
default layout:
- the centre stack holds Main, Scene, Pendulum, Controls, Assembly and Card;
- Document and Model are on the left;
- Selection is on the right;
- Timeline runs across the bottom.

Tabs drag to reorder, into other stacks, or onto an edge to split.

Two input fixes came with it:

- **An embed's background no longer hides what is beneath it.** An embed with no frame
  takes the bounds of its content, so its transparent area could cover a sibling
  composition's shapes. Pointer hits on an embed's own area (`Hit2D::background`) now
  rank below every shape hit.
- **A press is resolved where it happened.** The shell sends the press position
  (`left_bt_down_at`). On a slow frame, later moves no longer drag the press along
  with them.

**Pan and zoom per 2D view (2026-10-12)**, Figma's controls (`select::navigate_views`):
- the wheel or a trackpad pans, and Shift turns vertical scrolling horizontal;
- Ctrl/Cmd+wheel and pinch zoom about the pointer;
- Space-drag pans;
- Shift+1 fits the content, Shift+0 sets 100%, and Cmd/Ctrl +/− zoom about the centre.

Each view's pan and zoom live in `ViewCameras`, which replaced the Show-in-only pans.
Every 2D path (drawing, picking, the select tool, 3D embed textures) goes through
`PanelView::affine`, so they follow. The 3D camera controller now acts only on the view
under the pointer. Not yet saved with the layout.

**The session tier and the selection (2026-10-12)**, `iron_document`:
- **Session nodes and relations get ids from their own range** (`id::SESSION_BASE`), so
  content ids never shift. `serial::save` and the tree views leave them out.
- **`Store::apply_session` applies session ops** without a transaction, so the
  document's version, history and undo don't see them. It rolls back on error and
  bumps `session_version`.
- **The selection is a `selection` node under a `session` root**, with ordered `selects`
  relations whose `data` holds the panel and the chain the select tool reached the node
  through (`Store::selection`, `Store::set_selection`).
- **Tests:** `tests/session.rs`.

In the app:
- `bevy_app::select::sync_session_selection` writes the select tool's selection there
  each frame it changes, and reads back changes made elsewhere.
- `document_selection` (`__iron.session()`) returns the selection.
- The model gets a `SELECTED` block with each request, so "the selected one" resolves.

Selection in undo, with merging, is still to come; the representation is ready for it.

**A first inspector (2026-10-12).** The Selection view is now the Inspector. It shows
the first selected node's components and fields (`document_inspect`).
- A bound field shows its binding.
- A field edited through a linear binding says which source it moves.
- A locked field is disabled, with the reason.
- Setting a field is one transaction through the field's handle
  (`document_inspect_set`). Setting the shadow's bound x, for example, moves the square.

This is HTML, not yet the bound session composition this note describes. The semantics
are the same: edits go through handles and locks show why. The representation moves
to the session composition when hybrid views exist.

Follow-ups:

- **Save each view's pan and zoom** with the workspace layout.

- **Show in activates or opens the tab** that shows the counterpart's composition,
  instead of only panning views already visible.
- **The workspace moves into the store** (its own root), then the inspector.
