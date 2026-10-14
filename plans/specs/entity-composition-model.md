# The entity and composition model

**Status:** Proposed — implementation plan
**Date:** 2026-10-09
**Scope:** The changes to `iron_document`, the Bevy bridge, and the Svelte shell needed
to realise [`../vision/09_entities-compositions-and-ports.md`](../vision/09_entities-compositions-and-ports.md):
units and structured values, run-time definitions, entities with ports, connections that
carry signals, several compositions shown in separate panels, cross-representation
linking, obligations, encapsulation, and the lesson layer.
**Builds on:** [`document-spine.md`](document-spine.md). Sequenced in [`roadmap.md`](../roadmap.md).
**Works alongside:** [`constraint-system.md`](constraint-system.md). Constraints are
composition-local, and port kinds may imply them.

---

## 0. Decisions at a glance

| Question | Decision |
|---|---|
| What comes first | Value types and run-time schemas. Every later piece stores them, and the file format is cheapest to change now (doc 06 §5) |
| Units | `Value::Quantity { si, dim }`: an SI magnitude plus a dimension vector. The display unit is a presentation choice, not stored in the value |
| Structured values | `Value::Record` for small authored structures. Large data (time series, spectra) is runtime-only `Signal`, never written to the file |
| Domain variety | Lives in **definitions** (data), not the Rust registry. The registry keeps primitives only. `bar` becomes a definition |
| Field names for definition parameters | Interned symbols, so `LeafPath` stays `Copy` with fields not known at compile time |
| Entities, ports, connections | All nodes. Connections are nodes so that a wire or a cable can depict them, and so they have ids, undo, and views |
| Compositions | Root-level `composition` nodes with a notation. Their subtree is layout: figures, local geometry, annotations |
| Figure | A role, not a type. Any visual node with a `figures` relation to an entity, port, or connection |
| Signals | Derived values on ports, evaluated by the reactive graph as a third node kind (after bindings and constraint clusters) |
| Panels | Each Bevy panel shows one composition: its own camera viewport, render layer, picking, and 2D clip |
| Selection | Keyed by `NodeId` and lifted to entities. Highlighting an entity lights every figure of it in every panel |
| Obligations | Computed, not stored. Only "intentionally omitted" is stored |
| Annotations | Live in the composition they are drawn in (spatial home). A `step` relation names the owner, `targets` names the subject |

---

## 1. Where the code is today

Facts this plan changes, from branch `document-spine`:

- **One tree, one implicit scene.** The demo scene is a single root `group` holding 20
  rects, a circle, and the torus. Everything is drawn into the one `viewer` panel.
- **A static registry.** `TypeId(u16)` indexes a compile-time `TYPES` table (`group`,
  `rect`, `circle`, `mesh`, `slider`, `bar`, `timeline`, `clip`). Adding a kind of thing
  means adding Rust.
- **Static leaf paths.** `LeafPath { component: ComponentKind, field: &'static str }` is
  `Copy` and validated against each component's compile-time `FIELDS`. There is no way to
  address a field defined at run time.
- **Values** are `Number`, `Bool`, `Str`, `Color`, `Vec2`, `Vec3`, `Enum`, `Ref`. There
  are no units and no structured values, and expressions cannot read fields (doc 02's
  `Get` is deliberately absent). The spine plan's §15 says `Quantity` is "reserved", but
  `value.rs` chose to leave it out entirely.
- **Relations** have kinds `animates`, `after`, `during`, `aligns_to`, `references`.
- **Rendering.** One `Camera3d` with its viewport set to the `viewer` panel, and one
  full-window Vello 2D layer drawn with the viewer panel's affine. All document entities
  use `RenderLayers::layer(0)`. `materialize.rs` switches on type names.
- **Panels.** `set_panel_viewport(ptr, id, kind, …)` already supports several DOM-laid-out
  panels (`viewer`, `timeline`). This is the hook for one panel per composition.
- **Selection and hover** are keyed by Bevy `Entity`.
- **Views.** Only a tree view. The spine plan's `ViewQuery` lists `Tree`, `Timeline`,
  `Spatial`, and `Semantic` relations, but only `Tree` exists.
- **Spine steps 6 and 7 are done in the working tree** (uncommitted on 2026-10-10).
  Step 6: `time.rs` samples clips through `animates` relations into an animated layer
  of the graph. The read order is now overlay → binding → animated → constant, and the
  playhead is runtime state in the store. Step 7: `src-ui/model/` holds the manifest
  (from `document_schema()`), an `<edit>` parser with `create`, `set`, `add`, `delete`,
  `reparent`, `link` and `unlink` verbs, and an agent loop with one retry. It applies
  through a synchronous `document_apply` with `expect_version`, and views show
  `<driven-by>`. The model views of §11 and the new verbs extend this surface; they do
  not replace it.

---

## 2. Why each part must change

| Usage intent (vision 09 §1) | What blocks it today | Change |
|---|---|---|
| Laser power 9.58 mW flowing through a fiber | No units; a float cannot say mW, and mW plus nm is not an error | `Value::Quantity` (§3) |
| A Gaussian beam as a signal | No structured values; no field access | `Value::Record`, `Expr::Get` (§3) |
| An RF drive waveform | No time series anywhere | Runtime `Signal::Series` (§6) |
| LLM- and user-defined components such as laser, fiber, modulator | Types and fields are compile-time only | Definitions as data; interned field symbols (§4) |
| One modulator in four diagrams | One tree, no identity separate from the picture | Entities and `figures` (§5) |
| Schematic and 3D side by side | One viewer panel; everything on render layer 0 | One panel per composition (§7) |
| Cross-probing | Selection keyed by Bevy `Entity` | Selection by `NodeId`, lifted to entities (§7) |
| "This bolt needs placing" | No idea of what a composition should contain | Obligations (§8) |
| Precompose with an interface | No encapsulation operation | `Store::encapsulate` (§9) |
| Arrows that belong to a lesson | Annotations are just shapes | The lesson layer (§10) |

