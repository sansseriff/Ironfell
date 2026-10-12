# Handoff: picking up the roadmap build

**Written:** 2026-10-11, at commit `f239cb4` on branch `document-spine`.
**For:** the agent continuing the roadmap (`roadmap.md`). This is your main prompt. Read it
first, then the files it points at, in the order given. It records where the code is, the
decisions already taken, the conventions that are easy to break, and what comes next.

**Update, later on 2026-10-11 (at `12466a1`):** task 0 (the rename, schema 3) and task 1
(stage C1, all six steps) are **done**; see ECM §12 phase 2's progress notes and the roadmap
status row. Next is task 2 (stage E, the pen tool) or task 3 (stage F, which needs the
conservative-ports decision first). Known and left: `iron_solve`'s
`three_link_chain_reaches_a_target` fails (CS §14, phase 1 note). The smoke script now also
needs `GPU=1` for any frame-time measurement (see `SKILL.md`).

**Update, end of 2026-10-11:** stage E has begun with the **select tool**, at the user's
request: Figma's semantics, recorded in `plans/select-tool.md`. A plain click on a group
(the pendulum) now selects the group, and the smoke script holds Cmd for the IK steps.
The pen tool comes next and should reuse `select.rs`'s chains and its preview-and-commit
pattern. **2026-10-12:** `plans/panels-and-session.md` records the agreed direction for
panels, inspectors and the session tier (selection as relations in a `session` root);
built since:
- the dock presenter (dockview tabs and splits of views; the demo split into
  `pendulum` and `controls` compositions, embedded in `main`);
- pan and zoom per view;
- the session tier, with the selection as relations;
- the pen tool, with path and point nodes;
- a first HTML inspector.

The ports decision is recorded (ECM §6.1). **Stage F is next.**

---

## 1. Read, in this order

1. This file.
2. `plans/roadmap.md` — the stages, their exits, and the **Status** table (§3) saying what
   is done. Stages A, B, C2 (phases 1–3) and D are done. C1, E, F, G, H, I are not.
3. `plans/entity-composition-model.md` (ECM) §5–§7 and §12 phase 2 — the next stage.
4. `plans/constraint-system.md` (CS) §8–§9 and the **Done** notes under §14 — how drags,
   solves and model notifications work now.
5. `plans/document-spine.md` §5.10–5.12 and §12 — the store's rules and the record of
   steps 1–7.
6. `AGENTS.md` — worktree rules. Follow them exactly if you use a worktree; the main
   checkout on `document-spine` is where the last two stages were developed and is fine
   to continue on unless the user asks for a worktree.
7. `.claude/skills/run-iron/SKILL.md` — building, running, driving and screenshotting the
   app, and the smoke script's expected output, which you must keep current.

---

## 2. Where the code is

| Area | Files | Notes |
|---|---|---|
| The store | `src/iron_document/src/` | Bevy-free. `lib.rs` is `Store` (apply, undo/redo/replay, gestures, drags, clusters); `op.rs` the ops and applier; `document.rs` nodes/relations/indexes; `component.rs` typed components (`@dynamic` block for `params`); `registry.rs` node types; `expr.rs` expressions and the floor; `graph.rs` the reactive graph; `handle.rs` linear binding inversion; `geom.rs` world affines; `units.rs`/`value.rs` quantities, records; `definition.rs` definitions and instantiation; `constraint.rs` + `solve.rs` constraints and the solver adapter; `view.rs` the model's tree view; `serial.rs` the file format (schema 2; schema 1 still loads); `time.rs` clips; `sym.rs` interning |
| The solver | `src/iron_solve/` | Pure, no deps. Projected Levenberg–Marquardt with required/strong/weak tiers; `cluster.rs`, `analysis.rs`, `bench.rs` |
| The bridge | `src/document_bridge/` | `mod.rs` sync (PreUpdate) and `reconcile_gesture` (PostUpdate, before transform propagation), `DocumentChanges`, `Bounds2d`; `materialize.rs` node → entity (skips anything under a `library`); `render.rs` the 2D layer; `demo.rs` the demo scene |
| Interaction | `src/bevy_app/interaction.rs` | 2D drags through `Store::begin_drag`/`drag_to`; 3D drags still preview `transform3d.pos` directly; sliders preview `slider.value` |
| FFI | `src/web_ffi.rs` | `document_save/load/view/apply/next_ids/place/schema`, `solve_bench` |
| Model surface | `src-ui/model/` | `manifest.ts` (system prompt from the schema), `edit.ts` (`<edit>` parser; `exact="true"`; solver summary markup), `schema.ts` (kinds, coercion), `agent.ts` (one retry) |
| Shell | `src-ui/` | Svelte. `__iron` on `window` for driving (`save`, `load`, `view`, `edit`, `ask`, `bench`) |
| Tests | `src/iron_document/tests/*.rs`, `src/iron_solve/tests/phase1.rs` | 74 + 9 tests. `tests/golden/pendulum.xml` is a golden view |
| Driving | `.claude/skills/run-iron/` | `build.sh` (WebGL2 dev-opt wasm, ~20 s warm), `serve.sh start|stop`, `drive.ts` (Playwright over stdin), `smoke.txt` |

