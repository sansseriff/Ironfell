# Discrete Variation, Semantic Provenance, and Node Inspectability

**Status:** Adopted 2026-11-23. Replaces the reverted universal N-axis patch algebra
([`../archive/patches-universal-algebra.md`](../archive/patches-universal-algebra.md);
post-mortem in [`../notes/postmortem-universal-patch-algebra.md`](../notes/postmortem-universal-patch-algebra.md)).
**Work order:** [`../handoff-engine.md`](../handoff-engine.md) (`EN-12`–`EN-16`).
**Companion architecture:** [`engine.md`](engine.md).

---

## 1. Guiding principles

Three product principles govern everything in this plan:

1. **Legibility for humans and LLMs over global cleverness.**
   Never allow hidden CSS-style action-at-a-distance (wildcard suffix selectors or mystery
   cross-scope rules). When a human or an LLM inspects `#plot1`, every reason `#plot1`
   looks or moves the way it does must be locally discoverable from `#plot1`.
2. **Semantic derivation, not dumb copy-paste.**
   Making a variant of a component (`extends`), placing an instance (`<place>`), advancing
   a slide deck (`A → B → C`), comparing takes/ghosts, or defining shared-property machine
   states must preserve **provenance back to the original** instead of duplicating dead
   nodes whose relationship to their source is severed.
3. **UX uniformity is not engine uniformity.**
   The "patches / applies-to / provenance" mental model is unified at the **UX and
   inspection layer**, while the engine keeps four fundamentally different runtime concerns
   in four tiny, orthogonal mechanisms.

---

## 2. The four orthogonal layers

Instead of forcing every overlay and variation into one N-axis `PatchTable`, the engine
keeps four clean boundaries:

| Layer | Owns | Engine mechanism | Size / Complexity |
|---|---|---|---|
| **A. Lexical Component Structure & Lean Virtual Views** (`EN-13`) | Definitions, `extends` variants, `<place>` instances, direct overrides, `use.notation`, `repeat`/`when` | Shared `Arc<Template>` + flat lexical override maps (`outermost instance > inner instance > variant > definition`) + sparse resolved table + keyed `ItemTable`. **Deletes `Docs::copies`.** | Small; big net **negative** LOC |
| **B. Ephemeral Interaction & Solver State** (`EN-12`) | Live drag previews, live constraint solves, persisted copy solver poses | Three separate tiny `BTreeMap`s: `gesture_preview: BTreeMap<Leaf, Value>`, `live_solve_preview: BTreeMap<Leaf, Value>`, `Document::solved: BTreeMap<Leaf, Slot>` (`Op::Solve`). | ~120 lines total |
| **C. Continuous Time, Clocks & Simulation** *(Stage 1, built)* | Clocks, visibility gate, timeline clips, physics/networks, layout, transition playback, per-placed-instance simulation history | Clock tree + fixed `ProducerId` priority stack (`LAYOUT > CLIPS > MACHINES > NETWORK`) + isolated history per placed instance. | Already built |
| **D. Small Discrete Variation Layer ("Patches")** (`EN-15`, `EN-16`) | **Component machine states**, **decks/slides**, **takes/loop iterations**, **narrative moves & arrows** | Value-and-`hidden` overrides over tiny finite **Ordered** and **Unordered** axes, with direct finite-set evaluation, arrow diffs, and bubbling edit ops. | ~350–500 lines |

### Effective value precedence (fixed, human-obvious)

For any live leaf, highest precedence first:

1. **`gesture_preview`** *(ephemeral, owned strictly by `ActiveGesture`; cleared on cancel/commit)*
2. **`live_solve_preview`** *(ephemeral, owned strictly by `resolve_clusters_live`; cleared on transaction/coordinate change)*
3. **Binding cell (`{= …}`)**
4. **Layout producer (`ProducerId::LAYOUT`)**
5. **Clip producer (`ProducerId::CLIPS`)**
6. **Document discrete variation** *(active slide / narrative joint / take override)*
7. **Component machine producer (`ProducerId::MACHINES`)** *(interpolates between discrete state poses along transition progress)*
8. **Other runtime producers** *(network, items, theme, hosts — fixed `ProducerId` order)*
9. **Persisted solver pose (`Document::solved`, `Op::Solve`)**
10. **Lexical instance / variant override** *(outermost `<place>` → inner `<place>` → `extends` variant)*
11. **Base authored / template constant** *(or facet default)*