---

## 3. Values: units and structure

### 3.1 Quantities

```rust
pub enum Value {
    // existing variants …
    /// A physical quantity: magnitude in SI base units plus its dimension.
    Quantity { si: f64, dim: Dim },
    /// A small structured value (a beam, a pulse shape). Field order is canonical.
    Record { kind: RecordKind, fields: BTreeMap<Sym, Value> },
}

/// Exponents of the seven SI base dimensions (m, kg, s, A, K, mol, cd).
pub struct Dim([i8; 7]);
```

- **Storage is SI.** 9.58 mW is stored as `{si: 0.00958, dim: W}`. The display unit
  (mW, dBm, W) is a presentation choice: a property of the parameter's spec or of the
  figure, and an enumerated-choice leaf in doc 06's sense, so a user can flip a
  readout from mW to dBm. Logarithmic units like dBm convert at display time.
- **Arithmetic is dimension-checked** in the floor. `+` and `-` require equal dims;
  `*` and `/` combine them; a mismatch is an evaluation error shown on the leaf.
  Comparing across units is then correct by construction.
- **The file format** writes quantities as `{"q": [0.00958, "W"]}`, with a canonical
  unit string per dimension, so diffs stay readable.
- **Geometry stays plain numbers** in document pixels and radians. Physical size in a
  3D assembly is a separate question (open question I3).

### 3.2 Records

Records are for small authored structures, such as the default beam a laser emits.
`RecordKind` names a record schema (`gaussian_beam`, `pulse`) from the registry or from
a definition. `Expr::Get(expr, field)` reads a field, which adds doc 02 §4's `Get`.

### 3.3 Runtime signals

```rust
pub enum Signal {
    Value(Value),                                   // scalar, quantity, or record
    Series { t0: f64, dt: f64, unit: Dim, samples: Arc<[f64]> },
    Spectrum { f0: f64, df: f64, unit: Dim, samples: Arc<[f64]> },
}
```

Signals live in the reactive graph's derived layer and are never serialised. Sources
are authored as parameters (amplitude, frequency, waveform enum); the samples are
computed. This keeps the authored and derived split of doc 04 §4.1.

---

## 4. Run-time schemas: definitions as data

### 4.1 Interned field names

`LeafPath` gains a field reference that may be a run-time symbol:

```rust
pub enum Field { Static(&'static str), Sym(Sym) }   // Sym(u32): a per-process interner
pub struct LeafPath { pub component: ComponentKind, pub field: Field }  // still Copy
```

Symbols are interned on load and on apply. Files store strings, so ids never leak into
the file. `ComponentKind::Params` is the one component whose fields are symbols,
validated against the entity's definition rather than a compile-time list.

### 4.2 Definitions

A definition is a subtree in a `library` root:

```text
definition "fiber"
  param_spec  length   : Quantity[m]   default 1 m
  param_spec  loss_db_per_km : Number  default 0.2
  port_spec   in       : optical_power, in
  port_spec   out      : optical_power, out
  behaviour   out = in * 10^(-loss_db_per_km * length_km / 10)      (bindings over params and in-ports)
  template    notation "cartoon"   → template subtree
  template    notation "schematic" → template subtree
```

- Built-in definitions ship as data files, so they are the same kind of thing as
  user-authored ones.
- Behaviour is written as bindings in the existing expression language: data, editable
  by the LLM, evaluated by the reactive graph. A definition may instead name a floor
  function or a Model call (doc 02 §6.3) when the behaviour is not a short expression.
- **The registry holds primitives only:** `group`, `rect`, `circle`, `path`, `mesh`,
  `text`, `slider`, `constraint`, and the structural types of §5. **`bar` stops being a
  registry type.** It becomes a demo definition: a `rect` figure whose height is
  bound to a parameter. This is the discipline the earlier review asked for: domain
  vocabulary in data, not in Rust.

### 4.3 Instantiation

Placing a definition creates an `entity` node referencing it, with:

- a `params` component holding one slot per `param_spec`, each bindable;
- one `port` node per `port_spec`, as real children. Ports get stable ids because
  connections and annotations point at them;
- behaviour that is *not* copied. It is evaluated per entity from the definition, so
  editing a definition updates every instance. This is doc 05 §4's "templates mint
  derived edges".

Placing a figure copies the notation's template subtree into the composition as
ordinary authored nodes. Users and the LLM then edit layout freely.

---

## 5. Tree layout and new types

```text
library/            definitions (§4)
model/              entities, nested by refinement; ports under entities; connection nodes
compositions …      one root per composition: figures, local geometry, annotations, constraints
lessons/            lessons → steps
```

New registry types:

| Type | Required components | Children | Notes |
|---|---|---|---|
| `definition` | `name`, `definition` | `param_spec`, `port_spec`, `behaviour`, `template` | in `library` |
| `entity` | `name`, `instance` (definition ref), `params` | `entity`, `port`, `connection` | in `model` |
| `port` | `name`, `port` (kind, direction) | none | under an entity |
| `connection` | `connection` (domain) | none | two `connects` relations to ports |
| `composition` | `name`, `composition` (notation, dims) | any visual node | root level |
| `anchor` | `transform2d` or `transform3d` | none | under a figure; `figures` → a port |
| `lesson`, `step` | `name`; `step` (framing) | ordered steps | under `lessons` |
| `arrow`, `callout`, `label` | geometry, style | none | annotation primitives, inside compositions |

New relation kinds:

| Kind | From → to | Meaning |
|---|---|---|
| `figures` | visual node → entity, port, or connection | this shape is a picture of that |
| `connects` | connection → port (×2) | endpoints, with `data.end` = `a` or `b` |
| `realizes` | entity → entity | detailed realises abstract (refinement across notations) |
| `exposes` | boundary port → inner port | created by encapsulation (§9) |
| `targets` | annotation → entity, port, connection, or node | what the annotation is about |
| `step` | annotation → step | which step owns it (shows it) |
| `highlights` | step → entity or node | step-level emphasis |
| `omits` | composition → entity or connection | intentionally left out; suppresses obligations |

