# Layers panel

**Status:** Proposed 2026-11-06 (design discussion with the user, same day). **Phase 1
built 2026-11-06** on `t3/layers-panel` (§12); phases 2–4 open. Work order:
`handoff-product.md`, "Panels and session".

**Decision (user, 2026-11-06):** a layers panel in the HTML DOM, with virtualised
rows, scoped to one composition at a time. Its layout, interaction and styling follow
Figma's layers panel (§7.5). The other contexts an entity appears in show as markers on
its row, not as more rows. The whole document's structure is a separate concern (the
composition list, and later a Model outline), not one tree of everything. Start with a
targeted slice (phase 1) and leave timeline and narrative scope undecided. The
Document view keeps its place (the "+" menu; it was already not in the default
layout) until its general controls (undo, save, load, export) move to a File menu.

**Melt UI's Tree is not used** (checked 2026-11-06 against `melt` 0.44.0, the source of
`builders/Tree.svelte.js`); a WAI-ARIA tree over flattened rows is written instead
(§7.3).

A mockup from the discussion (static HTML, not in the repo) showed the composition
list, a tree with entity chips, an embed opened in place, derived parts, and a 14,444-row
virtualised import.

## 1. Why

The Document view prints the document as markup. That is the model's read (vision 09
§12) and useful for debugging, but not for a person finding their way around a
composition. The roadmap's Layers panel row (user, 2026-10-23) records the concrete
failure: in the 14k-node Prussia import, groups are found only by double-clicking
through them, one level at a time, and a click picks the outermost unopened group,
which may be a large sibling branch. There is no way to pick a single path from a list.

## 2. The question this answers

A component can be pictured in several contexts at once: a cartoon box, a schematic
symbol and a 3D body that are figures of one entity (component-model §5.1). Should a
layers panel for a 2D composition show the 3D context too, or should it be one plain
tree of the whole document?

Neither, because the document already separates them:

```text
model/        entity #m   (arguments, ports: shared)
cartoon/      use #u1     use.notation="cartoon"  figures → #m
3d/           use #u2     use.notation="3d"       figures → #m
```

A 2D composition's tree holds no 3D part to hide or show. Its row is `#u1`, one
picture of the entity; the 3D body is another picture in another composition's tree.
So:

- **The panel shows one composition's drawing tree:** groups, shapes, text, component
  uses, embeds and local geometry. It is not built for 2D; a 3D composition gets the
  same panel, and only the rules that depend on dimension change (§6).
- **Other contexts show up as markers on the row.** A row whose node figures an entity
  carries one chip per other composition that pictures it. A chip is Show in (§5).
- **One tree of the whole document is rejected.** It would mix three hierarchies
  (drawing order, entities and ports, the library), so a row's parent would mean
  different things in different places. Deleting a row would be ambiguous between
  removing this picture and removing the thing itself (`interaction-lifecycle.md`). At
  14k nodes it needs filtering by composition anyway.

`authoring-workflows.md` §8.3 asked for this split: the layers source is "the chosen
composition/open scope", a row "is a projection of a document object, not a duplicate
object", and the panel keeps "the drawing hierarchy" apart from "the semantic
entity/connection structure". This spec supersedes `panels-and-session.md` order of
work item 3 ("the outliner … from the model and composition views"); see §10.

## 3. What a row is

A row is one node **as reached through one composition**: the chain of nodes from the
composition's top level down to it, through every group and embed on the way. That is
exactly the select tool's `Reach::chain` (`src/bevy_app/select.rs`), which the session
already stores on each `selects` relation as `chain` (space-separated ids, panel
composition first, the node last).

- **A row's key is its chain.** The same node embedded twice gives two rows with two
  keys, and selecting inside one copy highlights that copy only.
- **Selection sync is a key comparison.** A selected node's row is the one whose key
  equals the `chain` the session recorded. A selection with no chain (set by a model
  edit, say) reveals the node's first reach in the panel's composition.
- **The panel adds no document state.** Open rows and the pin are panel state (§4);
  selection is the session's.

What a row shows:

| Part | Phase 1 | Source |
|---|---|---|
| Disclosure | yes, when the node has children | the tree |
| Icon by kind | group, path, rect, ellipse, text, `use` (multi- or single-context), embed, mesh, camera, other | node type and facets |
| Name | `name` facet, else the type and id (`path #1234`) | `Name` |
| Child count | on a closed group with many children | the tree |
| Embed target | `→ Pendulum` on an embed row | `embed.source` |
| Entity chips | one per other composition picturing the same entity | `composition::figure_of`, `figures_of` |
| Lock, hide | phase 2 | §8 |
| Derived parts | phase 3 | §8 |

**Multi- and single-context icons** (component-model §2): a `use` that figures an
entity with pictures in other notations gets the filled component icon; a standalone
`use` (no entity) gets the outline one and no chips. Local geometry has no entity and
no chips.

## 4. The panel's source

The panel shows one composition: the **source**.

- **Following (default).** The source is the composition of the composition view used
  most recently: the active dock tab with the `composition` notation, or the panel of
  the latest selection, whichever changed last. Clicking into the Schematic tab shows
  the Schematic's layers.
- **Pinned.** The source is fixed and does not follow. This lets you look through the
  3D assembly's layers while working in the schematic.
- **The source is the view spec.** A new `layers` notation has `source: "follow"` or a
  composition id. Pinning rewrites the spec, so dockview saves it with the layout
  (`workspace.json` per scope) and no new persistence is needed.
- **A Compositions section above the tree**, as Figma's Pages section sits above its
  Layers: every composition as a row with its dimension, the source highlighted.
  Choosing one goes there (§5). The split between the two sections is draggable, and
  the Compositions section collapses to its header. The Layers header carries a pin
  toggle (`IconButton` with Lucide `pin`) and collapse all, as Figma's does.

**Embeds and the editing scope.** When you work inside an embed, the panel stays on the
outer composition and reveals the row inside the embed, because its key is the full
chain. Phase 1 needs nothing more. A breadcrumb (Explainer › Pendulum) and dimming rows
outside the editing scope are phase 3; the scope they read is `contexts::Focus::scope`
(the containers the selection is inside, outermost first), which already exists per
view.

**Not decided here: the timeline's and the narrative's scope.** Timelines usually belong
inside components, mapping between states of a state machine, and there is no state
machine editor yet. A narrative can carry a timeline, but it can also branch, and a
linear timeline does not cover branching. Whether those panels read the same editing
scope is left open (§11). The layers panel keeps its scope in session data so a later
panel can read it.

## 5. Going to a composition, and Show in

Iron has no single canvas: several composition tabs can be open at once. So choosing a
composition does not swap a canvas, as a Figma page does. It means **go there**:

1. `showComposition(source)` (`src-ui/views/workspace.ts`) brings that composition's
   tab to the front, or opens a tab beside the main viewer if none is open. With two tabs
   open on one composition it picks the most recently active (today it picks the first;
   phase 1 changes that).
2. When following, the panel follows the focus there.

**A chip, or a Show in link elsewhere, adds two steps:** it selects the counterpart
(with its chain in that composition) and frames it (`document_focus`, which pans every
2D view showing that composition to centre the node). The tree opens down to the row,
scrolls it into view and briefly highlights it.

**When pinned,** choosing a composition in the header changes only the panel's source
and leaves the dock alone. A chip still moves the dock, because it asks to go to a
picture.

Today the Inspector's Show in calls `document_focus` alone, which frames the node only
in views that are already open and neither opens a tab nor selects. Phase 1 gives both
callers one client function, `showIn(node)`, that does all three.

## 6. Dimensions

The panel is the same for 2D and 3D compositions. What differs:

- **Order.** In 2D, row order is paint order, topmost first (Figma's convention: the
  first row draws on top). In 3D, order is only organisation. Phase 2's reorder writes
  child order in both; only 2D draws differently.
- **Icons.** 3D adds mesh, light and camera.
- **A 3D embed in a 2D composition** is a 3D branch in a 2D tree. The rules follow the
  branch's dimension.

## 7. Building it

### 7.1 The engine query

One new query in `iron_document`, so it is tested natively, with an `engine_api` wrapper
and a protocol op:

```text
layers { source, open_json: [key…] }
  → { ok, composition, dims, total,
      rows: [{ key, depth, id, kind, name, count, dims, embed?, entity?, chips: [{composition, name, dims, node, chain}] }] }
```

As built there is no `reveal`: the panel opens the ancestors of a revealed chain
itself, since they are the chain's prefixes.

