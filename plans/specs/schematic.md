# A schematic notation: electrical circuits as data

**Status:** Phases S1–S5 built 2026-10-27 (§7 has what was built and where it differs
from this text); "later" items open. Step 8 of [`contexts.md`](contexts.md) §5 (its
domain 13, "a first full notation"), with the pieces it needs from domains 6 (snapping),
7 (palette), 10 (domain libraries) and 11 (parts as data).
**Direction (user, 2026-10-27):** "proceed with the schematic notation. It would be good to
see the data system fully supporting the basic tools needed for an electrical schematic
generation. I assume this would rely on ports and nodes as well right? Where ports in this
context gain a specific meaning — electrical connection."
**Builds on:** the entity–composition model ([`entity-composition-model.md`](entity-composition-model.md)
§6: ports, connections, across/through pairs solved as equations, the thermal demo),
components ([`component-model.md`](component-model.md): definitions, templates per notation,
linked instances, anchors), contexts ([`contexts.md`](contexts.md): notations as bundles of
rules, the view focus, palettes and keymaps), and §6 of contexts.md (the demo of the whole).

---

## 0. Decisions at a glance

| Question | Decision | Why |
|---|---|---|
| What an electrical connection is | A **connection** between two `electrical` ports, the port kind that already exists (an across/through pair: volts, amps). Every port joined by connections is one **net**: equal voltage, currents summing to zero | It is the model the thermal network already solves; a net is the solver's union-find |
| What a part is | A **definition** with `electrical` pins (port_specs), inputs (`R`, `C`, `V`), **behaviour as equations** over its pins (`a.across − b.across = R · a.through`), and a **schematic template**: its symbol | Parts are ordinary components: linked instances, overrides, Make component, all work |
| Where the standard parts live | **Markup**, `std_parts.xml`, embedded and never saved, parsed into definition ops; copied into a document on first placement (`import.source = std:<key>`), as the Library tab's components are | Parts as data (domain 11): a reader sees a resistor's whole meaning in a dozen lines; a model can write a new part in the same form |
| What a wire is | A **figure of a connection**: a path in the schematic whose end points are **bound** to the pins' derived positions (`anchor.wx`, `anchor.wy`), its bends free points | Moving a part drags its wires by the graph, with nothing to keep in sync; the connection (the meaning) and the wire (the picture) stay distinct, as everywhere in Iron |
| T-junctions | A **junction** part: one pin, a dot; a wire ending on a wire splits it at a new junction | A net stays a set of ports; no wire-to-wire connections to special-case |
| What the notation changes | A built-in context `std:schematic`: its palette (select, wire, the parts, text), keymap, a **grid** (`snap.grid`), and selection rules | Contexts step 2's machinery; nothing in the shell names a part |
| How a part is placed | A palette entry **`place:<key>`**: a placement tool the engine owns, named with what it places. Click places on the grid; Space rotates 90° first | Altium's placement; the palette entry is data (which part) over one mechanism |
| Live values | The existing network solve gives each net its DC voltage and each pin its current; a **probe** part shows a net's voltage, bound to its pin | The stretch goal of contexts.md §6 falls out of ECM §6.1 |
| Generation by a model | Model verbs **`<place>`** and **`<wire>`**, computed by the store, and a **netlist** read (SPICE-like) | "Schematic generation": Claude writes a circuit compactly and reads it back as a netlist |

---

## 1. Ports become electrical terminals

Nothing new is needed in the port model, which is the point:

- `port.kind = "electrical"` is declared in `port.rs`'s `KINDS` as a `Pair` with across
  unit `V` and through unit `A`. A connection may join only ports of one kind
  (`validate_connection`), so a wire cannot join a pin to an optical port.
- A pair port may carry any number of connections (only `Signal` inputs are limited to
  one feed), so several wires on one pin are legal and mean one net.
- `port::network` collects every pair port, unions them by connections into nets, writes
  "across equal, through sums to zero" per net, and adds every entity's `equation`
  nodes. `Store::solve_network` solves it and publishes the values under the network
  producer. A resistor is two equations; a ground one (`p.across = 0 V`).
- **A pin is a port plus its place**: the port_spec (the meaning) and, in the part's
  schematic template, an `empty` with an `anchor` facet (where it sits, which way it
  faces). The graph derives each anchor's position in its composition
  (`anchor.wx`/`wy`/`wrot`), so a wire binds to it.