The existing tombstone semantics carry over. Deleting an entity leaves its figures
pointing at a tombstone, shown as broken links. Undoing the delete restores them.

**Migration of existing documents:** existing root groups move under one composition
named `scene` with notation `free`. Nothing else changes.

---

## 6. Connections and signals in the reactive graph

The graph gains a third node kind, after bindings and constraint clusters:

```rust
enum GraphNode { Binding(..), Cluster(..), Signal(SignalNode) }
```

- **An in-port's signal** is its connection partner's out-port signal, passed through
  the port kind's adapter if the types differ.
- **An out-port's signal** is the entity's behaviour evaluated over its parameters and
  in-port signals.
- **Readers** reach a signal through a derived, read-only leaf `port.signal`, using the
  same resolver as every other read (alongside the overlay, binding, and animated
  layers that `read_with` already checks). A meter readout is `text = #meter.in.signal`, bound
  like any other binding.
- **Structure** rebuilds on structural transactions (connect, disconnect, instantiate),
  following the same rule as bindings today.
- **Cycles** through signals are rejected at apply time, as for bindings. A real loop
  (an optical cavity) must be a simulation or a Model.
- **Time-varying sources** (an RF drive) depend on a time input: the store's playhead,
  which `time.rs` already holds for clips, or simulation time (doc 05). Only signal nodes downstream of a time-dependent source
  re-evaluate per tick. Their changes feed the reconciler's affected set like any other
  resolved change, and are never reported to the LLM (`constraint-system.md` §9).

---

## 7. Compositions in the bridge and shell

### 7.1 One panel per composition

Panels here are camera viewports inside the single full-window canvas
(`hybrid-ui-layout.md`, `single-canvas-migration.md`), not extra canvases. The
`viewer` and `timeline` panels already work this way, and "dynamic panel factory" is
listed there as enabled but not built. HTML chrome still overlays the canvas. If
per-panel cost proves too high on WebGL2, the fallback is one live composition panel at
a time, with other compositions shown as cached thumbnails.

- `BevyPanel.svelte` gains a `composition` prop. `set_panel_viewport` carries the
  composition id, and the `Panels` resource maps panel → composition.
- **Render layers.** Each open composition is assigned a layer index. `materialize.rs`
  sets `RenderLayers` from a node's composition ancestor instead of the fixed
  `layer(0)`.
- **3D.** One `Camera3d` per 3D composition panel, with its viewport set to that panel,
  rendering only its layer. The current `MainCamera3D` becomes the first of these.
- **2D.** `render_document_layer` loops over panels: for each, it draws the nodes of that
  panel's composition with that panel's affine, clipped to its rect. That is the same
  code as today, parameterised by panel.
- **Picking** maps the pointer to a panel, then to a composition, and only hits nodes on
  that composition's layer.
- **Cost.** Several cameras and several clipped 2D passes per frame. Measure with two
  and four panels before going further; the render-invalidation work in the roadmap
  applies per panel.

### 7.2 Linked selection and Show in

- `SelectionState` and hover move from Bevy `Entity` to `NodeId`, with a derived
  *entity selection*: selecting a figure selects what it depicts.
- **Cross-probe highlight.** Every figure of a selected or hovered entity, in every
  open panel, draws the highlight stroke. One index lookup per entity:
  `relations_to(entity)` filtered to `figures`.
- **Show in ▸** is an FFI call `focus(composition, node)`. The shell opens or activates
  that composition's panel; Rust frames its camera on the node's bounds.
- **The representations panel** (Svelte) lists, for the selected entity, its
  definition, parameters, ports with live signal values, and every figure with its
  composition.
- **Linking gesture.** Select two figures and choose Link. If neither has an entity,
  one is created from a generic definition (`thing`); if one has an entity, the other
  joins it; if both do, the user picks which survives (merging is one transaction).

### 7.3 Composition-local content

Brackets, posts, screws, mate features, constraints, and decoration are ordinary nodes
in the composition with no `figures` relation. They never appear in the model tree,
never generate ghost lines, and are invisible to other compositions.

### 7.4 Embedded compositions (decided 2026-10-11)

Compositions are content, not panels. As in After Effects, a composition can be pulled
into another one; a panel (a Blender-style editor) can show any composition, but most
building and side-by-side layout happens in the centre viewer, which shows a top-level
`main` composition that embeds the others. Section 7.1's one-panel-per-composition stays
as the editor mechanism; it is no longer how two compositions sit side by side.

- **`embed`** (decided name) is a node in a composition's layout with `embed.source` → the
  composition it shows, a transform for its own dimensionality, and an optional manual
  bounding box. It is a live reference: one composition, shown wherever it is embedded,
  edited once. A cycle (A embeds B embeds A) is refused at apply time.
- **2D in 2D, 3D in 3D:** the embed is a placed, clipped group *by reference*. Use a group
  when nothing needs sharing or a boundary.
- **2D in 3D:** a textured plane. Its extent is the bounding box of the inner content, or
  the manual box when one is set (larger, usually). Needs the 2D layer rendered to a
  texture.
- **3D in 2D:** a camera view. `embed.camera` optionally names a `camera` node in the 3D
  composition (fov, aspect, near, far), and the camera's aspect sets the embed's shape.
  Without one, the view is a derived default that frames all of the composition's
  content from a fixed direction; it is computed, never stored. Several embeds may name
  different cameras of one composition (front and side views).
- **Click-through (decided):** picking descends through embeds, so hover, selection,
  Link and drags reach the torus while looking at `main`. Selecting the embed itself
  (to move or resize it) takes a modifier or its frame. A pick is a path (embed chain
  plus node), because one composition can be embedded twice; highlights stay per node,
  so every appearance lights.
- **Show in** stays presentation: it pans and zooms the viewing panel to the embed
  that shows the counterpart. It never moves an authored camera.
