# The reactive engine: review history

How [`reactive-engine.md`](reactive-engine.md) reached its current form. Nothing here
is needed to build from the spec; it is kept so that a later reader can see why a
section says what it says and which alternatives were tried.

Order of events, all on 2026-11-15: a first handoff proposed patching the existing
engine (options A, B, C); the user widened the goals and chose a new engine; the spec
was written; two council rounds (Claude Opus 5.5 and GPT-6.1 Sol, both at high effort)
each returned "rework" and the spec was revised after each; the constraint solver, 3D
and parking were added afterwards at the user's request.

Section numbers below are the spec's. Its §14 held the first table until it was moved
here.

## Answers to the first handoff's questions

| # | Question | Answer |
|---|---|---|
| 1 | Expanding one scope exactly as a full expansion would | Expansion is one function per owner over a compiled template; expanding the document is that function from the roots (§5.4, §7) |
| 2 | Problems per scope | Held per owner (§7.5) |
| 3 | The template's own bindings | The prototype, evaluated only when observed (§5.3) |
| 4 | Index or value keys for plot ticks | Decision D7 |
| 5 | Edges across a scope boundary | Inward readers indexed per owner; they pin it (§7.4) |
| 6 | Heights | Local adjustment on insert; one heap (§6.3) |
| 7 | Context reads | Resolved at cell creation; an index name → readers (§7.2) |
| 8 | Derived signals | Per relation (§7.2) |
| 9 | Removing a binding | Per cell on disposal (§7.3) |
| 10 | When to fall back to a full rebuild | Never; loading is the same path over everything |
| 11 | Transactions on a working copy | In place, with rollback (§11) |
| 12 | Presence | A scheduled cell on the Item; disposal in the pass where it reaches 0 (§10.3) |
| 13 | Session lets | Dropped on disposal, kept on unload (§7.3) |
| 14 | Render analysis and sharing | Per template (§12.3); `Changes` names what moved (§12.2) |
| 15, 16 | Structure inside the graph | Yes (§6.2). Derived structure is never a transaction's op. In phase 2b owners still write copies into the document, reported through `Changes`; from 2c nothing is written |


## Council review, round one

Claude Opus 5.5 and GPT-6.1 Sol, both at high effort, each returned "rework". Every
claim about the code below was checked before it changed the spec.

