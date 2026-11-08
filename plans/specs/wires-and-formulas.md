# Wires and formulas: the node graph Iron already has, shown on demand

**Proposed 2026-11-08.** A design note: what of a node editor (Blender's shader and
geometry nodes, Origami, TouchDesigner) Iron should expose, and how. Builds on
component-authoring.md §3 (pins, links and the mapping popover, built 2026-10-18) and
chrome.md (component chrome).

## 1. The position

Iron is already a node graph. Every binding is an edge from the leaves it reads to the
leaf it sets; every connection joins two entities' ports; the reactive graph
(`graph.rs`) evaluates it all. The question is not whether to have nodes but which
part of that graph to **show** and to **let people draw**.

Node editors' strength is seeing relationships; their weakness is that a canvas of
boxes grows faster than what it expresses (a one-line formula becomes six boxes and
eight wires). So:

| Layer | What it is | Who writes it | How it is seen |
|---|---|---|---|
| **Formula** | a binding, one line: `{= ease('easy_ease', window(#p, 0.3, 1))}` | the agent; a person in the inspector | text, the source of truth |
| **Wire** | an edge between two real things on the canvas (a slider and a curve; a port and a port) | a person, by dragging | purple lines, **only around the selection** |
| **Chip** | a modifier on a wire (Easy Ease, Map range, Window, Clamp, Smooth) | a person, from a menu on the wire | a small pill on the wire; it *is* one call in the formula |
| **Component** | where depth goes: complicated math inside a definition, ports outside | a person (Make component) or the agent | a box with pins (`std:plot` has dozens of bindings inside, 19 inputs outside) |

There is **no free-floating node canvas.** Every node on screen is either a real object
in a composition or a chip on a wire between two. That bound is what keeps it from
becoming Blender's editor.

## 2. Formulas stay the source of truth

The document stores bindings as text, as today. The wire view is a projection of them;
editing a chip rewrites the formula, and editing the formula redraws the chips. The
projection is exact (injective both ways) for a chain of known calls on one input:

```
#vis.progress ──[Window 0.3 → 1]──[Easy Ease]──▶ curve.trim.end
              ≡  {= ease('easy_ease', window(#vis.progress, 0.3, 1))}
```

A formula the chips cannot express (two inputs combined, a lambda, a series
expression) shows as **one formula chip** holding its text, editable in place:
Houdini's wrangle node, the escape hatch inside a node system. Nothing is hidden and
nothing is lost.

No general scripting language goes into the document. Expressions with series, lambdas
and broadcasting cover what a node editor's math does, far more compactly; heavy
computation belongs to external models (`model-interop.md`). When a gap appears, the
expression floor grows by a function.

## 3. Wires: purple, and only around the selection

- **Show on selection, as Figma's prototype connections do.** Select a component (or
  any node with bindings) and the wires touching it draw: what feeds it on the left,
  what it feeds on the right, in the component violet (`chrome.accent` family), with an
  arrowhead at the reader. Nothing draws when nothing is selected. Hovering a wire
  lights both ends and shows its formula in a tooltip.
- **Ends are real places.** A wire runs from the source's pin (a port pin,
  component-authoring §3, or the property's row on the object, see below) to the
  reader's. A leaf with no pin (a slider's value, a rect's width) anchors at the
  object's edge with a small label (`value`, `w`).