- **Default layout (decided):** one centre viewer showing `main`, which embeds `scene`
  (2D) and `assembly` (a camera view) side by side; the right sidebar returns to a
  placeholder; `?panels=` still lets a panel show any composition.

Build order: (1) the `embed` and `camera` types, cycle check, views; (2) 2D in 2D:
recursive draw and pick with transform and clip; (3) 3D in 2D as a camera viewport at
the embed's on-screen rectangle (axis-aligned, drawn beneath the 2D layer: a stated
limitation until render-to-texture), with the derived default view; (4) the `main`
demo and smoke (link the box to the torus inside the viewer); (5) render-to-texture,
then 2D in 3D as a plane.

**Progress (2026-10-11):**

- *Types* — done: `embed` (`embed.source`, optional `embed_camera.camera`, optional
  `frame { x0 y0 x1 y1 }` — corner names, so a bare `x` on an embed still means its
  position; placement components are listed first for the same reason) and `camera`
  (`transform3d`, `camera { fov aspect near far }`). Loops are refused on create, set and
  reparent (`composition::validate_embed`). Deviation: the field is `source`, not `of`,
  because the composition view already uses `of=` for "this figure pictures entity".
  Tests: `tests/embeds.rs`.
- *2D in 2D* — done: the 2D layer and picker walk a panel's composition and recurse into
  2D embeds (`document_bridge::nest`), placed by the embed's transform, clipped to its
  frame or content bounds (computed from shapes, so it shows on the first frame). A hit
  carries the affine into its composition's space, so drags work through rotated and
  scaled embeds (checked: a 0.3 rad, 0.8× embed maps a drag exactly).
- *3D in 2D* — done as a camera viewport at the embed's on-screen bounding box
  (`EmbedViews`, `scene3d::sync_embed_cameras`), beneath the 2D layer; the 2D layer draws
  the embed's frame. A named camera node poses it and its aspect sets the shape;
  otherwise the derived default view frames the composition's meshes and holds still
  until the content leaves the framed sphere (re-framing every frame would chase a drag).
  Picking casts from that camera, and the 3D drag uses it. Camera orders come from a
  free pool between -9 and 9 (orders only grew before, and the vector composite at 10
  must stay on top). Show in locates a node through embeds and pans the panel to it.
- *Main layout* — done: the viewer shows `main`, which embeds `scene` at the origin and
  `assembly` as a camera view; the C1 exit runs there in the smoke script.
- *3D in 2D as a texture* — done 2026-10-11 (`view-textures.md` migration step 3): the
  embed's camera renders the whole embed at its on-screen scale into its own texture,
  and the 2D view draws that texture in painter order under the embed's full transform
  (rotation and scale work; 2D content sits over or under it by order; a clipped embed
  is cropped, no longer reframed). Picking treats the 3D view as opaque and casts
  through the embed's inverse transform.
- *2D in 3D as a plane* — done 2026-10-11 (`view-textures.md` step 4): rendering,
  derived texture density, picking and drags through the plane.
- *Not done:* selecting an embed itself
  (a modifier or its frame); an "open/enter" composition gesture; embeds inside 3D
  compositions.

**Open:** whether definitions converge with embeds. An embed is a parameterless,
live component; a definition instance is a parameterised copy that may diverge after
placement. A template could instead *be* a composition that figures embed with the
entity's params bound in, trading per-instance layout edits for central ones. Kept
separate until paths and ports show which figures want.

---

## 8. Obligations, ghost lines, and the tray

`Store::obligations(composition) -> Vec<Obligation>` is a pure function over the model,
the composition, and its notation:

- **Missing entity**: an entity connected to something depicted in C, whose definition
  has a figure for C's notation, but which is not depicted in C. Shown in the
  composition's tray, ready to place.
- **Unrealised connection**: both endpoint entities are depicted in C, C's notation draws
  the connection's domain, and no node in C depicts the connection. Shown as a ghost
  line between the port anchors, and listed.
- **Derivable facts are not obligations.** A notation that draws a domain implicitly
  (the cartoon draws an optical arrow between any connected optical ports) realises the
  connection automatically. No node is needed, and nothing is listed.

`omits` relations suppress specific obligations. Strictness beyond listing (blocking,
warnings on save) is deferred (vision 09 §13.1). The LLM reads obligations through a
view, and "realise the open items in the table assembly" becomes a task it can do.

---

## 9. Encapsulation

`Store::encapsulate(selection: &[NodeId], name) -> TransactionInput` builds one
transaction:

1. Create a parent `entity` (from a generated definition, editable afterwards) and
   reparent the selected figures' entities under it.
2. For each connection with exactly one endpoint inside the selection, create a
   boundary `port` on the parent with an `exposes` relation to the inner port, and
   re-point the connection's inside end to the boundary port. Connections wholly
   inside are untouched.
3. In the composition where this ran, replace the selected figures with one block
   figure of the parent (the originals move into a new internal composition
   reachable by drill-in).

Signals pass through `exposes` unchanged, so behaviour is identical before and after.
That is the black-boxing law, and it is a test.

Tests for the laws of vision 09 §11:

- **associativity**: encapsulating {A, B}, then {that, C}, gives the same ports and the
  same signals as encapsulating {A, B, C};
- **black-boxing**: every signal outside the selection is bit-identical before and after
  encapsulation;
- **undo**: one undo restores the original structure exactly.

---

## 10. The lesson layer

- `lessons/lesson/step` nodes. A step's `step` component holds framing: which
  compositions are open, a camera bookmark per composition, and an optional playhead
  range for scripted time.
- **Annotations** (`arrow`, `callout`, `label`) live inside the composition they are
  drawn in, so they share its coordinates and can be constrained. An arrow's tip can
  have a `coincident` constraint to its target figure's anchor, which reuses the
  constraint system for layout.
- **Ownership** is the `step` relation; **subject** is `targets`.
- **Visibility is derived**: an annotation is drawn while the playback step is one of
  its owner steps. The current step is editor state, like the playhead, and never stored
  in the document.
