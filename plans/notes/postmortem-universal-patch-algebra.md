# Post-mortem: The Universal Patch Algebra Experiment (`EN-3`–`EN-5c`)

**Date:** 2026-11-23 · **Range:** `05df865..85c4c45` (`EN-3`, `EN-4`, `EN-4b`, `EN-5a`, `EN-5b`, `EN-5c`) · **Status:** Warning & architectural guidance for future engine work.

> **TL;DR:** An elegant product idea — unifying slides, takes, shared-property machine states, and narrative arrows as discrete patches with regions — was over-generalized into a universal N-axis symbolic algebra meant to replace *every* variation and overlay mechanism in `iron_document` (instance overrides, `extends` variants, gesture previews, live constraint solves, persisted solver poses, optional facets, and continuous time clips). Across 7 commits, this "simplification" added **+13,399 / -1,599 (+11,800 net lines)** — growing `src/iron_document/src/` by **+6,904 lines (~15%)** — without shipping a single user-facing slide, take, or multi-state pose, without deleting `Docs::copies`, and ending in a council review (`5/10` Sol, `6.5/10` Neon) surfacing 7 categories of High/Critical cross-layer bugs.
>
> **Core lesson:** **UX uniformity is not engine uniformity.** Keep the patch idea — as both a guiding UX model *and* a small, focused discrete-variation layer (~400 lines) for the four things that genuinely share its shape (machine states, decks/slides, takes, and narrative moves/arrows) — and keep lexical instance overrides, ephemeral previews, solver poses, and continuous time in their own tiny, orthogonal mechanisms.

---

## 1. What actually happened (by the numbers)

Starting from clean Stage 1 (`05df865`, `EN-2c`: recorded-read graph, per-root makers, single write channel, `settle(mode)`, 47,366 lines in `src/iron_document/src/`), seven commits implemented `EN-3` through `EN-5c`:

| Scope (`05df865..85c4c45`) | Added | Deleted | Net |
|---|---:|---:|---:|
| `src/iron_document/src/` | +8,320 | -1,416 | **+6,904** (+14.6%) |
| `src/iron_document/tests/` | +4,270 | -71 | **+4,199** |
| `src/iron_patch/` | +608 | -27 | **+581** |
| `plans/` & other | +201 | -85 | **+116** |
| **Total (61 files)** | **+13,399** | **-1,599** | **+11,800** |

Where the production lines went:
- `src/iron_document/src/patch.rs`: **2,213 lines** (symbolic N-axis `PatchTable`, `Sel::Suffix { tail, not }`, anchor-scoped fractional ranks, `Product::minus`, tombstones, `ties`/`contradictions`/`dead`/`stale` lints, `plan_drag`, `ResolvedStore`).
- `src/iron_document/src/engine/views.rs`: **2,049 lines** (`ViewRegistry`, `ViewCoord` scope offsets, `diff_views`, virtual resolution — while `Docs::copies` remained alive).
- `src/iron_document/src/tree.rs`: **+1,457 lines** (dual base-vs-resolved read paths, virtual node lookups, session-patch mixing).
- `expand.rs` (+846/-), `lib.rs` (+812/-), `owners.rs` (+523/-), `document.rs` (+485/-), `template.rs` (+457/-).

At `85c4c45`, **zero** new user-facing variation features existed (no decks/slides, no takes/ghosts, no shared-property machine poses, no narrative arrows), `Docs::copies` was still not deleted, and a two-model council review (GPT-6.2 Sol `5/10`, Gemini 4 Neon `6.5/10`) uncovered 7 clusters of High/Critical defects — almost all caused by forcing unrelated engine mechanisms through one `PatchTable`:

1. **Live solves vs. gesture previews:** `resolve_clusters_live()` wrote into `GESTURE_PREVIEW_PATCH_ID` (`SESSION_LAYER`), leaking live solves across `set_point`, shadowing scratch session patches, and making `gesture_in_progress()` falsely return `true` after `set_time`/`publish`/`set_point`.
2. **P19 time-axis bans vs. save/load:** `PatchOp::SetEntry`/`SetRegion` skipped whole-patch validation, allowing transactions that `save()` wrote and `load()` rejected; time axes had to ban `Present`, `FacetPresent`, `Notation`, and `Slot::Bound`.
3. **Gesture commit & solver-free history:** `commit_gesture` left the session patch active through `cluster_problems()`; undo/redo/replay could still trigger `resolve_clusters_live` and shadow recorded poses with session previews; missing optional facets (`corners`) broke preview/unpreview notifications.
4. **Multi-axis solve-layer release (`(DiscretePoint, Leaf)`):** `post_apply` released solved poses across *all* discrete points based only on constraints active at the *current* point; `settle_repeats` read stale Tier 4 solve entries before `post_apply` cleared them.
5. **`exact` dry-run vs. real apply:** `scratch()` wiped session patches while real `apply_now` kept non-gesture scratch patches; `eval_at` at the live point leaked gesture previews and glides.
6. **Anchor & layer precedence leaks:** `expand::translate`, `plan_drag`, `winner_authored`, `patch_stack`, and `winning_region` had repeated edge-case mismatches between `anchor`, `layer`, and `rank`.
7. **Dual-path maintenance drag:** Every hardening pass (`EN-4b`: +1,152 net, `EN-5b hardening`: +1,182 net) added over a thousand more lines to patch up interactions between axes, anchors, session overlays, and `ViewRegistry`.

---

## 2. What the user actually wanted (preserved from the 2026-11-20 design discussion)

The tragedy of `EN-3`–`EN-5c` is that the **original product intent was sharp, concrete, and genuinely valuable** — and [`plans/specs/patches.md`](../specs/patches.md) gradually stripped out the concrete human scenarios while inflating the formal algebra around them.

These five scenarios are what the design was *actually* trying to make cohesive:

1. **Slide decks that build on themselves ("from here on" vs. "this slide only"):**
   - Slides `A → B → C → D` where `B`, `C`, `D` share features with `A` and build on it cleanly.
   - Modifying `A` flows forward into `B`, `C`, `D` unless a later slide overrides that property (`[A, ∞)`), while a one-slide callout (`{C}`) stays local to `C`.
2. **Multiple takes / loop iterations, ghosts, and chaos comparisons (generalizing After Effects precomps):**
   - Repeated takes of a system where Take 3 builds on Take 2 (flywheel added, speed ×2); editing Take 2 can either modify only Take 2 or flow to all later takes.
   - Placing another take underneath with a ghost style on the same local clock, or placing two sibling variants side-by-side on one clock (a Lorenz chaos comparison differing only by `ρ + 0.001`), each carrying its own simulation state (reset on loop wrap).
3. **Symmetric / cyclic state machines with per-property sharing (no forced single parent):**
   - In a cyclic graph like a 3-state triangle (`A ↔ B ↔ C`), there is no single "parent" state.
   - State `B` may share colour with `A` (`{A, B}`) and share position with `C` (`{B, C}`), so different properties / sub-components follow different sharing rules without forcing a tree hierarchy.
4. **Version-control-like editing & bubbling UX:**
   - Every edit obeys the lens rule: **"a drag always changes what you see."**
   - From there, small explicit region edits let an author **bubble up** a change (widen its region to more states/slides), **push down** (narrow it), or **partially bubble** (split: move colour to `{A, B}` while keeping position on `{B}`), ideally directly across a state graph or slide strip, with clear provenance (*"inherited from Slide A"*, *"Slides C and D override this colour"*).
5. **Narrative / pedagogical "arrows" between states:**
   - Lessons emphasize **arrows** (`x → y`, [`narrative-layer-and-storage-primitives.md`](narrative-layer-and-storage-primitives.md) §3) rather than static silos.
   - An arrow between two states/points corresponds directly to the patches entering and leaving and the diff of resolved values, deriving Keynote Magic Move-style transitions (refined by tweens where wanted).

Notice what is **not** on that list:
- Nobody asked for CSS suffix-exclusion patterns on nested instance paths (`*/o1/j not {*/a/o1/j, */c/o1/j}`).
- Nobody asked to run live mouse-drag previews or live constraint solves through a multi-axis patch table.
- Nobody asked to encode structural facet existence (`FacetPresent`) or continuous keyframe clips into the same symbolic hyperbox algebra.

