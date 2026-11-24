# Handoff: Building the Unified Engine

**Date:** 2026-11-23 *(revised after reverting universal patch algebra `EN-3`–`EN-5c`)* · **Branch:** `master` · **Keys:** `EN-n` (`plans/README.md`, "Work keys") · **Status:** Active.

**What changed on 2026-11-23:** The universal N-axis patch algebra (`EN-3`–`EN-5c`, preserved on `archive/universal-patch-experiment`) added +11,800 net lines without deleting `Docs::copies` or shipping user-facing variation features, and was reverted in `cc9f380`. Read [`notes/postmortem-universal-patch-algebra.md`](notes/postmortem-universal-patch-algebra.md) first so we never repeat that mistake. Retired keys `EN-3`–`EN-6b` are never reused; new work continues under `EN-12`–`EN-16` alongside existing keys `EN-7`–`EN-11`.

## 0. Read first

1. This file.
2. [`notes/postmortem-universal-patch-algebra.md`](notes/postmortem-universal-patch-algebra.md) — why the universal patch algebra was reverted and the guardrails it establishes.
3. [`specs/engine.md`](specs/engine.md) — target architecture (4 orthogonal layers, recorded-read graph & `settle(mode)`, demand/time/host, settled decisions E1–E11).
4. [`specs/patches.md`](specs/patches.md) — the grounded discrete-variation, semantic-provenance, and node-inspectability plan (`EN-14`–`EN-16`).
5. [`.claude/skills/run-iron/SKILL.md`](../.claude/skills/run-iron/SKILL.md) — build, test, smoke, and durations.

## 1. Baseline (Stage 1 complete at `05df865` / `cc9f380`)

Everything in Stage 0 and Stage 1 is built, green, and stays:

- **In-place journaled transactions & read boundary** (`Document` rollback journal, `Tree`, `Changes`, `Store::take_changes()`).
- **Recorded-read graph, single write channel & `settle(mode)` (`EN-1`):** `structure::Key`, journaled per-root `ScopedIndexes`, keyed `Wiring` (`Cx::graph()` deleted), `sync_touched`, `MakerCx`, choke-point canaries.
- **Per-root Makers (`EN-2a`, `EN-2b`, `EN-2c`):** Layout & Pins makers, Constraints & Contexts makers, Network & DerivedBinding makers — all legacy layout/grouping/network hand rules deleted.
- **No refusal after derivation** (`18e3e61`) & **Order-free producer priority** (`95ff421`, `b0e2c68`).
- **Composition clocks & visibility gate v1** (`clocks.rs`, `run = always` override) + `work` choke-point & scaling probes.

## 2. Master Work Order

Every phase lands committed, pushed, and recorded in §2 here and `roadmap.md`'s Status table. Major milestones go through the council (`AGENTS.md`). Per `plans/README.md`, keys are never reused or renumbered.

### Completed & Retired Keys