- **Carrying** an annotation to another composition creates a copy there with the same
  `targets` and `step`, positioned at the target's figure in that composition.
- **Data-bound annotations** are ordinary bindings (`label.text` bound to a signal,
  formatted with its display unit). **Claims** are text with `targets` plus optional
  doc 04 L6 `explains` and `cites` links.

---

## 11. Views for the language model

`ViewQuery.relation` gains:

- **`Model`**: entities, their definitions, parameters, ports, connections, and signal
  summaries. This is the default for semantic work. Example:

  ```xml
  <entity id="#e4" def="lithium_niobate_modulator" name="EOM" vpi="{q:3.5 V}">
    <port id="#p9" name="rf" kind="rf_voltage" dir="in" from="#e7.out"/>
    <port id="#p10" name="light_in" kind="optical_field" dir="in" from="#e2.out"/>
    <port id="#p11" name="light_out" kind="optical_field" dir="out" to="#e5.in" signal="series[2048] W"/>
    <figures compositions="#c1 #c2 #c3"/>
  </entity>
  ```

- **`Composition(id)`**: that composition's layout tree, with `figures` shown as
  `of="#e4"` attributes, plus its obligations.
- **`Lesson(id)`**: steps, annotations with targets, and framing.

Signals appear as summaries: type, length, and unit, never samples.

The existing model surface (`src-ui/model/`) gains:

- the new view relations, with `agent.ts` choosing `Model` by default instead of the
  full tree;
- edit verbs `<instantiate def=… />`, `<connect a=… b=… />`, `<figure node=… of=… />`,
  `<encapsulate nodes=… />`, and lesson verbs;
- definitions and port kinds in `document_schema()`, so `manifest.ts` describes them;
- quantity attributes (`power="9.58 mW"`) parsed in `edit.ts` by the schema's kind.

### 6.1 Port variables: one-way signals and across/through pairs (decided 2026-10-12)

The conservative-ports question (vision 09 §13.2, roadmap §7 item 5) is decided: **both,
in one general interface.** A port kind declares a bundle of variables, each one of:

- **`out` / `in`**: a one-way signal. Optical power along a fiber is one; a
  back-reflection is a second one-way signal running the other way.
- **`across` + `through`**: an acausal pair, as in Modelica, Simscape and SPICE.
  Temperature and heat flow, voltage and current.

A connection compiles to both kinds of graph node:

- **One-way variables become dataflow edges:** the in-port's signal is its partner's
  out-port signal. They are evaluated like bindings, every frame, and cycles are
  rejected.
- **Pairs become equations:** across values are equal at the joint, and through values
  sum to zero. With the behaviours' own equations, these form a cluster solved by
  `iron_solve`, which already groups independent equations into clusters. Loops are
  allowed, as they are for any cluster.