How to verify anything:

```sh
cargo test -p iron_document -p iron_solve --target aarch64-apple-darwin   # the host target is required
.claude/skills/run-iron/build.sh && .claude/skills/run-iron/serve.sh start
cd .claude/skills/run-iron && bun run ./drive.ts < smoke.txt; cd -        # ~2 min; compare with SKILL.md
.claude/skills/run-iron/serve.sh stop
```

The smoke script is the browser-level regression suite. Every stage so far added its exit
checks to it and updated the expected output in `SKILL.md`. Do the same.

---

## 3. Decisions already taken (do not reopen)

| Decision | Choice | Where recorded |
|---|---|---|
| Units | SI magnitude plus dimension vector; display unit is presentation; geometry in pixels and radians; plain numbers dimensionless | ECM §12 phase 1 notes |
| Model edits under constraints | adjust and report `unmet`; `exact: true` on the transaction (or `exact="true"` on `<edit>`) rejects instead | CS §9.2, done |
| Path handles (stage E) | leaves on the point node, not child nodes | roadmap §7 item 4 |
| **Naming** | **`depiction` becomes `figure`** (the role, the relation becomes `figures`… see task 0 below); the definition's template child becomes `template`. entity, definition, composition, notation stay | this file; roadmap §7 item 2 |
| **Merge on link (ECM I5)** | **ask before doing anything.** UI: a dialog naming both entities, their params and ports; the person picks the survivor. Model surface: the edit is refused with an `<edit-failed>` naming both and asking for `keep="#id"`, answered on the retry. Port-name collisions are shown in the same question. No default heuristic | this file; ECM §15 I5 |
| Template references | a definition's template binds to its own `param_spec.default` leaves; instantiation rewrites them to the entity's `params.<name>` | ECM §12 phase 1 notes |
| Solved poses | stored in the document as ordinary constants and recorded as `derived_ops` in the transaction; undo applies the inverse, redo/replay apply recorded ops, nothing re-solves | CS §7, done |

**Still open, raise before building on it:** what the model sees and may edit of
solver-maintained values. Today a view shows `x="320" x.by="#U"` and a model `<set>` on such
a leaf becomes a driver (adjust-and-report). The options discussed: hide the value by
default and show only `x.by`; make such leaves read-only for `Actor::Model` unless the edit
says `drive="true"`; or both. The user has not decided. Everything else marked as an open
question in the plans: stop and ask rather than choosing.

---

## 4. What to do next, in order

### Task 0 — Rename `depiction` to `figure` (small; do first)

Mechanical, but it touches the file format, so do it before anything else adds files.

