# Contexts: Iron's behaviour as data (master plan)

**Status:** §5 steps 0–7 built 2026-10-26/27 (4 in its first part; `track` along a path deferred; 7 as `chrome.md`'s revision, chrome as context fields the overlay reads, not yet chrome nodes); a schematic notation (step 8) next; the rest proposed. The user supports the direction ("I'm not afraid of
thinking big on this topic. Now is the time in this program to have these overarching
ideas"). Nothing built. This plan owns the shared foundation and the order of work;
domain specs own their detail: [`affordances.md`](affordances.md) (interaction and
motion) first, then [`chrome.md`](chrome.md) (the editor's chrome), then the rest of
§4. Where they disagree on the foundation, this plan wins.
**Direction (user, 2026-10-26):** interaction semantics, default motion and similar
human-interface choices may be implemented in Rust but are semantically data. Any 2D
or 3D context, a free canvas or an electrical schematic, is a context with default
metadata describing how interaction happens. Built-in defaults are data that never
needs saving; a document saves only what it overrides.
**Realises:** roadmap clarification 15 ("users and the LLM can define new notations,
as they can build editors": notations as data); [vision 09](../vision/09_entities-compositions-and-ports.md)
principle 2 ("a composition is a world with a notation. A notation's palette and
conventions push a composition toward one kind of diagram"); [vision 02](../vision/02_substrate-model-projection-control.md)
§9 (editors as artifacts; "making the binding layer data is the whole move");
[vision 06](../vision/06_the-computation-boundary.md) §2–3 (state machines before
handlers; enumerated choice).

---

## 0. The idea in one table

| | Mechanism (the engine, Rust) | Policy (context data) |
|---|---|---|
| Analogy | a browser's C++: layout, `transition`, `cursor`, event dispatch | the user-agent stylesheet, keymap files, page CSS |
| Changes | rarely; engine work | per notation, per document, per node; by people and models |
| Saved in a document | never | only what the document overrides |

A **context** is a set of **rules**; a rule **matches** nodes and **declares** values in
one or more **domains** (interaction, motion, chrome, keys, snapping, …). Built-in
contexts (`std:*`) ship with the engine as data. A composition's `notation` names its
context; containers (a stack, a table, an instance) are contexts inside it; a node's
`affords`-style facet is the rare inline exception. A **notation is a bundle of
context data** across every domain: `std:free-2d` is Iron's built-in one; a schematic
notation is a library someone can write, import and share.

---

## 1. Is it simpler?

**What shrinks.**

- `select.rs`'s ~51 node-type checks, its press cascade, `see_through`, and the
  slider-in-component rule (`affordances.md` §1).
- Duplicated or scattered constants: `SLOP` (twice), `DOUBLE_CLICK` (twice),
  `POINT_HIT`, `PORT_HIT`, `SNAP_PX`.
- Two hand-kept key whitelists (`input_manager.ts`, `engine_api.rs`'s key mapping) and
  the select tool's key arms.
- The UI's hand-listed enum options (`PropertyField.svelte`), because facet fields do
  not declare their domains.
- Fixed tool lists, new-object defaults scattered through tools, table presets in
  Rust, standard components built by Rust code emitting ops.
- Every new kind of content costing branches in several files.

**What is added.**

- A rules engine and a cascade: one more abstraction to learn and to debug.
- A data format for contexts, and a built-in library to maintain.
- Performance care: resolution must stay a cached table lookup.

**How the addition is kept honest.**

- **Matching stays small**: type, position (`self`, `children`), state, and a
  container's context. No names, no specificity arithmetic: within a context later
  rules win; across contexts the nearer wins. CSS's hardest parts are left out.
- **A "computed" view from the start** (§2.6), as a browser's devtools show computed
  styles: for any node, every domain's effective values and the rule each came from.
- **Mechanisms stay few**, and a mechanism is added only when a domain needs one.

### 1.1 Locality: nobody walks up the layers (user's concern, 2026-10-26)

Rules set by a container are a mild form of inheritance, with inheritance's cost:
behaviour that is clear only to someone who has read the parent. For a model in
particular, needing to climb the layers to learn what a gesture does is a source of
confusion. The answer is to **store sparsely but always read resolved**: readers
never see the storage form. These are requirements of the foundation, not options:

1. **Three sources, always the same.** A node's behaviour comes only from its
   notation's default for its type, its **direct** container, or its own inline
   facet. No descendant selectors; `children` means direct children. A document
   context may `extend` one built-in context; built-ins do not chain.
2. **Behaviour follows from visible, named roles.** A container that changes its
   children's behaviour says what it is in its own leaves (`table`,
   `layout.mode="stack_y"`), as an ARIA role does. The built-in defaults are a
   small, standard vocabulary, documented once in the manifest (as everyone knows
   what a `<button>` does).
3. **Views print effective behaviour with its source**, wherever it differs from the
   universal default: `affords="sequence" affords.by="#g (std:stack)"`, in the style
   of `.bind`, `.by` and `<driven-by>`, which already show derived values beside
   the value. A reader of a node learns what it does from that node's line.
4. **Every effect explains itself.** A refusal or surprise names its rule (as a
   locked drag names its reason today), and the computed query (§2.6) returns the
   resolved answer with its provenance. For a model, the views are the devtools.

Brevity where data is written, clarity where it is read: these are different
representations, so neither is traded for the other.

**One example.** Tables needed a double-click to reach a cell through its row. Today
that is a Rust function, `see_through`, and a branch in three places. As data it is one
rule in `std:table`: `<rule match="type:row" select="through"/>`. A context wanting
the first double-click to start editing a cell writes `edit="text"` on its cells. A
document wanting rows selectable overrides the rule. No code.

---

## 2. The foundation

Built in phase F together with `affordances.md` A1 (§5 step 1), and shaped for every
domain in §4 so later domains add declarations, not redesigns. One syntax everywhere:
declarations are `domain.field` attributes (`affords.drag`, `keys.nav`,
`motion.placement`, `chrome.selected`).

### 2.1 Rules

```xml
<context name="std:stack" for="type:group">
  <rule match="children"               affords.drag="sequence" keys.nav="list"/>
  <rule match="children"               motion.placement="out 160 layout"/>
  <rule match="children state:dragged" chrome.lift="std:chrome/lift"/>
</context>
```

- **`match`**: `all`, `self`, `children`, `type:<t>`, `state:<s>`, and combinations.
  `state` is the interaction state (hovered, selected, focused, pressed, dragged,
  drop target; `affordances.md` §8.1).
- **Declarations** are `domain.field` attributes. A domain is registered by the engine
  with its fields and their kinds, like a facet: unknown domains and fields are
  rejected when a context is parsed, with the field list in the message.
- **Cascade**, bounded by §1.1: within a context, later rules win field by field; a
  document context may `extend` one built-in context; a node takes values only from
  its notation's defaults, its direct container's context and its own inline facet
  (nearer wins); absent fields fall through; modes (presentation, a lesson step)
  apply on top.

### 2.2 Where contexts come from

| Source | Example | Saved |
|---|---|---|
| Built-in library (`std:*`), embedded in the engine | `std:base`, `std:free-2d`, `std:3d`, `std:stack`, `std:grid`, `std:table`, `std:instance`, `std:chrome`, `std:text-edit`, `std:path-edit` | never |
| A composition's `notation` | `"free"` → `std:free-2d`; `"schematic"` → a document or library context | the name only |
| A container that is a context | a group with `layout` → `std:stack`; a `table` → `std:table` | nothing extra |
| A document's own contexts, in its library | `<context name="std:table">` with a few rules, added after the built-in's (built 2026-10-26); a new name is a new context a notation may name | yes |
| A definition's rules for its parts | the dial's needle turns | with the definition |
| An inline facet on a node | `affords`, for one exception | yes |
| A mode | presentation, a lesson step | session only |

### 2.3 Two forms, one vocabulary

Built-in contexts are written in the markup above (`include_str!`, parsed at
startup). A document's contexts are `context` and `rule` nodes in its library, so they
are undoable, saved, and edited by the same ops as everything else; views print them in
the same markup. (Like CSS as a file or a `<style>` element.)

### 2.4 Compilation and cost

A context compiles into tables keyed by (type, position, state) per domain when it is
loaded or edited. A query is a few lookups along a node's chain, cached by (chain,
document version, session version). A document whose content behaves by default
stores no context data at all.

### 2.5 Where it lives

`src/iron_document/src/contexts.rs`: the markup parser, the built-in library, domain
registration, compilation, the cascade, and two queries of different shape:

- **Per node**: `query(store, chain, i, domain) -> Values`, for what a node does
  (interaction, motion, chrome, snapping).
- **Per view**: `focus(store, view) -> Focus`, the focus context of §2.7 (notation,
  the open scope's container roles, the tool state, the mode), and
  `query_focus(store, &Focus, domain) -> Values` for what the view offers (palette,
  keymap, inspector sections).

Bevy-free and tested natively. Each domain module (`interaction.rs`, the motion
interpolator, the chrome producer, …) asks it.

**One meaning of a notation name.** `use.notation` already picks which picture of a
definition an instance shows (`expand.rs`, `definition::template_of`), and
`composition.notation` names a world's context. They are the same name space: a
`"schematic"` composition's parts show their schematic templates and behave by the
schematic context.

### 2.6 Reading and debugging

- **Computed view**: `<computed node="#x"/>` (model surface) and an inspector section
  list each domain's effective values for a node and which rule, in which context,
  set each. The first tool anyone needs when behaviour surprises them.
- **Built-in library view**: a model or a person can read `std:*` in the same markup.
- **The manifest** gains a CONTEXTS section generated from the registered domains, as
  NODE TYPES is generated from the registry.

---

### 2.7 The focus context, and the shell that follows it (user, 2026-10-26)

The user wants the Svelte shell, though DOM, to depend deeply on the state and context
of the active view. So the engine computes, per view, one **focus context**: the
context keys of the current view, selection and open scope (the composition's
notation; the roles of the containers along the scope, a stack, a table, a
component's main; "editing text", "editing points", "a gesture in progress", the
mode). Everything the shell shows for a view resolves against it, and is pushed to the
shell by subscription (the typed UI protocol's, `infinite-canvas-architecture-patterns.md`
§5):

- **The tool palette** (domain 7): the palette of the context a tool would create
  into (§6).
- **The keymap** (domain 2).
- **The inspector** (domain 8), as **flat sections, not a hierarchy**. A hierarchy of
  contexts each inheriting tools is §1.1's problem. Instead each section declares
  where it applies with a `when` over the focus context and the selection's facets,
  as VS Code contributes views with `when` clauses over context keys, Blender's panels
  `poll` their context, and Figma's right panel shows Layout, Auto layout, Fill,
  Stroke, Effects or Component properties by what is selected. "Why is this section
  here" is answered on the section. A context only orders and emphasises sections (a
  schematic puts a part's value first and hides fill).

A section is a small editor as data (vision 02 §9): a title, a `when`, and controls
of a few kinds the shell renders generically: a **field** bound to a leaf (its control
from the leaf's kind and, for enums, the schema's options, domain 9), an **action**
calling an engine verb (add a row, make a component, promote a port), a **list** over
children (ports, overrides), and a few **rich widgets** that stay Svelte components
named by kind (font picker, colour picker, paint row), as a browser's native controls
are. Examples: Position applies to anything with `transform2d` in a 2D context; Fill
and Stroke where they are allowed and the context's role is drawing; Ports and
"promote port" when the open scope is a component's main; Table on a table or cell.

The engine's inspection today already returns most of the inputs (`components`,
`addable`, `component`, `main`, `port`, `table`); `SelectionView.svelte` then
interprets them with hard-coded sections. That interpretation becomes data, and the
shell a generic renderer of resolved sections, with the computed view (§2.6) able to
say which `when` put each one there.

## 3. Mechanism is still code, and stays small

The engine owns: the pointer state machine's core, picking, the drag kinds,
keyboard navigation models (`list`, `grid`, `tree`), the interpolator and its easing
curves, layout (Taffy), the solver, the reactive graph, rendering, `attach` and
outline geometry (chrome), the snapping search. Adding one is rare and deliberate.

---

## 4. Domains, in order

| # | Domain | Today | Spec | Depends on |
|---|---|---|---|---|
| F | **Foundation** (§2) | — | this plan | — |
| 1 | **Interaction**: select, drag, activate, drives, edit, grips | `select.rs` type checks and cascade | [`affordances.md`](affordances.md) A | F |
| 2 | **Keys and focus**: ARIA list, grid, tree; per-context keymaps | key arms, two whitelists | `affordances.md` B, then a keymap section here | 1; the focus context (§2.7) for keymaps |
| 3 | **Motion**: interaction state, declared transitions | none | `affordances.md` B (§8) | F |
| 4 | **Tolerances**: slop, double-click, hit radii, snap distance | constants in four files | a section of `affordances.md` | F |
| 5 | **Chrome** | `draw_select_overlay` and friends | [`chrome.md`](chrome.md) | 1, 3 |
| 6 | **Snapping**: what snaps to what, tolerances, guides | `iron_spatial::snap`, fixed rules | an extension of [`snapping.md`](snapping.md) (its phase 5, "settings") | F, 5 for guides |
| 7 | **Palette and creation defaults**: a notation's tools; new objects' look; presets and themes (table looks, the deck theme) | `CompositionTools.svelte`, tool code, `table.rs` presets | to write | F, the focus context |
| 8 | **Inspector sections**: flat sections, each with a `when` over the focus context and controls (field, action, list, rich widget); a context orders and emphasises them | `SelectionView.svelte`'s hard-coded sections | §2.7 here; to write in detail | F, the focus context, 9 |
| 9 | **Enum domains in the schema** | `PropertyField.svelte`'s lists | small; facet macro gains option lists | — (independent) |
| 10 | **Domain libraries**: port kinds, units, compatibility | `port.rs`'s `PortKind` table, `plug.rs` | to write | F |
| 11 | **Standard library as data**: components in markup | `standard.rs` op builders | to write | F |
| 12 | **Workspace defaults** per notation | `workspace.ts` | with panels-and-session | F |
| 13 | **A first full notation**: a schematic, bundling 1–12 | — | to write | most of the above |
| 14 | **Time-aware state machines** | clips only | to write (vision 06) | 3 |

Items 9 and the duplicated constants (part of 4) are small and independent: they may
land any time, before F.

---

### 4.1 What belongs here: the admission test

Something becomes context data only if **all three** hold:

1. **It is a human-interface choice**, not a fact about the document ("grips are
   8 px" is a choice; "this resistor is 4.7 kΩ" is a fact).
2. **At least two contexts plausibly want it different.** If every notation would
   choose the same, it is a constant.
3. **A mechanism exists, or is worth building, to evaluate it.** Data nothing reads is
   decoration.

Values, geometry and connections stay document facts; mechanisms stay code; anything
failing test 2 stays a constant. The test is what keeps this plan from becoming
"everything is configurable", CSS's other failure.

### 4.2 Candidate domains (passing the test; not yet scheduled)

| Candidate | Why it passes | Notes |
|---|---|---|
| **The model surface per focus context** | What a model is told (vocabulary, conventions, "this" and "here") differs by where the user is | Highest value given Iron's reliance on models: smaller, situated prompts; the manifest generated for the focus context, not whole |
| **Conventions and checks** | A schematic wants every pin connected; a slide a title | Vision 09's conventions, ECM phase 4's obligations, as lint rules shown as gentle warnings |
| **The world's appearance and templates** | Background, grid, a schematic sheet's border and title block, a slide master; what a new composition of a notation starts with | — |
| **Units and number display** | Engineering notation (4.7 kΩ) in a schematic, scientific in physics, rounded in a deck | — |
| **Menus and commands** | Right-click menus and a command palette are §2.7's actions with `when` clauses | VS Code's model |
| **Import, export, clipboard, drop** | A schematic exports a netlist as well as SVG; an image dropped in a deck is a picture, in a schematic nothing | — |
| **Navigation** | Wheel pans or zooms; CAD-style or Blender-style orbit; zoom to fit a sheet | keymap and gesture data |
| **An accessibility tree** | Having taken ARIA's semantics, export them: the shell mirrors the canvas's roles as a real accessibility tree | Screen readers on canvas content; it falls out of the design |
| **User-level contexts** | Larger grips, Figma or Altium keys, left-handed layouts: contexts belonging to a person, applied like a mode | Open question 4 |
| **Permissions** | Read-only for a reviewer is `select: none` and `inert`, per user | With shared projects (`notes/projects-labs-and-server.md`) |

Already in the model and only named by the bundle: which picture a definition shows
in a notation (definitions carry a template per notation).

---

## 5. Work order

Revised 2026-10-26 (user approved): test the foundation's second query shape (per
view) early, and make step 1 end in a visible change, rather than one long
behaviour-neutral refactor before anything shows.

0. **Small and independent**: enum domains in the schema (9); the duplicated
   constants gathered in one place (the start of 4).
   *Done 2026-10-26:* `facets!` takes `=> ["a", "b"]` after an enum field's default
   (every enum field lists its values; a test holds it), the schema carries them, the
   inspector reads them in place of `PropertyField.svelte`'s list, and the manifest
   prints `field:a|b|c`. The five copies of the 3 px slop and two of the double-click
   window are `bevy_app/tolerances.rs`; `materialize.rs` uses `geom::PATH_HIT`.
1. **F, minimal, + `affordances.md` A1**: `contexts.rs` (markup, `std:*`, the bounded
   cascade, the computed view) and `interaction.rs`'s `target`, `enter`, `up`,
   `children`, used by `select.rs`'s selection paths; behaviour unchanged, the guard
   scripts after each step. Exit: the tables double-click changed by a document rule
   alone, and shown by `<computed>`.
   *Done 2026-10-26* (the full smoke passes, its documented run-to-run lines aside; `tables.txt`, `layout.txt` and
   `snapping.txt` unchanged; `text.txt`'s expected block was already stale). Built:
   `contexts.rs`, `std_contexts.xml` (`std:free-2d`, `std:3d`, `std:table`),
   `interaction.rs`; five fields so far (`affords.select`, `affords.enter`,
   `affords.edit`, `chrome.lit`, `snap.target`). Decisions made while building:
   a context's name is `name=` (views keep `id=` for node ids); a document context is
   a `context` node in the library whose one leaf, `context_def.markup`, holds its
   markup (open question 1, for now: markup, so one parser and printer serve both
   forms); one with a built-in's name appends its rules there (CSS's page sheet over
   the user-agent's), one with a new name is a new context; a container whose node
   is `through` is passed over when finding a node's container (CSS's
   `display: contents`), which is how `std:table`'s rules reach cells through rows; a
   node's own context's `self` rules sit between the notation's and its container's.
   A context named after a notation already resolves for compositions of it.
   Model surface: the `<context name>` verb, views printing document contexts as
   markup, `__iron.computed(id)` (`<computed node>` with `.by` per value),
   `__iron.contexts()`, and the manifest's CONTEXTS section from the registered
   fields. Known gap: a summary view elides a long library's children, so a document's
   contexts can be hidden there; the edit's reply names the node.
2. **The focus context (§2.7), with the palette and keymap per view** (7, 2), proven
   on contexts that exist today: a free canvas, inside a table, editing text, a
   component's main. **Checkpoint:** `hover-sweep` before and after F; the user
   decides whether to continue.
   *Done 2026-10-27.* Rules gain `match="view"` and two list-valued fields,
   `palette.tools` (tools from the engine's registered `TOOLS`, space-separated,
   `a|b` sharing a slot) and `keys.tools` (`key:tool` pairs); view and node fields
   cannot mix (the parser says which). `Focus { world, scope, states }` and
   `Resolver::view_values`: the world's notation context's view rules, then those of
   the context of the container the selection is open in (past `through` nodes),
   each matching the focus's states (`editing-text`, `editing-points`, `main`: the
   world holds a component's main). `std:free-2d` declares today's toolbar and Figma's
   keys, plus the port tool where a main is; `std:3d` declares none (no toolbar, no
   keys). A document context may now `extends` one built-in context (one level), so
   a new notation (`deck`) keeps `std:free-2d`'s behaviour and changes its palette.
   App: `bevy_app/view_focus.rs` computes each view's focus on change and pushes
   `focus` events (palette, keys) per panel; `CompositionTools.svelte` draws the
   palette it is given (icons and labels by tool id stay in the shell); plain letter
   keys go to the engine when any view binds them (`runtime/tool_keys.ts`, replacing
   `input_manager.ts`'s `v p r o t` list) and become `tool:<id>` through the keymap
   of the view under the pointer, so tools match tool ids, not letters
   (`pen.rs`, `text_tool.rs`). `__iron.viewContext(panel)` shows a view's focus and
   each value's rule. Exit shown by `focus.txt`. Checkpoint, `bench-render.sh 1000
   10000` (`wasm-bench`, 1600×1000@1): `hover-sweep` changed frame 1.3 ms at 1k and
   10k (recorded 1.3 and 2.1 ms in `render-benchmark.md` §9), `select-group` 1.4 and
   1.3, `drag-one` at 10k 1.6; no regression (a sanity check against recorded
   numbers, not a same-day A/B). Not yet data: the Library and Stats views' "no
   toolbar" (the shell still checks their names), and tool icons and labels. The
   `main` state reads only the world's top level, so with nothing selected inside an
   embed whose scene holds a main, Main's palette lacks the port tool.
3. **`affordances.md` A2**: the remaining type checks (boxes, grips, edit, the text
   tool, slider-in-component).
   *Done 2026-10-27.* Two node fields: `affords.grips` (words: `box`, `shape`) and
   `affords.drag` (`none`, `point`, `track`). `std:free-2d`: targets offer `box`, rects
   and ellipses `box shape`, sliders drag `track`. In `select.rs` the selection box's
   resize and rotate grips need `box`; a lone node's corner and arc handles need
   `shape` (three places); double-click and Enter edit points only where
   `affords.edit="points"` (a path at once, a rect or ellipse by conversion); a
   slider, and a slider's knob inside a target container, scrub only where its
   context says `track`; the text tool edits where `affords.edit="text"`. "Is it
   a path" is now a facet check (`PathShape`), a fact, not a choice. What still
   names types, by the admission test (§4.1): `local_box`'s and `shape_grips`'
   per-type geometry (how a rect's box or an ellipse's arc handles are computed is a
   fact of the shape, not a choice any context would make differently), tool ids,
   the "is a component instance" check, the 3D gizmo's mesh check, and the empties'
   markers (chrome, `chrome.md` C2). Guards: the full smoke (only its documented
   run-to-run lines differ), `tables`, `layout`, `snapping`, `text` (its expected
   block refreshed: it had predated the 298-op demo), `contexts`, `focus`.
4. **Inspector sections** (8): `SelectionView` becomes a generic renderer of resolved
   sections.
   *Done 2026-10-27, as far as this:* a view field `inspector.sections` (words from
   `SECTIONS`: `main port component table position text layout appearance fill
   stroke effects other add`, today's order and the default, so a notation saying
   nothing shows everything). `inspect` returns the list resolved for the focus of
   the view holding the selection; `SelectionView.svelte` renders its sections in
   that order, each a rich widget that still shows only where its node has (or may
   add) what it edits. A context orders and hides: `focus.txt`'s deck shows Position
   and Fill only. **Not yet:** sections declared as data (title, `when`, field,
   action and list controls), so a new section is still a Svelte block; the
   availability half of each `when` (which facets it needs) lives in the block. That
   is the next step for this domain, when a notation first needs a section of its
   own (the schematic's part value).
5. **`affordances.md` B**: interaction state, transitions, sequence with gliding
   siblings, ring, activate, ARIA keys, tolerances as data. Demos 1–2.
   *Partly done 2026-10-27:* **keys** (`keys.nav`: `std:table` a `grid`, groups laid
   out in a line `std:row`/`std:column` (a context's `for` may now test a leaf,
   `type:group layout.mode=stack_y`) a `list-x`/`list-y`; `interaction::key`: arrows
   through merged cells, Tab to the next cell and past the last a new row, Enter
   edits, Alt+arrow reorders a list item; `keys.txt`, demo 2); **sequence**
   (`affords.drag="sequence"` for a stack's items; `Store::set_layout_preview` lays
   the stack out in the previewed order with the item floating at the pointer, never
   an edit; release commits one `Reorder`; `sequence.txt`); **motion**
   (`motion.placement="<ease> <ms> [<when>]"`; the store eases what layout moves,
   on a clock the host advances, `document_bridge/motion.rs`, marking the moving
   composition live; off without a host, so tests and headless reads see final
   values; `std:row`/`std:column` items glide "out 160 layout"; a document slows or
   removes it; demo 1 apart from Figma's lifted item). Deviation from
   `affordances.md` §8.2: the eased value is published, so picking and bounds lag
   with drawing for the glide's 160 ms (it keeps the render caches and damage
   correct for free). *Finished 2026-10-27:* **interaction state**, readable two
   ways (affordances.md §8.1): the store holds which nodes are hovered, selected,
   pressed or dragged (the app reports them, `bevy_app/interaction.rs`), so rules
   match `state:`; and an opt-in `interaction` facet whose leaves the host publishes
   (`interaction.hovered` …), so bindings read it, only to drive how a node looks
   (appearance, fill, gradient, stroke, shadow; anything else is refused
   `out_of_scope`); the Pitch card lights while hovered (`hovered ? 1 : 0.45`, the
   expression language's ternary). **Tolerances** are view fields
   (`tolerance.slop`, `tolerance.double_click` in ms, `tolerance.hit` scaling grips'
   reach), resolved with the palette and read by the select tool for the view it
   works in. `ring` and `activate` landed with C.
6. **`affordances.md` C**: document contexts (decide open question 1 here), `notation`
   resolving to a context, definition rules, the inline facet. Demos 3–4.
   *Done 2026-10-27, demo 3 and the core of C:* the inline `affords` facet (`style`,
   CSS-like `drag: ring; drives: transform2d.rot`, a bare field meaning `affords.*`;
   `scope` `self` or `children`), applied after a node's contexts, the container's
   `children` facet before its own; values it sets print `.by="inline on #n"`; bad
   style is refused (`bad_context`). **Definition rules need no mechanism of their
   own:** the facet on a template's part is copied into every instance by expansion,
   so editing the definition changes every instance's next drag (the standard meter's
   pivot, natively). New fields `affords.drives` (one of the node's own leaves),
   `affords.activate` (`toggle`, `step`, `cycle`), `affords.step`, `affords.axis`, and
   `ring` among the drag kinds. A press on a control (the deepest node on the chain
   declaring one; Cmd still selects) drives its leaf through `Store::preview_target`,
   so a bound leaf moves its source and one that cannot follow is refused with its
   reason; an activation is a click, `cycle` taking the enum's values from the schema.
   Shown by `controls.txt` (demo 3, with plain nodes rather than components: a needle
   turning a slider through its binding, a bar sliding it, toggle, stepper, choice).
   Open question 1 stays settled as markup in one leaf. **Not done:** a `context`
   facet naming a document context for one container (`notation` and built-in names
   cover today's cases), presentation-only bindings on interaction state (needs B's
   readable state), `track` along a path, and demo 4 (a table and charts of one
   dataset, edited from either side: a bar drag already edits bound data; typing in a
   bound cell needs an inverse for formatted text).
   *Finished 2026-10-27:* a `context` facet names a container's context outright
   (`context.name`), in place of the one its type carries. **Demo 4** (Data tab):
   three sliders shown as a table of bound readouts and as bars; a bar's drag edits
   the data, and typing in a cell does too, because words bound to `fmt(x, …)`,
   `str(x)` or `sci(x, …)` now invert through the number (`handle.rs`: typed text
   parsed, a plain number taking the format's unit), so the text tool edits such a
   cell and its commit writes the source, the binding kept. **Deferred:** `track`
   along a path. A bead on a path has no leaf of its own holding its parameter (a
   plain node has none; a component instance's `args` would), and its bound x and y
   are not linear in anything, so §5.4's own-leaf rule refuses it; it comes with a
   component demo that gives the bead an input.
7. **`chrome.md`** C1–C5.
   *Done 2026-10-27, revised* (`chrome.md`'s revision note): chrome's choices and looks
   are context fields the existing overlay drawer reads, rather than session nodes.
   View fields `chrome.accent`, `chrome.grip`, `chrome.grip_shape`, `chrome.hover`
   (resolved into each view's `Offer` by `view_focus`) and node fields `chrome.lit`
   (`shape`, `frame`, `none`) and `chrome.handle` (`dot`: a part's own handle at its
   origin while it or its container is selected or hovered). The defaults draw exactly
   today's chrome (an accent replaces the built-in blues only when a context sets one).
   The showcase shows them: Pitch orange with round grips, Outline touch-like (no hover
   light, 14 px grips, `tolerance.hit` 2), the Dials needle's pivot dot. Shown by
   `chrome.txt`, which counts pixels by colour (`tint`). Chrome as picture nodes
   (C1–C5 proper) waits for a notation whose chrome differs in shape: step 8.
   Checkpoint, a same-day A/B (`bench-render.sh 1000 10000`, `wasm-bench`): the first
   version resolved `chrome.handle` for the selection's and hover's children every
   frame and cost ~0.7 ms a frame at 10k (`hover-sweep` frame p50 0.4 → 1.1 ms);
   caching which chains are dotted, by document version, chains and interaction
   states, put every scenario back on the base commit's numbers (`hover-sweep`
   frame p50 0.3 ms, changed frame 1.8 ms, at 1k and 10k).
8. **A schematic notation** (13), with what it needs: snapping settings (6), domain
   libraries (10), parts as data (11), the wire tool. §6's demo.
9. **State machines** (14), on motion's primitives; workspace defaults (12) whenever
   panels-and-session needs them.

`affordances.md` D (drop, regions, modes, 3D) slots in after 6 or 7 as demos need it.

---

## 6. The demo of the whole: a notation changes the editor (user, 2026-10-26)

Two compositions side by side: one in `std:free-2d` (the usual drawing tools) and one
whose `notation` names a small **schematic** context, in the spirit of a basic
Altium. Switching tabs swaps the whole editor's behaviour, all from data:

- **The tool palette** at the bottom changes: drawing tools (move, rectangle, ellipse,
  pen, text, table) in one; select, wire, resistor, capacitor, ground, source, net
  label and text in the other. A palette entry is data (a tool kind the engine owns,
  what it places, its icon, its key, the new object's defaults); a part's icon can be
  its own schematic symbol from its definition.
- **Keys** change with it: R is rectangle in one, resistor in the other; Space rotates
  a part while placing it (Altium's).
- **Snapping** to a grid and to pins in the schematic; edges and centres in the other.
- **Interaction**: wires connect pin to pin (`relation`), dragging a part keeps its
  wires attached; no motion in the schematic.
- **Chrome**: pins show as terminal dots, wires carry bend handles.

**The palette follows where a tool would create**, the container a new node lands in
(the paste and drop code's `drop_target`): double-clicking into a component whose
template's `notation` is `schematic` (the field exists: the optics demo has cartoon and
schematic templates), or into an embed of a schematic composition, switches the
palette, keys and snapping; Esc back out switches them back. It is the same "where
are you" question the select tool answers, so they cannot disagree.

**New pieces it needs**, each a domain or mechanism of §4: the wire tool (click to
click polylines snapping to pins and the grid, routed orthogonally), an electrical
port domain (domain 10; "electrical parts" is open in the product handoff), grid
snapping (domain 6), part definitions as data (domain 11), the palette and keymap
domains (2, 7). **A stretch:** Iron already solves across/through networks (the
thermal demo), so the drawn circuit could show live DC voltages at its nets.

---

## 7. Risks

- **A large refactor of interaction code** (`select.rs`, 3,722 lines). Mitigated by
  doing F and phase A behaviour-neutral, in small steps, each checked by the guard
  scripts; by keeping the press cascade's order intact until chrome makes it stacking.
- **The cascade becoming CSS's specificity problem.** Mitigated by §1's small matching
  language and the computed view.
- **Performance.** Every query cached; the render benchmark's hover-sweep and the 1k/10k
  stress runs measured before and after F, A and chrome C2.
- **Two forms of one vocabulary** (markup built-ins, node-based document contexts).
  Mitigated by one parser and one printer and a round-trip test; open question 1.
- **Scope creep.** Each domain is optional after F and 1; the plan delivers value at
  every step (demos per phase), and stops wherever the user decides.

---

## 8. Open questions

1. **Document contexts as nodes or as markup in one leaf?** Nodes give undo and ops
   per rule; markup is compact and matches the built-ins exactly.
2. **Domain registration.** Fixed in the engine, or could a library declare a new
   domain (with no mechanism behind it, only data other rules read)?
3. **Sealing.** May a component's rules resist the context it is placed in?
4. **Per-user preferences** (larger grips for one person, a different keymap): a mode
   from the session, or user-level contexts outside any document?
5. **3D**: when does `std:3d` gain more than the gizmo and point drags?