**Editing from either end is not a port feature.** It is the store's existing
gesture machinery: dragging a derived reading (the meter's `port.signal`) pins it as a
finger, and the solver moves the free parameter upstream (the laser's power) through
the behaviour equations. This generalises linear binding inversion to any chain the
solver can invert.

**Geometry stays out of ports.** Placement constraints (a mount bolted to a table) may
share the solver, but never travel through physical ports. Physics flows through ports
and models; placement goes through constraints (vision 09 §1).

Order inside phase 3:
1. The port-spec format with both variable kinds, so files never need migrating.
2. One-way signals and dragging the meter's reading: laser → fiber → meter.
3. Pairs solved as equations, with a thermal or electrical example.

---

## 12. Phases and exit criteria

### Phase 1 — Values and run-time schemas

`Value::Quantity`, `Value::Record`, `Expr::Get`, the dimension-checked floor, `Field::Sym`
interning, `params` validated against definitions, the `library` root, the definition
format, and `bar` moved to a definition.

*Exit:* `9.58 mW + 1 nm` is an evaluation error and `9.58 mW * 2` is not; a quantity
round-trips through save and load with its unit string; an entity's parameter is bound
to a slider; the demo scene's bar works as a definition; every existing test still
passes.

**Progress (branch `roadmap`, 2026-10-10):**

- *Registry primitives only* — done first, as the roadmap asks: `bar` is out of
  `registry.rs`; the demo's bar is a plain `rect` until the definition lands.
- *Quantities* — done: `units.rs` (`Dim`, prefixed and compound unit parsing, canonical
  coherent units for the file, engineering-prefix display), `Value::Quantity { si, dim }`
  stored as `{"q": [si, "W"]}` and accepted as `{"q": "9.58 mW"}` from the model surface,
  quantity literals in expressions (`9.58 mW`, printed canonically as `0.00958 W` so the
  round trip is exact), dimension-checked arithmetic, comparison and floor functions
  (`sqrt` halves, `pow` needs an integer exponent, trig needs plain numbers), clips
  interpolating quantities, `quantity` as a schema kind the `<edit>` parser coerces.
  Decided as the roadmap §7 defaults: SI magnitude plus dimension; display unit is
  presentation; geometry stays in pixels and radians (`deg` and `rad` are scales on a
  plain number); plain numbers are dimensionless, so `2 mW * 3` works and `2 mW + 3`
  is an error. Tests: `expr::tests::quantities_are_dimension_checked`, `units.rs`,
  `tests/values.rs` (file round trip with the unit string, views, binding errors, clips).
  The exit's first two clauses hold.
- *Run-time fields* — done, with one deviation from §4.1: instead of `Field::Sym`, run-time
  names are interned to a deduplicated `&'static str` (`sym.rs`), so `LeafPath` keeps one
  representation and stays `Copy`; files still store strings. `params` is the one dynamic
  component (`component.rs`, `@dynamic`), validated against the definition at apply time;
  a `Set` on a parameter the component lacks is an error, so every inverse is a `Set` back.
  The registry schema marks it `"*": "any"` and the `<edit>` parser accepts `params.<name>`.
- *Records and `Get`* — done: `Value::Record { kind, fields }` as `{"rec": ["kind", {…}]}`,
  `Expr::Get` (`#3.params.beam.waist`). Record kinds are names only until definitions
  declare record shapes (deviation: no `RecordKind` schema yet).
- *Definitions as data* — done (`definition.rs`): `library`, `definition`, `param_spec`,
  `depiction` and `entity` types; `RelKind::Depicts` (renamed `template` and `figures` on
  2026-10-11, file schema 3); `params_of`, `validate_entity`
  (definition exists; parameter names match the specs exactly; a constant has the spec's
  kind and, for a quantity, the default's dimension; `ErrorKind::BadInstance`), and
  `instantiate`, which creates the entity with every parameter at its default, copies
  the notation's template subtree as ordinary nodes, rewrites each reference to a spec's
  `param_spec.default` into the entity's `params.<name>` (and template-internal
  references onto the copy), and links each copied root to the entity with `figures`.
  *Design choice not in the plan:* templates reference their own definition's
  `param_spec.default` leaves rather than a placeholder for "the entity", so a template
  is a live, evaluable subtree (it shows its defaults) and needs no new syntax. The
  reconciler never materialises anything under a `library`. The file schema is 2;
  schema 1 files still load. `bar` is now a definition in the demo's library with one
  parameter, `level`, instantiated into an entity under the scene plus a rect figure;
  the entity's `level` is bound to the slider, and dragging the figure still inverts
  onto the slider through the parameter. Tests: `tests/definitions.rs`. Not done from
  the §4 list: `behaviour` nodes (they need ports, phase 3) and the `library/*.json`
  built-in definition files (the demo builds its definition from ops; the file form is
  what `save` already writes). Entities live wherever a group admits them until phase 2's
  `model` root exists. The exit's last three clauses hold; every existing test passes.

### Phase 2 — Entities and compositions (no ports yet)

`model` and composition roots, `figures`, migration of the demo into a `scene`
composition, one panel per composition, selection by `NodeId`, cross-probe highlighting,
Show in, the Link gesture, and the `Model` and `Composition` views.

*Exit:* a 2D cartoon composition and a 3D composition side by side; linking the cartoon
box to the torus creates one entity; hovering either highlights both; Show in frames
the counterpart; undo of the link is one step; the model view lists the entity with
both figures.

**Progress (branch `document-spine`, 2026-10-11):**

- *Rename* — done first (handoff task 0): `depiction` → `template` (node type and
  component), `depicts` → `figures`; file schema 3, schemas 1 and 2 load with the old
  names mapped (`serial::renamed`). Test: `definitions.rs` loads a schema-2 spelling.
- *Types and roots* — done: `composition` (name; `composition { notation, dims }`, dims
  `2d|3d`) and `model` (children: entity) are **root-only** (`TypeSpec::root_only`, so a
  nested one is refused on apply and on load). `composition.rs` holds the reads:
  `composition_of`, `compositions`, `dims`, `figured_by`, `figure_of` (lifts a node
  through its ancestors to the figure and entity, stopping at the composition), and
  `figures_of`. Migration as in §5, with one deviation the handoff asked for: the
  pendulum group stays a loose root rather than moving under `scene`; loose 2D roots
  draw in the viewer panel. The demo gains a 3D `assembly` composition holding the
  torus, a cartoon `box` in `scene`, and a `model` root holding the bar's entity.
  Entities are not *required* to live under `model` (older files keep theirs where they
  are). Tests: `tests/compositions.rs`.
- *Panels per composition* — done (§7.1): the shell's `BevyPanel` takes a `composition`
  (node id with `#`, or name); `set_panel_viewport` carries it; `document_bridge::
  panel_views` resolves every `composition`-kind panel each frame into `PanelViews`
  (composition, dims, render layer). Layer 0 is shared scenery (ground, lights) plus
  loose 3D nodes; each composition gets its own layer on first use. One `Camera3d` per
  3D panel (`scene3d::sync_panel_cameras`), clear colour `None` (a clear load op wipes
  the whole shared target, erasing panels drawn earlier; the background camera clears
  once); the free-look controller goes to the first. The 2D layer loops over 2D panels
  with each one's offset and clip; picking finds the panel under the pointer first and
  hits only what it shows; a drag keeps the panel it started in. The sidebar became the
  assembly panel (decided with the user). **Measured** with headless Chrome on the GPU
  (`GPU=1`, 1600×1000, `?panels=1|2|4`): 60 fps in all three, worker tick p50 3.2 / 3.0 /
  3.2 ms, rAF p95 16.7 ms. No per-panel cost worth the §7.1 fallback at this content
  size. A side effect: the old flake where a press on the ball over the torus picked the
  mesh cannot happen now, since 2D and 3D hits never come from the same panel.
- *Selection by `NodeId`* — done (§7.2): `SelectionState` holds nodes in pick order
  (shift-click adds); `document_bridge::Highlights` derives what lights up — a picked
  node inside a figure stands for the figure's entity, so every figure of it lights in
  every panel; 3D outlines read the same set. (Since 2026-10-14 a lit group shows its
  frame in 2D panels rather than every descendant's outline, as in Figma; 3D meshes and
  plane views light under a lit ancestor. `render-benchmark.md` §9.) The
  shell receives selection and hover as ids. Found on the way: **3D outlines had never
  rendered** (the `bevy_gizmos_render` feature was missing); now enabled.
- *Link and Show in* — done. Deviation from the handoff's signature: `Store::link(a, b,
  keep, actor) -> Result<TransactionInput, LinkError>`, where `LinkError::Question` is
  the `LinkQuestion` (both entities' name, definition, params, figures, ports, and port
  collisions), alongside `Invalid` and `AlreadyLinked`; the ops come from
  `composition::link_ops`. Neither picture has an entity → a `thing` entity named after
  the first named picture, under `model` (model root, library and `thing` definition are
  created if missing); one has → the other joins; both have → the question until `keep`,
  then the other's figures move and it is deleted. One transaction each, so undo is one
  step. FFI `document_link`, `document_counterparts`, `document_focus`; the shell has a
  Link button, "Show in ▸" buttons and the merge dialog. **Show in** frames: a 3D panel's
  camera keeps its direction and backs off to the node's bounds; a 2D panel pans (a
  per-panel `PanelPans` offset, presentation only — not in the document, not undoable).
  There is no "open a panel" yet: our panels are fixed, so focus on a composition no
  panel shows returns an error. Also fixed: a click on a mesh committed a float-noise
  "move mesh" (moves now wait for 3 px of travel), and a press and release inside one
  slow frame were lost (edges now come from button events).
- *Views* — done (§11): `ViewQuery.relation` is `tree` (default), `model`, or
  `{composition: id}`. The model view lists every entity with its definition (by id and
  name), parameters and a `<figure>` per picture with its composition; the composition
  view is that composition's subtree with each figure's entity as `of="#e"`. Golden files
  `tests/golden/model-view.xml` and `cartoon-view.xml`. `agent.ts` sends the model view,
  then the tree (deviation: not the model view alone — the tree still holds the library,
  timelines and loose roots the model must be able to edit). The model surface links with
  `<link-figures a b keep?>`, alone in its `<edit>`; a merge question comes back as
  `<edit-failed>` with a `<question kind="merge">` naming both entities and asking for
  `keep` (ECM I5 as decided). The manifest describes compositions, figures and the verb.
- *Demo and smoke* — done. **Exit verified** in headless Chrome by the run-iron smoke
  script (expected output in `SKILL.md`): the 2D scene and 3D assembly side by side;
  select the box, shift-click the torus, Link → one `link` transaction, the model view
  lists entity `box` with figures `#l` (scene) and `#Q` (assembly); hovering either
  lights both (`c1-hover-box.png`, `c1-hover-torus.png`); Show in frames the counterpart
  each way (`c1-show-in-assembly.png`, `c1-show-in-scene.png`, focus log lines); one undo
  removes the link; the model surface's link, question and `keep` merge. Native:
  `tests/compositions.rs` (7 tests).

### Phase 3 — Ports, connections, scalar signals

Port kinds (`optical_power`, `rf_voltage`, `electrical`, `heat_flow`, `value`),
`port_spec`, port instantiation, `connection` nodes, signal graph nodes, the `port.signal`
leaf, and drawing connections in notations that show their domain.

*Exit:* laser → fiber → power meter in a cartoon composition and in a schematic
composition. Raising the laser's power or the fiber's length changes the meter readout
in both panels in the same frame. Connecting `rf_voltage` to `optical_power` is
rejected. The model view shows the chain.

**Done 2026-10-12 (steps 1 and 2 of §6.1).** In `iron_document`:
- **Types and components:** `port_spec`, `port` and `connection`, the `port` and
  `connection` components, and the `connects` relation.
- **Port kinds** (`port.rs`): `value`, `optical_power`, `rf_voltage` (one-way);
  `electrical`, `heat_flow` (pairs, carried but not yet solved).
- **Signals:** an in-port's signal is a binding the graph derives from its connection
  (`port::derived_signals`), never stored. An out-port's signal is its definition's
  behaviour, copied at instantiation, which now creates the entity's ports.
- **`definition::place_figures`** draws an existing entity in a second notation.
- **Connections are checked after the transaction** (`ErrorKind::BadConnection`): one
  kind, out to in, one feed per input, no signal loop.
- **Handles** follow derived signals and quantities. With several free sources they
  pick the only one the value is linear in, so the meter's reading inverts to the
  laser's power while the fiber's length and loss, which sit in an exponent, hold.
- **Views:** the model view lists ports with live signals, and connections as
  `entity.port` names.
- **Tests:** `tests/ports.rs`.

In the app:
- **The demo** has `laser`, `fiber` and `meter` definitions with cartoon and schematic
  templates, in the **Optics** and **Schematic** tabs.
- **Sliders** drive the laser's power and the fiber's length.
- **Dragging the meter gauge's edge** sets the power slider back through the chain.
- **The model surface's manifest** explains ports and connections.

Every exit was verified by the smoke script's stage F section:
- lengthening the fiber dims both gauges in one transaction;
- the model view lists the chain;
- an `rf_voltage` output to the meter's optical input is `bad_connection`.

**Deviation:** the exit's "same frame" is met by one transaction resolving both
compositions. They are separate tabs by default, and the smoke script measures values,
not two panels on screen.

**Step 3 done 2026-10-12: across/through pairs as solver equations.**
- **Behaviour equations:** `equation` nodes (`equation.lhs` = `equation.rhs`, expressions
  over ports and parameters) live on definitions and are copied onto entities like
  ports.
- **The network** (`port::network`) takes every pair port's unbound across and through
  values as unknowns. Its equations are:
  - each connection net's: across values equal, through values summing to zero, and an
    unconnected terminal carrying no flow;
  - every entity's behaviour equations.
- **Solving:** `Store::resolve_physics` solves the network with `iron_solve` after every
  transaction, preview and playhead move. The solution goes in the graph's physics
  layer, which is derived and never stored, so bindings read it in the same pass. A
  network that does not converge keeps its last solution (`Store::physics_error`).
- **Editing:** solved values are locked against direct edits. Edits go through a source
  or a parameter. Pinning a solved value as a finger, to edit from the other end, is
  not built.
- **Demo:** a heater, a thermal link and an ambient sink in the **Thermal** tab, with
  sliders for power and resistance and gauges for the temperature rise and the heat
  flow.
- **Verified** by `tests/ports.rs` (`a_heater_settles_through_its_thermal_link`) and the
  smoke script's thermal section.

**Next for this phase's area:** pins on solved values (drag the heater's temperature,
solve for its power), electrical parts, and adapters between port kinds.

