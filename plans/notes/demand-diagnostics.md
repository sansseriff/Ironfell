# Demand diagnostics: seeing what sleeps and what runs

Status: proposal, not scheduled (2026-11-24). Asked for by the user during the
council review of `EN-8c.0` and `EN-8c`. Written against the engine as of that
review; check the names below still exist before building on them.

## The questions it should answer

1. With one composition on screen, is every other composition costing nothing
   (no evaluation, no clock, no queries, as little memory as possible)?
2. When the shown composition reads another, does the engine keep the
   smallest set of linked compositions running, and say why each one runs?
3. Within the shown composition, is work spent on what is off screen?

## What exists today

- **Owner-level demand (`EN-8c`).** `Store::demand` is the set of owners
  (compositions, pictured entities, `Free`) whose cells are evaluated.
  `OwnerIndex::compute_active` works it out as a closure over cross-owner
  reads (`read_by`) and `Figures`, seeded by the shown compositions, the
  `run = always` ones, `Free` and the owners constraint clusters hold.
  Clocks have a second closure of the same kind (`demand = false`).
- **The invariant, checked only in tests.** `Store::check_demand`
  (`IRON_OWNER_CHECK=1`, differential builds) asserts that demand matches a
  fresh decision, and that the resting cells are exactly the cells of the
  owners outside it. `check_owner_index` does the same for the owner index.
- **Work counters (`work.rs`) and the scaling probe.** These count passes
  over the document in tests only (`work` feature).
- **Render culling.** The encoder culls what is off screen and reports
  `commands` / `culled` / `drawn` (`perf.rs`, shown in the Latency view).
  Evaluation is not culled: `Interest::Region` carries a rect, but demand
  uses only its composition (`observed_compositions`).

## Proposed architecture

**1. One engine query, `Store::demand_report()`.** It is built on request,
never per frame, so it may walk the whole document. Each owner gets:

- its kind and name;
- `evaluated: bool` and `clock: running | paused`;
- **why**: the first reason the closure reached it. That is one of shown;
  `run = always`; held by a cluster; read by owner X through cell C; or
  pictured by composition Y. `compute_active` records the parent of each
  owner it adds, so the report can print the chain back to a shown
  composition;
- cells: total, resting, resting-and-behind (epoch stamps), awake;
- evaluations since the last report (point 3);
- a memory estimate (point 4).

Put it beside the existing inspect APIs in `engine_api.rs`, as JSON.

**2. The invariant as a button.** Ship `check_demand`'s comparison in normal
builds behind a call (`Store::verify_demand() -> Vec<String>`), not an
assertion. The diagnostic view runs it on request and shows any mismatch in
red. Tests can't cover every document a person makes, so this lets a person
check the one they have open.

**3. Per-owner evaluation counts.** `Reactive::evaluations` is a single
global counter. Attributing each evaluation to its owner costs an
`owner_of` per evaluation, so do it only while the diagnostic view is open
(a flag on `Reactive`). It should be a map keyed by owner, cleared when the
report is read.

**4. Memory per owner, estimated by counting.** Count what each owner holds:

- its graph cells and natives;
- its virtual-view items and overridden nodes (`EN-13` views, the
  `NODE_BUDGET` accounting is already per owner);
- step, clip, query and machine state.

Multiply by measured sizes per kind, calibrated once against
`instance_cost`. This is the same accounting `EN-17` needs to explain the
memory per meter, so build the counting there and reuse it.

**5. A diagnostic view in the shell.** Model it on `LatencyView.svelte`: a
table of owners, with a chip for evaluated, resting or clock paused, plus the
why-chain, cells, evaluations per second and estimated memory. Highlight the
closure reached from the focused composition. Check it in the run-iron smoke
with one tab open (see the single-tab worst-case memory note): every row but
the shown composition and what it reads should say resting.

## Off-screen work inside the shown composition

This is not built. It would be a real design change, not a diagnostic.
Demand is per owner, and an owner is a whole composition or a pictured
entity. Evaluating only what is on screen needs either smaller owners or
per-node demand from the `Region` rect. Two things make that harder than
render culling:

- Layout, constraints and cross-references couple off-screen nodes to
  on-screen ones. Much of what is off screen is still read.
- The cost may already be small. Unpatched virtual-view nodes are borrowed,
  not materialized (`EN-13`), so memory for off-screen repeated items is
  mostly shared already.

So measure first. Add an "evaluated but off screen" count to the report:
evaluations whose node's bounds miss every observer's rect, using the bounds
the encoder already has. Only if that number is large in real documents is a
finer owner (for example a repeat's items, or a group marked as a cull unit)
worth designing.

## Where it fits

- After `EN-17`, which builds the memory accounting (point 4).
- Points 1–3 are small, since they read state the engine already keeps.
- Point 5 is shell work.
- The off-screen measurement waits until the report exists.