| Key | Stage | Summary | State |
|---|---|---|---|
| **EN-0** | 0 · Stabilize | Round-3 review fixes (`b908d7b`) | **Done 2026-11-22** |
| **EN-1** | 1 · Spine | One write channel, locked-down `Cx`, scoped indexes, `settle(mode)` (`7a5840e`) | **Done 2026-11-22** |
| **EN-2a** | 1 · Spine | Per-root Layout & Pins makers (`6e5511e`) | **Done 2026-11-22** |
| **EN-2b** | 1 · Spine | Per-component Constraints & Contexts makers (`a26395a`) | **Done 2026-11-22** |
| **EN-2c** | 1 · Spine | Per-root Network & DerivedBinding makers (`05df865`) | **Done 2026-11-22** |
| **EN-3**..**EN-6b** | *(Retired)* | Universal N-axis patch algebra experiment (`14e4534`..`85c4c45`, archived on `archive/universal-patch-experiment`, reverted in `cc9f380`; see post-mortem) | **Retired 2026-11-23** |
| **EN-12** | 2 · Lean Core | Flat `Document::solved` (`Op::Solve`, schema 16, solver-free undo/redo/replay/load), split `gesture_preview` & `live_solve_preview`, zero-alloc `NodeId::expanded_parts()`, recorded-read `TemplateCache` (deleted `Document::scopes`, `Owners::solved`/`edited`/`wanted_slots`, `keep_solved`, `apply_recorded`, `release`, `solve::writes`, and `check_owners` solver exemption) | **Done 2026-11-23** |
| **EN-13** | 2 · Lean Core | **Lean Lexical Virtual Views + minimal `ItemTable`:** virtual `Tree` over shared `Arc<Template>` + sparse facets + keyed `ItemTable` (`Arc<[ItemRecord]>`) + derived relations (`Arc<[Relation]>`) + in-place piece updates + single transaction-wide net `ViewDelta` + independent per-owner `NODE_BUDGET` safety cap (deleted `Docs::copies`, `apart`, `with_copies_in_document`, static `Owners` tables, and cross-owner budget accounting) | **Done 2026-11-24** |
| **EN-7** | 2 · Lean Core | **Deleted `regraph` & remaining wiring/repeats on virtual `Tree`:** per-parent `Key::LetsIn` `@name` lookup, `ThemeMaker` with cached light/dark tables, keyed `embeds_in` + post-commit 2D/3D `EmbedTopology` with O(1) `Docs::cyclic_embeds`, `ScopedIndexes` virtual global indexes, O(1) repeat root lookup, and `Scene` empty-source unframed embed restoration (deleted `regraph`, `Rescope`, `after_transaction` classifier, `theme_at`, `REPEAT_INPUTS`, `update_drawn`/`ensure_drawn`; removed 7 rows from `document_bridge::scaling`) | **Done 2026-11-24** |

### Active Work Order (`EN-8` → `EN-9` → `EN-10` → `EN-14` → `EN-15` → `EN-16` → `EN-11`)

| Key | Stage | Builds | Deletes | Net-LOC target / Exit | State |
|---|---|---|---|---|---|
| **EN-8** | 3 · Scaling | **Owner-level activation, `bridge_cone` & epoch version stamps** (split into small sub-steps that each delete their target): epoch stamps + `verified_at`, `ActiveOwners` + `cells_by_owner`, `visited`-set `bridge_cone` (zero per-cell refcounting), scoped pulls | `Store::necessity`, `Reactive::necessity`, `relaxed`, `pinned`, `relax`, stale set, `exact()` (incl. `run` & `engine_api`), `compute_heights` (non-test), `IDLE_SWEEP` | Exact oracle under `observe_all(false)`; constant/structural edit & layout necessity counters flat | **Next** |
| **EN-9** | 3 · Scaling | **Visibility gate & retiring `ROOTED`:** cached node owners, cross-owner read counts, delta clock gating; retire `ROOTED`; captures observe what they draw | `Store::paused` scans, `Clocks::gated`, `Reactive::reader_nodes`, `Docs::reshaped`, `Reactive::wired`, `op::reshapes`, `ROOTED` | All gate & capture tests pass; §8.8 consistency with paused pages passes; playing & gate counters flat | Open |
| **EN-10** | 3 · Scaling | **Host-side scaling:** per-composition embed/control/3D indexes, touched spatial scopes only, per-composition paint order, incremental timeline lane planning, `gas::find` & `interaction` indexes | Host whole-document scans | Every row of `document_bridge::scaling` flat across both axes (store + host); `document_scaling_time` +6 ≤ 1.2× +0 | Open |
| **EN-14** | 4 · Inspect & Vary | **First-class Node Inspectability & Provenance ("Ask the node"):** unified read-only `Store::inspect(node)` returning tree location, semantic derivation (`extends`/instance/slide/take), property provenance stack, incoming drivers (bindings, clips, machines, constraints, layouts), and outgoing consumers; expose in XML `<look id="#…" connections/>` and UI Inspector | Ad-hoc disconnected inspection helpers | Agent and UI can trace any animated/bound/derived component in one query; unit + agent-probe tests pass | Open |
| **EN-15** | 4 · Inspect & Vary | **Small Discrete Variation Layer (~350–500 lines) + Shared-property Machine States:** finite `Ordered`/`Unordered` axes (stable IDs + tombstones), values & `hidden` only, finite-set resolution/arrows/provenance, bubbling ops (`drag`, `widen`, `narrow`, `split`, `reset`); migrate `machine.rs` poses onto unordered local state patches so cyclic/partial-sharing machines work end-to-end | Legacy per-state channel-value storage in `machine.rs`; historical toy crate `src/iron_patch` | Cyclic 3-state triangle machine (`{A, B}` colour + `{B, C}` position) + smooth arrow transitions + bubbling ops + `plot-view.txt` pass; discrete variation core ≤ ~500 lines | Open |
| **EN-16** | 4 · Inspect & Vary | **Decks, Takes, Ghosts & Narrative Arrows:** ordered document axes (`slide`, `take`, narrative joints), `@slide`/`@take` readable by `when` for structural/topological changes, `<place … at="take=2" clock="…"/>` with isolated simulation state for ghosts/chaos comparisons, Inspector & State Graph bubbling/provenance UI | Narrative extent special-cases | Slide deck (`[B, ∞)` vs `{C}` + `when` schematic branch), pendulum take+ghost race on shared clock, chaos comparison, and bubbling UX checks pass | Open |
| **EN-11** | 5 · Storage | **Chunked snapshots & point loading:** revise `specs/patch-storage.md` for the grounded engine (cut premature Postgres index tables), implement content-addressed chunks, Merkle manifests, partial loading | Whole-scope snapshot requirement for large scopes | `patch-storage.md` exits met | Open |