### Phase 4 — Consistency and encapsulation

Obligations, ghost lines, the tray, `omits`, `Store::encapsulate`, drill-in panels and
breadcrumbs, and the law tests of §9.

*Exit:* a ground connection drawn in the schematic appears as an obligation in the 3D
composition until a bolt figure is linked; encapsulating the laser and fiber yields a
"source" block with one `out` port and an unchanged meter reading; drill-in shows the
original parts.

### Phase 5 — Time-domain signals: the modulator demo

`Signal::Series`, time-dependent sources driven by the lesson playhead or simulation
time, and a modulator definition (an optical field in, an RF voltage in, a modulated
field out).

*Exit, the four-representation demo:* the modulator appears in a cartoon, a schematic,
and a 3D composition (a mesh placeholder until CAD import exists). An animated RF drive
visibly modulates the light in the cartoon, and a scope trace in the schematic plays the
same waveform from the same signal. Selecting the modulator anywhere highlights it
everywhere.

### Phase 6 — Lessons and annotations

`lesson` and `step` nodes, annotation primitives, `targets` and `step` relations, step
playback in the shell, carrying an annotation, the `Lesson` view, and model-surface
support.

*Exit:* a three-step lesson that frames the cartoon, then the schematic, then the 3D
view, with a callout that follows the modulator across all three; the LLM adds a step
through the lesson view.

