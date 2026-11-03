# The timeline as a composition

**Status:** Draft 2026-11-03; open questions decided by best guess (§10), to build. To be built after the scope menu
(`scopes.md` phase 3) and the narrative panel's phases 1–2, or in a gap between them:
nothing depends on it until the narrative's moves and clock.
**Replaces:** `src/bevy_app/timeline.rs` (a special-cased panel), the `timeline`
notation and panel kind, and the `TIMELINE_*` vector layer orders.
**Background:** vision 05 (three kinds of time, and its 2026-11-02 amendments);
[`../notes/narrative-layer-and-storage-primitives.md`](../notes/narrative-layer-and-storage-primitives.md)
§6 and §6.7 (seekable values, narrative clocks); [`panels-and-session.md`](panels-and-session.md)
(the session tier, "an inspector is a bound composition", and "the playhead… would move
into the session root"); [`affordances.md`](affordances.md) and [`contexts.md`](contexts.md)
(behaviour as context data); [`document-spine.md`](document-spine.md) §5.10 (a bound leaf
has a handle only if its binding inverts); [`free-running.md`](free-running.md) (the
Stats composition, live sources).

---

## 1. What a clip is

**A clip is a node.** It is not a component in the ECM sense, nor an entity. It is a
node of type `clip`, a child of a `timeline` root (`registry.rs`), with three facets:

| Facet | Fields | Meaning |
|---|---|---|
| `timing` | `start`, `dur`, `easing` (`linear`, `ease_in`, `ease_out`, `ease_in_out`) | when, in seconds on the clock |
| `clip` | `prop` (a leaf path such as `transform2d.x`), `from`, `to` | what changes, and between which values |
| `name` | | optional |

Its **targets are relations**, not fields: an `animates` relation from the clip to each
node it drives, whose `data.offset` staggers that target's start (vision 05 §3). One
clip can drive the same property on several nodes. `time::sample` evaluates every clip
at the clock's time and publishes the values into the reactive graph's animated layer,
beneath gesture previews and above authored constants. Before a clip starts, its target
shows its authored value; after it ends, the target holds `to`.

So a clip is **one property changing over one interval, from one value to another**:
two keyframes with an easing between them, applied to one or more nodes. A motion with
more keys is a sequence of clips on the same property.

**The view is Blender's dope sheet, not a lane of clips.** Users think in "this object's
x changes from 1 s to 3 s", which is a property of an object over time. So the timeline
lists **animated properties grouped by object**, as Blender's dope sheet and After
Effects' layers do, and shows each clip as a segment on the row of each property it
drives. A clip with three targets appears on three rows, drawn as linked. Blender's
**graph editor** (value against time for one property) is a later mode of the same
composition (§9); because clip endpoints are seekable (note §6.7), the curve can always
be drawn.

**Relation to the narrative.** A narrative move is an arrow between states; its derived
sub-moves (note §6.2) are, per leaf, exactly what a clip is. Stored clips are the
primitive the narrative builds on: later, a move's derived sub-moves appear on the same
rows, marked as derived, and overriding one stores a clip-shaped override.

## 2. What it does today, and why that is not enough

`timeline.rs` draws one bar per clip in document order, a fixed 30 s grid and a
playhead. A press anywhere in the panel moves the playhead. That is all: no labels (a
bar does not say what it animates), no playback control, no editing, no zoom, no link to
the canvas. It is also outside the system every other view uses: it draws in screen
space into a special panel, so selection, contexts, snapping, chrome, undo and the model
surface do not apply to it.

## 3. The design in one paragraph

The timeline is a **2D session composition** generated from the document's clips,
shown in an ordinary composition view, with its behaviour declared by a built-in
context, `std:timeline`. Its pieces are ordinary nodes (rects, text, paths) whose
positions are **bound** to the clips' leaves and to a session **clock**. Dragging a bar
edits the clip through binding inversion, as one undoable transaction, because that is
what dragging a bound node already does. Dragging the playhead edits the clock the same
way. Nothing in it is special-cased except the generator that builds it.

## 4. The engine changes

### 4.1 The clock becomes a node

Today the playhead is a field, `Store::time: f64`, set with `set_time`. Bindings can
only read leaves (`#node.facet.field`), so nothing can say "the playhead line's x is the
time times pixels per second", and a drag cannot write the time back, because binding
inversion writes a leaf.

**Proposal:** a `clock` node in the session tier, with a `clock` facet:

| Field | Kind | Meaning |
|---|---|---|
| `t` | Number (s) | the time clips are sampled at |
| `playing` | Bool | advancing with wall time |
| `rate` | Number | seconds of `t` per wall second (1 = real time) |
| `loop_in`, `loop_out` | Number (s) | the loop range; equal means no loop |

- **Session, so never saved and never undone.** Session ops skip the undo stack and
  `serial::save` skips the session (panels-and-session.md). Scrubbing is not an edit.
- **`Store::time()` reads `clock.t`; `set_time` writes it** as a session op, so the
  existing callers and `tests/time.rs` keep working. Any change to `clock.t` (an op, a
  gesture preview, the player) resamples the clips.
- **One clock per store** for now, created when missing like the Stats composition.
  Every timeline view shares it. Several clocks (a narrative's, a preview's) are the
  narrative's later concern; the clock being a node is what makes them possible.
- **The player** is a bridge system: while `playing`, it advances `t` by
  `Δwall · rate`, wraps at `loop_out`, and stops at the end of the last clip otherwise.
  Playback already changes leaves every frame, so views showing animated content render
  without a live source (free-running.md §2); a playing clock is also listed as one so
  the timeline view itself redraws.

### 4.2 Axis leaves are fixed during a drag

A bar's x reads the clip's start and the timeline's pan and zoom:
`x = label_w + (start − axis.t0) · axis.pps`. The handle rule (`handle.rs`) sees three
free numeric leaves the value is linear in (`start`, `t0`, `pps`), finds none of them of
the dragged leaf's kind, and locks the drag.

**Proposal:** an `axis` facet (`t0`, the time at the left edge; `pps`, pixels per
second; `label_w`, the header column's width), and a rule in `handle.rs`: **leaves of an
`axis` facet are knowns for a drag, never handle sources**, like clip-driven leaves. A
drag on a projection edits what it shows, never how it is being looked at. The rule is
general: an XY graph's axes are the same thing, and a graph component's points then
drag their data the same way.

Pan and zoom edit the axis leaves directly (§6). Zoom to fit needs the view's width,
which the bridge has from the panel rect and writes into `pps`.

### 4.3 Generated content

A generator in `iron_document` (beside `standard.rs`'s `stats_ops` and `library_ops`)
builds the timeline composition as session ops from the document's clips. Following
panels-and-session.md's inspector:

- **Structure is regenerated; values are bound.** The generator reruns when the set of
  clips, their `animates` relations, their `prop`, or their targets' names change, and
  when the ruler's tick spacing changes with zoom. It does not rerun when a clip is
  retimed: bars follow through their bindings. Retiming by drag therefore costs one
  transaction and no regeneration.
- **Reconciled by key**, not rebuilt: a row is keyed by (target, prop), a segment by
  (clip, target), so session ids, and the selection, survive regeneration.
- **Each segment `projects` its clip**: a session → content relation (allowed:
  references go one way, session to content). Selection, the inspector and the probe
  follow it to the clip.

## 5. The composition

```
 ┌ header (label_w) ┬ ruler: 0s    1s    2s    3s    4s  ▼ playhead ────────┐
 │ ▾ square         │                                      │                 │
 │     x            │       [slide 500 → 700 ─────────]    │                 │
 │ ▾ torus          │                                      │                 │
 │     rot          │            [spin ╱‾‾‾‾‾‾‾‾╲───────────│──────]          │
 │ ⚠ shadow  x      │     [bad clip: x is bound]           │                 │
 └──────────────────┴──────────────────────────────────────┴─────────────────┘
   ▶ ⏸  ⟲   0:01.84 / 0:04.00   1×
```

| Piece | Nodes | Bindings |
|---|---|---|
| **Ruler** | rect, tick lines, text labels | ticks regenerated per zoom (1, 2, 5 × 10ⁿ s, about 80 px apart) |
| **Playhead** | a line and a grip | `x = label_w + (#clock.t − t0) · pps`, which inverts to `clock.t` |
| **Object rows** | text (the target's name), a disclosure toggle | name bound to the target's `name.text` |
| **Property rows** | text (`x`, `rot`, …) | |
| **Segments** | a rounded rect per (clip, target) | `x` from `start + offset`, `w = dur · pps` |
| **Easing curve** | a path inside each segment | regenerated when `easing` changes |
| **End values** | text at each end of a segment | `fmt(#clip.clip.from)`, `fmt(#clip.clip.to)`: readouts invert, so typing edits the value |
| **Errors** | segment drawn in the error colour, with a mark in the header | from `Store::time_errors()`; the reason in the probe and the tooltip |
| **Loop range** | a band on the ruler | bound to `loop_in`, `loop_out` |
| **Transport** | play/pause, loop toggle, time readout, rate | the readout is `fmt(#clock.t)` and inverts, so typing a time seeks |

A segment's y is bound to a literal expression, which reads no free leaf, so the y axis
is locked during a drag (spine §5.10) and segments move only in time. A clip staggered
across targets has its per-target `offset` in relation data, which is not a leaf: in v1,
dragging any of its segments moves the clip's `start` for all of them, and the offset is
edited in the inspector (§10 Q3).

## 6. What you can do in it

**Navigate.** Scroll scrolls the rows; Shift-scroll or a horizontal trackpad swipe pans
time (`t0`); Ctrl/⌘-scroll or a pinch zooms time about the pointer (`pps`); `F` fits
every clip. Rows scroll with the view's camera; time pans and zooms through the axis, so
text and corner radii never stretch.

**Play and seek.** Space plays and pauses; the ruler and the playhead scrub; Home and
End go to 0 and to the last clip's end; ← and → step one frame (1/30 s), with Shift one
second; `I` and `O` set the loop range at the playhead; typing in the time readout
seeks.

**Retime.** Drag a segment to move its clip (`start`); drag its right edge to change
`dur`; drag its left edge to change both, keeping the end where it is. Each drag is one
transaction ("retime spin"), undone with ⌘Z. Segments snap to the playhead, to other
clips' starts and ends, and to whole seconds, through the existing snapping
(`snapping.md`), with the targets declared by `std:timeline`. Alt-drag copies the clip.

**Edit values and easing.** Select a segment and the inspector shows its clip: timing,
easing, prop, from, to and its targets. Type in an end value on the segment itself. The
easing menu is in the inspector and on the segment's context menu.

**See and select across views.** Selecting a segment selects its clip and highlights its
target in every canvas view; selecting a node in the canvas highlights its rows and
scrolls them into view. Clicking an object's name selects the object. A node that is
clip-driven at the current time says so in the probe, with "open in timeline"; a drag on
it is refused with that reason, as now.

**Create clips, explicitly.** Vision 05's corollary stands: a keyframe is never created
silently. Three ways:

1. **Animate the last change.** Select an object, change it in the canvas or the
   inspector, then "Animate change" (in the timeline's toolbar and the canvas's context
   menu). The last transaction's number, vector and colour `Set` ops on the selection
   become clips from the old value to the new one, starting at the playhead, 1 s long,
   and the object's authored value goes back to the old one: one transaction. This is
   the note's "capture after the fact" (§6.5), and expected to be the common path.
2. **Edit an end at the playhead.** With a segment selected and the playhead at or past
   its end, a drag on its target in the canvas writes the clip's `to`; at or before its
   start, its `from`. The canvas view shows a coloured frame and "editing *slide* at
   1.84 s" while this holds (note §6.5's view source); Escape leaves it. Inside the
   interval the drag is refused: that would need a keyframe in the middle, which is two
   clips.
3. **The model.** `<edit>` already creates clips and `animates` links (`manifest.ts`).

**Delete, copy, duplicate.** Delete removes the selected clips; copy and paste go through
the existing clipboard, which already places a pasted clip in the first timeline
(`copy.rs`).

## 7. `std:timeline`, the context

The timeline's behaviour is context data, so nothing above is code except mechanisms.
Proposed rules (fields marked *new* must pass `contexts.md` §4.1's admission test):

| Match | Declares |
|---|---|
| segments | `affords.select="target"`, `affords.drag="point"`, `affords.grips="ends"` (*new value*: the two horizontal edges of the box grip), `snap.target="yes"` |
| playhead grip, ruler | `affords.drag="point"`; a press on the ruler first moves the playhead to the pointer (*new*: `affords.press="jump"`) |
| header text, ticks, curves | `affords.select="through"` (they pass the press to what is under them) |
| `view` | `palette.tools="select"`, `keys.tools` for the transport (Space, Home, End, ←, →, I, O, F), `snap.targets="playhead edges seconds"` (*new*), `view.nav="scroll-y axis-x"` (*new*: what scroll and pinch do; `std:free-2d` pans and zooms the camera), `inspector.sections="clip timing targets"` |

`view.nav` and `affords.grips="ends"` pass the admission test: each is a human-interface
choice, wanted differently by at least two contexts (a free canvas, a timeline; later a
table's column edges), with a mechanism to read it.

## 8. What goes away

`src/bevy_app/timeline.rs`, `TimelinePlugin`, `TimelineState`, `panels::TIMELINE_PANEL`,
the `order::TIMELINE_*` constants, the `timeline` notation and its entry in
`views.ts` (`NOTATIONS`, `FIXED_VIEWS`). The default layout's Timeline view becomes
`{ source: 'timeline', notation: 'composition' }`, the session composition's name, as
Stats is `{ source: 'stats', notation: 'composition' }`. The layout key bumps so saved
layouts reset once.

## 9. Phases

| Phase | Builds | Done when |
|---|---|---|
| **1. Clock and axis** | the `clock` node and facet; `Store::time`/`set_time` through it; the player; the `axis` facet and the handle rule (§4.2) | Rust tests: a session op on `clock.t` resamples; a binding reading `clock.t` follows it; a drag on `x = label_w + (start − t0)·pps` inverts to `start` with the axis held; `clock` is not saved and not undone |
| **2. Parity, as a composition** | the generator, keyed reconcile, `projects` relations; `std:timeline`; rows by object and property; scrub, play, pause; retime by drag with undo; delete `timeline.rs` | a driving script (`timeline.txt`): scrub moves the square; drag a segment and the clip's start changes; undo restores it; `SKILL.md` updated |
| **3. Editing** | trim edges, snapping, keys, zoom and pan, end values, easing curves, errors, loop, cross-view selection and highlighting | the script extended to each |
| **4. Creating** | Animate change; edit an end at the playhead | the script: change a rect's x, animate it, play, see it move |
| **5. Seekable endpoints** (demo 2 of the note's list) | `from`/`to` may be bound when a trial evaluation at the clip's end reads only document leaves and clocks (note §6.7); otherwise the clip errors with the source that blocked it | Rust tests both ways; the script: a clip whose `to` follows a signal derived from the clock draws its curve; rebinding `to` to a slider is refused, with the reason |

Later, not in this spec: the graph editor mode (value curves for one property, Blender's
graph editor); read-only bands (a pinned signal's sparkline, a machine's state as
bands; note §6.7, to be tried); rows for a narrative's derived sub-moves; a narrative
position axis in place of seconds; several clocks.

## 10. Decisions (best guesses, 2026-11-03)

The user asked for best guesses on the open questions and to build on them. Each can be
revisited; none is expensive to change.

1. **Rows by object, then property.** It matches how people describe motion. A clip
   with several targets shows on each of their rows, linked.
2. **One clock per store.** All timeline roots sample at it, as today. Several clocks
   come with the narrative.
3. **Stagger offsets stay in relation data**, edited in the inspector for now. Dragging
   any segment of a staggered clip moves the clip's `start`.
4. **The ruler scrolls with the rows in v1.** With few rows this rarely matters; a
   sticky ruler waits for the camera as session leaves.
5. **Frame steps are 1/30 s**, a constant in the player.
6. **"Animate change"** is a button in the timeline's toolbar (HTML over the view, as the
   composition settings button is) and a key, `K` ("key it"), in canvas views.

## 11. Done

### Phase 1 (2026-11-03)

- **The clock.** `clock` facet and type in `iron_document`; the session root's children
  are now `selection` and `clock`. `Store::clock()` finds it, `ensure_clock()` creates it
  (and the session root) at the current time. `set_time(t)` writes `clock.t` as a
  session op; `time()` is the `t` the clips were last sampled at, which follows the
  clock as read, so a gesture previewing `clock.t` scrubs and Esc puts it back
  (`sync_clock`, run after session ops, previews, unpreviews and gesture ends).
- **Session ops re-evaluate.** `apply_session` used to rebuild the reactive graph only for
  creates and deletes; a session `Set` now invalidates its dependents (or rebuilds when it
  changes a binding), so anything bound to a session leaf follows it.
- **Gestures on session leaves.** `commit_gesture` splits the driven leaves of session
  nodes into `apply_session`; the rest is the transaction. Dragging a playhead line bound
  to `clock.t` therefore commits no transaction.
- **The axis.** `axis` facet, allowed on `group`, `empty` and `composition`. `handle.rs`
  treats its leaves as knowns, so a bar at `label_w + (start − t0)·pps` inverts to the
  clip's `start`.
- **The player.** `document_bridge/clock.rs`: while `playing`, advance by
  `Δwall · rate`, wrap in a loop range, otherwise stop at the end of the last clip; the
  compositions of animated nodes are live sources meanwhile. The old timeline panel
  (`bevy_app/timeline.rs`, until phase 2 replaces it) scrubs by writing the clock and
  draws `store.time()`.
- **Engine calls.** `clock_read()` → `{ok, id, t, playing, rate, loop_in, loop_out, end}`;
  `clock_set(json)` sets any of those fields.
- **Tests.** `tests/clock.rs`: a session op resamples; a binding reading `clock.t`
  follows it (and panning the axis moves it); a drag on a bar retimes the clip with the
  axis held, and undoes; dragging the playhead scrubs without history, and Esc restores
  it; the clock is not saved and not undone.
- **Not yet.** A content binding can read the session clock, but the reference is to a
  session id, which is not stable across a reload. Phase 2's bindings live in the
  session composition, so this does not matter there; a content binding on the clock
  (the narrative's later concern) needs a stable name, e.g. a `clock()` function in the
  expression language.