A slide/take/narrative override that sets a component's **state input port** (`args.vis = "shown"`)
feeds `machine.rs` naturally, which smoothly animates the component's channels/leaves through
its transition arrow.

---

## 3. The Small Discrete Variation Layer (Layer D)

This is the **one** place where patches with regions live in the engine.

### 3.1 Only two axis kinds

| Kind | Examples | Selection forms |
|---|---|---|
| **Ordered** | Slides in a deck (`A, B, C, D`), takes of a loop (`1, 2, 3`), narrative joints | `{v}` ("this only"), `[a, ∞)` ("from here on"), `[a, b]` (span), `{a, b}` (explicit set) |
| **Unordered** | A component's machine states (`ice, water, steam`), orthogonal local modes (`vis`, `mode`) | `{a, b}` (subset) or `*` (all) |

- **Stable value IDs & tombstones:** Axis values have stable IDs. Deleting Slide C leaves a
  tombstone in place so `[C, ∞)` still starts where C was and renaming/inserting never
  rewrites stored regions.
- **Tiny finite domains — no symbolic hyperbox solver:** Authored discrete axes hold a
  handful of values (typically 2–100). Resolution, arrow diffs (`x → y`), blast radius,
  shadowing checks, and bubbling ops run directly over finite sets — no `Product::minus`,
  no cofinite `not {…}`, no suffix-pattern exclusion trees, no fractional rank allocation.
- **Within one scope (e.g. a deck or one component's machines):** Specificity is simple
  and explicit: narrower region wins when nested (`{C}` beats `[B, ∞)` beats `[A, ∞)`;
  later start `[C, ∞)` beats earlier `[B, ∞)` on an ordered axis); for incomparable
  overlaps on the same field (e.g. `{A, B}` vs `{B, C}` both setting `fill.color` at `B`),
  explicit patch order breaks the tie and the inspector flags the overlap.
- **Orthogonal local states supported cheaply:** A component's state patch may constrain
  one or more of that component's own local unordered state axes (e.g. `vis = shown & mode = hover`).

### 3.2 Values and `hidden` only (no structural `Present` in `Tree`)

- Discrete patches set **constant field values** (numbers, colours, strings, enums, vectors,
  `hidden`, port args) on existing nodes — **never** node presence (`Present`), optional
  facet presence (`FacetPresent`), `use.notation`, or swapped formulas (`Slot::Bound`).
- **Visual elements appearing/disappearing across slides or takes:** Toggle `hidden`
  (an ordinary leaf; fades smoothly with opacity). Creating a visual annotation while
  viewing Slide C can automatically set base `hidden = true` and `[C, ∞): hidden = false`.
- **Topological / physical / layout structure changing across slides or takes** (adding a
  flywheel on Take 3, adding a resistor on Slide B, adding a row to an auto-layout stack):
  Expose the active discrete coordinate as a readable context value (`@slide`, `@take`,
  `@state`) and use Iron's **existing `when` conditional** (`<resistor when="{= @slide >= $b}">`,
  `<group when="{= @take >= 3}">`) or a component **`extends` variant**.
- **Behavioural / equation changes across slides or takes:** Parameterize the formula via
  a port or mode (`args.harmonics`, `args.model = "nonlinear"`) driven by the discrete
  override, or toggle two nodes via `hidden`/`when`.

### 3.3 Arrows (`x → y`)

An **arrow** between two points `x → y` (two machine states, two slides, two takes, or two
narrative joints) is computed directly:
- **Entering patches:** patches matching `y` and not `x`.
- **Leaving patches:** patches matching `x` and not `y`.
- **Field diff:** every `(node, field)` whose resolved value differs between `x` and `y`.

Transitions animate that exact field diff (Keynote Magic Move style), refined by authored
tweens/timings where specified (`machines.md`). An interrupted transition retargets smoothly
from the currently drawn values.

### 3.4 Editing & bubbling operations

Every interactive edit obeys one rule: **"a drag always changes what you see."**

