# Component authoring

**Status:** approved 2026-10-18, building. **Builds on:**
[`component-model.md`](component-model.md) (definitions, linked instances, overrides,
ports, anchors, plugging and mates). **Goal:** every part of the Components tab's demo
(laser, fiber, dial meter, slider, their wiring) can be made from the UI, without code.

## 0. Decisions

| Question | Decision |
|---|---|
| Where a definition is edited | On the canvas, through its **main**: a placed picture of the definition whose edits change the definition itself (Figma's main component) |
| How a component is made from shapes | Select shapes, **Make component**: the shapes move into a new definition's template, and a main takes their place |
| How instances are made | Duplicate or copy a main (or an instance), or drag one in from the Library tab |
| Colours | Mains solid violet with a `◆◆ name` chip; instances a lighter violet with a `◇ name` chip; parts inside either keep the selection blue (fixing Figma's complaint that both look alike); ports teal |
| Physical ports and settings | A port with an anchor on a picture is physical (plugs, snaps, mates); one without is a setting (laser power, fiber length, meter range), reached by a connection line |
| Pivots | Not a special verb: **add empty**, **wrap in empty**, and a **link** from an input to the empty's rotation, with a mapping |
| Port matching | Same kind; for `value` ports, the same dimension of their values; matching connector and genders when set |
| Fiber to fiber | Allowed: the dropped end follows the end it lands on, which holds |
| A joint's two parts | A split handle on hover: drag a half to pull that part off, the centre to move the joint |
| Standard components | A **Library** dock tab beside Model: a session-tier composition of previews, built at startup from the app's templates; dragging one onto a canvas places an instance |

## 1. Mains

A main is a `use` with an `instance` facet whose `mode` is `main` and whose `def` names the
definition. It figures no entity: it is the definition shown, not a placed thing in the
model. Expansion derives its template like a linked instance's; references to port_specs
stay on the port_specs (there is no entity to copy them to), so a main's ports are never
pluggable.

**Edits inside a main are edits to the definition** (`expand::translate`), so every
instance follows in the same transaction:

| Edit on `#main/part` | Becomes |
|---|---|
| Set a leaf, add or remove a facet | The same on the template node `part` |
| Create a node under `#main/part` or under the main | Created under `part`, or under the template |
| Delete, reparent, reorder | The same on the template node |
| Inside an instance nested in the main (`#main/inner/part`) | An override on the template's `inner` (a definition-level override) |

Deleting a main removes only its picture; the definition stays in the library.
Duplicating or copying a main makes a linked instance (an entity in the model and a
`use`), as Figma's copy of a main is an instance.

## 2. Making and editing

- **Make component** on plain shapes: one transaction that creates the definition and its
  template, moves the shapes into it (positions kept), and places a main where they were.
  On instances, phase 4's make component is unchanged.
- **The port tool** (`P` is the pen; the port tool is in the shape menu): inside a main,
  a click places a port there: a `port_spec` in the definition and an anchor empty in
  the template, named `port 1`, kind `value`, direction `in`. The inspector's Port section
  edits its name, kind, direction, loose end, connector and gender; the empty's rotation
  is its facing, drawn as a short arrow while it is hot or selected.
- **Settings**: the inspector's Component section on a main has **+ input**, which adds a
  `port_spec` with no anchor.
- **Empties**: **Add empty** (a tool in the shape menu: click to place) and **Wrap in
  empty** (Cmd+Alt+G, and in the inspector): the selection becomes children of a new empty
  at its centre, positions kept.

## 3. Links: lines from ports

- When a main or an instance is selected, its ports are shown as **pins**: physical ones
  at their anchors, settings along the left (inputs) and right (outputs) edges of its box,
  labelled.
- **Dragging a pin** draws a line to the pointer.
  - Onto another instance's compatible pin: a **connection** (port to port, as plugging).
  - Inside a main, onto a part: a **link**. The drop asks which property (rotation,
    position, size, opacity…) and opens the **mapping popover**: "when `in` goes from
    *a* to *b*, `rotation` goes from *c* to *d*", defaults from the port's range and the
    property's current value. It writes one binding in the definition,
    `scale(#in.port.signal, a, b, c, d)`.
- Lines of existing connections and links are drawn while either end is selected.

## 4. Ports: units, connectors, gender

- `port.connector` (a free string, `""` matches any) and `port.gender` (`any`, `male`,
  `female`, `neutral`). Two ports snap when their connectors are equal or either is empty,
  and their genders pair: male with female, neutral with neutral, `any` with anything.
- A `value` port's dimension is its signal's (`10 mW` is a power); two `value` ports
  connect only when the dimensions agree. Physical kinds (`optical_power`, …) carry theirs.
- The standard slider's `value` output carries a unit (its `unit` setting, `1 mW` by
  default), so slider to laser is a plain connection.

## 5. Fiber joints and the joint handle

- Two loose ends may plug together. The connection's relation to the end that was landed on
  carries `held`; that end keeps its place and the other follows it. Dragging the held end
  moves the joint.
- **The joint handle**: hovering a connected port shows a pill along the way the port
  faces (`anchor.wrot`), a half on each part's side (the parts' centres decide which) and
  a circle in the middle. Until 2026-10-20 it lay along the line between the two parts'
  box centres, which tilted it when a part's box held more than its body (the dial
  meter's readout pulls its centre below the dial).
  A half drags that part off (a plug drag from its port); the circle moves the joint (the
  held end of a fiber joint, or the rigid part of a mate or a plugged end, whose partner
  follows). In 3D the axis is projected; when it points at the camera, the halves become two
  rings.

