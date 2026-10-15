# Handoff: the product track (document, compositions, tools, signals)

**Written:** 2026-10-14 on branch `document-spine`, replacing the 2026-10-11 handoff
(now [`archive/handoff-2026-10-11.md`](archive/handoff-2026-10-11.md), whose tasks are all
done). **For:** an agent continuing roadmap work other than rendering performance.
Rendering performance (stage H) has its own handoff, [`handoff-render.md`](handoff-render.md).

## 1. Read, in this order

1. This file.
2. [`roadmap.md`](roadmap.md): the stages and the **Status** table, the record of what
   is done.
3. The spec for the task you pick (§4 below names it).
4. [`specs/document-spine.md`](specs/document-spine.md) §5.10–5.12 for the store's rules,
   if you will touch `iron_document`.
5. [`../AGENTS.md`](../AGENTS.md) for worktree rules, and
   [`.claude/skills/run-iron/SKILL.md`](../.claude/skills/run-iron/SKILL.md) for building,
   driving and the smoke script's expected output.
6. [`guide2/index.html`](guide2/index.html) if you want a readable tour first.

## 2. Where things stand

Done (details in the roadmap's Status table):

- **A** groundwork; **B** values, units, records, definitions as data (schema 3);
- **C1** compositions, entities, figures, linking, cross-probing, one view per
  composition, embedded compositions;
- **C2** solver and constraints, and **D** the IK pendulum;
- **E** the select tool (Figma semantics) and the pen tool with path and point nodes;
- **F** phase 3: ports, connections, one-way signals, across/through pairs solved as
  equations (optics chain and thermal network demos);
- panels and session: dockview workspace, pan and zoom per view, the session tier with
  the selection as relations, a first HTML inspector;
- **H** in part: see [`handoff-render.md`](handoff-render.md).

Not started: ECM phases 4–7, stage G's semantic views and lessons, CS phases 4–7.

## 3. Where the code is

| Area | Files | Notes |
|---|---|---|
| The store | `src/iron_document/src/` | Bevy-free. `lib.rs` is `Store` (apply, undo/redo/replay, gestures, drags, clusters, physics, session); `op.rs` ops and applier; `document.rs` nodes/relations/indexes; `component.rs` typed components (`@dynamic` block for `params`); `registry.rs` node types; `expr.rs` expressions and the floor; `graph.rs` the reactive graph; `handle.rs` linear binding inversion; `geom.rs` world affines; `units.rs`/`value.rs` quantities and records; `definition.rs` definitions and instantiation; `composition.rs` compositions and figures; `port.rs` ports, connections, the physics network; `constraint.rs` + `solve.rs` constraints and the solver adapter; `view.rs` the model's views; `serial.rs` the file format (schema 3; 1 and 2 still load); `time.rs` clips; `sym.rs` interning |
| The solver | `src/iron_solve/` | Pure. Projected Levenberg–Marquardt with required/strong/weak tiers |
| Spatial | `src/iron_spatial/` | Bounds and `understory_index` per composition; see `handoff-render.md` |
| The bridge | `src/document_bridge/` | `mod.rs` sync and `reconcile_gesture`, `DocumentChanges`, `GeometryRevision`; `materialize.rs` node → entity; `render.rs` the 2D walk per view; `spatial.rs` bounds preparation; `nest.rs` embeds; `panel_views.rs` views per panel; `planes.rs` 2D in 3D; `highlight.rs`; `demo.rs`, `stress.rs` |
| Interaction | `src/bevy_app/` | `select.rs` the select tool, selection cache, navigation; `pen.rs`; `interaction.rs` 3D, plane and slider drags; `picking.rs` |
| Rendering | `src/vector/` | Display lists and the `vello_hybrid` backend; see `rendering/architecture.md` |
| FFI | `src/web_ffi.rs` | Document, session, inspector, stress, timings, spatial toggles |
| Shell | `src-ui/` | Svelte. `views/` the dockview workspace and view registry; `model/` the model surface (manifest, `<edit>` parser, agent); `runtime/document_client.svelte.ts`; `__iron` on `window` for driving |
| Tests | `src/iron_document/tests/*.rs`, `src/iron_spatial`, `src/iron_solve/tests/` | Golden views in `tests/golden/` (pendulum, cartoon, model) |
| Driving | `.claude/skills/run-iron/` | `build.sh`, `serve.sh`, `drive.ts`, `smoke.txt`, and the regression scripts listed in `SKILL.md` |

How to verify anything:

```sh
cargo test -p iron_document -p iron_spatial -p iron_solve --target aarch64-apple-darwin
.claude/skills/run-iron/build.sh && .claude/skills/run-iron/serve.sh start
cd .claude/skills/run-iron && bun run ./drive.ts < smoke.txt; cd -   # compare with SKILL.md
.claude/skills/run-iron/serve.sh stop
```

`iron_solve`'s `three_link_chain_reaches_a_target` has failed since 2026-10-11 (a known
regression, CS §14 phase 1 note); every other test passes. The smoke script is the
browser-level regression suite: add each stage's exit checks to it and update the
expected output in `SKILL.md`. The build left served may be `wasm-bench` from render
work; rebuild `dev-opt` (`build.sh`) before the smoke script.