---

## 3. Root causes: why the universal approach blew up

### 3.1 Five different things forced into one data structure

| Mechanism | True lifetime | Structural legality | Evaluation rate | What broke when forced into `PatchTable` |
|---|---|---|---|---|
| **Instance overrides & `extends` variants** | Saved, authored | Can add/override parts & `use.notation` | On edit | Lexical nesting became `Sel::Suffix { tail, not }` + `anchor` + `ViewCoord` scope offsets + anchor-scoped rank bugs. |
| **Gesture & live-solve previews** | Ephemeral (milliseconds, never saved) | Constants on leaves only (including absent optional facets) | 60–120 Hz | Conflated live solves with user gestures; broke `gesture_in_progress()`, `commit_gesture`, `scratch()`, and `corners` unpreview. |
| **Persisted solver poses** | Saved, system-owned | Numeric pose leaves only | On transaction | Keying by multi-axis `DiscretePoint` created cross-point P17 release bugs and races with `settle_repeats`. |
| **Continuous time clips** | Saved, authored | Continuous functions of `t`; never structural | 60 Hz | Immediately needed P19 bans on `Present`/`FacetPresent`/`Notation`/`Slot::Bound`, `-0.0` hacks, and stayed on Tier 3 cells anyway. |
| **Discrete authored variation** *(slides, takes, states, narrative moves)* | Saved, authored | Field values & visibility (`hidden`) over small finite sets/intervals | On state/step change | **The one place patches actually belong** — never even got built because the first four consumed 11,800 lines! |

Forcing five mechanisms with incompatible lifetimes and legality rules into one `PatchTable` did not eliminate their differences; it multiplied them into an **N × M interaction matrix** (e.g., *"what happens when a `PatchOp::SetRegion` edits a `FacetPresent` on a `Time` axis inside an anchored nested view while a `SESSION_LAYER` live solve shadows a `DiscretePoint` `Op::Solve`?"*).

### 3.2 Symbolic hyperbox algebra on a finite, tiny domain

`PatchTable` built a general symbolic box-difference solver (`Product::minus`, `within`, `same`, `meets`, `ties`, `contradictions`, `dead`, `axis_keys`, `OrderKey` ranks, suffix-tree exclusion reduction) directly inside `iron_document`'s core read path.

For actual authored discrete variation (a deck of 5–50 slides, a machine of 2–10 states, a loop of 2–20 takes), every axis is a **tiny finite ordered or unordered list**. Checking which patches apply at a point, computing an arrow diff `x → y`, computing blast radius, or checking shadowing is trivial direct iteration — dozens of lines of clear, bug-free Rust instead of 2,200 lines of symbolic geometry.

### 3.3 Encoding structure (`Present` / `FacetPresent`) inside `Tree` resolution

Allowing patches to create/remove arbitrary nodes and optional facets forced `Tree` to maintain dual `base_facet()` vs. effective `facet()` read paths, `ResolvedStore`, `ResolvedDelta`, `FacetPresent(false)` diffing, and P18 enclosing-view rules.

Yet Iron **already has** two clean, working primitives for structure and visibility:
- **Visibility (`hidden`):** an ordinary field on every node. Making a callout appear on Slide C (`[C, ∞)`) or disappear on Slide D is just setting `hidden` (and transitions cleanly with opacity!).
- **Conditional structure (`when` and `extends`):** Iron already built `when` conditionals (`reactivity.md`) for parts that only exist under a condition (e.g. `<group when="{= @take >= 3}">`), and `extends` variants for reusable component structure.

Keeping discrete patches **value-and-visibility-only** eliminates 100% of `Present`, `FacetPresent`, `P18`, `P19`, `P21`, and dual-`Tree` complexity.

### 3.4 Why CSS's cascade was the wrong metaphor for Iron

