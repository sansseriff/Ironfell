# Plans: where to start

**Updated:** 2026-11-11 for the 2D view fixes (cheap dots, grid snapping, text edits under a pan, token rebinding). Update this file whenever a plan is added,
finishes, or is superseded.

## 1. Read first (a new agent)

1. **[`roadmap.md`](roadmap.md)**: the stage map (A–I) and its **Status** table, the one
   place that records what is done.
2. **The handoff for your track**, the one place that says what to do next and in
   what order:
   - **Product work** (documents, compositions, tools, signals, lessons):
     [`handoff-product.md`](handoff-product.md). Several tasks are open; it says to ask
     the user which.
   - **Rendering performance** (stage H): [`handoff-render.md`](handoff-render.md) §4.
3. **[`.claude/skills/run-iron/SKILL.md`](../.claude/skills/run-iron/SKILL.md)**: build,
   serve, drive, the smoke script and its expected output.
4. **[`guide2/index.html`](guide2/index.html)**: a readable walkthrough of the system as
   built, refreshed 2026-10-28 against `5334ab8`: Foundations and Parts 1–14,
   including components, caches, layout, contexts, schematics and shared hosts.

Where documents disagree, the higher one in this list wins: roadmap Status, then the
handoffs, then `specs/` and `rendering/architecture.md`, then `vision/`, then `archive/`.

## 2. Layout

| Path | Holds | Changes |
|---|---|---|
| `README.md`, `roadmap.md`, `handoff-*.md` | Entry points: status and work order | Every time work lands |
| `specs/` | What each stage builds, with exit criteria and "Done" notes | As phases land |
| `rendering/` | Current 2D rendering architecture and its measurements | As rendering work lands |
| `vision/` | Why: the architecture's reasoning | Rarely |
| `guide2/` | Generated walkthrough (`guide2/_src/build.py`) | When the system changes enough to mislead |
| `notes/` | Ideas discussed but **not adopted**; do not build from them | Freely |
| `archive/` | Superseded plans and finished handoffs, kept for provenance | Only by adding to it |

## 3. Specs