- It walks the composition from its top level, descends only into rows whose key is in
  `open` (and the ancestors of `reveal`), and returns the **visible rows, flattened**.
  The client never holds the whole tree, and a closed 360-child group costs one row.
- An embed's children are the embedded composition's top-level nodes, keyed by the
  extended chain.
- `chips` lists the other compositions picturing the row's entity, one per composition
  (the first figure in each).
- The client subscribes as the Document view does (`doc.subscribe`, re-read on
  `documentChanged` and when `open` changes).

**Budget.** Measure the query with Prussia fully expanded (about 14,400 rows). If one
response takes more than a few milliseconds or its JSON grows too large, add a row range
to the query and fetch only the window. Phase 1 starts without a range.

The existing `view` query (`ViewQuery { relation: Composition(c), scope, depth }`)
returns markup, not rows, and has no notion of open sets or chains. It stays as is for
the Document view and the model.

### 7.2 Setting the selection from the panel

HTML can set the selection today only through the narrative's cross-probe
(`engine_api::narrative_probe`, action `select`): one node, written straight to
`SelectionState`, with no chain and no multiple selection. Phase 1 adds
`select { items: [{ node, panel, chain? }], mode: replace | add | toggle }` (a range is
computed by the panel from its visible rows and sent as `replace`; a missing chain is
the node's lineage in its own composition), applied as
a session op through `Store::set_selection`. The select tool already reads session
changes back (`select.rs`, the system that compares `session_version`), so the canvas
follows. `panel` is the view the panel is following (or, when pinned, the most recent
view of that composition), so the select tool's reach is right. The narrative's
`select` can then move onto the same command.

**Hover both ways.** Hovering a row lights the node on the canvas, and its counterparts
in other compositions, through the same probe's `hover` and `unhover` (Figma outlines
the hovered layer on the canvas). Hovering on the canvas tints the row if it is
mounted (`doc.hovered`); it never scrolls the tree.

Click replaces the selection; Cmd/Ctrl-click toggles; Shift-click selects the visible
range between the anchor and the row, as Figma does.

### 7.3 The Svelte component

`src-ui/views/html/LayersView.svelte`, registered as the `layers` notation in
`views.ts`.

- **The tree pattern.** Bits UI has no tree. Melt UI's Tree builder
  (`melt/builders`, 0.44.0, pre-1.0) does not fit, from its source:
  - it takes the whole nested item tree, and its navigation and range selection walk
    all of it (`Tree.getAllChildren`), where the panel holds only the visible rows the
    engine sends (§7.1);
  - arrow keys move focus with `document.getElementById(…).focus()` on the
    neighbouring item, which fails when that row is scrolled out and not mounted;
  - it keeps its own `selected` and `expanded` sets, while the selection here belongs to
    the session and is keyed by chain;
  - it renders nested `role=group` containers, while a virtual window is one flat list.

  So implement the WAI-ARIA treeview pattern directly over the flattened rows:
  `role=tree`, `treeitem` with `aria-level`, `aria-expanded`, `aria-selected`,
  `aria-setsize` and `aria-posinset` (the attributes the pattern provides for trees
  that are not nested in the DOM), focus moved through `aria-activedescendant` on the
  tree so it never depends on a row being mounted, and the keys of §7.4. Melt's
  behaviour list is the checklist: typeahead with a 500 ms timeout, range selection to
  the anchor, expand on click.
- **Virtualisation.** Rows have a fixed height, so a window is a few lines: a scroll
  container, a spacer of `rows × height`, and only the rows in view plus an overscan
  mounted. Add `@tanstack/svelte-virtual` only if variable row heights ever appear.
- **Styling.** Figma's panel (§7.5) in Iron's tokens (`src-ui/app.css`): row hover
  `--hover`, selected `--accent-soft`, names `--fg`, icons `--fg-faint`. Header controls
  like `CompositionSettings.svelte`. Lucide icons, each from its own path
  (`@lucide/svelte/icons/<name>`). Icon-only controls have an `aria-label` and a title.
  Check light and dark, at 1× and 2×.
- **Default layout.** Layers joins the left group in the base layout (`workspace.ts`)
  as its front tab, beside Model and Library (a document's first narrative still opens
  in front of it). The Document view was already not in the default layout; it stays
  in the "+" menu. It holds undo and redo, save, load and SVG export
  (`DocumentView.svelte`), which move to a File menu later (§11, question 5).