CSS achieves its expressive cascading selectors because browsers cheat in four ways Iron cannot:
1. **Strictly one-way:** browsers never solve the inverse drag problem (*"which rule across five stylesheets and `:not()` exclusions should be mutated or split?"*).
2. **Never structural:** CSS paints and hides (`display: none`), but never inserts/deletes DOM nodes or swaps templates.
3. **No whole-space symbolic algebra:** browsers match one concrete element at one concrete moment with hash buckets + bloom filters; they never run `Product::minus`, `ties`, or `contradictions`.
4. **No transactions, undo inverses, or live constraint solver.**

Even with those shortcuts, CSS suffers from **action-at-a-distance** — a hidden selector in another scope unexpectedly changing how an element looks. Iron's core design requirement is **legibility and inspectability for both humans and LLMs**: when a person or agent looks at `#plot1`, everything driving `#plot1` (its provenance, its animations, its bindings, and what it drives) should be directly, locally discoverable from `#plot1` without hunting for wildcard rules.

### 3.5 Process miss: no scope-creep circuit breaker

`EN-3`–`EN-5c` crossed +4,000, +8,000, and +11,800 net lines across seven commits while still in "preparation" phases, with each hardening pass adding another ~1,200 net lines. No agent stopped to warn the user that the simplification had inverted into net bloat.

---

## 4. What was worth keeping from the experiment

Not everything learned in `EN-3`–`EN-5c` was wasted:
- **Zero-alloc `NodeId::expanded_parts()`** (`id.rs`): fast, tiny, clean.
- **Recorded-read `TemplateCache`** (deleting `scope_stamp`): cleanly aligns templates with Stage 1's recorded-read graph.
- **`views_spike.rs` benchmark result:** a sparse resolved map over shared `Arc<Template>` reads in 15–22 ns with 0 allocations and uses 2.5% of the memory of materialized `Docs::copies` (0.77 MB vs 31.30 MB at 700 meters × 62 nodes). Virtual views are genuinely a huge win — **when built directly over flat lexical overrides instead of a universal `PatchTable`**.
- **Flat `Op::Solve` for solver-free undo/redo:** storing persisted copy solver poses in one flat `BTreeMap<Leaf, Slot>` on `Document` (without multi-axis `DiscretePoint` keys!) still deletes `Owners::solved`, `keep_solved`, `release`, and `apply_recorded` cleanly.

---

## 5. What the engine structurally demands (the grounded redesign)

To realize the user's actual 2026-11-20 vision — **slides, takes/ghosts, cyclic shared-property state machines, narrative arrows, and bubbling UX** — with a small, fast, rock-solid engine, split the architecture into **four crisp, orthogonal layers** and put **Patches** in the one place where they shine:

### Layer A — Lexical Component Structure & Lean Virtual Views *(core engine)*
- **Owns:** Definitions, `extends` variants, `<place>` instances, direct instance overrides (`override` / flat `BTreeMap` keyed by local part + leaf, plus `use.notation`), and `ItemTable` for `repeat`/`when`.
- **Rule:** Simple lexical precedence: `outermost instance override → inner instance override → variant override → definition base`.
- **Why:** Lexical nesting is a tree, not an N-axis suffix-pattern language. Virtual views (`Arc<Template>` + sparse resolved map) become ~500 lines instead of ~4,000 and cleanly **delete `Docs::copies` and the bulk of `expand.rs`**.

### Layer B — Ephemeral Interaction & Solver State *(core engine)*
- **Owns:** Three tiny, completely separate tables (~120 lines total):
  1. `gesture_preview: BTreeMap<Leaf, Value>` — owned strictly by `ActiveGesture`; cleared atomically on cancel/commit; `gesture_in_progress()` checks only this; handles missing optional facets cleanly.
  2. `live_solve_preview: BTreeMap<Leaf, Value>` — owned strictly by `resolve_clusters_live`; cleared on transactions/point changes.
  3. `Document::solved: BTreeMap<Leaf, Slot>` (via `Op::Solve`) — flat, saved, cleared when an authored edit moves that leaf or its last holding constraint is deleted; undo/redo/replay never re-solve.