**Change needed:** reading an anchor's derived position from outside an instance is
refused today (`expand::check_scope`: outside readers may read an instance's ports only).
A pin's position is as public as the pin itself, so the rule admits
`anchor.wx|wy|wrot` of an anchor empty, and nothing else of the picture.

## 2. Parts as markup

```xml
<part key="resistor" name="Resistor" prefix="R">
  <in name="R" value="1 kΩ"/>
  <pin name="a" x="-30" y="0" face="left"/>
  <pin name="b" x="30" y="0" face="right"/>
  <equation lhs="a.through + b.through" rhs="0 A"/>
  <equation lhs="a.across - b.across" rhs="R * a.through"/>
  <symbol>
    <path d="M-30 0 H-18 L-15 -6 L-9 6 L-3 -6 L3 6 L9 -6 L15 6 L18 0 H30"/>
    <label y="-12" text="{designator}"/>
    <label y="20" text="{fmt(R, &quot;Ω&quot;)}"/>
  </symbol>
</part>
```

- `<in>`: an input port (`value`, `in`), the instance's argument (`args.R`), with a
  default; `designator` is always an input, set at placement (`R1`, `R2`, … from
  `prefix`).
- `<pin>`: an `electrical` port_spec, and in the symbol an anchor empty at `x`,`y`
  facing `face`.
- `<equation>`: behaviour, names resolved to the part's own ports (`a.across` →
  `#pin.port.across`, `R` → `#in.port.signal`).
- `<symbol>`: the schematic template, drawn in a 10 px grid with pins on grid points:
  `path` (SVG path data, stroked), `circle`, `rect`, `label` (text, `{…}` an expression
  over inputs), and a filled `dot`.

The parser lives in `iron_document::parts` beside `contexts` and turns a part into
the same ops `standard.rs` builds by hand: a definition, port_specs, equations, a
template for notation `schematic`. A part's behaviour is then readable, overridable
(copied into the document, editable through its main) and writable by a model.

**First set:** resistor, capacitor (DC: open), inductor (DC: short), voltage source,
current source, ground, junction, probe. Later: diode (an exponential equation; the
solver is nonlinear already), switch (an `activate: toggle` over its own input),
op-amp (ideal: three equations), net label (§5).

## 3. The schematic notation (`std:schematic`)

```xml
<context name="std:schematic">
  <rule match="type:use|text|path|group|empty" affords.select="target" affords.drag="point" snap.target="yes"/>
  <rule match="type:use|group" chrome.lit="frame"/>
  <rule match="type:text" affords.edit="text"/>
  <rule match="view" palette.tools="select wire place:resistor|place:capacitor|place:inductor place:vsource|place:isource place:ground place:probe text"
        keys.tools="v:select w:wire r:place:resistor c:place:capacitor l:place:inductor e:place:vsource i:place:isource g:place:ground p:place:probe t:text"
        snap.grid="10" inspector.sections="main component position text other"/>
</context>
```

- Built-ins do not chain, so it states its own selection rules.
- **`snap.grid`** (a new view field, px; `0` off): placement, the wire tool's clicks and
  a dragged selection's origin round to it. Figma-style object snapping stays for
  free notations.