| Operation | User intent | Behaviour |
|---|---|---|
| **Drag / Edit** | Change a value while viewing point `x` in edit region `E` (e.g. `[B, ∞)` or `{water}`) | If a patch in this scope already wins at `x` and lies within `E`, edit it (telling the user what else shares it, e.g. *"Editing Ice & Water"*). Otherwise write to the patch for `E ∩ winner_region` (or `E` if none), creating one if needed. |
| **Bubble up (`widen`)** | Apply this change to more states/slides | Expand the patch's region (e.g. `{water}` → `{ice, water}`, or `{C}` → `[B, ∞)`). |
| **Push down (`narrow`)** | Make this change more local | Shrink the patch's region (e.g. `{ice, water}` → `{water}`, or `[B, ∞)` → `{C}`). |
| **Partial bubble (`split`)** | Bubble only *some* properties (e.g. colour, not position) | Move the selected entries into a new patch with the wider/narrower region; leave the remaining entries untouched. |
| **Reset** | Return to inherited value | Remove the entry from the winning patch, revealing what lies underneath. |

---

## 4. Worked product scenarios (preserved as acceptance targets)

Every phase of `EN-14`–`EN-16` must deliver these concrete scenarios cleanly:

1. **Slide deck building on itself (`A → B → C → D`):**
   - Editing Slide B in *"from here on"* mode (`[B, ∞)`) sets parameters and unhides labels
     across B, C, and D.
   - Editing Slide C in *"this slide only"* mode (`{C}`) adds a highlight ring and callout
     that automatically vanish on Slide D.
   - Editing Slide A flows into B, C, and D everywhere not overridden; touching a property
     overridden on C warns *"Slide C overrides this colour"*.
   - A structural change on Slide B (e.g. inserting a load resistor into a live schematic)
     uses `<resistor when="{= @slide >= $b}">`, updating the live network solver cleanly.
2. **Cyclic 3-state machine with per-property sharing (`ice ↔ water ↔ steam`):**
   - No single parent state and no cycle problem.
   - `{ice, water}` shares container colour; `{water, steam}` shares fluid motion; `{ice, steam}`
     shares an extreme-temp badge.
   - Tweaking colour while viewing `water` edits `{ice, water}` with one-click **This state
     only** or **Bubble to Steam**, and **Split** lets the user bubble colour without moving
     position. Every transition leg interpolates only the properties that differ on that leg.
3. **Takes, ghosts & chaos comparisons (generalizing After Effects precomps):**
   - Ordered `take` axis: Take 2 builds on Take 1 (`[2, ∞)` doubles mass); Take 3 adds a
     flywheel (`when="{= @take >= 3}"`). Editing the base mount in Take 1 updates all takes.
   - **Ghost overlay:** `<place def="Pendulum" at="take=1" clock="#sim"/>` placed behind
     Take 3 on the same clock. Each placed instance keeps its own simulation/integrator
     state (reset on loop wrap), so Take 3 races Take 1 frame-for-frame.
   - **Chaos comparison:** Two `Lorenz` placements side-by-side or overlaid on one shared
     clock differing only by `args.rho = 28.000` vs `28.001` (or two sibling states
     `{baseline, perturbed}`).
4. **Narrative / pedagogical arrows:**
   - A lesson's moves connect points `x → y`; the narrative panel and LLM read the arrow's
     entering/leaving overrides and diff directly, and stepping forward plays derived
     Magic Move + component machine transitions automatically.

---

## 5. First-class Node Inspectability & Provenance ("Ask the node")

Iron's Stage-1 recorded-read reactive graph (`05df865`) already tracks incoming and outgoing
dependencies across bindings, layouts, constraints, networks, clips, and machines. `EN-14`
exposes one unified, read-only **`inspect(node)`** query — used by both the **UI Inspector**
and the **agent's XML `<look>` verb** (e.g. `<look id="#plot1" connections/>`):

Given any node (authored or virtual instance part), `inspect(node)` answers four questions
in one shot:

1. **Identity & semantic derivation:**
   - Where it sits in the tree (composition / parent chain).
   - What it derives from (definition, `extends` variant chain, `<place>` instance, active
     slide/take/state coordinate).
