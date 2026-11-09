# Frame constraints: Figma's resizing model for people

**Council 2026-11-08:** name the facet `constraints` (Figma's word): "pin" already means a port shown on a component's edge (wires-and-formulas.md §4a). Pull a minimal Resizable workflow (the toggle and per-child constraints) ahead of the plot's axes/series split: it is the human path for sized instances, which shipped without one.

**Proposed 2026-11-08, deferred until after `std:plot`** ([`plot.md`](plot.md) phase 3).
How a person, not only the agent, makes a component (or any frame) that resizes well.

## 1. Why

Sized instances (plot.md §3, built 2026-11-08) let a component's picture have its own
box, and its parts follow the box through bindings (`{= #T.size.w - 20}`). That suits
the agent and code (`std:plot`), not people: nobody should type bindings to make a card
whose button stays in the corner. Figma's answer is familiar to most users: **Resizing
on** a frame, **constraints** on each child, and **auto layout** for rows and columns.

## 2. Decisions

| # | Decision |
|---|---|
| F1 | Each child of a sized container may carry a **`pin`** facet: `h` = `left` \| `right` \| `left_right` \| `center` \| `scale`, `v` = `top` \| `bottom` \| `top_bottom` \| `center` \| `scale`. Default (no facet) is left/top: nothing moves, as today. |
| F2 | Pins are **data the layout step interprets**, not generated bindings. The child keeps plain `x`, `y`, `w`, `h` measured at the container's **default** size; the layout producer (`layout.rs`, which already publishes auto-layout children's `x/y/w/h`) publishes where they land at the current size. Editing the main at its default size is then ordinary dragging; a generated binding would instead send a drag to the container's size. |
| F3 | Not the constraint solver. The solver is two-way (dragging either end moves the other); Figma's constraints are one-way, parent box to child, which is what the layout step already does for auto layout. |
| F4 | A sized container is a sized template (and so each sized `use`), or a `group` with `size` and no auto layout (a Figma frame). Pins under an auto-layout group are refused (the layout places those children). |
| F5 | A leaf is pinned, bound, or animated, never two (the existing one-writer rule): pinning a part whose `x` is bound is refused with the reason, and binding a pinned leaf too. |

## 3. What each pin publishes

With default size `W0 × H0`, current `W × H`, `dW = W − W0`, and the child's own `x`,
`w` (vertical the same with `y`, `h`):

| `h` | x | w |
|---|---|---|
| `left` | x | w |
| `right` | x + dW | w |
| `left_right` | x | w + dW |
| `center` | x + dW / 2 | w |
| `scale` | x · W / W0 | w · W / W0 |

By type: a rect, slider or table takes `w`/`h`; an ellipse its radii; **text** rewraps
(`left_right` makes it fixed width) and never scales its glyphs; a **drawn path** under
`scale` moves its anchors, keeping stroke width (as the select tool's resize does); a
nested sized `use` takes the new box as its size. Rotated children pin by their
bounding box, as Figma's do.

## 4. The flow

1. Draw, select, **Make component** (built).
2. On the main, the Component section's **Resizable** toggle gives the template a size
   equal to its parts' bounds (moving them so the box starts at 0, 0). Main and
   instances get corner grips.
3. Select a part: the Position section shows Figma's **constraints widget** (a square
   with pin bars, plus two dropdowns). Bits UI `ToggleGroup` and the shared `Select`.
4. Drag the main's corner to try it: the main previews at a new size without changing
   the default (Figma's behaviour; release restores, or Alt keeps it as the new
   default). Instances follow at their own sizes.
5. Rows and columns: an **Auto layout** inspector section for the existing layout
   engine (direction, gap, padding, align; children's fill/hug), Shift+A to wrap the
   selection in one.
6. For the agent: `pin="right bottom"` on `<create>` (fewer tokens than bindings).

## 5. Phases

| Phase | Builds | Exit |
|---|---|---|
| 1 | `pin` facet, layout producer for sized templates and frames, F5 refusals, agent syntax | a card's corner button stays in the corner and its title stretches as an instance is resized; undo one step |
| 2 | Resizable toggle, constraints widget, main preview resize | the same card made entirely by hand |
| 3 | Auto layout inspector section and Shift+A | a legend row of swatches and labels reflows |
| 4 | `std:plot`'s frame and title move from bindings to pins | no visible change |

## 6. Open questions

1. Figma's "Scale" also scales stroke widths and text when the whole frame scales (K
   key). Iron keeps scaling as the `use`'s own `sx/sy`, separate from resizing?
2. Min and max sizes on a template (plot.md §3.2) belong here too.

## 7. Done

**2026-11-08: phases 1 and 2, minimal (step 4 of the current order).**

- A `constraints` facet (`horizontal`: left, right, left_right, center, scale;
  `vertical`: top, bottom, top_bottom, center, scale), named for Figma's word (the
  council: "pin" means a port on the edge). Allowed on every 2D type.
- `layout::constrained`: on every picture of a sized template at a size other than
  the template's, each top-level part with constraints is placed for the new box,
  read from the picture's own copy (an instance's overrides count) and published
  under the layout producer on the copy; a bound leaf keeps its binding (F5). Rects
  and anything sized move and stretch by their box, ellipses about their centre,
  fixed-width text by its box; other parts (paths, groups without a size, auto-sized
  text) move by their origin only.
- `author::resizable` (the main's **Resizable** checkbox): the template gets its parts'
  bounds as its size, parts move so the box starts at 0, 0 and every picture moves the
  other way (nothing moves on screen), every picture gets the size. Off removes the
  sizes. One undo step.
- Inspector: on a main's part, a Constraints widget (a box with lit bars and two
  selects) in the Position section.
- The agent writes `constraints.horizontal="right" constraints.vertical="bottom"`.
- Not yet: the main previewing a resize and springing back on release (today a
  resized main stays resized and its constrained parts are locked until it is set back
  to the template's size); paths stretching under `left_right`/`scale`; frames
  (sized groups) outside components; auto layout's inspector section (phase 3); moving
  `std:plot` onto constraints (phase 4).