- **`place:<key>`** joins `palette.tools`' and `keys.tools`' vocabulary: `place:` and a
  key in the parts library. The shell labels and draws it from the part (its name; an
  icon by key until a part's own symbol can be its icon).
- No motion rules: nothing glides in a schematic.

## 4. The tools

**Placement** (`place:<key>`): a ghost of the part's symbol follows the pointer, snapped
to the grid; Space rotates it 90°; a click places a linked instance (one transaction:
the definition copied on first use, the entity, the `use`, its designator), and the tool
stays armed for the next; Esc or V returns to select.

**Wire** (`wire`): Altium's. A click on a pin (or junction) starts a wire; each click on
empty canvas adds a corner, the segment drawn orthogonally (horizontal first; Space
flips to vertical first); a click on another pin finishes: one transaction "wire" with
the connection (in the model), the path (in the schematic) whose first and last points
bind to the two pins' anchors, and the `figures` relation. A click on a wire's segment
finishes there: the wire is split at a new junction (its connection replaced by two),
and the new wire connects to the junction. Esc drops the wire in progress. Pins under
the pointer light while the tool is armed.

**Select** keeps working: a part drags on the grid and its wires follow (their end points
are bound); a wire's bend points edit as a path's points.

## 5. Phases

| Phase | Builds | Shown by |
|---|---|---|
| **S1** | `check_scope` admits anchor positions; `iron_document::parts` and `std_parts.xml` (resistor, capacitor, inductor, vsource, isource, ground, junction, probe); `Store::place_part`; tests that a divider solves | `tests/schematic.rs` |
| **S2** | `std:schematic`, `snap.grid`, `place:<key>` in the tool vocabulary; the placement tool (ghost, grid, Space); the shell's labels for part tools | `schematic.txt` |
| **S3** | The wire tool: pin to pin with orthogonal corners, ends bound to anchors; split at a junction | `schematic.txt` |
| **S4** | The Circuit showcase tab: a divider and an RC with probes showing live volts; a slider on a resistor | the app |
| **S5** | Model verbs `<place>` and `<wire>`, the netlist read, net labels joining nets by name | `schematic.txt`, model surface |
| Later | Rubber-banding orthogonal legs when a part moves; ERC (unconnected pins as gentle warnings: "conventions and checks"); a grid drawn in the view; pins as chrome nodes (chrome.md §§2–7); transient analysis (capacitors charging over time, ECM phase 5) | — |

## 6. Open questions

1. **DC only for now.** Capacitors are open and inductors short in a DC operating
   point; a transient solve (time steps, `C · dV/dt`) belongs with ECM phase 5's
   time-domain signals.
2. **Wire ownership.** Deleting a wire should delete its connection, and deleting a part
   its wires. Today deleting a figure leaves its entity; the schematic wants a rule for
   connections' figures (a `figures` relation that owns). To decide in S3.
3. **Where entities live.** Placement puts entities in the document's `model` root, as
   the Library tab does; a sheet-scoped model (one per schematic) may read better.

---

## 7. Built (2026-10-27)

**Store** (`iron_document`): `parts.rs` reads `std_parts.xml` (resistor, capacitor,
inductor, vsource, isource, ground, junction, probe) into definitions: inputs and a
`designator` input, `electrical` pins with anchors, equations with the part's names
resolved to its ports (a word after a number is a unit), and a `schematic` template
from SVG path data (`M L H V C Z`), circles, dots and labels. `Store::place_part` /
`place_ops` copy a part's definition in on first use and place a linked instance
named by its next designator; a turned part's labels get overrides that keep them
upright, at the part's `turned` offsets (§2's markup gained `turned="x,y"` and
`turn="with"`). `wire_ops` makes a connection and its figure, a path whose ends bind
to the pins' `anchor.wx`/`wy` and whose corners next to an end share that end's
coordinate along the leg (rubber-banding); `wire_between` takes `Terminal`s, a pin or
a point on a wire, splitting the wire at a new junction. `delete_subject_ops`
deletes what a figure figures: an entity with every figure and the connections on its
ports and their wires, or a connection with its figures. `circuit_ops` is the model's
`<circuit>`; `netlist` its read. `expand::check_scope` admits an anchor's derived
position from outside an instance. Units print `Ω` (was `Ohm`).

**Contexts:** `std:schematic` (notations now find `std:<notation>`), and new fields:
`plug.mate` (node: whether a connection between rigid anchors holds the parts
together; `no` in a schematic, read by `plug::implied_mates`), `snap.grid` (view),
`affords.delete` (node: `self` or `subject`), `chrome.component` (view: the violet
frame, chip and input pins of instances; `off` in a schematic). `palette.tools` and
`keys.tools` accept `wire` and `place:<part>`.

**Solver:** the network solve no longer adds least-change rows: they stalled a
kΩ–mA divider short of the tolerance (a network is a square system, not a drag).

**App:** `ActiveTool::Wire` and `Place(key)`; `bevy_app/schematic_tool.rs` (the ghost,
Space to turn or flip the elbow, grid rounding, hot pins and wires, the wire in
progress drawn by the overlay); moves round the lead mover's origin to the grid
(`SnapSession::grid`); Delete reads `affords.delete`; the toolbar labels and draws
the part tools (Lucide icons by tool id). Showcase: the **Circuit** tab (a divider
with a probe, R2 bound to a slider). The model view carries each schematic sheet's
netlist, and the manifest a SCHEMATICS section.

**Shown by** `tests/schematic.rs` (8) and run-iron `schematic.txt`.

**Rough edges:** labels are upright only as placed (a later turn of a placed part
keeps the overrides of its first); the model's `<delete>` removes only the node it
names (the subject rule is the Delete key's); a wire's ends are bound, so dragging a
wire's end point drags the part; DC only.
