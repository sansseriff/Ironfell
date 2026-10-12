# Panels, inspectors and session state

**Status:** design note, 2026-10-12. Nothing here is built. It came out of a conversation
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