## 6. The Library tab

- A dock tab beside Model, a composition view like any other. Its composition lives in the
  **session tier** (never saved, never in undo), built at startup from the app's standard
  templates (`iron_document::standard`): each entry a baked preview (the template's shapes
  with resolved values) and a name.
- **Dragging** a preview onto a 2D canvas is one transaction: the definition is copied into
  the document's library on first use (an `import` facet records `std:<key>`; later drags
  reuse it), and a linked instance is placed at the drop point.
- Entries: slider (settings `min`, `max`, `unit`; output `value`), readout, laser, fiber,
  dial meter. A cloud library later adds sources; editing the standard definitions
  themselves is not in scope.

## 7. Built

- **Store, 2026-10-18.** Ports carry `connector` and `gender`; `value` ports match by
  dimension; two loose ends join, the one landed on `held` (`plug.rs`, `anchor.rs`).
  Mains: a `use` with `instance.mode = main` expands its definition's template with no
  entity, and `expand::translate` sends edits inside it (sets, facets, creates, deletes,
  reparents, bindings) to the definition; one level deeper they are definition-level
  overrides. Make component on plain shapes moves them into a new definition and places
  a main at their top-left (`make::make_main`, `Store::extent`). A pasted or duplicated
  main becomes a linked instance (`copy::mains_to_instances`). `author.rs`: `add_port`,
  `add_input`, `edit_port` (each rewriting every instance's `args` in the same
  transaction; `AddFacet` now replaces a dynamic facet whole for this), `wrap_in_empty`,
  `connect`. Tests: `tests/authoring.rs`, `tests/anchors.rs` (joints, gender).
- **Canvas, 2026-10-18.** Mains draw solid violet with a `◆◆ name` chip, instances
  lighter violet with `◇ name`; parts inside keep the blue. Make component works on
  shapes and selects what it made; rectangles and ellipses drawn on a main join its
  definition. The shape menu has Port (a click on a main) and Empty; Cmd+Alt+G and the
  inspector's Wrap in empty wrap the selection. The inspector's Main section (name,
  ports, + input) and Port section (name, kind, direction, loose end, connector,
  gender, value or default) go through one engine call, `document_author`. Selected
  and hot ports show a facing arrow.
- **Lines, 2026-10-18.** A selected component's settings are pins on its box (inputs
  left, outputs right; a main's placed ports at their anchors). A line from an
  instance's pin to another's connects (`author::connect`); from a main's pin onto one
  of its parts it raises `LinkDrop`, and the shell's link popover writes
  `scale(#port.port.signal, a, b, c, d)` on the chosen property through the
  inspector's bind. Existing connections and links are drawn while an end is
  selected. A generic engine event (`send_event_from_worker`) carries `LinkDrop`.
- **Joint handle, 2026-10-18.** Hovering connected ports shows the pill along the line
  between their parts' centres; a half is that part's plug drag, the centre moves a
  rigid part (what is plugged in follows) or a joint's held end. It replaces the
  topmost-wins rule for ports on one spot.
- **Library, 2026-10-18.** `iron_document::standard`: slider (settings `level`, `min`,
  `max`, `unit`; output `value` = level × unit), readout, laser, fiber, dial meter.
  `Store::place_standard` copies a definition in on first use (`import.source =
  std:<key>`) and places a linked instance, one history entry. The Library dock tab
  shows the session composition `library` the bridge builds at startup (baked
  previews, never saved). A drag out of it places the entry at the drop point. A press
  on a slider's knob inside an instance scrubs it, writing through the slider's
  binding to the instance's `level`. A plain-number input (the readout's) takes any
  unit; an input with a unit takes only that unit.
- **Not built:** pin hover cursor; links from an instance's pin to a main's part are
  refused by design (a link lives in a definition); the Library is not searchable; the
  standard definitions are not editable (no main); 3D.

