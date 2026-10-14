# Plans: where to start

**Updated:** 2026-10-14, at `bba1ee3` on `document-spine`. Update this file whenever a
plan's status changes, a plan is added, or one is superseded.

Plans accumulated faster than they were retired, so several overlap. This index says
which ones are current. Where two documents disagree, the one listed higher wins.

## 1. Read first (a new agent)

1. **[`roadmap.md`](roadmap.md)** — the stage map (A–I) and the **Status** table. The one
   place that records what is done.
2. **The handoff for your track** — the one place that says what to do next:
   - **Product/document work** (stages F phase 4–5, G, I):
     [`handoff.md`](handoff.md). Its §2 code map, §3 decisions and §5 conventions are
     current. Its §4 task list is **finished**; the next step is ECM phase 4
     (consistency and encapsulation) or phase 5 (time-domain signals, the modulator
     demo). Ask the user which.
   - **Rendering performance** (stage H): [`handoff-render-perf.md`](handoff-render-perf.md),
     §4 for the work order, §5 for the measuring traps.
3. **[`.claude/skills/run-iron/SKILL.md`](../.claude/skills/run-iron/SKILL.md)** — build,
   serve, drive, smoke script and its expected output.
4. **[`guide2/index.html`](guide2/index.html)** — a readable walkthrough of the system as
   built, including the rewritten foundations (store, changes, bindings, bridge).

## 2. Specifications for current and next stages

| Plan | What it specifies | State |
|---|---|---|
| [`document-spine.md`](document-spine.md) | `iron_document`: ops, transactions, ids, history, file format | Implemented; the store's rules |
| [`entity-composition-model.md`](entity-composition-model.md) (ECM) | Units, definitions, entities, compositions, ports, lessons | Phases 1–3 done; 4–7 open |
| [`constraint-system.md`](constraint-system.md) (CS) | Solver, constraints, interaction-time causality | Phases 1–3 done; 4+ open |
| [`select-tool.md`](select-tool.md) | Select and pen tool semantics | First cut done; "Not yet" list open |
| [`panels-and-session.md`](panels-and-session.md) | Views, dock presenter, session tier, inspectors | Partly built (see its "Built so far") |
| [`view-textures.md`](view-textures.md) | Every view renders to its own texture | Done except the custom final pass and §4 tiling |
| [`infinite-canvas-architecture-patterns.md`](infinite-canvas-architecture-patterns.md) | Gesture ownership, typed UI protocol, bounds, overlays, chunks | Phase 3 slice done (§13); phases 1–2 open, independent of rendering |

## 3. Vision (why; rarely changes)

[`vision/`](vision/) 01–09. Start with 01 (layered architecture), 02 (substrate, model,
projection), 07 (2D backends and the hybrid architecture) and 09 (entities, compositions
and ports). 03 reconciles the vocabulary of 01 and 02: "Projection" is the ontological
term; the rendering stage is "draw intent" or the "presentation model".

## 4. Rendering reference (evidence behind `handoff-render-perf.md`)

| Plan | Use it for |
|---|---|
| [`render-benchmark.md`](render-benchmark.md) | Measurements, protocol, raw data in [`benchmarks/`](benchmarks/) |
| [`renderer-sanity-checks.md`](renderer-sanity-checks.md) | Build/DPR pitfalls, Canvas2D/ThorVG/standalone Vello controls, renderer-family experiments (§10) |
| [`vello-retained-subtree-renderer-proposal.md`](vello-retained-subtree-renderer-proposal.md) | Correctness constraints and acceptance tests for caching; read its **Decision** block first |
| [`vello-retention-and-render-plan.md`](vello-retention-and-render-plan.md) | What Vello does and does not retain; Zulip sources |
| [`vello-hybrid-migration-scope.md`](vello-hybrid-migration-scope.md) | `vello_hybrid` internals, transform/caching taxonomy, upstream trajectory |

## 5. Historical (do not build from these)

Kept for provenance. Each is superseded or already implemented.

| Plan | Superseded by |
|---|---|
| [`bevy-vello-hybrid-design.md`](bevy-vello-hybrid-design.md) | Its "NO-GO" header predates the migration; Hybrid is now the default backend. See vision 07 and the migration scope |
| [`bevy-vello-hybrid-open-questions.md`](bevy-vello-hybrid-open-questions.md) | Same |
| [`deferred-3d-image-target.md`](deferred-3d-image-target.md) | `view-textures.md` |
| [`single-canvas-migration.md`](single-canvas-migration.md) | Implemented; then `view-textures.md` and `panels-and-session.md` |
| [`hybrid-ui-layout.md`](hybrid-ui-layout.md) | `panels-and-session.md` (dockview workspace) |
| [`perf-grid-runbook.md`](perf-grid-runbook.md) | The 5K frame-skip investigation; its trace method is still cited by `renderer-sanity-checks.md` |
| [`bevy-frame-scheduling.html`](bevy-frame-scheduling.html) | A reference map of Bevy's schedule, not a plan |
| [`guide/`](guide/) | `guide2/` (its Foundations part rewrites the spine guide against current code) |

## 6. Notes

[`notes/`](notes/) holds ideas discussed but **not adopted**. Do not build from them until a
plan adopts them.

## Keeping this tidy

- Record completion in `roadmap.md`'s Status table, not in a new document.
- Keep "what to do next" only in the track handoffs. Other plans may describe work,
  but should point to a handoff for its order.
- Give every plan a `**Status:**` line under its title, and update it when work lands.
- When a plan is superseded, add a one-line pointer at its top and move its row to §5.