- Node type `depiction` (the definition's template holder) → `template`; component
  `Depiction { notation }` → `Template { notation }`; `definition::template_of` already
  uses the word.
- Relation kind `Depicts` / `"depicts"` → `Figures` / `"figures"` (a figure *figures* an
  entity; the view attribute the plan uses is `of="#e4"`).
- Prose: "depiction" → "figure" in `plans/roadmap.md`, `plans/entity-composition-model.md`,
  `plans/vision/09_…`, `plans/constraint-system.md`, the manifest text in
  `src-ui/model/manifest.ts`, doc comments. Leave the vision doc's history intact where it
  explains the rename (add one line: "depiction was renamed figure on 2026-10-11").
- Bump `serial::SCHEMA` to 3 and keep loading 1 and 2 (`OLDEST_READABLE`); `tests/spine.rs`
  checks the current schema and that an older one loads.
- Regenerate `tests/golden/pendulum.xml` only if the view changed (`UPDATE_GOLDEN=1`); it
  should not, as the pendulum has no figures.
- Update `SKILL.md`'s expected output if any printed line changes; run the smoke.

### Task 1 — Stage C1, compositions and entities (ECM phase 2)

Exit (ECM §12 phase 2): a 2D cartoon composition and a 3D composition side by side; linking
the cartoon box to the torus creates one entity; hovering either highlights both; Show in
frames the counterpart; undo of the link is one step; the model view lists the entity with
both figures.

Suggested order, one commit each, tests with each:

1. **Types and roots** (`registry.rs`, `document.rs`): `composition` (name, `composition`
   component with `notation` and `dims` 2d|3d; children any visual node), `model` root
   (children: entity), relation `figures` (done in task 0), `realizes`, `omits` can wait.
   Migration (ECM §5): the demo's root `group "scene"` becomes a `composition "scene"`
   with notation `free`; the pendulum group and the library stay as they are (the library
   is not a composition). Entities move under `model`. Keep every existing test green.
2. **Panels per composition** (ECM §7.1): `set_panel_viewport` carries a composition id;
   `Panels` maps panel → composition; `materialize.rs` sets `RenderLayers` from the node's
   composition ancestor; one `Camera3d` per 3D composition panel; `render_document_layer`
   loops over panels with that panel's affine and clip; picking maps the pointer to a
   panel first. The shell's `BevyPanel.svelte` gains a `composition` prop. Measure with two
   and four panels (frame time) before going further; `DocumentChanges` and `Bounds2d`
   already exist for stage H.
3. **Selection by `NodeId`** (`SelectionState`, hover): lift to entities through `figures`;
   cross-probe highlight in every panel.
4. **Show in** (`focus(composition, node)` FFI; the shell opens or activates the panel;
   Rust frames the camera) and the **Link** gesture (`Store::link(a, b, keep: Option<NodeId>)
   -> Result<TransactionInput, LinkQuestion>` so both the UI and the model surface can ask).
5. **Views**: `ViewQuery.relation` gains `Model` and `Composition(id)` (ECM §11);
   `agent.ts` chooses `Model` by default. Golden views for both.
6. **Demo and smoke**: a second, 3D composition panel; the cartoon box and the torus linked;
   the exit checks in `smoke.txt` with screenshots; `SKILL.md` updated.

Decision needed before step 2 if per-panel cost is too high on WebGL2: the fallback in ECM
§7.1 (one live composition at a time, thumbnails for the rest). Measure first, then ask.

### Task 2 — Stage E, tools and the pen tool (roadmap §4)

Independent of C1; can go before or after. Tools are state machines emitting ops and
previews; a `path` node with `point` children each carrying `transform2d`; point handles
are leaves on the point (decided). Each placed anchor is one transaction; the rubber band
is a preview; undo removes one point. Consider `understory_event_state`,
`understory_selection`, `understory_precise_hit` (roadmap §4, "Borrow"), pinned to a git
commit since the core crates are unpublished.

### Task 3 — Stage F, ports and signals (ECM phases 3 and 5)

After C1. Needs the conservative-ports decision (roadmap §7 item 5) before heat or current;
directed signals only are enough for the laser → fiber → meter exit.

---

## 5. Conventions that are easy to break

- **Every mutation is an op in a transaction.** Tools, drags and the FFI never write Bevy
  state or the `Document` directly. Drags preview through `Store::preview*` /
  `begin_drag`/`drag_to`; release is one `commit_gesture`. The reconciler rewrites entities
  from the store; `reconcile_gesture` does it in-frame for previews.
- **Authored versus derived.** The file holds only authored intent plus solved poses; cluster
  membership, resolved bindings, clip samples, previews and obligations are derived and never
  serialised. Don't add a stored field that a function over the document could compute.
- **Ids.** `NodeId`s are minted with `doc.next_ids(n)` and must be *created in ascending
  order* within a transaction (a lower id after a higher one is `BadId`). The demo reserves
  `next_ids(50)`: when you add nodes to `demo.rs`, grow that number, and keep the library
  block and the `instantiate` call last (their ids come from a scratch store). Ids print in
  base 62 (`#1`…`#Z`, then `#a`…); the smoke's expected output names ids, so adding nodes
  shifts later ids and `SKILL.md` must follow.
- **`TransactionInput` has an `exact` field**; every literal needs it (`exact: false`). Test
  files have a `tx()` helper; use it.
- **Bindings validate at apply time**: referenced nodes must already be live, so order
  `Create` ops accordingly, and a binding into a `params` field needs that parameter to
  exist.
- **Every transaction re-solves every cluster** (warm-started, cheap). A `Set` on a cluster
  variable is a driver. Solved values are snapped to 1e-9; differences under 1e-9 never
  become ops.
- **Painter order is sibling order**, not creation order. A node you want on top for picking
  must be a later sibling (the pendulum's arms get earlier keys than the joints for this
  reason).
- **`params` is the one dynamic component.** Its fields are interned `&'static str` (`sym.rs`);
  a `Set` on a parameter the component lacks is an error (parameters are created with the
  component so every inverse is a `Set` back). Validation against the definition runs on
  create, set and add_comp.
- **Quantities** are `Value::Quantity { si, dim }`, never dimensionless (that is a `Number`);
  `9.58 mW` parses in expressions and prints canonically (`0.00958 W`).
- **Views** are what the model sees; a change to their text is a behaviour change. The golden
  file guards the pendulum's; add goldens for new views.
- **Smoke before commit.** `cargo test` for both crates, then build and run the smoke script,
  then update `SKILL.md`'s expected output. Look at the screenshots; a "Loading…" frame means
  the engine never came up.
- **Plans are the record.** When something lands, add a "Done <date>: what, where, how
  verified" note to the plan's phase and a row to the roadmap's status table; record any
  deviation with the reason.
- **Don't push. Don't remove a worktree with unmerged work.** Commits end with the
  `Co-Authored-By` line the harness gives you.

Things that bit this session, so you need not rediscover them:

- `cargo test` must pass `--target aarch64-apple-darwin`; the workspace defaults to wasm.
- A worktree session's shell guard refuses compound commands; put multi-step edits in a
  script under the scratchpad and run it as one plain command.
- `drive.ts` must be run as `bun run ./drive.ts` from `.claude/skills/run-iron/`, and the
  Vite server must be up (`serve.sh start`), or every `eval` fails with `__iron is not
  defined`.
- Under SwiftShader a press at the exact point where two 2D nodes meet occasionally resolves
  to the wrong one; grab off-centre in scripts.
- Rust raw strings: `r#"…"#` ends at the first `"#`, so a string containing `id="#…` needs
  `r##"…"##`.
- A one-line `grep -rn … --include=*.rs` fails in zsh without quoting the glob.
- `tsc` writes `tsconfig.app.tsbuildinfo`; it is gitignored now, don't commit it.

---

## 6. Known rough edges (not blockers)

- The solver hits its 50-iteration cap on the frame where a chain straightens (~2 ms in the
  opt-level-1 wasm build). Fine for now; a step limit or sub-stepping (CS §5.5) is the fix if
  it shows.
- Grabbing a free-rotation node away from the point the constraints act on rotates it a
  little (least change). Role overrides with modifier keys are CS phase 4.
- `ConstraintSpec` has no units column; constraint values are plain numbers.
- Record kinds are names only; no record schemas yet.
- `behaviour` nodes on definitions and `library/*.json` built-in definition files are not
  done (they need ports).
- The "why is this moving" probe beyond `.by`, and the context ledger (CS §9.4–9.5), are not
  done.

---

## 7. How to report when you stop

What is done, with the evidence for each exit (test names, smoke lines, screenshots); what
is next; and every place a plan turned out wrong, with what you did instead and why. Keep
the plans' status notes in step with that report.