| Finding | From | Checked | Result |
|---|---|---|---|
| Rollback by inverses leaves changes behind: ops mutate before a check that can fail; counters advance; disposal drops session, presence, trace and machine state; effects escape | both | True (`op.rs:524`, `:615`, `:690`; `lib.rs:1476`) | §11 rewritten: atomic ops, saved counters, pure validation before the engine is told, disposal held until commit. Sol's full transaction journal is not adopted, because validation no longer touches engine state |
| Overrides replace slots (a binding included) and can add facets; they can change a nested instance's definition; constant references are relocated; the `sized` flag and `port_spec` → `port` | both | True (`expand.rs:647`, `:487`, `:706`) | §5.2, §5.4, §5.5 |
| The crate's own readers walk derived nodes through `&Document`; `Store::selection` would drop derived nodes | Opus | True (`lib.rs:2251`) | §12.1; phase 1 covers the crate |
| §8.8 is false for history: a dormant conditional that toggles, traces sampled per frame, machines started by polling, presence integrating `dt`, glides, queries | both | True (`machine.rs:282`, `step.rs:134`, `lib.rs:1436`) | §8.3, §8.8, §10.4, D1 |
| A last-known bound can hide content that moved into view | Sol | Judgment; agreed | §8.5, D2 |
| Reading a cell that is not current can write stale values into the document; reads through `&Store` cannot load | Opus | True (`post_apply`, `translate`, `resolve_clusters_live`) | §8.9 |
| An Unloaded picture drawn as a reference shows the template's values, wrong with arguments | Opus | True (`render.rs:573` draws the template's own nodes) | §8.5 |
| Value slices cannot hold structure that depends on time; no clock edge does not mean seekable | both | Judgment; agreed | §10.5, D10 |
| Two clocks are merged; formulas cannot read time today; undo snaps machines | Opus | True (`lib.rs:1940`, `:2481`; no time function in `expr.rs`) | §10.1, §9.3 |
| The scheduler misses clip starts, hold keys, jumps, `ages`, layout motion; machines are not pure | Sol | True (`time.rs:204`) | §10.3, §9.1, §9.3 |
| "Sized by its parent" is not containment; one layout cell cannot both size the box and place its contents | both | True of today's single Taffy run (`layout.rs:318`) | §9.2 |
| Network islands follow equations, not connections; the solve warm-starts | Sol | True (`port.rs:274`, `lib.rs:2146`) | §9.1, §8.8 |
| `Changes` is too coarse; `apply_session` is a second path | both | True (`geom.rs:399`, `mod.rs:520`, `lib.rs:1216`) | §12.2, §11.3 |
| The meter targets cannot be met in phase 2 | Opus | True: a constant edit is 14.1 ms at 1,000 meters | §15, phase 5 |
| "Never revisits a height"; references to items that do not exist yet; Collections reading each other | both | Agreed | §6.2, §6.3, §7.4 |
| The harness misses effects, failed transactions, the host, and intended differences | both | Agreed | §16.1 |
| Phase 2 is too large; lifecycle before dormancy | both | Agreed | §17 |
| Patch the old graph first (the first handoff's option A) | both | Not adopted as such: 2b writes the new cell graph directly but keeps today's stored copies, which gives the same early plot fix and an exact comparison without code that is thrown away |
| A copy-on-write document instead of rollback | Opus | Kept as the fallback (§11.3) |
| Materialise derived records per loaded owner so reads keep returning whole facets | Opus | Adopted for the Tree (§5.4); the engine itself still evaluates from the shared template |

## Council review, round two

Both members reviewed the revision. Opus: rework of §11 and §17 only, the rest
approvable with changes. Sol: rework. Sol confirmed eleven round-one resolutions as
sound and Opus eight.

| Finding | From | Checked | Result |
|---|---|---|---|
| A failed `post_apply` cannot be undone by inverses and a second stabilise: the first stabilise records events (a machine's transition, a trace's pulse), the second records their reverse; solver writes were missing from the steps | both | True (`lib.rs:2556`–`2615`) | §10.4 and §11: `stabilise()` is pure, recording and effects happen at commit, solver writes are a step. This is a different answer from the journal of engine state both proposed: with recording outside `stabilise()`, there is no engine history to journal |
| Validation is not a function of the document and templates: item liveness comes from values, and `post_apply` reads values | both | Partly. `post_apply` was already after the engine in the steps. Item liveness is true: today's checks use the pre-transaction items (`lib.rs:2661`) | §11.1: an id inside an item validates by template path; cycle and look-only checks walk templates |
| Op inverses do not restore the document: tombstones, counters | Opus | True (`document.rs:502`, `serial.rs:45`) | §11.1: a document journal; atomic ops are no longer required |
| 2a promises template validation before templates exist; 2b would validate before copies are written, breaking create-and-connect in one transaction | Opus | True (`op.rs:720`) | §17: 2a keeps `expand` before the checks; in 2b op-driven copies are written before validation |
| Pictures hold model state by design; §8.1, §8.3 and §8.4 contradicted each other | both | True (reactivity.md §5.1; `expand.rs:330`) | §8.3: stateful is a property of the template, by node kind; stateful owners are never unloaded |
| Presence decides disposal, and so state, not only appearance | Sol | True (`lib.rs:1436`, `:1476`) | §8.3, §10.4: a stateful item's exit is logged and its disposal scheduled |
| Authored content had no observation rule | Opus | Agreed | §5.3 (Composition owners), §8.2 |
| `analysis.rs` cannot identify what decides a bound | both | True (`analysis.rs:57`) | §8.5: an extent cell, and clipping as the way to make it cheap |
| §9.2 as written refused `std:plot`; one-axis containment needs a third step; `iron_layout` lacks what CSS has; pins and table tracks were missing | both | True (`plot.xml:89`; `iron_layout/src/lib.rs:68`; `layout.rs:443`) | §9.1, §9.2: a sized `use` is contained; both axes in phase 5, one axis left for later; CSS reach stated as layout-engine work |
| `clock()` resolved like `@name` cannot be read by bodies, and a body under two clocks would have two values | Opus | Agreed | §10.1: `now()` and `clock()`; a body resolves time along the model's tree |
| Sources cannot be computed statically; `machine.at` logs more than a state change; the laser example read the playhead | both | True (`machine.rs:309`, `graph.rs:511`) | §10.4, §10.5 |
| `Changes` lacked slot changes and marks | Sol | True (`mod.rs:646`, `lib.rs:1834`) | §12.2 |
| The template key does not key one owner's records; `Ref` lacked a body variant and item depth; the materialised cache went stale on an override | both | True (`expand.rs:526`–`607`, `:349`) | §5.4 |
| Lazy `&T` through `&Store` needs a concrete representation; visible owners materialise in full; the debug stale-read error would fire on every dormant draw | both | Agreed | §5.4, §8.9, §15 |
| The host reference needs a headless Bevy world | both | True (`materialize.rs:352`) | §16.1, phase 0 |

**Not resolved by this revision, stated as limits:** one-axis containment (§9.2);
structure per slice (D10); an unclipped owner with moving geometry saves little by
sleeping (§8.5); ops on content that is not loaded (§13).


## After the council: constraints and 3D

Added at the user's request and reviewed by neither member. §9.4 brings the constraint
solver into the engine as cells, where both earlier versions left it as it is; §11.2's
steps 4, 5 and 7 and phase 5 change with it, and D11 is new. §8.10 lists what the spec
wrote in 2D terms. Checked against the code before writing: clusters are regrouped for
the whole document after each transaction (`solve.rs:130`, `lib.rs:2341`);
`resolve_clusters_live` has eleven call sites and solves every cluster (`lib.rs:415`);
`graph.rs` has no cluster node, though constraint-system.md §6 planned one;
implied mates require both anchors in one composition (`plug.rs:251`). Not checked,
and a phase 5 exit instead: that solving from a converged pose with unchanged inputs
returns that pose.

## After the council: D1

**D1 changed (2026-11-15, at the user's request).** Round two's
"stateful owners are never unloaded" kept too much: an owner at rest needs only its
state, not its cells. §8.1 and §8.3 now park it and load it again on a change to what
it reads or a timed event; a simulation range is the application's opt-in for state
that is running far from every observer.

## Council round three (2026-11-16): a cold read as the builder

Both members were asked to read the handoff, the spec and the kernel as the builder
would, describe their first days on phases 0 and 1, review the parts no one had
reviewed (§9.4, parking, §8.10, §14) and try to break the kernel. Opus: ready after
fixes. Sol: not ready. Every claim below was checked against the code before acting.

| Finding | From | Checked | Result |
|---|---|---|---|
| Phase 0's exit required "a failed call leaves nothing", which the old engine does not meet: `apply_session` rolls back by inverse ops; a rejected transaction keeps new `expansion_problems` | both | True (`lib.rs:1235`, `:2663`) | Handoff §4.1 task 3: agreement and atomicity are two checks; phase 0 measures the second and lists its failures |
| The kernel left heights wrong after a cycle | both | True: `check()` failed once the cyclic test called it | Heights are put back; test added |
| The kernel reported a cycle when two tracked reads changed in one pass and the first met the other's old edge | Sol | True: reproduced as a test | R6 rewritten: the reader waits, and fails only when a round of waiting cells finishes nothing |
| A cell that failed on a cycle was never told the cycle had ended | Opus | True: reproduced | Failed cells are kept and run again when an edge is removed |
| (found while fixing) A reader raised above a dependency that looked current, while a cell below it was still queued, finished on a value about to change | neither | A new random test of cells selecting each other found it | R5: a raised reader is queued again if anything below its new height is queued |
| 58 public `&mut self` methods, not 55; `StoreCore` used and never defined; the differences file under `tests/` cannot be read from `src` | both | True | Handoff §4.1 tasks 2 and 4 |
| "Compare every return value" contradicted "ops for derived structure are not compared" | both | True | Everything is compared exactly through 2a; relaxations enter by phase as listed entries |
| The snapshot missed tombstones, a failed leaf's kept value, previews with no slot, signals, earlier history, pending buffers and other store state | both | True | Handoff §4.1 task 3; spec §16.1 |
| `post_apply` solves every cluster, commits live poses of clusters the transaction did not touch, and fails on any cluster that does not converge; the spec said "every cluster the transaction touched" | Opus | True (`lib.rs:2575`) | §9.4 "At commit", §11.2 step 5: today's rule, without the solves |
| A6 is false while a finger is held: the least-change term is measured from each solve's start and a strong term has finite weight | Sol | True (`iron_solve/src/lib.rs:243`, `solve.rs:452`) | §9.4: the new schedule goes into the old engine first, so the two can be compared exactly; A6 restated |
| When a pose is adopted outside a transaction was undefined; a `Set` changes the start pose as well as adding a driver; undo would leave a stale solved layer | both | True (`lib.rs:421`, `solve.rs:354`) | §9.4 "The pose is state"; §11.2 steps 6 and 7; §11.3 |
| The Clusters cell reads things that are not cells (`plug.mate`, world transforms); a Solve cell's real reads are more than `Cluster::inputs`; `Cluster::id` is not unique; nothing validates that an explicit constraint stays in one composition; constraints can be derived | both | True (`plug.rs:239`, `solve.rs:333`, `:170`, `constraint.rs:97`, `expand.rs:451`) | §9.4 cells; A11; `constraint` counts as state in §8.3 |
| Parking can miss events: tracked reads, arguments and providers, a query's reply, `set_state`, a publish onto a leaf inside; a query's state was not all kept; "at rest" was not operational | both | Agreed | §8.3: an explicit at-rest list, a wake cell, the list of things addressed into an owner; A13 |
| §8.3 parks machines while §8.4 said a Body is never unloaded; §8.4 and §8.5 disagreed on unloading pure pictures | Opus | True | §8.4 |
| Recording after the last `stabilise` leaves a zero-delay trace's `out` and an instant transition stale | Sol | True (`step.rs:131`) | §10.4: record and stabilise alternate until nothing is recorded |
| Derived-id order depends on what the process interned first | both | True (`id.rs:255`) | A12, with a two-process test in phase 0 |
| A simulation range cannot be built from its description; the declared 3D box has no schema | both | Agreed | Both marked as not designed, in §8.3, §8.10 and §14's open list |
| Stale line references; §12.1 said `document()` remains and is renamed; counts differed | both | True | Fixed |

**Not taken as written.** Sol's verdict of "not ready" covers phases 4 and 5; both
members agree phases 0 and 1 can start once the handoff's fixes are in, which they
are. The later phases now have rules for each point, but those rules are new and
unreviewed, so §14 lists them as open and the handoff (§4.3) makes checking them the
first task of each phase. Not changed: the kernel still never frees a cell and breaks
ties by cell id; `lib.rs` there now says so.