### 7.5 Following Figma

Taken from Figma's panel (UI3, in a browser, 2026-11-06), by looking and interacting:

| Figma | Iron |
|---|---|
| Pages section above Layers, each a section header with its own actions (search, add) | Compositions above Layers (§4) |
| Rows 32 px tall, a rounded fill inset from the panel edges, no tree guide lines | the same |
| Indent 24 px per level; the disclosure chevron left of the type icon, shown only when the row has children | the same |
| A small muted type icon per kind (frame `#`, group dashed square, text `T`, vector, image) | Lucide icons per kind (§3) |
| Names in the regular weight at the UI's body size; frames are not bold | the same |
| Hover: light grey fill, the layer outlined on the canvas, and lock and eye icons appear at the right | the same; phase 1 shows the icons only once phase 2 makes them work |
| Selected: a light blue fill across the row | `--accent-soft` |
| A set lock or hide stays visible without hover | phase 2 |
| Layers header: collapse all | the same, plus the pin |

Not taken: Figma's left rail (File, Assets and so on) and its single page canvas. Iron's
dock already holds its views.

### 7.4 Keyboard

Up and Down move the selection; Right opens a row or moves to its first child; Left
closes a row or moves to its parent; Home and End go to the first and last rows; Enter
renames (phase 2); typing a name jumps to the next matching visible row. Moving the
selection with the keyboard selects on the canvas too, as clicking does.

## 8. Phases

**Phase 1: browse and select.** §3–§7: the `layers` notation in the default layout;
following and pinning; the Compositions section; the `layers` query with open sets;
virtualised rows; selection both ways (`select` FFI, reveal on canvas selection); embeds
opened in place; entity chips and one `showIn` for chips and the Inspector.
*Exit:*
- With Prussia imported, select one path deep inside a province from the panel alone,
  and select a path on the canvas to see its row revealed. Scrolling the fully opened
  tree keeps fewer than 60 rows mounted.
- In the demo scene, a chip on the optics chain's laser in the cartoon composition
  opens or brings forward the schematic tab, selects the laser's symbol there and frames
  it.
- With one composition embedded twice in another, selecting inside one copy highlights
  only that copy's row.
- Native tests for the query (chains, open sets, embeds, chips); the run-iron smoke
  script gains a layers section and its expected output is updated.

**Phase 2: edits.** Rename (double-click or Enter, one transaction); drag to reorder and
to move into a group (child order and reparent ops, one transaction each; refused across
compositions and into derived nodes); lock as `select: none` in an `affords` facet
(`affordances.md`); hide (needs a visibility leaf, which does not exist yet: decide its
facet first); delete removes this picture, never the entity.

**Phase 3: scope and derived parts.** The breadcrumb and dimming outside the editing
scope (`Focus::scope`); opening a linked instance's derived parts, italic and dimmed,
where editing one writes an override (component-model §5.3); an "also shown in" marker
on an embed row, since an embed is one shared composition and editing inside it changes
it everywhere it appears.

**Phase 4: the entity in the Inspector.** An Entity section above the per-picture
sections: shared arguments and the list of pictures with Show in (vision 09 §4's
Representations panel). Placement and actions for this picture stay in the sections
below it (`authoring-workflows.md` §8.3, the resistor example).

## 9. Not in this spec

- A Model outline (entities, ports, connections, definitions). It would be a second
  view built from the `Model` view relation, with a figures list under each entity.
- Filtering and search in the panel. Type-to-jump covers the first need.
- Drawing the panel on the canvas as a session composition (`panels-and-session.md`'s
  "a panel is a view"). An HTML panel now does not prevent that later.
- Multiple-selection editing from the panel (rename several, lock several).

## 10. Changes to other plans

- `panels-and-session.md` order of work item 3: now this spec. Its outliner was "a
  composition generated from the model or composition view … drawn as rows"; this spec
  builds it in HTML from a dedicated query, scoped to one composition.
- `roadmap.md`'s Layers panel row points here.
- `authoring-workflows.md` §8.3's layers candidate is answered by §2–§4.

## 11. Open questions

1. **Timeline and narrative scope.** Whether the timeline and the narrative panel follow
   the same editing scope (the view plus the embeds entered). Waits on a state machine
   editor, and on how a branching narrative relates to a timeline.