## 4. What to do next

Several independent tasks are open. **Ask the user which to take**; do not pick one
silently. Each line names the spec that owns its detail and exit criteria.

| Task | What | Spec |
|---|---|---|
| ECM phase 5 | Time-domain signals and the modulator: the four-representation demo (cartoon, schematic, 3D, scope trace from one signal) | [`specs/entity-composition-model.md`](specs/entity-composition-model.md) §12 phase 5 |
| ECM phase 4 | Obligations, ghost lines, the tray, `omits`, `Store::encapsulate`, drill-in and breadcrumbs, the law tests | ECM §8–9, §12 phase 4 |
| Phase 3 follow-ups | Pins on solved values (drag a heater's temperature, solve for power), electrical parts, adapters between port kinds | ECM §12 phase 3, "Next for this phase's area" |
| Panels and session | The outliner; the workspace root (save each view's pan and zoom); Show in opening the right tab; selection in undo | [`specs/panels-and-session.md`](specs/panels-and-session.md) |
| Select and pen gaps | Group/ungroup, reparent by drag, snapping and guides, pen point insertion and conversion (duplicate is done) | [`specs/select-tool.md`](specs/select-tool.md) "Not yet" |
| Alt-drag to duplicate | After gesture ownership: duplicate mid-drag, one history entry | [`specs/clipboard-and-assets.md`](specs/clipboard-and-assets.md) phase 2 |
| Project container | Decide the bundle format, durable storage, document identity, where libraries and the workspace live; it unblocks assets, drag and drop and cross-document paste | [`specs/clipboard-and-assets.md`](specs/clipboard-and-assets.md) §6 |
| Gesture ownership, rest | First pass done 2026-10-15 (store ownership, conflicts, undo). Next: one pointer state machine with manipulators for 2D and 3D, folding in `interaction.rs`; then the 3D gizmo on `transform-gizmo`; then Alt-drag | [`specs/infinite-canvas-architecture-patterns.md`](specs/infinite-canvas-architecture-patterns.md) §4 |
| Typed UI protocol | Typed requests, subscriptions, request generations, disposal | same, §5 |
| CS phase 4 | Degrees-of-freedom display, conflict highlighting, constraint glyphs, gesture-created constraints | [`specs/constraint-system.md`](specs/constraint-system.md) §14 phase 4 |
| Stage G | Semantic model-surface views and verbs, then lessons (ECM phase 6) | roadmap stage G, ECM §10–11 |

Open questions to raise rather than decide: ECM §15, CS §18, and the panels plan's open
questions. Notably still undecided: what the model sees and may edit of
solver-maintained values (hide them, make them read-only unless the edit says
`drive="true"`, or both; CS §9.3).

## 5. Decisions already taken (do not reopen)

| Decision | Choice | Where recorded |
|---|---|---|
| Units | SI magnitude plus dimension vector; display unit is presentation; geometry in pixels and radians | ECM §12 phase 1 |
| Model edits under constraints | Adjust and report `unmet`; `exact: true` (or `exact="true"`) rejects instead | CS §9.2 |
| Path handles | Leaves on the point node | roadmap §7 item 4 |
| Naming | `figure` / `figures` (was depiction); the definition's template child is `template` | roadmap §7 item 2 |
| Merge on link | Ask before doing anything; the model surface gets an `<edit-failed>` asking for `keep="#id"` | ECM §15 I5 |
| Template references | A template binds to its own `param_spec.default` leaves; instantiation rewrites them | ECM §12 phase 1 |
| Solved poses | Stored as constants, recorded as `derived_ops`; undo applies the inverse; nothing re-solves | CS §7 |
| Ports | One port interface with one-way and across/through variables | ECM §6.1 |
| Embedded compositions | Embeds draw their source in place, through their transform and clip | ECM §7.4 |
| Selection | Session relations under a `session` root, outside history and the file | panels-and-session |
| Rendering | No Vello fork; Iron owns the render plan; no new transform hierarchy | `rendering/architecture.md` §5 |

## 6. Conventions that are easy to break

- **Every mutation is an op in a transaction.** Tools, drags and the FFI never write
  Bevy state or the `Document` directly. Drags preview through `Store::preview*` /
  `begin_drag`/`drag_to`; release is one `commit_gesture`. Session state goes through
  `Store::apply_session`.
- **Authored versus derived.** The file holds authored intent plus solved poses. Cluster
  membership, resolved bindings, clip samples, previews, physics solutions and
  obligations are derived and never serialised.
- **Ids.** Mint with `doc.next_ids(n)` and create in ascending order within a
  transaction. The demo reserves a block in `demo.rs`: grow it when adding nodes, and
  keep the library block and `instantiate` call last. Ids print in base 62; the smoke's
  expected output names ids, so adding nodes shifts later ids and `SKILL.md` must follow.
- **`TransactionInput` has an `exact` field**; test files have a `tx()` helper.
- **Bindings validate at apply time**: referenced nodes must already be live.
- **Every transaction re-solves every cluster and the physics network.** A `Set` on a
  cluster variable is a driver; solved values are locked against direct edits.
- **Painter order is sibling order**, not creation order.
- **`params` is the one dynamic component**, with interned field names (`sym.rs`).
- **Quantities** are `Value::Quantity { si, dim }`, never dimensionless.
- **Views are what the model sees**; a change to their text is a behaviour change.
  Golden files guard them.
- **Plans are the record.** When something lands, add a dated "Done" note to the spec's
  phase and update the roadmap Status row. Record deviations with the reason. If a
  plan's status changes, update [`README.md`](README.md).
- **Don't push.** Commits end with the `Co-Authored-By` line the session gives you.

Things that have bitten earlier sessions:

- `cargo test` needs `--target aarch64-apple-darwin`; the workspace defaults to wasm.
- `drive.ts` runs as `bun run ./drive.ts` from `.claude/skills/run-iron/` with the Vite
  server up, or every `eval` fails with `__iron is not defined`.
- Don't rebuild the wasm while a driver run is navigating.
- Under SwiftShader a press where two nodes meet can resolve to the wrong one; grab
  off-centre. A second press soon after a first is a double-click; wait it out.
- Rust raw strings: a string containing `id="#…` needs `r##"…"##`.
- In zsh, quote globs such as `--include='*.rs'`.
- `bun run check` already reports about 5,000 errors in five files (`Github.svelte`,
  `utils.ts`, generated wasm glue); compare counts, don't expect zero.

## 7. How to report when you stop

What is done, with the evidence for each exit (test names, smoke lines, screenshots);
what is next; and every place a plan turned out wrong, with what you did instead and why.
Keep the specs' status notes, the roadmap and `README.md` in step with that report.