### Phase 7 — Implied constraints and CAD tagging

After `constraint-system.md` phase 6 (3D mates): port kinds declare implied mates per
notation; connecting a lens to a beam creates the rail; CAD import detects geometric
features automatically, and an authoring pass (or the LLM) tags ports.

*Exit:* drag a lens along its beam rail while the spot size at the detector updates
through the signal graph.

---

## 13. Change map

| Where | Change | Phase |
|---|---|---|
| `src/iron_document/src/value.rs` | `Quantity`, `Record`, `Dim`; JSON forms | 1 |
| `src/iron_document/src/units.rs` (new) | dimensions, unit strings, display conversion | 1 |
| `src/iron_document/src/expr.rs` | `Get`; dimension-checked arithmetic; `port.signal` reads | 1, 3 |
| `src/iron_document/src/component.rs` | `Field::Sym`; `ComponentKind::Params`; new components | 1, 2 |
| `src/iron_document/src/sym.rs` (new) | interner | 1 |
| `src/iron_document/src/registry.rs` | primitives only; new structural types; `bar` removed | 1, 2 |
| `src/iron_document/src/definition.rs` (new) | definition format, validation, instantiation | 1, 3 |
| `src/iron_document/src/document.rs` | new relation kinds; roots; migration | 2 |
| `src/iron_document/src/graph.rs` | `GraphNode::Signal`; time inputs | 3, 5 |
| `src/iron_document/src/obligations.rs` (new) | §8 | 4 |
| `src/iron_document/src/lib.rs` | `encapsulate`, `obligations`, link and merge helpers | 2, 4 |
| `src/iron_document/src/view.rs` | `Model`, `Composition`, `Lesson` relations | 2, 6 |
| `src/document_bridge/materialize.rs` | per-composition render layers; skip structural nodes | 2 |
| `src/document_bridge/render.rs` | per-panel 2D passes; cross-probe highlight; ghost lines | 2, 4 |
| `src/bevy_app/scene3d.rs` | one camera per 3D composition panel | 2 |
| `src/bevy_app/interaction.rs`, `src/lib.rs` | selection by `NodeId`; panel-aware picking | 2 |
| `src/panels.rs`, `src/web_ffi.rs` | panel ↔ composition; `focus` FFI | 2 |
| `src-ui/lib/BevyPanel.svelte` and new panels | composition prop; representations panel; tray; obligations; lesson player | 2–6 |
| `src-ui/model/*.ts`, `document_schema()` | new views, verbs, quantities, definitions in the manifest | 1–6 |
| `library/*.json` (new) | built-in definitions and notations | 1, 3 |

---

## 14. Risks

- **Schema dynamism versus type safety.** Run-time fields push errors from compile time
  to apply time. Mitigate with strict validation at apply, good error messages (the LLM
  is the main author), and tests that load every library definition.
- **Too many concepts for a user.** Entity, definition, figure, composition,
  notation, port, connection, lesson. The UI should let most people never see the
  words: they draw, link, and connect; promotion happens behind those gestures.
- **Per-panel rendering cost.** Several cameras and 2D passes. Measure in phase 2 before
  adding panels freely.
- **Units friction.** Every expression that mixes plain numbers and quantities needs a
  rule. Decide: plain numbers are dimensionless, so `9.58 mW * 2` works but
  `9.58 mW + 2` is an error.

---

## 15. Open questions

1. **I1. Sym stability.** Interned ids are per process and files store strings. Is that
   enough for the model surface, which also exchanges paths as strings? Likely yes.
2. **I2. Definition versioning.** Editing a definition changes every instance. Do
   instances ever pin a version?
3. **I3. Physical units in 3D.** Is a 3D composition in metres (CAD) while 2D is in
   pixels? Probably yes: a composition declares its length unit, and the bridge scales.
4. **I4. Default figures.** Generate a labelled box with port anchors for any
   notation a definition lacks, or require explicit templates?
5. **I5. Merge on link.** When both figures already have entities, which identity
   survives, and what happens to connections on the other? **Decided 2026-10-11: ask
   before doing anything.** The UI shows both entities with their parameters and ports and
   the person picks the survivor; the model surface refuses the edit with an `<edit-failed>`
   naming both and asking for `keep="#id"`, answered on the retry. Port-name collisions are
   part of the same question. No default heuristic.
6. **I6. One undo stack** for the whole project (assumed) versus per composition.
7. **I7. Notation format.** How much of a notation is data (palette, domains, glyphs)
   versus code (layout engines, constraint conventions)? Start with data plus a fixed
   set of code-defined behaviours selected by enum, as doc 02 §9.6 suggests.
8. **I8. Conservative ports** (vision 09 §13.2): whether heat and current need
   across/through pairs.
