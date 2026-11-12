# Ports in 3D: frames, contacts and conductors

**Status:** Proposed 2026-11-12, not built; waiting on the user. Depends on
[`component-3d.md`](component-3d.md) (a `use` placed in 3D). Extends
[`component-model.md`](component-model.md) §4.2–4.3 (anchors, plugging, mates) and
vision 09 §6 (ports). The motivating object is a Samtec ERM8-EM → ganged SMP (GRF7)
breakout board, photo-documented, with no design files (§8).

## 1. Why ports first

A port is where something an explanation cares about crosses an object's boundary.
For a breakout board that is everything: where the ERM8 mates, where its pins land on
pads, which pad a trace leaves from, which SMP jack it reaches. If those links are
ports and connections in the document, then:

- **placement** follows from them: plugging a connector onto a footprint, or a cable
  onto a jack, locks position and rotation (a mate);
- **signals** follow from them: a pulse put on S5 travels the same chain of
  connections in every picture;
- **pictures** follow from them: the 3D body, the 2D layout and the cartoon all read
  one net, so they cannot disagree about which pin goes where.

Ports today are 2D and carry one value. This spec makes them 3D frames that carry many
typed contacts, and adds conductors so signals have a path to travel between ports.

## 2. What exists (verified 2026-11-12 against `2cce73a`)

- **`port` facet** (`facet.rs`): `kind` (one of `port::KINDS`, a static list:
  `value`, `optical_power`, `rf_voltage` one-way; `electrical`, `heat_flow`
  across/through), `dir`, `signal`, `across`, `through`, `connector` and `gender`
  (plug matching), `datatype`, `show`, `group` (UI folding only).
- **Connections** join exactly two ports of one kind (`port::validate_connection`).
  One-way signals become derived bindings; across/through ports form nets (union-find)
  whose equations the solver holds (`port::network`), DC only.
- **`anchor` facet**: an `empty` marks a port on a picture; the graph derives `wx`,
  `wy`, `wrot` (`anchor.rs`). 2D only.
- **Mates** (`plug.rs`): a connection between two rigid anchors implies a *fastened*
  mate: 3 equations in 2D, never stored. Plugging is a drag gesture with a snap.
- **Time**: `trace` nodes record change events and publish `out` delayed and `dash`
  (where a pulse is along a path); `std:fiber` delays, `std:laser` pulses
  (reactivity.md phase 6). Presentation time, not seekable.
- **3D**: `transform3d`, `mesh` with four primitives, AABB picking, the gizmo, 2D
  compositions embedded as planes in 3D (`planes.rs`). No 3D anchors, no 3D `use`, no
  mesh import.

## 3. The model

### 3.1 A port is a frame with contacts

A port has two parts: **where it is** (a frame) and **what crosses it** (contacts).

- **Interface port.** A connector face: the ERM8's mating face, its footprint, one SMP
  jack. It has a kind (`erm8-020`, `erm8-020-foot`, `smp`), a gender, a mate type, and
  child ports, its **contacts**.
- **Contact.** One conductor crossing the interface: ERM8 pin 7, an SMP centre pin, an
  SMP shell. A contact is an ordinary port today's way (`electrical` across/through),
  with a `number` and a name. A contact may itself be any kind, so an interface can
  carry mixed signals (a fiber-coupled photodiode: optical in, electrical out, thermal).

So "a port carries arbitrary numbers and types of signals" means a port is a small tree:
interface → contacts → each contact's variables. A plain single-signal port is an
interface with no children, as now.

### 3.2 Interface kinds are data

`port::KINDS` stays the list of variable kinds (electrical, optical power…). Interface
kinds become **data** in a library, as `std_parts.xml` holds parts:

```xml
<interface-kind name="erm8-020" mates="erf8-020" mate="fastened" gender="male">
  <contact n="1" kind="electrical"/> … <contact n="20" kind="electrical"/>
</interface-kind>
<interface-kind name="smp" mates="smp" mate="revolute">
  <contact n="sig" kind="electrical"/> <contact n="gnd" kind="electrical"/>
</interface-kind>
```

- `mates` names the partner kind; contacts pair by number unless the kind gives a map
  (ERM8 pin n ↔ ERF8 pin n is identity; a crossover cable is not).