2. **Property provenance (`why`):**
   - For each non-default or overridden leaf, the exact ordered stack of contributors from
     §2's fixed precedence list (e.g. `base: #333 → variant "Plot · two-slit": #2563eb → slide [C, ∞): #d33 (winning)`),
     plus which later slides/states shadow an edit here.
3. **What drives this node (incoming):**
   - Bound formulas (`{= …}` and their upstream nodes), active clips/timelines, component
     machines & current transition progress, layout parent, constraint clusters, network
     islands, and discrete variation patches targeting it.
4. **What this node drives (outgoing):**
   - Every downstream binding, plot, constraint, network connection, `when`/`repeat`
     condition, or child machine that reads from this node.

Because there are no hidden wildcard selectors or multi-scope symbolic patches, this single
inspection view is **complete**: if something affects `#plot1`, it appears right here.

---

## 6. Explicit non-goals (what stays OUT of patches)

To protect engine simplicity and legibility, the following are **explicitly out of scope**
for discrete variation:

- **No wildcard/exclusion instance selectors** (`Sel::Suffix { tail, not }`). Use definitions,
  `extends` variants, or direct overrides on specific instances.
- **No ad-hoc document patches targeting a component's internal state axis**
  (`slide = 3 & vis = hover`). Expose an input port or create a lesson `extends` variant.
- **No `AxisKind::Time` in patches.** Continuous animation stays on clocks + `ProducerId::CLIPS`.
- **No `Session` or `Solve` layers in patches.** Gesture previews, live solves, and
  `Document::solved` live in their own tiny `BTreeMap`s.
- **No `Present` / `FacetPresent` / `Notation` / `Slot::Bound` in discrete patches.** Use
  `hidden` for visibility, `when` / `extends` for structure, and ports/modes for behaviour.
- **No symbolic hyperbox solver** (`Product::minus`, `Excl`, cofinite `not {…}`).
- **Note on `src/iron_patch`:** `src/iron_patch` is the historical standalone toy prototype
  from the retired universal algebra experiment and is **not** depended on by `iron_document`.
  Delete or replace it when `EN-15` lands.

---

## 7. Phased plan and net-LOC discipline

Ordered in [`../handoff-engine.md`](../handoff-engine.md) using fresh `EN-12`–`EN-16` keys
(retired `EN-3`–`EN-6b` keys are never reused):

1. **`EN-12` — Clean solver poses, split previews & Stage-1 carryovers *(net negative LOC)*:**
   Flat `Document::solved` (`Op::Solve`) deleting `Owners::solved`/`keep_solved`/`release`/
   `apply_recorded`; separate `gesture_preview` & `live_solve_preview` maps; zero-alloc
   `NodeId::expanded_parts`; recorded-read `TemplateCache` deleting `scope_stamp`.
2. **`EN-13` — Lean Lexical Virtual Views + `ItemTable` *(big net negative LOC)*:**
   Virtual views over `Arc<Template>` + lexical `instance > variant > definition` overrides
   + keyed `ItemTable` for `repeat`/`when`, **deleting `Docs::copies` and the bulk of `expand.rs`**.
3. **`EN-7` — Delete `regraph` & remaining wiring/repeats on virtual `Tree` *(net negative LOC)*.**
4. **`EN-8`–`EN-10` — Stage 3 Zero-Slope Scaling** (local need counts, visibility gate /
   retiring `ROOTED`, host-side scaling).
5. **`EN-14` — First-class Node Inspectability & Provenance** (`inspect(node)` + XML
   `<look connections/>` + Inspector provenance readout).
6. **`EN-15` — Small Discrete Variation Layer (~350–500 lines) + Shared-property Machine States**
   (2-axis discrete patches, arrows, bubbling ops `drag`/`widen`/`narrow`/`split`, migrating
   `machine.rs` poses so cyclic/partial-sharing machines work end-to-end).
7. **`EN-16` — Decks, Takes, Ghosts & Narrative Arrows** (ordered document axes, `@slide`/`@take`
   readable by `when`, `<place … at="take=2" clock="…"/>` with isolated sim state, plus
   Inspector / State Graph bubbling UX).
