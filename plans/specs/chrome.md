# The editor's chrome as context-aware data

**Status:** Proposed 2026-10-26; nothing built. Follows [`affordances.md`](affordances.md),
whose phases A and B it needs; to be scheduled after them.
**Scope:** Everything the editor draws over content that is not content: selection
boxes and grips, hover and selection outlines, component frames and chips, ports'
arrows, path points and handles, snapping guides, empties' markers, the marquee, tool
previews, insertion markers, the focus ring, the text caret. Today it is drawn by Rust
(`draw_select_overlay`, `draw_text_overlay`, `draw_gizmo_overlay`); this spec makes it
pictures of session state, defined as data, chosen by context, with affordances and
motion of their own.
**Direction (user, 2026-10-26):** interaction semantics and default motion are human
choices that should be data even when built in; "all the 2D chrome could, or perhaps
should, be thought of as context-aware data-driven state"; think big here.
**Master plan:** [`contexts.md`](contexts.md) (this is its domain 5).
**Builds on:** [`affordances.md`](affordances.md) (contexts, rules, the cascade,
interaction state, transitions); [`free-running.md`](free-running.md) §4 (the Stats
composition: session-tier editor UI made of document nodes, published by a producer);
the Library tab (a session composition); [vision 02](../vision/02_substrate-model-projection-control.md)
§9 (editors as artifacts: "the control surface and the state it tunes are the same
kind of thing").

---

## 0. Decisions at a glance (proposed)

| Question | Proposal | Why |
|---|---|---|
| What chrome is | **Pictures of session state**: ordinary nodes, in a per-view session composition drawn over the view, never saved | It is a projection of state (the selection, hover, a gesture) like any other picture; Stats already is |
| Where its look is defined | **Chrome definitions** in the built-in library (`std:chrome/*`), components like any other, written as data | One vocabulary; overridable; readable by a model |
| Which chrome a node gets | **Context rules**: `match="state:selected" chrome.selected="std:chrome/box"` | A schematic, a deck, a touch context can each look and behave differently |
| How chrome sits on content | An `attach` mechanism: placed relative to a content node's frame, outline or anchor, **sized in screen pixels** | Grips must not grow with zoom; outlines must follow shapes |
| How handles are operated | **Chrome parts have affordances** (a corner grip is a `box` drag; the rotate zone a `ring`) | The press cascade's rules 2–10 become ordinary picking |
| Handle priority | **Stacking**: chrome is above content, later chrome above earlier | The cascade's order becomes painter order, as a browser hit-tests |
| Motion | `affordances.md` §8's transitions on chrome (hover fades, the focus ring easing in, the insertion marker sliding) | One motion system |
| What stays code | The pointer state machine's core, picking, the interpolator, `attach` and outline geometry, the 3D gizmo (for now) | Mechanisms, not choices |

---

## 1. Today's chrome

| Element | State it shows | Interactive | Drawn in |
|---|---|---|---|
| Selection box, resize and rotate grips | selection frame | yes (box) | `draw_select_overlay` |
| Lit outlines of selected and hovered leaves; container boxes | selection, hover | no | same |
| Component frames (violet) and chip | selection or hover of an instance or main | no | same |
| Port arrows; hot port highlight | ports selected or hot | yes (plug) | same |
| Path points, handle diamonds, the edited path's outline | path editing | yes (points, handles, bend) | same |
| A lone shape's corner dots, arc handles | a lone selected rect or ellipse | yes (track) | same |
| Snapping guides and crosses | the gesture's snap | no | `draw_guides` |
| Empties' markers, fading with pointer distance | pointer, scope | pick only | `draw_select_overlay` |
| Pins and link lines of a component | its selection, a link being drawn | yes (relation) | same |
| Joints of plugged ports | two ports joined | yes | `draw_joint` |
| The marquee | a region drag | no | same |
| Tool previews: the pen's segment, a shape's or text box being dragged out, a Library entry on its way out | a tool's gesture | no | same |
| Text caret, selection, box | text editing | yes (text) | `draw_text_overlay` |
| 3D gizmo | 3D selection | yes | `draw_gizmo_overlay` |
| Cursors | the hovered affordance | — | `cursor_for` and friends |

Every one shows **session or gesture state** about **content**, at a **screen size**,
and many are **operated**. None is saved.

---

## 2. The idea

Chrome is a picture, like content: a picture of the selection, the hover, a gesture
in progress. Iron already draws pictures of state: it has nodes, bindings, the session
tier, producers and contexts. So:

- **Chrome definitions** are components in the built-in library: `std:chrome/box`
  (a frame and eight grips and a rotate zone), `std:chrome/outline`,
  `std:chrome/component-frame` (frame and chip), `std:chrome/port`,
  `std:chrome/path-points`, `std:chrome/guide`, `std:chrome/marker`,
  `std:chrome/marquee`, `std:chrome/insertion`, `std:chrome/focus-ring`,
  `std:chrome/caret`. Their inputs are what they show: a frame, an outline, a list of
  points, a colour, a state.
- **Chrome instances** are session-tier nodes in a **chrome composition** per view,
  drawn over it in screen space (its own overlay layer, as now). They are made and
  removed as state changes, with no history, as session ops are today
  (`Store::apply_session`). Their inputs are bound to state: the selection's frame,
  the hovered chain, the pointer, the zoom, the gesture's guides or previewed order.
- **Context rules choose chrome**, as `chrome.<slot>` declarations (`contexts.md`
  §2.1's `domain.field` form; slots such as `selected`, `hovered`, `lift`, `focus`,
  each naming a chrome definition or `none`): `std:free-2d` says a selected leaf gets
  `std:chrome/outline` and the selection gets `std:chrome/box`; `std:instance` says a
  selected instance gets `std:chrome/component-frame` and its ports; a schematic
  context can give parts terminal dots and wires bend handles instead; a presentation
  mode gives nothing.
- **Chrome parts have affordances**: a corner grip's rule says `drag="box"` with its
  corner; the rotate zone `drag="ring"`; a path point `drag="point"` on the point
  node; a port `drag="relation"`. Picking runs over the chrome composition first,
  then content: **the press cascade's rules 2–10 become stacking.** A newly declared
  handle is reached by being drawn above what it handles.
- **Chrome moves by declared transitions** (`affordances.md` §8): a hover outline
  fades in, the focus ring eases, an insertion marker slides with the gap, guides fade.

So the editor's look and feel becomes data a context can change, a document can
override sparsely, and a model can read: "make the selection orange and the grips
round for this deck" is an override of `std:chrome/box`'s inputs or rules, not a code
change.

---

## 3. Precedents

- **The browser's own controls** are rendered from internal markup and styled by the
  user-agent stylesheet; pages restyle their parts (`::-webkit-slider-thumb`,
  `::placeholder`, `accent-color`). Chrome made of the same stuff as content.
- **tldraw** lets an application replace its selection foreground, handles and
  overlays with its own components (`TLComponents`): the editor's chrome is a slot,
  not a constant.
- **Blender's gizmos** are defined per tool and editor through its gizmo API (Python),
  each with a target property they drive: chrome bound to properties.
- **Game engines' editors** (Unity, Godot) draw handles from per-component editor
  code bound to the component's fields.
- **Figma**'s handles are not data, but they are consistent with this model: one
  frame with grips, outlines for hover, a chip for components, a violet for mains.

---

## 4. Mechanisms the engine adds

### 4.1 The chrome composition

Each 2D view has a session-tier composition (`#view/chrome`), drawn after content in
the view's overlay layer, in the view's screen space. Its nodes are session-tier: not
saved, not in history, not shown in a model's views unless asked. A **chrome
producer** keeps it current: when interaction state, the selection or a gesture
changes, it matches context rules (`affordances.md` §4) to decide which chrome each
relevant node gets, makes or removes instances, and publishes their inputs, as the
Stats producer publishes its text.

### 4.2 Attach

A chrome node is placed by an `attach` facet rather than a free transform:

| Field | Meaning |
|---|---|
| `to` | The content node (or the selection as a whole) it belongs to |
| `at` | Where on it: the frame's corners, edges, centre; its outline; one of its anchors or points |
| `offset` | An offset in **screen pixels** |
| `size` | `screen` (px, constant under zoom) or `content` (scales with it) |

Attach is resolved by the engine each frame from the content node's world affine and
the view's transform, so grips stay 8 px at any zoom and on rotated frames.

### 4.3 Outline geometry

A chrome shape may take its geometry from a content node's outline (`outline_of`),
drawn with a stroke in screen pixels: lit outlines, the edited path's outline, a
component's frame.

### 4.4 Picking chrome

Picking walks the chrome composition first, by stacking, then content. A chrome part's
affordance comes from its definition's rules; its `drives` names a leaf of the content
it is attached to (a grip drives the selection's frame; a path point its point node;
a port its connection), so the store realises it as any gesture.

### 4.5 What stays code

The pointer state machine (press, slop, drag, release, Esc, double-click), picking,
the interpolator, `attach` and outline geometry, the producer's matching, modifiers'
meanings. The 3D gizmo stays code until a 3D chrome composition is worth it.

---

## 5. Contexts and overrides

- **`std:free-2d`'s chrome rules** reproduce today's look exactly (phase C2's exit).
- **`std:table`**: a selected cell block gets a cell-range outline; a focused cell a
  focus ring inside its box; row handles at the left edge (`sequence`).
- **`std:stack`**: the insertion marker during a `sequence` drag; padding and gap
  handles when the stack is selected (Figma's).
- **`std:instance`**: the component frame and chip; ports' arrows when hot.
- **A touch context**: larger grips (an input of `std:chrome/box`), no hover chrome.
- **Presentation and lesson modes**: no chrome, or only a focus ring on live controls.
- **A document** overrides sparsely: one rule changing a colour input, or its own
  chrome definition extending a built-in one.
- **A component** may declare chrome for its own parts: a gauge whose arc carries a
  range handle when selected, a curve component with its own control points. Figma
  cannot do this; here it is a rule in the definition.

---

## 6. Performance

Chrome changes on every hover and every gesture frame, so:

- Chrome instances are few (O(selection + hover + one gesture)); their nodes live in
  their own small composition, and their draw stays in the overlay layer, so content
  chunks are untouched (render-cache.md's rule that hover re-encodes no content; the
  render benchmark's `hover-sweep` must still show content `encode.n` = 0).
- Session ops skip history and validation that only content needs.
- The producer reuses instances across frames (moving a hover updates inputs, it does
  not remake nodes); bound inputs re-evaluate through the reactive graph incrementally.
- Budget: a hover change and its redraw within today's `overlay_encode` time, measured
  before and after phase C2 at 1k and 10k shapes.

---

## 7. Phases and exit criteria

**C1: mechanisms.** The chrome composition per view, `attach`, outline geometry, the
chrome producer, state inputs. *Exit:* one passive chrome element (the hover outline)
drawn from a definition, pixel-identical to today's, within budget.

**C2: passive chrome.** Outlines, container boxes, component frames and chips, guides,
empties' markers, the marquee, the insertion marker, the focus ring. *Exit:* the full
smoke's screenshots and the appearance and spatial pixel checks unchanged; the render
benchmark's hover-sweep within budget.

**C3: handles.** The selection box's grips and rotate zone, shape grips, path points
and handles, ports, pins, joints, each with affordances; picking over chrome first.
*Exit:* the press cascade's rules 2–10 are gone from `select.rs`; every guard script
from `affordances.md` §14 unchanged.

**C4: tool previews and text.** The pen's segment, drag-out boxes, the Library ghost,
the caret and text selection. *Exit:* `text.txt` and the pen sections of the smoke
unchanged.

**C5: contexts and demos** (§8).

---

## 8. Demos

1. **A schematic's chrome.** The same selection in a free composition and in a
   schematic-notation one: a box with grips in the first; terminal dots and wire bend
   handles in the second. One context rule apart.
2. **A component with its own handles.** A gauge whose selected arc shows a draggable
   range handle (a `ring` driving its `range`), declared in its definition.
3. **Restyled by a model.** "Make selection orange and grips round for this deck": the
   model overrides `std:chrome/box`'s inputs in the document; every view follows.
4. **Touch.** A touch context with 24 px grips and no hover chrome, toggled per view.

---

## 9. Open questions

1. **Inputs or rules for looks?** Colours and sizes as a chrome definition's inputs
   (overridden per context), or as rules on its parts?
2. **Cursors.** Part of chrome definitions (each part names its cursor), or derived
   from affordances as now?
3. **Multiple views.** One chrome composition per view (as proposed), or one per
   composition shared by its views?
4. **Can a model see chrome?** Views omit the chrome composition by default; a query
   could show it, for a model asked to restyle it.
5. **3D.** When does the gizmo become chrome in a 3D chrome composition?