### Layer C — Continuous Time, Clocks & Simulation *(already built in Stage 1)*
- **Owns:** The clock tree (with visibility gate), clips (`ProducerId::CLIPS`), physics/networks (`ProducerId::NETWORK`), layout (`ProducerId::LAYOUT`), transition interpolation (`ProducerId::MACHINES`), and per-placed-instance simulation state (path-dependent history reset on loop wrap).
- **Why:** Continuous interpolation and path-dependent history stay cleanly outside discrete overrides and compose via fixed `ProducerId` priority.

### Layer D — Discrete Variation ("Patches" where patches actually belong — ~350–500 lines)
- **Owns:** The exact four authored variation concepts from 2026-11-20:
  1. **Component machine states** (unordered local axis: `{A, B}` sets colour, `{B, C}` sets position — no forced single parent, works on cyclic graphs!).
  2. **Decks / slides** (ordered document axis: `[B, ∞)` "from here on", `{C}` "this slide only", `[B, D]` span).
  3. **Takes / loop iterations** (ordered axis driven by a loop clock's `n`, or explicitly placed as `<place def="…" at="take=2" clock="#main"/>` for ghosts and side-by-side chaos comparisons).
  4. **Narrative / pedagogical moves & arrows** (ordered or graph joints with entering/leaving overrides and arrow diffs `x → y`).
- **Strict boundaries that keep it tiny and bug-free:**
  - **Only 2 axis kinds:** `Ordered` (stable IDs + tombstones; selections `{v}`, `[a, ∞)`, `[a, b]`, `{a, b}`) and `Unordered` (stable IDs; selections `{a, b}` or all).
  - **Values & `hidden` only:** no `Present`, no `FacetPresent`, no `Notation`, no `Slot::Bound` inside discrete patches. Use `hidden` for visibility and Iron's existing `when` / `extends` for structural additions.
  - **Fixed, human-obvious effective precedence:**
    `gesture_preview` > `live_solve_preview` > `binding` > `layout` > `clip` > `document discrete variation (narrative / slide / take)` > `component machine (interpolated via ProducerId::MACHINES)` > `solved pose` > `instance override` > `variant` > `definition base`.
  - **Clean interaction between layers:**
    - A slide/narrative/take patch that sets a component's state input (`args.vis = "shown"`) drives the component's machine, which resolves the target state's pose from its discrete state patches (`{A, B}`, `{B, C}`) and animates the **arrow diff** (`from_pose → to_pose`) smoothly via `ProducerId::MACHINES`.
    - Placing two instances of the same component at different discrete coordinates or port args (`take=2` ghost + `take=3`, or `ρ` vs `ρ + 0.001`) on one shared clock works out of the box with isolated simulation state.
  - **First-class UX operations (small & exact over finite discrete sets):**
    - `drag(point, edit_region, target, value)` — *"a drag always changes what you see"*.
    - `widen` ("bubble up"), `narrow` ("push down"), `split` ("partial bubble"), `reset`.
    - `arrow(x, y)` — entering patches, leaving patches, and resolved field diff for Magic Move transitions and narrative explanations.
    - `provenance(point, leaf)` — walks the fixed precedence stack to explain *"why is this value what it is?"* and *"which later slides/states override this?"*.

---

## 6. Guardrails for all future engine work

To prevent this pattern from ever recurring (mirrored in `AGENTS.md`):

1. **Net-LOC circuit breaker:** Any refactor or architecture change billed as a *simplification* must pause and check in with the user as soon as it exceeds **+1,500 net lines** without having deleted the old mechanism it was meant to replace.
2. **Delete as you go, never "later":** Do not build a parallel subsystem across 4+ commits while keeping the old one (`Docs::copies`, legacy overrides, `regraph`) alive. If a phase cannot delete its target cleanly, stop and narrow the design.
3. **One real consumer before generalization:** Never generalize an abstraction across 3+ subsystems until at least one concrete user-facing workflow uses it end-to-end.
4. **Respect engine boundaries:** Ephemeral UI previews, live solver scratchpads, persisted solver poses, continuous time interpolation, and lexical component structure are distinct runtime concerns. Unify them in the **read-only UX provenance layer**, not by cramming them into one mutable storage table.