---

## 3. Checkpoints

| When | Decision / Gate |
|---|---|
| End of `EN-12` | Confirm net-negative LOC, solver-free undo/redo/replay/load, strict `gesture_in_progress()`, and recorded-read `TemplateCache`. |
| End of `EN-13` | Confirm `Docs::copies` is completely deleted, net LOC dropped significantly, and read performance matches `views_spike` expectations. |
| End of `EN-7` | **Cohesive Engine milestone:** `regraph`, `Docs::copies`, and all legacy hand refresh rules are gone. |
| End of `EN-10` | **Zero-Slope Scaling milestone:** all store and host scaling probes flat on both axes. |
| End of `EN-14` | Confirm `inspect(node)` / `<look connections/>` makes derivations, animations, bindings, and consumers legible in one query. |
| End of `EN-15` | Confirm discrete variation core stays small (~350–500 lines) and delivers cyclic shared-property machines cleanly before building decks/takes in `EN-16`. |

---

## 4. Immediate Next Step (`EN-13`)

### EN-13: Lean Lexical Virtual Views + `ItemTable` (delete `Docs::copies`)
1. Build lean virtual views directly over shared `Arc<Template>` + flat lexical overrides (`outermost instance > inner instance > variant > definition`) + sparse resolved map for overridden/rewritten virtual nodes.
2. Add minimal keyed `ItemTable` (`repeat view id → Vec<ItemRecord>`) with `item.value`/`item.index` published as producer signals.
3. Wire `Tree` and `ViewDelta` directly into `Reactive`, `Changes`, and `iron_spatial` (including vanished-ID spatial root fallback).
4. Delete `Docs::copies`, `apart`, `with_copies_in_document`, and the bulk of `expand.rs`.

---

## 5. Rules

- **One track, one set of keys (`EN-n`).** Never reuse or renumber retired keys (`EN-3`–`EN-6b`).
- **Scope-creep circuit breaker (`AGENTS.md`):** If a simplification phase reaches **+1,500 net lines** without having deleted the old mechanism it replaces, **stop immediately and check in with the user**.
- **Delete as you go.** A phase is not done until its "Deletes" column is actually gone.
- **One real consumer before generalizing.** Never generalize a new abstraction across 3+ subsystems before one concrete user-facing workflow uses it end-to-end.
- **No new op classifiers or stamps.** Until a family is converted to cells, it refreshes coarsely by touched keys in its scope.
- **Commit and push** each completed phase, updating §2 here and `roadmap.md`'s Status table (`AGENTS.md`).