2. **Linked repeats in imports.** Prussia imports with repeated pieces as references to
   definitions (`instance-cache.md` §7). Until phase 3 opens derived parts, a path inside
   a repeat is not reachable from the panel. **Measured 2026-11-06:** with every group
   open the panel lists 7,933 rows of the 14,407 nodes; the rest are inside references.
   A canvas selection inside one reveals and outlines the reference's row instead.
3. **The visibility leaf** for hide: which facet holds it, and whether hidden nodes are
   skipped by picking, bounds and export.
4. **Which picture a chip names** when an entity has several figures in one composition
   (phase 1 takes the first), and whether such a chip should list them.
5. **A File menu for the Document view's controls** (undo, redo, save, load, SVG
   export), so the markup view can leave the default layout. It could grow out of the
   scope menu (`scopes.md` §10.1). Until it exists, Document stays in the default
   layout.

## 12. Done

**Phase 1, 2026-11-06** (`t3/layers-panel`).

- **Engine.** `iron_document::layers` (`rows`, `lineage`, `key`): the visible rows of a
  composition, keyed by chain, opened by key, through embeds (the embedded
  composition's top level, up to 8 deep), 2D topmost first, chips one per other
  composition picturing the row's entity (with the figure's chain). Layer rows are
  groups, shapes, text, tables, paths, meshes, sliders, embeds, cameras, empties and
  uses; points, contours, overrides and expanded or session nodes are not. A text row
  with no name takes its first line. `engine_api::document_layers` and
  `document_select` (dispatch `layers`, `select`; protocol regenerated). Native tests:
  `tests/layers.rs` (order and open keys, an embed shown twice with distinct keys,
  chips per composition, points and contours).
- **UI.** `LayersView.svelte` (notation `layers`, in the default layout's left group
  and the "+" menu). Compositions section (fits its rows up to 40% of the panel,
  collapsible, resizable). A WAI-ARIA tree over flattened rows with
  `aria-activedescendant`, `aria-level`, `aria-setsize`, `aria-posinset`; 32 px rows,
  24 px indent, rounded inset fills, `--hover` and `--accent-soft`; only rows in view
  mounted (28 while scrolling Prussia). Click selects, Cmd toggles, Shift selects the
  visible range; Up, Down, Home, End, Right, Left and typeahead. A canvas selection
  opens the rows above it, scrolls the row into view and flashes it; a node inside a
  shared reference reveals and outlines the reference's row. Hovering a row lights
  the node through the narrative probe; canvas hover tints the row. Following
  (`active_view.svelte.ts`: the dock's active composition tab, or the view a new
  selection was made in) or pinned (the view spec's source, saved with the layout).
  Chips and the Inspector's Show in share `show_in.ts`: open or bring forward the tab
  (`showComposition`, now the most recently used of several), select with the chain,
  frame. A repeated, unchanged selection event no longer pulls a following panel back
  to the view the selection was made in.
- **HTML views take the pointer only under `.html-view`** (Workspace.svelte), whose
  global `.row` style is why the panel's rows are `.layer`.
- **Measured.** Prussia fully opened: 7,933 rows, 19–20 ms and 1.1 MB per `layers`
  call. Above the "few milliseconds" of §7.1, but only when every one of 893 groups is
  open; a row range in the query stays the remedy if it shows in use.
- **Checks.** `layers.txt` (20 assertions: Main's embeds, an embed opened, row → canvas
  with chain, canvas → row, Cmd and Shift, keys and typeahead, hover, follow and pin,
  a chip from Optics to Schematic, the Inspector's Show in, rows mounted); fast smoke
  (6 assertions), `narrative.txt` unchanged, `workspace.txt` with the new view;
  `iron_document` tests and 52 JS tests pass; `bun run check` 0 errors.
- **Not yet:** the embed-twice exit is covered natively, not in the browser (the demo
  embeds nothing twice); flash on a chip lands in the target's panel only when
  following.

**2026-11-09: the composition picker (the layout council).** The Compositions section
above the tree (Figma's Pages) became one row in the tree's header: the composition shown,
with its 2D/3D icon, and a menu of every composition (the one shown checked, its dims
beside it) and New composition…. It repeated the composition tabs and took about half the
panel (300 px of the demo's 715). Following, choosing goes to that composition; pinned, it
shows that composition here. The pin and Collapse all stay in the same row.