- `mate` is the mate type (§3.4). SMP is **revolute**: it seats along its axis and
  stays free to spin. ERM8/ERF8 is **fastened**.
- A **footprint** is an interface kind too (`erm8-020-foot`): its contacts are pads,
  and its partner is a footprint site on a board. Placing a part on a board is
  plugging its `foot` into a site.

### 3.3 One connection, derived contact connections

The user authors one connection between two interfaces. The contact connections are
**derived** (as in-port signals are today), never stored: plug an ERM8 into an ERF8
and twenty electrical connections exist. Nets (`port::network`) are built over the
derived contact connections, so a net can run SMP shell → board ground → ERM8 ground
pins without anyone drawing it.

A component's own wiring is ordinary connections between its parts' contacts, inside
the definition: the ERM8 part connects `mate.7` ↔ `foot.7` (the pin's body); the board
connects site pads by conductors (§3.5).

### 3.4 Frames and mates in 3D

- **The 3D frame.** An anchor in a 3D picture derives `wx wy wz` and an orientation.
  Convention, as CAD mate connectors: **+z points out of the port** (the mating
  direction), **+x is the key** (towards contact 1). Multi-contact connectors need the
  key: rotating an ERM8 180° about its axis is a different (wrong) mating.
- **Storage.** Orientation is derived, so only the anchor's own `transform3d` is
  authored. Open question 1: whether derived world rotation is a quaternion leaf or
  three basis-vector leaves (bindings read one component at a time).
- **Mates in 3D** are the same derived constraint as in 2D, now 6 equations for
  fastened (frames coincide, +z opposed, +x aligned), 5 for revolute (the spin about z
  is free), 4 for slider. A drag of a mated assembly moves it as one body, as in 2D.
- **Snapping.** Dragging a component near a compatible free port previews the pose
  that satisfies the mate (the rigid motion taking one frame onto the other) and
  releasing connects. This is `plug.rs`'s gesture with a 3D pose; hit-testing a port
  in 3D is a screen-space radius around the projected frame origin.

### 3.5 Lifted anchors: 2D ports on 3D surfaces

A board's pads are 2D facts: positions and rotations on a face. They are drawn once,
in a **layout composition** (2D, mm), which the 3D board shows as a draped plane
(`planes.rs`). An anchor in an embedded 2D composition is **lifted**: its 3D frame is
the embed plane's transform applied to its 2D position and facing, with +z the plane's
normal. So a footprint site drawn in 2D is a 3D port with no second authoring, and a
part plugged onto it in 3D lands on the pads. A board's bottom face is a second
embed with a flipped normal.

This is the same fact viewed twice, as vision 09 wants: the layout is a picture of
the board, the 3D is another, and the site is one port on one entity.

### 3.6 Conductors: where signals travel between ports

A **conductor** is a small definition with two (or more) contacts and a path: a PCB
trace, a pin through a connector body, a coax run, a fiber. It is how a signal gets a
*where* between two ports.

- **Geometry.** In 2D it is a path (a trace in the layout, traced over a photo). In 3D
  it is a polyline between two anchors (a pin from mating face to pad, straight by
  default). A conductor on a draped layout lifts like an anchor.
- **Behaviour.** Electrically ideal at DC (its contacts share one net). Over time, a
  delay: `τ = length / v`, `v` from the kind (≈ 150 mm/ns on FR4 microstrip, an
  input). Not an EM model; enough that a pulse takes a visible, proportionate path.
- **Visualising.** A conductor publishes where a pulse is along it (0–1, as `trace`'s
  `dash` does today), so a 2D trace, a 3D pin glow and a cartoon line all draw the
  same pulse.

Conductors are what link "pins in one 3D object to traces in the PCB": the ERM8's
internal pin conductors meet the board's trace conductors at pad contacts, by
connection, and the chain is in the document.

## 4. Signals across time

The breakout carries time-varying electrical signals. The aim is a faithful picture
of where a pulse is, which pin it uses, and which way current flows on signal and
return. It is not a field solve.

- **What a contact carries over time**: events (edges and pulses with an emission time
  and amplitude), generalising `trace`'s change events. Analogue waveforms sampled at
  presentation rate come later (notes/time-domain-signals.md §1).
- **Propagation**: an event entering a net at one contact reaches each other contact
  after the conductor delays along the way (a shortest-delay walk of the net's
  conductor graph). Branches split; unterminated ends may reflect (open question 4).
- **Current and return.** For a coax or a signal pin over ground, the event carries
  current out on the signal contact and back on the return (the shell, the ground
  pins), so a picture can show both.
- **Time scale.** Nanoseconds are invisible. A composition (or lesson step) sets a
  presentation rate (e.g. 1 ns per second), vision 05's simulation time slowed. Where
  the rate lives is open question 5.

## 5. Phases and exits

Each phase ends with a run-iron script and `iron_document` tests.

1. **P1 — 3D frames and mates.** 3D `use` (component-3d.md 3D-1, 3D-2); `anchor` in
   3D derives position and orientation; fastened and revolute mates in 3D through the
   solver; the plug gesture with a 3D snap. *Exit:* two cube-bodied components with
   ports snap face to face in the Assembly tab; dragging either moves both; a revolute
   mate spins; undo removes the connection and the mate.
2. **P2 — Interface ports and contacts.** Nested ports; `interface-kind` library data;
   derived contact connections; nets across interfaces; the netlist shows them.
   *Exit:* a stand-in ERM8 (a box with a 20-contact `mate` and `foot`) plugged into an
   ERF8 stand-in gives 20 nets; a keyed mate refuses the 180° pose.
3. **P3 — Lifted anchors and the board.** Anchors in an embedded 2D composition derive
   3D frames; a board component: an outline slab with top and bottom layout planes and
   footprint sites. *Exit:* a part plugged onto a site in 3D sits on the pads drawn in
   2D; moving the site in the layout moves the part in 3D.
4. **P4 — Conductors and pulses.** The conductor definition (2D path, 3D polyline,
   delay); event propagation over nets; the presentation rate. *Exit:* a pulse put on
   S5 is seen crossing the SMP pin, the trace and the ERM8 pin, in the 3D view, the
   layout and the cartoon at once, and arriving at the pin the pin map says.
5. **P5 — The real breakout.** Vendor STEP → glTF meshes (a separate asset task, can
   run in parallel from P1); the board from photos (tier 1 of §8); traces traced over
   them (tier 2); the cartoon derived from the net map.

## 6. What stays out

- EM accuracy: impedance, crosstalk, loss, dispersion. A conductor may later carry an
  impedance for a reflection picture; never a field solver.
- Mate features detected from CAD geometry (vision 09 §6, "two tiers"): later.
  Interface frames on imported bodies are placed by hand (or by an agent from the
  datasheet) in P5.
- Board manufacturing data (stackup, drill files, DRC).

## 7. Open questions

1. Derived world orientation: quaternion leaf, basis vectors, or Euler?
2. Contacts matched by number only, or by name with a declared map per kind pair?
3. Is an interface kind a definition (with pictures of its own: the ERM8 face drawn in
   2D) or plain library data?
4. Reflections at open ends: show them (honest, and teaches termination) or not?
5. Where the presentation rate lives: the composition, the lesson step, or the clock.
6. Differential pairs and ground returns: are returns always explicit contacts, or can
   a kind declare an implicit reference (a ground plane)?

## 8. The breakout board, as the first test object

"Import what you have": no design files, so the board is built in tiers, each adding
to the same component.

| Tier | Source | Gives |
|---|---|---|
| 0 | Pin map from the silkscreen and a continuity check | Nets; the cartoon and schematic, complete |
| 1 | Vendor STEP (ERM8-EM, GRF7), calipers, the two photos | 3D bodies; the board slab; photos straightened and scaled (0.8 mm pitch) as the top and bottom layouts |
| 2 | Traces traced over the photos | Conductors with real lengths; pulses along real routes |

Parts and ports:

| Part | Interface ports |
|---|---|
| ERM8-EM (20 pos.) | `mate` (`erm8-020`, fastened, keyed), `foot` (`erm8-020-foot`, pads on both faces: edge mount) |
| GRF7 8-way SMP (×2) | `jack1…8` (`smp`, revolute), `foot` |
| Board 75-0816 | sites `J1` (top and bottom edge), `J2`, `J3`; trace conductors between site pads |
| Breakout (parent) | exposes `J1.mate` and `S1…S16` |
