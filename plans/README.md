# Plans: where to start

**Updated:** 2026-10-23 on `master`. Update this file whenever a plan is added,
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
   built, rebuilt 2026-10-17 with Parts 10–13 on the engine boundary (vision 10).

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
| [`document-spine.md`](specs/document-spine.md) | `iron_document`: ops, transactions, ids, history, file format | Implemented; the store's rules |
| [`entity-composition-model.md`](specs/entity-composition-model.md) (ECM) | Units, definitions, entities, compositions, ports, lessons | Phases 1–3 done; 4–7 open |
| [`constraint-system.md`](specs/constraint-system.md) (CS) | Solver, constraints, interaction-time causality | Phases 1–3 done; 4–7 open |
| [`select-tool.md`](specs/select-tool.md) | Select and pen tool semantics | First cut done; "Not yet" list open |
| [`svg-import-export.md`](specs/svg-import-export.md) | SVG export and import, compound paths, stroke style | Phases 1–4 done; initial opacity, two-stop gradient fills and group clip frames built; follow-ups in §8 |
| [`clipboard-and-assets.md`](specs/clipboard-and-assets.md) | Copy, paste, duplicate; the clipboard; assets and drag and drop | Phases 1–2 done (copy closure, Cmd+D, Cmd+C/X/V, Alt-drag); assets wait on the project container (§6) |
| [`panels-and-session.md`](specs/panels-and-session.md) | Views, dock presenter, session tier, inspectors | Items 1–2 built; 3–5 open |
| [`view-textures.md`](specs/view-textures.md) | Every view renders to its own texture | Done except the custom final pass and §4 tiling |
| [`free-running.md`](specs/free-running.md) | When a view renders every frame: live sources, the run mode (auto / always / on demand), bubbling through embeds; the settings menu; the Stats composition that replaced the `bevy_ui` overlay | Approved and built 2026-10-18 |
| [`infinite-canvas-architecture-patterns.md`](specs/infinite-canvas-architecture-patterns.md) | Gesture ownership, typed UI protocol, bounds, overlays, chunks | Phase 3 done; phase 1 done (store ownership, one pointer state machine, the 3D gizmo); phase 2 a product candidate; 4–5 in the render handoff |
| [`text.md`](specs/text.md) | Text nodes, fonts (catalog, on-demand loading, installed fonts natively), glyph rendering, editing, `fmt` readouts, the Text inspector | Scaffold done 2026-10-17; §9 lists what is open |
| [`component-authoring.md`](specs/component-authoring.md) | Making components from the UI: mains, the port and empty tools, links and lines, the joint handle, the Library tab | Approved and built 2026-10-18 |
| [`component-model.md`](specs/component-model.md) | Components: authored vs expanded instances, ports as the one interface, overrides, scope, libraries, render analysis; amends ECM §4, §6.1, §9 | Approved 2026-10-18; phases 0–4 done (the `Facet` rename; `in` ports replace parameters; linked instances, schema 7; overrides, edit translation, scope; anchors and empties; make component with nesting; plugging, mates and nested overrides). 5 waits on the project container, 6 is render-track work |
| [`snapping.md`](specs/snapping.md) | Object snapping and smart guides in 2D compositions (Figma's edges and centres), the core in `iron_spatial::snap` | Phases 1–2 and most of 3 built 2026-10-20 (moving, resizing, path points, the pen); the shape tools, 4–5 open |
| [`packed-paths.md`](specs/packed-paths.md) | A path's anchors packed into one `path.d` value by default; `point` nodes only when editing, binding or constraining them. Fixes the out-of-memory abort on large SVG imports | Approved and built 2026-10-23 (phases 0–4); the Prussia SVG imports. Follow-ups in §9 |
| [`render-cache.md`](specs/render-cache.md) | Chunks (vector subgroups) per composition, raster layers of 256 px tiles at √2 scale buckets drawn stretched during zoom, damage and promotion for edits, the automatic heuristic; vector retention for live layers if measurement asks | Approved 2026-10-23; phases 0–1 built (§10: idle frames 10.7 → 0.3 ms, pans re-record nothing); phase 2 next |
| [`render-inspector.md`](specs/render-inspector.md) | A debug view that observes another 2D view: chunk tints, its viewport box and culling, live layers, tiles by state, a legend | Approved 2026-10-23; phase A built (chunk colours, viewport and culling, legend); phase B with render-cache phase 2 |
| [`model-interop.md`](specs/model-interop.md) | External computational Models: jobs, live sessions, optional local acceleration; shared web/native data and lifecycle semantics | Proposal drafted 2026-10-16; pending user approval. No implementation; conditional work order in `handoff-product.md` §4.2 |

## 4. Rendering

| Document | Use it for |
|---|---|
| [`architecture.md`](rendering/architecture.md) | How 2D rendering works now, what Vello can retain, the decisions, caching contracts and acceptance tests |
| [`render-benchmark.md`](rendering/render-benchmark.md) | In-app measurements and protocol; raw runs in [`benchmarks/`](rendering/benchmarks/) |
| [`renderer-sanity-checks.md`](rendering/renderer-sanity-checks.md) | Build/DPR pitfalls, Canvas2D/ThorVG/standalone Vello controls, renderer-family experiments (§10) |

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
Human input/display testing and comparison gates remain. A bounded experiment no longer waits
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
