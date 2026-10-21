# Free-running compositions and the Stats composition

**Status:** Approved and built 2026-10-18 (decisions taken with the user in
conversation). Amends `view-textures.md`'s render on demand.

## 1. The problem

Render on demand (`view-textures.md`, migration step 1) renders a frame only when
something the window shows changed, and a 3D view's camera only when `Doc3d` content
on its layers moved. That is right for a static diagram and wrong for anything that
changes by itself:

- **The gas in the piston demo freezes** a few frames after the last input. Its
  particles are Bevy entities with no `Doc3d`, so `sync_view_images` never sees them
  move; the simulation keeps stepping, unseen.
- **The FPS overlay** was the one other thing that updated by itself. It was `bevy_ui`
  text, which kept `bevy_ui`, `bevy_ui_render`, `taffy`, `bevy_text` and Bevy's
  embedded default font in the wasm for one corner readout.

## 2. Free running

A composition is **free running** when its views render every frame instead of on
demand. It is decided per frame, not stored, from two inputs:

- **Live sources.** A producer that changes something every frame marks the
  composition its nodes are in (`LiveSources`): the gas producer while it runs, the
  Stats producer. A component instance's internals are drawn in the host composition,
  so a live component makes its host's composition a source.
- **The run mode**, `composition.run`, stored on the composition and undoable:

| `run` | Meaning |
|---|---|
| `auto` (default) | Free running when it is a live source, or anything it shows is free running (bubbling, below) |
| `always` | Free running regardless |
| `on_demand` | Never free running. What it shows does not bubble out of it, and producers seen only through it are paused |

**Bubbling.** Liveness flows outward through embeds: a composition embedding a free
running one is free running too, up to the composition a panel shows. Pulling an
animated 2D composition into a static one makes the static one animate. `on_demand`
stops it: set on a deep, unimportant visual it keeps that visual from waking every
view that contains it; set on the outer composition it freezes the whole view.

`composition::reach` (iron_document) computes it, as a pure function over the
document: from a view's root it walks embeds depth first, carrying whether an
`on_demand` composition lies on the path. A composition reached on an open path that
is a source or `always` makes the view live; the sources reached that way are the ones
**running**. Embeds cannot loop (`validate_embed`), so the walk terminates.

**Frozen means paused.** A producer runs only while some view reaches its composition
on an open path (`Liveness::running`). An `on_demand` view of the gas shows a still
gas that does not advance behind the picture, and redraws on input like any static
view.

### 2.1 What a free-running view does

- Its 3D camera stays active (the panel's, and each 3D embed's camera reached on an
  open path), so `sync_view_images` renders it every frame.
- `decide_redraw` renders the frame while any visible view is live.
- A 2D view re-renders when its display list changes, as today; a running producer
  changes it.

Only live views pay. A static view beside a running one keeps its texture.

**Not covered.** A per-instance override (freeze one component instance and not its
siblings) is not built; the run mode is per composition. Timeline playback already
changes leaves every frame and needs no source.

## 3. The composition settings menu

Each composition view gets a settings button in its top right corner (HTML over the
canvas, `CompositionSettings.svelte`, a bits-ui `DropdownMenu` like
`CompositionTools`): **Run: Auto / Always / On demand**, with what Auto currently
resolves to ("running: gas"). Choosing one sets `composition.run` in one transaction
(`document_set_run_mode`; a session composition is set with a session op).
`document_composition_info` reports a composition's mode and whether it is live.

## 4. The Stats composition

The FPS overlay becomes a composition, drawn by Vello like everything else:

- **Built in, never saved.** A session-tier composition named `stats`, built when
  missing, like the Library's (`standard::stats_ops`). It holds a `definition` with
  card and four monospaced text lines (`STATS_LINES`: fps, renderer, build, canvas).
  The producer publishes each line's `text.content` directly. Numbers on `port_spec`s
  bound with `fmt` were the first design; a `port_spec` needs a `definition` under a
  `library` root, and `make.rs` takes the first `library` root it finds, so a session
  one could capture new definitions.
