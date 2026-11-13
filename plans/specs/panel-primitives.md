# Panel primitives: panels from the same parts as everything else

**Status: written 2026-11-09; the view selector strip built the same day (§5); grab pill hover corrected 2026-11-10; global settings modal and quieter default panel chrome built 2026-11-13.**

Iron's long-term rule for panels: a panel is not special shell code. It is a view (a
source drawn in a notation, `views.ts`), and the parts that make it up are the parts the
rest of the app is made of, so that an agent working inside the app can make a new kind
of panel with what it can already write: compositions, contexts, session leaves and
standard components. This spec names those parts, says how today's panels map onto
them, and builds the first piece: Blender's property-tab strip, as data.

## 1. The parts

| Part | What it is | Already exists as |
|---|---|---|
| **Source** | what a view shows: a composition, the selection, the document, a narrative, a generated session composition | `ViewSpec.source` |
| **Notation** | how it is drawn: by the canvas (a composition's world), by HTML (a Svelte component), or both | `ViewSpec.notation`, `NOTATIONS` |
| **Context** | what it affords and offers: gestures, tools, keys, which sections show, which views a strip offers | `std_contexts.xml`, document `<context>` overrides, view fields (`match="view"`) |
| **Session state** | per-view state that is not the document: what a strip has chosen, a timeline's axis, the clock, a pinned state | the session tier (`apply_session`, session nodes and leaves) |
| **Standard components** | reusable pieces with ports: a slider, a plot, a tabs control | `std:` definitions, the Library |

A panel kind is then *a notation + a context + the session leaves its controls bind to*,
and where its drawing must be HTML, a small set of standard widgets the notation places
(text fields, colour pickers, the easing field) rather than bespoke Svelte. The timeline
is the working proof of the pattern: a plan generates a session composition, the canvas
draws it in the `timeline` notation, `std:timeline-*` contexts give it gestures, and
its DOM column is the only HTML.

## 2. A selector, from the parts

Blender's Properties editor shows a vertical strip of tabs (tool, render, output, scene,
object, modifiers, material…): the categories the selection has, one chosen, the panels
below switching with it. Decomposed:

1. **The options** — a list that depends on context: a **view field** resolved from
   contexts (as `inspector.sections` lists the inspector's sections for the selection's
   view, and `palette.tools` a view's tools).
2. **The choice** — per editor, not saved in the document: a **session leaf** (or, for a
   strip that switches dock views, the dock's own active view, which is session state the
   dock already keeps per layout).
3. **The control** — renders the options and sets the choice: a standard tabs component
   (`std:tabs`: an `items` input, a choice `value` bound to the session leaf), drawn by
   the canvas in a composition, or by its standard HTML widget beside a DOM view.

Nothing in it is particular to Blender's case, so the same three parts make an inspector
section strip (options: `inspector.sections`; choice: the inspector view's `tab` leaf),
the left column's view strip (§5), or a lesson's tabbed figure (options: the author's
list; choice: a component input a narrative can set).

## 3. Toward agent-made panels

What an agent needs to make a panel, in order of how much exists:

1. **A context** with the panel's view fields and gesture rules — exists (the model's
   `<context>` verb; rules are validated against the field registry).
2. **A source** — a document composition it builds (exists), or a generated session
   composition (exists for the timeline in Rust; an agent-written generator is open:
   probably a composition whose structure is bound to data, `repeat`, rather than code).
3. **Session leaves** for its choices — the session tier exists; a way for a document's
   context or composition to declare a view's own leaves (`view.tab`) is open.
4. **Standard widgets** for what must be DOM — today each built-in panel is its own
   Svelte component; the path is to move their controls into a widget set notations
   place by data (the inspector's fields are nearly there: `PropertyField` is driven by
   field kinds).

Order of conversion for the built-ins: the strip (§5, done), the inspector's sections
(options already a view field), the Layers tree (already follows a source), the timeline
(already a generated composition; its DOM column last).

## 4. Rules

- **No panel resizes or rearranges itself.** Selection, state editing and agent activity
  never move docks; layouts change only when a person chooses (a preset, a drag).
- **State a view chooses lives in the session tier**, never the document, and survives a
  republish of the layout where the dock keeps it.
- **A strip shows what the context offers, in its order;** a view it names that does not
  exist yet is opened on demand, not pre-created.

## 5. Built: the view selector strip (2026-11-09)

The left column's single tab group hid the agent chat behind its overflow (the layout
council). It now has Blender's strip:

- **Options** are the view field `workspace.strip` (kind `Views`), resolved from the
  built-in context `std:workspace` (`layers model narratives library`); a document
  overrides it like any context (`<context name="std:workspace"><rule match="view"
  workspace.strip="model layers composition:library"/></context>`). Items are a view's
  own name, `notation:source`, or a built-in's short name (`layers model library
  narratives inspector timeline document`; `narratives` is every narrative).
  `Resolver::workspace_values`, the `workspace_strip` command.
- **The choice** is the dock's active view in that group.
- **The control** is a vertical icon strip at the left edge of every view the strip
  offers (`ViewStrip.svelte`): click shows that view in the same group (opening it there
  if it is not open, moving it there if it is open elsewhere); Shift-click opens it in a
  split below, so two can be seen at once (Layers and the chat, or Layers and a lesson).
  A group with a strip shows only its active tab, beside the scope menu.
- Tests: `tests/contexts.rs` (`the_workspace_strip_is_context_data`).

### Hiding a group's tab bar (2026-11-09)

A group's header gains **Hide tab bar** (beside Settings and +), a person's choice saved
with the layout (dockview's `hideHeader`). A hidden group's view starts at the very top;
a small grab pill at its top centre (`GroupGrab.svelte`) stands in for the tabs:

- **Click:** the group's views (to switch), Show tab bar, Close.
- **Drag:** this dockview build drags with its own pointer controller (not the browser's
  drag), which it does not export, so the pill hands the press to the group's hidden
  active tab (with Alt, to the header's empty part, which drags the whole group) and
  dockview runs its usual drag: threshold, ghost, drop zones, split or move.
- **The scope menu** sits in the leftmost top tab bar that is showing, so hiding the
  left side's bar moves it to the centre's, beside the document's composition tabs.
- **Hover:** the visible pill retracts to a small tab when idle and grows over
  150 ms when approached. Its button keeps one fixed hit area, and the invisible
  menu anchor never receives pointer events; this prevents the pill and cursor
  from alternating under a stationary pointer.

### The default layout (2026-11-09)

The layout council's sizes: the sides run the full height and the timeline sits under the
canvas only (1680×1000: left 340, Inspector 300, timeline 250; `baseLayout` scales them
with the window), and the left side's tab bar starts hidden, since its strip switches its
views; the scope menu then sits beside the composition tabs. A saved layout keeps its own
arrangement (Settings → Reset layout gives the new one).

## 6. Not yet

- `std:tabs` as a document component (the strip is its HTML widget only).
- The inspector's section strip; a view's own session leaves declared by data.
- Workspace presets (Build, Write, Animate, Present) as saved layouts chosen on purpose.

### Frontend refresh (2026-11-13)

The selector uses a white rail in light mode, distinct from the gray composition
tab background. One workspace-level Settings menu sits at its lower-left foot,
remaining reachable when left views switch or close. It offers System/Light/Dark
appearance and opens a Settings modal above the workspace. Its sidebar selects
Appearance, Performance, Workspace or Developer, with controls on the right and
independent content scrolling. Bits UI Dialog manages modal focus and dismissal;
vertical Tabs provide keyboard section navigation. Close and Escape return focus
to the lower-left settings button without changing the dock layout. Canvas and
timeline shortcuts stay idle while the modal is open, including when focus is
in a portalled select menu. At narrow
widths, controls stack within the content column. The settings dock notation and
its tab-opening path were removed. Tab groups no
longer repeat the app-settings gear or offer duplicate Settings views in their
add-view menus. Composition run/FPS/render-inspector menus and agent settings
retain their contextual access.

Fresh layouts hide the Inspector and Timeline tab bars as well as the left panel's.
Existing saved layouts retain the person's choice; grab menus still switch views,
drag panels and restore tab bars. Layout and app preferences remain session/browser
state, without authored document edits.

`frontend-refresh.ts` checks the menu, theme shortcuts, sectioned settings modal, keyboard dismissal, unchanged dock layout,
narrow-window sizing, left-view switching and hidden-header recovery at DPR 1 and 2. Light/dark screenshots
are saved under `.agents/frontend-refresh/dpr<N>/`. Frontend checking and fast smoke
pass; timeline rendering verification is recorded in `timeline.md`.

The modal follow-up passes the refreshed DPR 1/2 fixture, fast smoke and frontend
checking (zero errors, 11 existing warnings) after rebasing onto the 2D transform
controls work. The WebGL2 build and all 15 timeline Rust tests pass on that base.

Settings visual polish: the dialog is 800×480 CSS px at full size, with a softly
tinted sidebar, a raised selected section and consistent 16px header/sidebar
insets. Section titles, setting labels and descriptions use a clear 20/14/13px
hierarchy in the shared UI font. Settings use label/description rows without
horizontal rules, aligned 32px controls, larger switches and concise help copy.
Build details use a structured definition list. Light/dark and narrow-window
screenshots pass at DPR 1/2; modal focus, section navigation and document/shortcut
isolation remain covered.