- **One hop by default.** Only wires touching the selection; Alt-click a far end to
  follow the chain. A selection with many wires (a plot's 19 inputs) bundles them by
  source object into one wire with a count, expanding on hover.
- **Off-canvas ends** (a node in another composition, a narrative value, the clock)
  draw to the panel edge with a chip naming the far end; clicking it reveals it
  (Show in, layers-panel.md).
- **Read-only bindings stay visible**: layout-placed and solver-driven leaves show as
  grey dashed wires with the reason (`placed by #g's layout`), so a person can see why a
  value will not move.

## 4. Drawing wires and chips

- **Drag from a pin or a value to a property** to bind it (component-authoring §3's
  link drag, generalised beyond mains): drop on an object, choose the property, and the
  mapping popover opens. Its result is a wire with a **Map range** chip,
  `scale(#src, a, b, c, d)`, as today.
- **Click a wire, or its `+`**, for the chip menu. First set, each one floor function:

| Chip | Formula | Edits |
|---|---|---|
| Map range | `scale(x, a, b, c, d)` | the four ends (the existing popover) |
| Easy Ease, Ease In, Out, In-out, Custom | `ease(curve, x)` | a preset, or the curve's handles (`cubic-bezier`) |
| Window (delay, stagger) | `window(x, a, b)` | start and end of the slice of 0–1 |
| Clamp | `clamp(x, lo, hi)` | the bounds |
| Smooth (lag) | `smooth(x, ms)` (later: a stateful filter, needs §6 Q2) | the time |
| Invert | `1 - x` | none |
| Formula | any expression | the text |

- Chips sit along the wire in evaluation order; dragging one reorders the calls;
  Delete removes one (its call unwraps). Undo is per edit, as any binding change.
- **The agent** writes the formula directly; the wires appear when the person selects
  the result. Agent and person edit the same thing.

## 4a. Pins that collapse, and dropping onto a component

A selected component shows its ports as pins (component-authoring §3). With many
inputs (`std:plot` has 19) a column of pins down each edge is noise. Pins are for
**wiring**; settings belong in the inspector's Component section, which lists every
input, grouped.

**Which pins show.** A `port_spec` gains `port.show`: `pin` (always a pin), `auto`
(default) or `settings` (never a pin; inspector only). A selected component shows:

- every `pin` port, and every `auto` port that is **active**: connected, bound, or set
  away from its default (the last as a small dot, not a wire);
- physical ports (anchors on the geometry) always, where they are;
- the rest collapsed into one **"+ N more"** pill per side, which opens a compact list
  of the hidden ports, each row draggable as a pin.

`std:plot` marks `xs`, `ys` and `f` as `pin` and its outputs `y_at_cursor` and the
ranges as `auto`; style, colours, labels and the rest are `settings`. The inspector
groups inputs by a `port.group` (Data, Range, Labels, Cursor, Look); the definition's
author sets both, in the Port section.

**Dropping onto a component.** A wire dragged from a pin or a value need not hit a tiny
pin:

1. While the drag is over a component, its pins expand to the **compatible** inputs
   (kind and unit match: a series to series inputs, mW to optical power), the rest
   dimmed. A drop on a pin connects to it, as now.
2. A drop on the **body**:
   - one compatible input: connect to it;
   - several: a **menu at the drop point** lists them, best guess first (a name that
     matches the source's, `xs` → `xs`; unconnected before connected; `pin` ports
     before others), each with its kind and unit and whether it is already connected.
     Typing filters; Enter takes the first; Escape cancels the drag;
   - none: no connection, and a note says why ("plot has no input that takes mW").
3. The result is a connection (port to port, both entities) or a binding (a value to an
   input: the input's argument bound, `args.ys = {= #src…}`), with a Map range chip
   when the units or ranges differ (the existing mapping popover).

The same menu serves a drop onto a plain object (the properties it can take: rotation,
size, opacity, trim…), which is component-authoring §3's "the drop asks which property".

## 5. Easing as a function

For chips (and transitions, machines.md) easing must be callable in a formula, not only
a key's setting:

- **`ease(curve, t)`**: t clamped to 0–1 and shaped by `curve`, a preset name or
  `cubic-bezier(x1, y1, x2, y2)` as CSS writes it. Presets: `linear`, `ease_in`,
  `ease_out`, `ease_in_out` (today's quadratics, kept so files read the same), `ease`
  (CSS's, `cubic-bezier(0.25, 0.1, 0.25, 1)`), `easy_ease` (After Effects' default,
  `cubic-bezier(0.33, 0, 0.67, 1)`), `easy_ease_in`, `easy_ease_out`, `hold`.
- **`window(t, a, b)`**: the slice a→b of t mapped to 0–1, clamped: a stagger or delay.
- Clip keys' `easing` accepts the same strings (`time::ease` gains the bezier form), so
  one vocabulary serves keys, chips, transitions and layout glides.
- **Trim Paths** (`trim` facet: `start`, `end`, `offset`, fractions of the outline,
  After Effects' Trim Paths) makes "draw this line on" one leaf; it renders by cutting
  the outline (any stroked shape) and exports to SVG as the dash trick.

Then a transition's tracks (machines.md) are bindings off its progress:

```
axes.trim.end  = {= ease('easy_ease', window(#appear.progress, 0,   0.4))}
ticks.trim.end = {= ease('easy_ease', window(#appear.progress, 0.3, 0.6))}
curve.trim.end = {= ease('easy_ease', window(#appear.progress, 0.5, 1))}
```

The timeline draws them as three staggered segments (a window is a segment; its curve
is the segment's easing); the wire view draws three wires from `progress` with two
chips each. One data, two pictures, and machines.md's tweens become this form rather
than a separate mechanism (to be folded into machines.md when it is built).

## 6. Open questions

1. Wire colour: the component violet for bindings and connections alike, or violet for
   connections (entities) and the selection blue for bindings (properties)?
2. `smooth` (lag, spring) needs memory between frames, which a formula does not have;
   it belongs with machines' presentation-time state, not the expression floor.
3. Bundling thresholds and how far a chain is followed by default.
4. Wires in 3D compositions (to and from mesh properties): later.

## 7. Phases

| Phase | Builds | Exit |
|---|---|---|
| 1 | `ease()`, `window()`, `cubic-bezier` and the presets in `time::ease` and the floor; the `trim` facet, renderer and SVG export | a line draws on with Easy Ease from a binding on a slider; keys accept `easy_ease` |
| 2 | Read-only wires around the selection (bindings and connections, pins as ends, off-canvas chips, hover formula); collapsing pins (`port.show`, `port.group`, the "+ N more" pill, §4a) | selecting the plot shows three pins, its wires, and "+ 16 more"; selecting a slider shows what it drives |
| 3 | Drawing: drag value to property (generalised link drag), drop on a component's body with the compatible-input menu (§4a), chips menu, editing chips, formula chip | a person wires a slider into a plot by dropping it on the plot and picking `cursor`, and makes the three staggered draw-ons of §5 without typing |
| 4 | Bundling, chain following, read-only reasons | a selection with 19 wires stays legible |

## 8. Done

**2026-11-08: phase 1.**

- `time::Curve` and `time::curve(name)`: `linear`, `ease_in`, `ease_out`, `ease_in_out`
  (the quadratics, unchanged), `ease`, `easy_ease`, `hold`, and `cubic-bezier(x1, y1,
  x2, y2)` (x1, x2 in 0..1; Newton then bisection, as browsers solve it). `time::ease`
  uses it, so clip keys accept `ease` and `easy_ease` (added to `key.easing`'s options).
  Custom beziers on keys wait for `key.easing` to accept text (it is an enum); formulas
  take them now. The `easy_ease_in`/`_out` presets are left out: After Effects' names
  run opposite to CSS's and would confuse.
- Floor: `ease(curve, t)` and `window(t, a, b)`, both per element over a series.
- `trim` facet (`start`, `end`, `offset`) on rect, ellipse and path:
  `geometry::trim` cuts an outline by arc length (every subpath in order as one run,
  wrapping by `offset`); `materialize::trimmed` draws a trimmed rect, ellipse (not an
  arc) or path as a path whose stroke is the kept part and whose fill is whole; SVG
  export writes `pathLength="1"` with a dash (when no dash is set). The inspector's
  Stroke settings popover has a Trim group (add, remove, start, end, offset) and Path
  length.
- `std:plot`'s series draw on through `trim.end = reveal` instead of the dash trick.
- Not yet: the easing menu with curve previews (phase 3 and the timeline), Trim on
  text or tables, `smooth` (§6 Q2).