- **The `stats` producer** publishes the frame diagnostics onto those `port_spec`s,
  four times a second (the text reshapes only when its string changes), and marks the
  composition a live source while it is shown: a visible Stats view keeps the window
  free running, which is what makes its FPS a measurement of rendering. Set it to On
  demand to get the old behaviour (numbers update on input only).
- **Docked** to the right of the timeline in the default layout, where the overlay
  was; openable anywhere from the views menu.

With it, the FPS overlay and its `bevy_ui` dependencies go: the `bevy_ui`,
`bevy_ui_render`, `bevy_text` and `default_font` features, the UI change query in
`decide_redraw`, and `IsDefaultUiCamera`. `bevy_sprite_render` stays: the view
composites are `Material2d`s.

## 5. Exits

- The gas in the piston demo moves continuously with no input, embedded or in its own
  view; set to On demand, it stops, and resumes when set back.
- A static 2D composition in front renders nothing at rest.
- A Stats view shows live FPS and canvas size; the smoke script reads them.
- `bevy_ui`, `taffy`, `bevy_ui_render` and `bevy_text` are absent from
  `cargo tree` for the wasm target; the shipped size before and after is recorded here.
- `composition::reach` has native tests: auto/always/on_demand, bubbling through
  nested embeds, on_demand blocking, running sets.

## 6. Done

**2026-10-18.**
- Store: `composition.run` (`auto` default; old files load unchanged, the model view
  shows `run="auto"`), `composition::{RunMode, run_mode, reach}`, `standard::stats_ops`;
  `tests/free_running.rs` (8 tests). All `iron_document` tests pass; the
  `cartoon-view.xml` golden gained the attribute.
- Engine: `document_bridge::liveness` (`LiveSources`, `Liveness`, after
  `collect_embed_views`), `EmbedView::open`; `sync_view_images` keeps live cameras
  active; `decide_redraw` renders while anything visible is live; the gas runs only
  when reached on an open path; `document_bridge::stats` replaces `fps_overlay.rs`.
  Calls `composition_info` and `set_run_mode` (web_ffi and dispatch), `__iron.runMode`
  and `__iron.compositionInfo`.
- UI: `CompositionSettings.svelte` on every composition view; Stats docked right of the
  timeline (layout key v6, so a saved layout resets once).
- Verified in headless Chrome: the gas moves with no input (it froze before), freezes
  on On demand and resumes on Auto; Stats on demand leaves an idle window rendering
  nothing. Smoke: only expected changes (44 nodes, `run="auto"`, the torus after a
  narrower timeline); its other differences were measured on HEAD first.
- **Size** (shipped WebGPU build, `build-wasm.sh webgpu`): 22,539,391 → 19,657,522
  bytes raw (−12.8%), brotli-11 4,831,650 → 4,362,367 (−9.7%), gzip-9 7,120,872 →
  6,332,526. `bevy_ui`, `bevy_ui_render`, `taffy`, `bevy_text`, `bevy_input_focus` are
  out of the wasm graph.
- Not done: per-instance overrides; the timeline as a composition (its own plan).

**Fixed 2026-10-20: a stale live-solve pose after undo.** With Stats showing, its
producer publishes every frame, and `Store::publish` re-solves every constraint cluster
live, writing the pose into the gesture overlay. Outside a gesture nothing cleared
those previews, so once an undo removed a mate, the meter's y kept its mated value (the
smoke's plugging section read `[545,475]` for `[545,175]`; HEAD had no producer in that
tab). `Store::after_transaction` now drops the overlay when no gesture owns it (no
gesture, drivers or fingers; a gesture's own commit keeps its previews) and re-solves
live against the new document. Regression test: `tests/constraints.rs`
`a_live_solve_leaves_nothing_behind_once_a_transaction_applies`. The gas producer could
cause the same before this work, in the Piston tab.
