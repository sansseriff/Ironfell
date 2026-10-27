# Affordances: interaction and motion as context data

**Status:** Proposed 2026-10-26 (third draft, after review the same day). Phases A1 and A2
built 2026-10-26/27 with `contexts.md`'s foundation (its §5 steps 1 and 3); B in part
(ARIA keys for lists and grids, sequence, declared transitions; `contexts.md` §5
step 5 says what and what is left); C–D to come. Grew out of
[`layout-and-tables.md`](layout-and-tables.md) §10.
**Scope:** How input on 2D content is resolved (which node, which manipulation, which
leaf, what entering means, how focus moves) and how its presented values move when
they change. The engine supplies the mechanisms; every choice of behaviour, including
the built-in defaults, is data.
**Direction (user, 2026-10-26):**
- Iron's canvas need not expose everything a browser or rich UI toolkit does, but
  should push that way: complex interactive visuals end up much like browsers and
  Figma.
- Interaction semantics and default motion are human-interface *choices*. They may be
  implemented in Rust, but semantically they are data: a 2D or 3D context (a free
  canvas, an electrical schematic) is a context with its own default metadata about
  how interaction happens. Built-in defaults are data that never needs saving; a
  document saves only what it overrides.
- Not afraid of thinking big here: now is the time for overarching ideas.
**Master plan:** [`contexts.md`](contexts.md) owns the shared foundation (contexts,
rules, the cascade, the built-in library, the computed view) and the order of work;
this spec is its first domain (interaction and motion). Where they disagree on the
foundation, `contexts.md` wins.
**Follow-on specs:** [`chrome.md`](chrome.md) (the editor's own chrome as
context-aware data, after this one) and a later time-aware state machine spec (§8.4).
**Builds on:** [vision 02](../vision/02_substrate-model-projection-control.md) §8–9
(writeback by binding kind; editors as artifacts); [vision 06](../vision/06_the-computation-boundary.md)
§2–3 (state machines before handlers; enumerated choice);
[vision 09](../vision/09_entities-compositions-and-ports.md) (a thing is not its
picture; a composition is a world with a notation); [`component-model.md`](component-model.md)
(closed instances, ids not names, libraries); [`select-tool.md`](select-tool.md)
(Figma's selection, kept); [`infinite-canvas-architecture-patterns.md`](infinite-canvas-architecture-patterns.md)
§4 (one pointer state machine, manipulators); [`constraint-system.md`](constraint-system.md)
§3 (roles per solve, which stay below this spec).
**Vocabulary:** *affordance*: what a node offers to input in its context (not
*role*, which constraint-system.md uses). *Context*: a set of rules saying what
things in it afford and how they move. *Drag kind*: vision 02 §9.3's archetypes.

---

## 0. Decisions at a glance (proposed)

| Question | Proposal | Why |
|---|---|---|
| Who decides what input means | A **context**: rules matching the nodes in it, declaring their affordances, keyboard model and motion | The same rect is a point in a free canvas, an item in a stack, a cell in a table: what it sits in decides |
| Where behaviour is defined | **As data, always.** The engine holds mechanisms (resolver, drag kinds, interpolator); every behaviour, built-in defaults included, is context data in the document's own vocabulary | Defaults are choices that suit humans, not laws of the engine; a browser's defaults are a stylesheet |
| Built-in defaults | Shipped with the engine as data, loaded read-only, **never saved** (a user-agent stylesheet) | A document carries only what it changes |
| A composition's context | Named by its `notation` (exists; `"free"`) | Vision 09: a composition is a world with a notation |
| How much data per node | **None by default.** Rules match by type and position; an `affords` facet on a node is the rare exception (an inline style) | Ten thousand ordinary shapes store nothing |
| What a gesture may change | **One of the node's own leaves** (`drives`), realised through the existing bindings: direct, inverted, solver, override, or refused with a reason | Vision 02 §9.2; no second channel from gestures to state |
| Manipulation kinds | A fixed set the engine owns: point, box, track, ring, sequence, region, relation, drop, activate, choose | Vision 02 §9.3 and vision 06 §3: few archetypes, many bindings |
| Motion | **Interaction state as readable values** (hovered, selected, dragged…) and **declared transitions** (ease a channel when it changes), evaluated by one interpolator | CSS's pseudo-classes and transitions cover most UI motion; a later state machine spec builds on the same two |
| Keyboard | ARIA's composite-widget model, declared by the context | The web's tested answer to "the container decides how its children are operated" |
| Where it runs | `iron_document::interaction` (Bevy-free); the select tool, keyboard, renderer, model surface and native host ask it | One answer for every client |
| First phase | Behaviour-neutral: the built-in contexts, as data, replace `select.rs`'s node-type checks | The structure later phases read is built once |

---

## 1. Why: where the code is today

### 1.1 What is general and what is not

**What a gesture writes is data.** When layout took a child's position, no
interaction code changed: the position became a published leaf, `handle.rs` locks
published leaves, and the drag was refused with layout's reason. Binding inversion,
overrides, solver drivers and locks all work this way.

**What a gesture is about is code.** `select.rs` (3,722 lines) resolves a press by
about fifteen ordered rules and about 51 node-type checks; the text tool, pen and
manipulators add a dozen more. Tables added `"table"` to four lists and a rule for
rows.

**Motion does not exist.** Nothing eases; editor feedback appears and disappears.

### 1.2 The press cascade

1. Space held: pan. 2. The 3D gizmo's handles. 3. The Library tab. 4. A selected
component's pins. 5. The edited path's points and handles. 6. A lone rect's corner
dots or ellipse's arc handles. 7. A double-click on a lone path, rect or ellipse: edit
points. 8. The selection's resize and rotate grips. 9. A joint. 10. A hot port.
11. Empty canvas: drag the selection or a marquee. 12. Content: the target along the
hit's chain, a double-click entering. 13. Sliders in components, meshes, planes, 3D.
14. A top-level embed's background. 15. Select and move.

Rules 2–10 are **handles** on top of content; 12–15 are **content**; 1 and 11 are the
view's. This spec makes content resolution data and has nodes say which handles they
offer; [`chrome.md`](chrome.md) makes the handles themselves data, so their order
becomes their stacking.

### 1.3 `Manip` is most of the drag kinds already

`Drag` is point; `Transform` box; `Slider` and `ShapeGrip` track; `PathHandle` and
`Bend` point and track; `Plug`, `Link`, `JointMove` relation; the marquee region;
`Library` drop; `Plane`, `World`, `Gizmo` point and box in 3D. Missing: **sequence**,
**ring**, **activate**, **choose**, and track along any path.

### 1.4 The node-type checks, by what they decide

| Decides | Today | Becomes |
|---|---|---|
| What a press selects, enters, leaves; Enter, select all, marquee, snapping candidates; hover frames | `target_index`, `see_through`, `children`, key arms, marquee, snapping, highlights | `select` (§5.1) |
| A node's box and what resizing writes | `local_box`, `resize_capabilities`, `parts` | `grips` (§5.6) plus the drawable's shape |
| Entering in place | double-click branches, Enter, the text tool's `editable` | `edit` |
| Which handles a node offers | shape grips, path editing, pins, ports | `grips` |
| Which manipulation | slider-in-component, meshes, planes | `drag` |
| Labels, creation tools | `"move {ty}"`; shape, pen, empty, table tools | unchanged: tools create, affordances interpret |

---

## 2. Prior art

### 2.1 Defaults as data: the browser's user-agent stylesheet

A browser's default behaviour is largely **a stylesheet built into it**: `button`
gets a cursor, `details` toggles, form controls their look, all as ordinary CSS the
page's own CSS overrides in the same language. Nobody saves it into a page. Mechanisms
(`transition`, `cursor`, layout) are C++; their *use* is data. Likewise **Blender's
default keymap** is a keyconfig file of the same form a user's is, and **VS Code's
default keybindings** are JSON that user settings override entry by entry. This is
§3's split.

### 2.2 Context-dependent interaction on the web

| Web mechanism | Does | Here |
|---|---|---|
| `pointer-events: none` | Looked through | `select: through` (a table's row) |
| ARIA `role="presentation"` | Gone from the structure, children kept | the same, for focus |
| ARIA composite widgets (`grid`, `listbox`, `tree`, `tablist`) | **The container's role decides how its children are operated** | `keys` (§5.7) |
| `touch-action` | An element declares which gestures it handles | `drag` |
| `contenteditable` and `false` inside it | An inherited edit context with sealed islands | `edit`; closed instances |
| `inert` | A subtree stops responding | modes (§11) |
| The cascade (not inheritance) | Declare on a container; exceptions override | rules and precedence (§10), bounded to the direct container |
| Event delegation | An ancestor interprets its children's events | the context resolves |
| `scroll-snap-type` / `scroll-snap-align` | The container sets the rule, children take part | context rule plus a child's own |
| `tabindex`, roving tabindex | What takes focus; focus within a composite | focus (§5.7) |
| `:hover`, `:focus`, `:active`, `transition` | Values by interaction state, eased on change | §8 |

What the browser does not constrain is what a handler does. Iron adds that a gesture
only drives one of the node's leaves, through bindings that run both ways.

### 2.3 Others

- **Figma:** an auto layout frame's children reorder with siblings sliding aside;
  frames and groups select differently; locked layers; closed instances. **Smart
  Animate:** variants are states, transitions interpolate what changed.
- **Android touch interception, iOS's responder chain:** a parent claims a gesture
  before its children.
- **Blender:** keymaps per editor and mode, most specific first; operators' `poll`.
- **tldraw:** per-type `ShapeUtil` flags (resize, bind, handles).
- **Vega-Lite:** interaction as data: a few selection kinds bound to parameters.
- **SwiftUI:** UI as a function of state; motion an implicit "animate changes" modifier.
- **Rive:** authored, data-driven state machines with inputs and blends (the later
  state machine spec's precedent).
- **Direct-manipulation research** (Sketch-n-Sketch, Apparatus): a drag on output
  edits the program's parameters; Iron's handle inversion.

| Part of this design | Precedent |
|---|---|
| Built-in behaviour as data, never saved | user-agent stylesheet, Blender and VS Code keymaps |
| The container decides for its children | ARIA composites, touch interception, Figma's auto layout |
| Looked through, sealed, inert | `pointer-events`, `contenteditable=false`, `inert` |
| Rules matching by type and position, inline exceptions | CSS selectors and the `style` attribute |
| Resolution by context, most specific first | the cascade, Blender's keymaps |
| Keyboard within a container | ARIA's authoring practices |
| Motion as state plus transitions | CSS, SwiftUI, Smart Animate |
| A declared gesture kind driving a named value | Vega-Lite |
| **Every gesture lands on a document leaf through reversible bindings** | Iron's own (vision 02) |

---

## 3. Mechanism and policy

| The engine (mechanism, Rust) | Context data (policy) |
|---|---|
| The resolver: walk a hit's chain, apply contexts, pick a target | Which nodes are targets, looked through, closed, inert |
| The drag kinds: point, box, track, ring, sequence, region, relation, drop, activate, choose | Which kind a node's drag is, and which leaf it drives |
| Keyboard navigation for `list`, `grid`, `tree` | Which containers navigate which way |
| Interaction state (hovered, selected, focused, pressed, dragged, drop target) | What values change with it |
| One interpolator, a fixed set of easing curves | Which channels ease, when, how long, which curve |
| The order handles are tried in (until [`chrome.md`](chrome.md) makes it stacking) | Which handles a node offers |

Adding a mechanism is rare engine work, like a browser adding a CSS property; using
one is data. Nothing in the engine says "a rect", "a stack" or "150 ms": those are in
contexts.

---

## 4. Contexts

### 4.1 What a context is

A **context** is a set of **rules**. A rule **matches** nodes and **declares** parts
of their affordance and motion:

```xml
<context name="std:free-2d">
  <rule match="type:group"  affords.select="target" affords.drag="point" affords.grips="box"/>
  <rule match="type:rect"   affords.select="target" affords.drag="point" affords.grips="box shape" affords.edit="points"/>
  <rule match="type:text"   affords.select="target" affords.drag="point" affords.grips="box" affords.edit="text"/>
  <rule match="type:use"    affords.select="closed" affords.drag="point" affords.grips="ports pins"/>
  …
  <rule match="all" keys.nav="none"/>
</context>

<context name="std:stack">         <!-- a group with `layout` (stack_x, stack_y) -->
  <rule match="children" affords.drag="sequence" keys.nav="list"/>
  <rule match="children" motion.placement="out 160 layout"/>   <!-- siblings glide into the gap -->
</context>

<context name="std:table" for="type:table">
  <rule match="self"            keys.nav="grid" affords.grips="box"/>
  <rule match="type:row"        affords.select="through"/>
  <rule match="type:text"       affords.select="target" affords.drag="region" affords.edit="text"/>
</context>
```

- **`match`**: `all`, `self` (the container itself), `children` (direct children
  only), `type:<t>`, `children type:<t>`, or `state:<s>` combined with any of these
  (§8.1). Matching is by type, position and state, never by a node's name: names are
  content, ids are for the rare per-node exception (§4.4).
- **Declarations** are `domain.field` attributes (`contexts.md` §2.1): `affords.*`
  (§5), `keys.*` (§5.7) and `motion.*` (§8.2).
- **Cascade**: later rules override earlier ones, field by field. An absent field
  falls back to the notation's default for the node's type, never to a grandparent's
  context (§10; `contexts.md` §1.1).

### 4.2 Built-in contexts: the user-agent stylesheet

The engine ships a small set: `std:free-2d`, `std:3d`, `std:stack`, `std:grid`,
`std:table`, `std:instance`, and the editor's own (in [`chrome.md`](chrome.md)). They
are written in the markup above, embedded in the engine, and parsed once at startup
into a read-only built-in library, as the standard components are found by
`std:` keys (`standard.rs`). **They are never copied into a document and never
saved.** Everything §1.4 lists as a type check becomes a rule in one of them.

A model can read them, through a view of the built-in library, in the same markup a
document's own contexts use: how things behave is something it can look up, not guess.

### 4.3 Which context applies

- **A composition's** `notation` (`composition.notation`, which exists and nothing
  reads yet) names its context: `"free"` is `std:free-2d`. A schematic composition's
  notation names a schematic context; that is where "dragging a wire re-routes it"
  and "parts snap to the grid" live, for that world only.
- **Containers that are contexts**, inside it: a group with `layout` is `std:stack`
  (or `std:grid`); a `table` is `std:table`; a component instance is `std:instance`.
  A facet or type names its context, the way `display: grid` makes an element a grid
  container.
- **A document's own contexts** live in its library, beside definitions: a context may
  add rules to a built-in one (`<context name="std:free-2d">` with a few rules; built
  for A1) or be new (`<context name="schematic">`), and be
  named by a composition's `notation` or a container's `context` facet. Through
  component-model's libraries (§10 there), a schematic context can be imported and
  shared rather than copied.
- **A component's definition** may carry rules for its template's parts (the dial's
  needle turns; §5.4). Its instances read them through their expanded ids; nothing is
  copied per instance.

### 4.4 The inline exception: the `affords` facet

For a one-off, a node carries an `affords` facet, CSS's `style` attribute: the same
declarations, for that node alone (`scope="self"`) or for its children
(`scope="children"`). Rare by construction.

### 4.5 What it costs

Contexts compile into lookup tables (by type and position, per context) when loaded
or edited. Resolving a press is a few table lookups along the chain, cached by (chain,
document version, selection version) as picking caches hits. A composition of ten
thousand ordinary shapes stores no affordance data and resolves through
`std:free-2d`'s table.

---

## 5. The affordance vocabulary

| Part | Values |
|---|---|
| `select` | `target`, `through`, `closed`, `window`, `inert`, `none` |
| `drag` | `point`, `box`, `track`, `ring`, `sequence`, `region`, `relation`, `drop`, `none` |
| `activate` | `none`, `toggle`, `step`, `choose` |
| `drives` | one of the node's own leaves, or the kind's default |
| `about`, `step`, `options` | a ring's centre or a track's path; a step; a choice's options |
| `edit` | `none`, `text`, `points` |
| `enter` | `select`, `edit`: what entering the node from its container does (built in A1) |
| `grips` | any of `box`, `aspect`, `shape`, `ports`, `pins`; or `none` |
| `keys` | `none`, `list`, `grid`, `tree` (on a container) |
| `cursor` | derived from `drag` and `select`, overridable |

### 5.1 Select

`target`: selectable, enterable if it has children. `through`: never a target; presses
and focus pass to its children. `closed`: a target never entered; its ports and pins
are its handles (component-model's encapsulation). `window`: a top-level embed; its
content selects directly, its background selects it. `inert`: drawn, but nothing in it
responds; Cmd does not reach inside. `none`: not hit.

Along a hit's chain: skip `through`; stop at the first node whose container is not
open (Figma's rule; the session tier's selection says what is open); never pass
`closed`; stop at `inert`. Cmd selects the hit node itself, except under `inert`.

### 5.2 Drag kinds

| Kind | Value | Drives (default) | Commit | Today |
|---|---|---|---|---|
| `point` | the pointer in the context's space | `transform2d.x/y` via `begin_drag` | "move …" | `Drag` |
| `box` | the selection frame's affine | the parts' size, scale, rotation | "resize …", "rotate …" | `Transform` |
| `track` | `t` along `about` or a slider's track | named | "set …" | `Slider`, `ShapeGrip` |
| `ring` | the angle about `about`, unwrapped | named | "turn …" | new |
| `sequence` | an index among the context's items | the node's place among its siblings | "reorder …" | new |
| `region` | the children a rectangle touches | the selection (session) | none | marquee; cells new |
| `relation` | the port or pin released on | a connection, a link | "connect …" | `Plug`, `Link`, `JointMove` |
| `drop` | the container and index released over | parent and place | "move into …" | `Library` |

### 5.3 Activate

A click without a drag, or Space when focused. `toggle` flips a Bool leaf (a
checkbox, a switch); `step` adds `step` to a Number (a stepper); `choose` picks the
next of a declared enum's values, or opens a popup of them (**enumerated choice**,
vision 06 §3: segmented control, cycle-click, hover popup).

### 5.4 Drives

An affordance names **one of its node's own leaves**: `transform2d.rot` for a dial's
needle, `slider.value` for a knob, a bead's `args.t`. The drag kind turns input into a
value for it; the store previews and commits it as any drag, and the leaf's handle
decides what that means: an authored constant changes; a leaf bound linearly to one
source moves that source (the needle's rotation moves the meter's `in`, through the
binding the meter already has); a solver variable drives its cluster; a derived node's
leaf becomes an argument or an override; anything else is refused with a reason.

An affordance therefore adds no route from gestures to state. What cannot be inverted
is refused, not guessed; vision 02 §8.3's model-proposed, reviewable repair is the
route for those and is not part of this spec.

### 5.5 Edit

`text` starts the text tool's editing; `points` starts path editing (a rect or ellipse
converts first, as now).

### 5.6 Grips

The handles a node offers when selected: its `box` (resize and rotate; `aspect`
keeps its proportions), `shape` grips (corner radii, arc), `ports` (plugging), `pins`
(component authoring's links). Until [`chrome.md`](chrome.md), the editor tries them in
the cascade's order (§1.2); after it, they are parts of chrome definitions with
affordances of their own.

### 5.7 Keys and focus

| `keys` | Arrows | Tab | Enter, Esc | Like |
|---|---|---|---|---|
| `list` (a stack) | previous/next along the axis; Alt+arrows move the item (`sequence`) | out | enter the item; leave | `listbox` |
| `grid` (a table) | to the neighbouring cell, through merges | next cell; a new row past the last | edit the cell; back to the table | `grid` |
| `tree` (groups of groups) | siblings; Right enters, Left leaves | out | as `list` | `tree` |
| `none` (a free canvas) | nudge (today's) | next sibling (today's) | enter, leave (today's) | — |

A focused node with a value affordance changes it by keyboard in its domain: a track
or ring by its step, a point by a pixel (Shift ten), `activate` by Space. Focus is the
selection (session tier), shown by a focus ring ([`chrome.md`](chrome.md)).

---

## 6. Sequence, as Figma does it

During a `sequence` drag the item follows the pointer on top; the **other items open a
gap** where it would land. The gesture holds a **previewed order**, a session-tier
permutation (never a document change); the layout producer lays the stack out with it,
so Taffy computes the gap and the siblings move by ordinary layout. `std:stack`
declares a placement transition for its children (§8.2), so they **glide** rather than
jump; with no transition declared they snap. Release commits one `Reorder` op and
layout settles where the preview showed; a drop where it started writes nothing.
Leaving the stack while dragging turns the gesture into a `drop` (phase D), and the
stack it enters opens its own gap.

---

## 7. Ring, track, region

**Ring.** The angle about `about`'s origin in its space, unwrapped across ±π during
the drag; Shift snaps to `step`. **Track along a path** (phase C): `t` is the nearest
parameter on `about`'s outline, normalised by length; the expression language gains
`path_point(#p, t)` and `path_nearest(#p, x, y)`. **Region** (phase D): a rectangle
selects the children it touches; in a table, a block of cells, Shift extending.

---

## 8. Interaction state and motion

### 8.1 Interaction state is readable

Every node in a view has derived, never-saved **state values** from the session tier
and the resolver: `hovered`, `selected`, `focused`, `pressed`, `dragged`,
`drop_target`; a view adds `pointer` and `zoom`. They are readable two ways:

- **In rules**, as a match: `match="children state:dragged"` declares how a dragged
  item looks (CSS's `:hover`, `:active`).
- **In bindings, on presentation leaves only**: `appearance.opacity = if(#self.state.hovered, 1, 0.6)`.
  A binding that reads state is marked presentation-only and can only target leaves
  that are never saved as constants (appearance, the drawn offset); it cannot reach a
  model value, so hovering never edits the document.

### 8.2 Transitions are declared

A rule may declare that a **channel** of the nodes it matches eases when its resolved
value changes, as `motion.<channel>="<ease> <ms> [<when>]"` (for example
`motion.placement="out 160 layout"`; `motion.opacity="none"` turns it off):

| Part | Values |
|---|---|
| channel | `placement` (position, rotation, scale), `size`, `opacity`, `color`, `all` |
| `ease` | `none`, `linear`, `in`, `out`, `in_out`, `spring`: the vocabulary `timing.easing` already uses, extended |
| `ms` | duration |
| `when` | `any` (default), `layout` (the change came from the layout producer), `state` (from interaction state), `gesture` (during a drag) |

One **interpolator** in the presentation layer evaluates every declared transition:
when a node's resolved value in that channel changes, its *presented* value eases from
where it was drawn to the new one. Picking, bounds, snapping and the document use the
resolved value; only drawing lags. While any transition runs, the view renders every
frame (free-running's mechanism), then returns to rendering on demand.

### 8.3 What the built-in contexts declare

`std:stack`: children glide on `layout` changes (the reorder gap; 160 ms out). Dropped
items ease from the pointer to their slot (`gesture`). A cancelled drag eases back.
`std:free-2d`: nothing (moves follow the pointer exactly). Hover and selection
feedback are chrome, declared in [`chrome.md`](chrome.md)'s contexts. All of it is
overridable: a schematic context declares no motion; a slide deck's, softer and longer.

### 8.4 State machines, later

These two primitives are what CSS, SwiftUI and Figma's Smart Animate cover most UI
motion with. Behaviour with **memory or sequence** (a component open or closed, "play
the entrance, then idle", vision 06's scrubbable transitions) needs explicit states.
A later spec builds vision 06's state machine on them: its current state is an enum
value rules and bindings read like any state (§8.1), its transitions animate through
§8.2's interpolator, and its progress is a scalar on a track. That spec also takes in
the playhead, clips and `notes/time-domain-signals.md`. One animation system, not two.

---

## 9. Drives through bindings: the examples

- **The dial** (a `meter`): its definition's rule on the needle's pivot,
  `drag="ring" drives="transform2d.rot"`. The rotation is bound linearly to `in`, so
  the store moves `in`; the readout follows. Every dial in every document turns.
- **The bead**: `drag="track" about="#wire" drives="args.t"`; its position is bound to
  `path_point(#wire, #bead.args.t)`.
- **A toggle**: `activate="toggle" drives="args.on"`; its look binds to `on`, and a
  rule eases its knob (`motion.placement="out 120"`).
- **A chart's bar**: `drag="track" drives="size.h"`, the height bound linearly to a
  data value: dragging the bar edits the data, and the table cell showing it follows.

---

## 10. Resolution and precedence

Bounded by `contexts.md` §1.1 (locality): a node's values come from three sources
only, and never from a grandparent. From weakest to strongest, field by field:

1. The composition's context (by `notation`) for the node's type, from the built-in
   library or the document's.
2. The context of its **direct** container (`std:stack`, `std:table`, a `context`
   facet, a component definition's rules for its parts), including that container's
   inline `affords` with `scope="children"`.
3. The node's own `affords` (`scope="self"`).
4. Modes (§11), on top.

A table in a stack: the table's own drag comes from `std:stack` (its container); its
cells' from `std:table`. Nothing the stack declares reaches the cells. Within one
context, later rules win. Cmd (deep select) is the engine's escape hatch,
except into `inert`. Whether a component may **seal** its parts against the context
it is placed in is open (§17).

---

## 11. Modes

A **mode** is a context applied on top for a view, held in the session tier:

- **Presentation** (a slide deck): content `inert`, controls live (`activate`,
  `track`, `ring`, `choose` on control components), a click elsewhere advances.
- **A lesson step** (stage G): everything `inert` except the parts the step names.
- **Locked** (the planned layers panel): `select: none` on a node, an `affords` facet.

---

## 12. API

`src/iron_document/src/interaction.rs`, Bevy-free:

```rust
pub struct Affordance { select, drag, activate, drives: Option<LeafPath>, about: Option<NodeId>,
                        step: f64, edit, grips: Grips, keys, cursor }
pub struct Motion { channel, when, ms: f32, ease }

pub fn affordance(store: &Store, chain: &[NodeId], i: usize, mode: &Mode) -> Affordance;
pub fn motion(store: &Store, chain: &[NodeId], i: usize, state: StateSet, mode: &Mode) -> Vec<Motion>;
pub fn target(store: &Store, chain: &[NodeId], open: &dyn Fn(&[NodeId]) -> bool, deep: bool, mode: &Mode) -> usize;
pub fn enter(store: &Store, chain: &[NodeId], at: usize, mode: &Mode) -> Enter;
pub fn up(store: &Store, chain: &[NodeId], at: usize) -> Option<usize>;
pub fn children(store: &Store, context: NodeId, mode: &Mode) -> Vec<Vec<NodeId>>;
pub fn key(store: &Store, chain: &[NodeId], key: Key, mode: &Mode) -> KeyAction;
```

Contexts are a module of their own (`contexts.rs`): the markup parser, the built-in
library (`std:*`, embedded), compilation into tables, the cascade. The presentation
interpolator lives beside the render walk (`document_bridge`), reading `motion`.
Model surface: views print a document's `context` nodes and `affords` facets, and an
`affords="…"` attribute where a node's effective drag differs from its type's
built-in; a `<context>` view of `std:*`; the manifest gains an AFFORDANCES section.

---

## 13. Migration (phase A)

| Today | Becomes |
|---|---|
| `target_index`, `see_through` | `interaction::target` with `std:*` rules |
| double-click's enter loop, path and convert branches | `interaction::enter` |
| `"escape"`, `"enter"`, `children` | `interaction::up`, `interaction::children` |
| select all, marquee, snap candidates | `interaction::children` |
| highlight frames for `group use empty embed` | `select` plus has-children |
| `resize_capabilities`, `local_box`'s `Kind` | `grips` plus the drawable |
| shape grips' `rect | ellipse`, path checks, text `editable` | `grips`, `edit` |
| slider-in-component | `std:instance`'s rule for a slider's knob |
| the `"tab"` arm | `interaction::key` with `keys="none"` |

---

## 14. Testing

- **Native, `tests/interaction.rs`:** (document, chain, selection, mode, modifiers) →
  (target, enter, up, drag, drives, motion), covering every `std:*` rule, nested
  contexts (a table in a stack in a component template), Cmd, `inert`, the cascade,
  `affords` facets, a document context extending `std:free-2d`, and `key` per `keys`.
- **The built-in contexts round-trip:** parsed, printed, parsed again, equal; the
  printed form is what a model reads.
- **Phase A's guard:** the full smoke, `layout.txt`, `tables.txt`, `snapping.txt`,
  `text.txt`, `piston.txt` unchanged.
- **Each demo (§16)**: a driver script with expected output.

---

## 15. Phases and exit criteria

Phase A is split around the focus context (`contexts.md` §5, steps 1–3):

**Phase A1: contexts and selection.** `contexts.rs` (markup, built-in `std:*`,
compilation, the bounded cascade, the computed view); `interaction.rs` (`affordance`,
`target`, `enter`, `up`, `children`); `select.rs`'s targeting, entering, leaving,
select-all, marquee and highlight checks use them. *Exit:* the guard scripts match;
those paths name no node type; a model can read `std:*` and `<computed>`; and one
visible change made by data alone: a document override makes the first double-click on
a table reach its cell.

**Phase A2: the rest of the checks** (boxes, grips, editing, slider-in-component, the
text tool, `Geo`), after the focus context. *Exit:* `select.rs` names no node type for
selection, boxes, grips or editing; every `std:*` rule is covered natively.
*Done 2026-10-27* (`contexts.md` §5 step 3): `affords.grips` and `affords.drag` added;
the per-type box and shape-grip *geometry* stays code, by `contexts.md` §4.1's test.
Both done in small steps, the guard after each.

**Phase B: drag kinds, keys and motion.** `sequence` with the previewed order,
`ring`, `activate` (toggle, step, choose); `interaction::key` for `list` and `grid`;
interaction state (§8.1); declared transitions and the interpolator (§8.2).
*Exit:* demos 1 and 2.

**Phase C: document contexts and inline exceptions.** `context` nodes in a document's
library (`extends`), a `context` facet on containers, `notation` resolving to a
context, rules in definitions read through expanded ids, the `affords` facet,
presentation-only bindings on state, `track` along a path, views and the manifest.
*Exit:* demos 3 and 4; a model edit to a definition's rule changes every instance's
next drag; a document context makes a composition behave differently from
`std:free-2d` with nothing else changed.

**Phase D: the rest.** `drop`, `region` for cell blocks, modes (demo 5), 3D through the
same resolver. The schematic notation is not here: it is `contexts.md` §5 step 8 and
§6, after chrome.

Then [`chrome.md`](chrome.md); then the state machine spec (§8.4).

---

## 16. Demos

1. **Auto layout reordering, as Figma does it** (B). A slide outline (a stack of
   bullet rows) and a strip of slide thumbnails. Drag an item: it lifts, the others
   glide aside to open a gap, release lands it; one "reorder" undo step; back where it
   started writes nothing. Override `std:stack`'s transition in the document (`ms="0"`)
   and the siblings snap instead: the motion was data. With phase D, drag a bullet
   into another list.
2. **ARIA keyboard: a table is a grid, a stack a listbox** (B). Arrows between cells
   (through merges), Tab past the end adds a row, Enter edits, Esc returns; in a
   stack, Alt+Down moves an item (the same `sequence` as dragging). The container
   decides, by keyboard alone.
3. **A controls kit made only of data** (C). A toggle, a stepper, a segmented choice,
   a turnable dial and a knob, declared entirely by rules in their definitions, with
   eased knobs. Wired to the laser; operated by pointer and keyboard. No control code
   in the editor.
4. **Linked representations, edited from either side** (C). One dataset as a table, a
   bar chart and a line chart: drag a bar and the cell and point follow; type in the
   table and the bars move (vision 09: one thing, many pictures).
5. **Presentation** (D). A deck in presentation mode: content inert, controls live, a
   click advances. (The schematic notation is `contexts.md` §6's demo.)

---

## 17. Open questions

1. **Sealing.** May a component's rules for its parts resist the context it is placed
   in (a control that must behave one way)?
2. **Modifiers.** Cmd is the escape hatch, Alt duplicates. May a rule claim Shift or
   Alt, or are modifiers the drag kind's?
3. **What views print.** Effective non-default affordances (`affords="…"`), every
   node's, or authored data only?
4. **Constraints.** Is a solver cluster a context (its members "drag through the
   solver"), or do constraint-system.md's roles stay below `drives`?
5. **Focus and selection.** One here; browsers keep them apart (a focused cell, a
   selected range). Needed once `region` selects cell blocks?
6. **Markup or nodes.** Built-in contexts in markup, a document's as `context` and
   `rule` nodes: one vocabulary, two forms (as a page's CSS can be a file or a
   `<style>` element). Or a document's contexts as markup in one leaf?
7. **Hover cost.** Measure resolution and motion with `bench-render.sh`'s
   `hover-sweep` before phase A lands.