| Spec | What it specifies | State |
|---|---|---|
| [`agent-vision.md`](specs/agent-vision.md) | Pasted/dropped raster references and GPU composition pictures for the built-in agent | Built on `t3/agent-image-vision`; 2D capture, bounded visual refinement; native/live-provider verification pending |
| [`agent-runtimes.md`](specs/agent-runtimes.md) | Loopback runtime interoperability, Codex bootstrap, Claude CLI adapter, provider selection and 2D authoring tests | Bootstrap built 2026-11-03 in `agent-runtimes` worktree; live Codex create/edit/undo passes in worker/main modes; live Claude, native and full smoke pending |
| [`document-spine.md`](specs/document-spine.md) | `iron_document`: ops, transactions, ids, history, file format | Implemented; the store's rules |
| [`entity-composition-model.md`](specs/entity-composition-model.md) (ECM) | Units, definitions, entities, compositions, ports, lessons | Phases 1–3 done; 4–7 open |
| [`constraint-system.md`](specs/constraint-system.md) (CS) | Solver, constraints, interaction-time causality | Phases 1–3 done; 4–7 open |
| [`select-tool.md`](specs/select-tool.md) | Select and pen tool semantics | First cut done; "Not yet" list open |
| [`svg-import-export.md`](specs/svg-import-export.md) | SVG export and import, compound paths, stroke style | Phases 1–4 done; initial opacity, two-stop gradient fills and group clip frames built; follow-ups in §8 |
| [`clipboard-and-assets.md`](specs/clipboard-and-assets.md) | Copy, paste, duplicate; the clipboard; assets and drag and drop | Phases 1–2 done (copy closure, Cmd+D, Cmd+C/X/V, Alt-drag); assets wait on the project container (§6) |
| [`prompt-topics.md`](specs/prompt-topics.md) | The agent prompt as a core plus topics chosen by document and request, the rest on `<topic>` request | Built 2026-11-09 |
| [`panel-primitives.md`](specs/panel-primitives.md) | Panels from the same parts as everything else (source, notation, context, session state, standard components); the view selector strip | Strip built 2026-11-09 (§5); `std:tabs`, inspector sections strip, presets open |
| [`panels-and-session.md`](specs/panels-and-session.md) | Views, dock presenter, session tier, inspectors | Items 1–2 built; composition canvas settings added 2026-11-10; selection-wide mixed-value properties, grid snapping and cheap dots 2026-11-11; 3–5 open |
| [`view-textures.md`](specs/view-textures.md) | Every view renders to its own texture | Done except the custom final pass and §4 tiling |
| [`free-running.md`](specs/free-running.md) | When a view renders every frame: live sources, the run mode (auto / always / on demand), bubbling through embeds; the settings menu; the Stats composition that replaced the `bevy_ui` overlay | Approved and built 2026-10-18 |
| [`infinite-canvas-architecture-patterns.md`](specs/infinite-canvas-architecture-patterns.md) | Gesture ownership, typed UI protocol, bounds, overlays, chunks | Phase 3 done; phase 1 done (store ownership, one pointer state machine, the 3D gizmo); phase 2 done 2026-10-26 (typed protocol, shared input routing, lifecycle and scoped reads); 4–5 in the render handoff |
| [`text.md`](specs/text.md) | Text nodes, fonts (catalog, on-demand loading, installed fonts natively), glyph rendering, editing, `fmt` readouts, the Text inspector | Scaffold done 2026-10-17; §9 lists what is open |
| [`component-authoring.md`](specs/component-authoring.md) | Making components from the UI: mains, the port and empty tools, links and lines, the joint handle, the Library tab | Approved and built 2026-10-18 |
| [`component-model.md`](specs/component-model.md) | Components: authored vs expanded instances, ports as the one interface, overrides, scope, libraries, render analysis; amends ECM §4, §6.1, §9 | Approved 2026-10-18; phases 0–4 done (the `Facet` rename; `in` ports replace parameters; linked instances, schema 7; overrides, edit translation, scope; anchors and empties; make component with nesting; plugging, mates and nested overrides); 7 done 2026-11-06 (the model's `<place>`, `<make-component>`, `<look expand>`). 5 waits on the project container, 6 is render-track work |
| [`snapping.md`](specs/snapping.md) | Object snapping and smart guides in 2D compositions (Figma's edges and centres), the core in `iron_spatial::snap` | Phases 1–2 and most of 3 built 2026-10-20 (moving, resizing, path points, the pen); the shape tools, 4–5 open |
| [`layout-and-tables.md`](specs/layout-and-tables.md) | A layout engine for 2D compositions (Taffy inside `iron_layout`: stacks and grids on groups, published by a layout producer), tables on it (rows of text cells, merges, boxed and light presets, the `<table>` model op and compact view, spreadsheet paste), | Phases 0–3 done 2026-10-26 (style inheritance deferred) |
| [`interaction-lifecycle.md`](specs/interaction-lifecycle.md) | Shared removal meanings and operation planning, typed context consumers, one interaction coordinator/lifecycle, minimal scope warnings and explanation provenance | Direction agreed 2026-10-28; phased implementation plan proposed, no code changes |
| [`authoring-workflows.md`](specs/authoring-workflows.md) | Design investigation: path-to-fiber authoring, related representations and embeddings, computation bindings and pedagogical context scopes | First shared point-control and port-exposure slice built 2026-11-03 (§10); broader workflow/panel design remains open |
| [`contexts.md`](specs/contexts.md) | **Master plan**: Iron's behaviour as data. The engine holds mechanisms; human-interface choices (interaction, motion, chrome, keymaps, tolerances, snapping, palettes and defaults, inspector layout, enum domains, port domains, the standard library, workspace) are context rules, built-in ones never saved, a notation a bundle of them; the shared foundation, the domains in order, and the work order | Steps 0–8 built 2026-10-26/27 (step 7 as chrome fields, 8 as `schematic.md`); the rest proposed |
| [`affordances.md`](specs/affordances.md) | Interaction and motion as context data: the engine holds mechanisms (resolver, drag kinds, keyboard models, one interpolator); behaviour, built-in defaults included, is contexts of rules (a user-agent stylesheet, never saved; a composition's `notation` names one); gestures drive a node's own leaf through the existing bindings; interaction state and declared transitions; prior art; phases A–D and five demos | Phases A–C built 2026-10-26/27 (track along a path deferred); D proposed |
| [`chrome.md`](specs/chrome.md) | The editor's chrome (selection boxes, grips, outlines, guides, points, ports, previews, caret) as pictures of session state: built-in chrome definitions chosen by context rules, attached in screen pixels, with affordances (handle priority becomes stacking) and declared motion | Revised and built 2026-10-27 as context fields the overlay reads (accent, grips, hover, lit, part handles); chrome as nodes waits for the schematic |
| [`schematic.md`](specs/schematic.md) | A schematic notation: electrical parts as markup becoming definitions (pins are `electrical` ports, behaviour equations), the wire tool (connections drawn as bound paths, junctions), live DC values from the network solve, the model's `<circuit>` and netlist | Phases S1–S5 built 2026-10-27; later items open |
| [`packed-paths.md`](specs/packed-paths.md) | A path's anchors packed into one `path.d` value by default; `point` nodes only when editing, binding or constraining them. Fixes the out-of-memory abort on large SVG imports | Approved and built 2026-10-23 (phases 0–4); the Prussia SVG imports. Follow-ups in §9 |
| [`render-cache.md`](specs/render-cache.md) | Chunks (vector subgroups) per composition, raster layers of 256 px tiles at √2 scale buckets drawn stretched during zoom, damage and promotion for edits, the automatic heuristic; vector retention for live layers if measurement asks | Approved 2026-10-23; phases 0–2 built (§10: idle frames 10.7 → 0.3 ms; a fitted Prussia pan 177 → 1.2 ms from raster tiles); phase 3 only if the inspector shows the need |
| [`render-inspector.md`](specs/render-inspector.md) | A debug view that observes another 2D view: chunk tints, its viewport box and culling, live layers, tiles by state, a legend | Approved 2026-10-23; phases A and B built (chunk colours, viewport and culling, raster squares by state, legend) |
| [`instance-cache.md`](specs/instance-cache.md) | Drawing components by what each part depends on (component-model §11): static, keyed and live parts; groups moved rigidly; equal instances sharing drawings and tiles; SVG repeats as references to definitions | Approved and built 2026-10-24 (phases 0, A–D, §7): the Prussia near-whole drag 235.5 → 0.6 ms; Prussia imports as 14,407 nodes (was 42,667). Open: incremental expansion |
| [`model-interop.md`](specs/model-interop.md) | External computational Models: jobs, live sessions, optional local acceleration; shared web/native data and lifecycle semantics | Proposal drafted 2026-10-16; pending user approval. No implementation; conditional work order in `handoff-product.md` §4.2 |
| [`scopes.md`](specs/scopes.md) | Durable local storage and global identity: scope ids, schema 9, snapshot plus op log in OPFS, boot into the last scope, the scope menu, workspace and assets per scope; replaces `clipboard-and-assets.md` §6 | Approved 2026-11-02; **phases 1–2 done 2026-11-02** (§10: durable OPFS scopes, boot into the last scope, `&demo` for scripts); **phase 3 done 2026-11-03** (the scope menu, §10.1); phases 4–5 (workspace, assets) later. Background in `notes/narrative-layer-and-storage-primitives.md` |
| [`narrative-panel.md`](specs/narrative-panel.md) | ProseMirror narrative, references and model block edits; shared T3-inspired Model/Lesson conversation components and layout modes | Narrative built 2026-11-03 (§7–8); shared chat built in `agent-runtimes` (§9): worker/main UI tests and real Codex prose/visual/edit pass; full smoke/native pending |
| [`component-3d.md`](specs/component-3d.md) | Components pictured in 3D compositions: `use` placed by its composition's dims (`transform3d`), 3D templates, `<place>` in 3D | Proposed 2026-11-06; waits on the user |
| [`layers-panel.md`](specs/layers-panel.md) | A Figma-style layers panel: one composition's drawing tree per panel (following the view or pinned), rows keyed by the select tool's chain, entity chips linking to the other compositions that picture an entity, Show in that opens the tab and selects, HTML with virtualised rows | Proposed 2026-11-06; phase 1 (browse and select) built 2026-11-06 (§12); phases 2–4 open |
| [`agent-streaming.md`](specs/agent-streaming.md) | Live agent progress: one event stream from provider to chat (phases, reasoning summaries, streamed prose, Stop, idle timeout), a T3-style work log, and partial-edit previews through provisional store transactions | Phases 1–2 built 2026-11-06 (§6: live work log, Stop, streamed prose and forming edit, idle timeouts); 3–4 (partial-edit previews) proposed |
| [`timeline.md`](specs/timeline.md) | The timeline as a session composition: what a clip is, a session `clock` node, `axis` leaves fixed during drags, generated rows by object and property bound to clips, `std:timeline`, retiming, explicit clip creation | Phases 1–2, most of 3 and Animate change (4) done 2026-11-03 (§11); snapping, easing curves, ends at the playhead, phase 5 next |
| [`plot.md`](specs/plot.md) | Series values (numbers with a unit), broadcasting floor functions and lambdas (`x => …`); paths bound to SVG path data (`points`, `circles`, `segments`); instances with their own size that reflow on a corner drag; the `std:plot` standard component | Phases 1–3 built 2026-11-08 (§12); `std:plot` is markup (`std/plot.xml`) since 2026-11-09 |
| [`machines.md`](specs/machines.md) | Component state machines, first slice: states as poses over channels, transitions with tweens over a `progress` input; progress driven by a clock clip (a derived band, edited per occurrence) or by the machine itself; the strip/graph timeline panel; how the narrative sets state | Proposed 2026-11-08; waits on the user |
| [`wires-and-formulas.md`](specs/wires-and-formulas.md) | The node graph Iron already has, shown on demand: formulas as the source of truth, purple wires only around the selection, modifier chips on wires (Easy Ease, Map range, Window…) that are calls in the formula, components for depth, no free node canvas; `ease()`, `window()`, `cubic-bezier`, Trim Paths | Proposed 2026-11-08; phase 1 next |
| [`frame-constraints.md`](specs/frame-constraints.md) | Figma's resizing model for people: a `pin` facet per child (left, right, stretch, center, scale) interpreted by the layout step, the Resizable toggle, the constraints widget, an auto-layout section | Proposed 2026-11-08; phases 1–2 (minimal) built 2026-11-08 (§7) |
| [`repeat.md`](specs/repeat.md) | A repeat (a `repeat` facet on a group): one derived copy of its children per element of a series or count, keyed by index or value, entering and exiting through machines; `let` named values; node markup, in which the standard components are written | Phase 1, `let` and node markup built 2026-11-09 (§9): `std:plot` and the other standard components are markup; reworked the same day onto item nodes and live settling (reactivity.md); value keys, enter/exit open |
| [`reactivity.md`](specs/reactivity.md) | One model for values, structure, state and events, after the council review of repeat and let and a study of Svelte 5's runtime: scopes (one expansion for instances, repeat items and conditionals), keys in ids, structure settling with values, errors that propagate, lists, `let` as state with tiers, `on` actions, tracked reads, queries and accumulators | Phases 1–5 built 2026-11-09 (§13): errors propagate, problems as data, item scopes, structure settling with values; lists and records, keys in ids, `when`; state (`let.keep`) and `on` actions; tracked reads (`deref`). Phase 6 part 1 done 2026-11-10: context (`@name`), traces and accumulators, the delayed `std:fiber`. Part 2 done the same day: the `query` node (a slow Model's answer with pending and stale states). Phase 7 part 1 done too: enter and exit (`presence`). Part 2: a parent's machine over its children's (`machine.at`, in machines.md). Drag and wheel actions, the zoomable `std:plot` and a pulsed `std:laser` done too. Open: declared context reads, reorder by drag, `std:plot` split into parts |
| [`themes.md`](specs/themes.md) | Colour tokens with light and dark values: `@name` in formulas, a palette of token nodes, the built-in validated theme added on first use, the theme published by the store, the picker's Tokens section, `std:plot` on tokens | Built 2026-11-08 (step 5) |
| [`types-and-units.md`](specs/types-and-units.md) | A typed component boundary: `port.type` (quantities with display units, `choice(a\|b)`, `bool`, `color`, `series`, `fn(n)`, `\| auto`), checks at set and connect, widgets by type, literals that keep their unit, pin visibility (`port.show`, `port.group`) and its author flow. Prior art for plots beyond `std:plot`: `notes/plot-system-vision.md` | Proposed 2026-11-08; phase 1 with widgets and pins built 2026-11-08 (§9) |
| [`variants-and-imports.md`](specs/variants-and-imports.md) | Lesson variants (`extends`, differences only, re-applied when the base changes); `iron://owner/package@range` imports, pinned and copied in with a hash | Proposed 2026-11-08; syntax only, building waits on narrative moves and the server |

## 4. Rendering

| Document | Use it for |
|---|---|
| [`architecture.md`](rendering/architecture.md) | How 2D rendering works now, what Vello can retain, the decisions, caching contracts and acceptance tests |
| [`render-benchmark.md`](rendering/render-benchmark.md) | In-app measurements and protocol; raw runs in [`benchmarks/`](rendering/benchmarks/) |
| [`renderer-sanity-checks.md`](rendering/renderer-sanity-checks.md) | Build/DPR pitfalls, Canvas2D/ThorVG/standalone Vello controls, renderer-family experiments (§10) |
| [`native-frame-pacing.md`](rendering/native-frame-pacing.md) | Native (macOS) frame loop: where input-to-display latency comes from, what helped and what didn't, measurement traps, open questions |
| [`latency-view.md`](rendering/latency-view.md) | The in-app Latency view (developer mode, `?latency=`), the OS-pointer + screen-capture probe for regression checks, and measured web/native results (2026-11-05) |
| [`worker-frame-hold.md`](rendering/worker-frame-hold.md) | **Planned:** hold the page's frame (SharedArrayBuffer, cross-origin isolation) so heavier worker ticks still land in the DOM's frame; phases, measurements first |

## 5. Archive (do not build from these)

| Document | Superseded by |
|---|---|
| [`handoff-2026-10-11.md`](archive/handoff-2026-10-11.md) | `handoff-product.md` (tasks done) |
| [`vello-hybrid-migration-scope.md`](archive/vello-hybrid-migration-scope.md), [`vello-retention-and-render-plan.md`](archive/vello-retention-and-render-plan.md), [`vello-retained-subtree-renderer-proposal.md`](archive/vello-retained-subtree-renderer-proposal.md) | `rendering/architecture.md`; still the place for their source citations and Zulip quotes |
| [`bevy-vello-hybrid-design.md`](archive/bevy-vello-hybrid-design.md), [`bevy-vello-hybrid-open-questions.md`](archive/bevy-vello-hybrid-open-questions.md) | The migration happened; see `rendering/architecture.md` |
| [`deferred-3d-image-target.md`](archive/deferred-3d-image-target.md) | `specs/view-textures.md` |
| [`single-canvas-migration.md`](archive/single-canvas-migration.md) | Implemented; then `view-textures.md` and `panels-and-session.md` |
| [`hybrid-ui-layout.md`](archive/hybrid-ui-layout.md) | `specs/panels-and-session.md` |
| [`perf-grid-runbook.md`](archive/perf-grid-runbook.md) | The 5K frame-skip investigation; its trace method is still cited |
| [`bevy-frame-scheduling.html`](archive/bevy-frame-scheduling.html) | A reference map of Bevy's schedule |
| [`guide-spine/`](archive/guide-spine/) | `guide2/` |

## 6. Vision

[`vision/`](vision/) 01–09. Start with 01 (layered architecture), 02 (substrate, model,
projection), 07 (2D backends) and 09 (entities, compositions and ports). 03 reconciles
the vocabulary: "Projection" is the ontological term; the rendering stage is "draw
intent" or the "presentation model". Vision 07's current-state sections are superseded
by `rendering/architecture.md`.

[`08_native-target-via-tauri.md`](vision/08_native-target-via-tauri.md) was revised
2026-10-16: the macOS feasibility implementation is now in the main checkout on
`document-spine`, transferred from the `native-tauri` worktree at the user's request.
The standalone composition probe passes dev/packaged checks; the packaged workspace
passes 15 checks and the 100,000-runtime-entity concurrency fixture passes 17.
The shared typed UI protocol, input dispatch, session lifecycle and scoped reads
landed 2026-10-26; the rebuilt packaged workspace now passes 22 checks, including
a real webview reload. Human input/display testing and comparison gates remain. A bounded experiment no longer waits
for raster caching. Its work order is in `handoff-product.md` §4.1; the full port
and other platforms remain unscheduled.

[`10_engine-boundary.md`](vision/10_engine-boundary.md) (2026-10-17) settles who
computes world transforms (Iron, for 2D and 3D; Bevy consumes), what a Bevy entity is
for (the working set and 3D renderables, by presentation mode), and how values return
from Bevy and Models (runtime signals, never per-frame ops). It answers CS I5 and
roadmap §5's ownership question and gives a phased work order in its §7. Phases 1
(the group-local spatial scene), 2 (flat entities; Bevy consumes Iron's transforms),
3 (entities only for the Live working set) and 4 (the runtime signal layer and the
piston demo's gas producer) landed 2026-10-17, with the Bevy 0.20 / `vello_gpu` 0.3
upgrade between 3 and 4; phase 5 (chunks and bakes) is `handoff-render.md` §4 item 4.

The Model interop proposal in [`specs/model-interop.md`](specs/model-interop.md)
clarifies that bulk computed results enter the engine independently of the Svelte
UI transport. It proposes two portable behavior contracts and optional local
acceleration. It is **not adopted**; wait for user approval before implementation.

## Keeping this tidy

- Record completion in `roadmap.md`'s Status table and the spec's "Done" notes, not in a
  new document.
- Order work only in the two handoffs. Specs describe phases and exits; they point to a
  handoff for sequence.
- Give every plan a `**Status:**` line under its title and update it when work lands.
- When a document is superseded, add a "Historical" banner at its top, `git mv` it to
  `archive/`, rewrite links to it, and move its row here.
- A finished handoff goes to `archive/handoff-<date>.md` and a fresh one replaces it.
