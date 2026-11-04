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
| **4. Keyframes** (§12) | keys as children of clips; drags on animated objects write keys by the visible mode (auto-keyframe or not); the hybrid view, its left column in Svelte | the script: with auto-keyframe on, drag a shape at two times and play it; off, drag it and the whole animation moves; ‹ ◇ › |
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

## 12. Keyframes, auto-keyframe and a hybrid view (2026-11-03, replaces §6's "Animate change")

Asked for by the user, after Figma Motion: animations grouped by what they apply to,
keyframes rather than from/to clips with an invented duration, an auto-keyframe mode, and
an animated object that can still be moved. Decided with the user: keyframes are the
stored primitive; with auto-keyframe off and the playhead between keys, a drag moves the
whole animation. And the timeline becomes a hybrid view: its left column is DOM
(Svelte), its right side the canvas composition.

### 12.1 What is stored

- **A clip is one property's animation**: `timing.start` (its origin on the clock),
  `clip.prop`, its `animates` targets with their stagger offsets, and two or more
  **`key` children**. `timing.dur`, `timing.easing`, `clip.from` and `clip.to` go away;
  no migration (no old files are kept).
- **A key** (type `key`, facet `key`): `t` (seconds after the clip's start), `value` (the
  target leaf's kind), `easing` (into the next key: `linear`, `ease_in`, `ease_out`,
  `ease_in_out`, `hold`).
- **Sampling** (`time.rs`): at clock time `T`, for each target, local `u = T − start −
  offset`. Before the first key's time the clip does not drive the leaf (its authored
  value shows, as before); between keys it interpolates with the left key's easing;
  after the last it holds the last value. Durations are the gaps between keys: nothing is
  invented.

### 12.2 What a drag on an animated object does

A drag (or an inspector edit) of a leaf that a clip animates is redirected by the store
when the gesture commits (`commit_gesture`), never silently and always by what the visible
mode says. While dragging, the leaf is previewed as any leaf is, so the object follows
the pointer.

| Auto-keyframe | Playhead | The drag writes |
|---|---|---|
| on or off | on a key (within half a frame) | that key's `value` |
| on | between, before or after keys | a new key at the playhead (before the first: the clip's start moves to the playhead and every key shifts to keep its time) |
| off | between keys, or after the last | every key's value moved by the drag's delta: the whole animation moves |
| off | before the clip starts | the authored value (the clip is not driving it) |
| on, leaf not animated | after 0 | a new clip at 0 with two keys: the value before (at 0) and the dragged value (at the playhead) |
| off, leaf not animated | any | the authored value, as today |

`handle.rs` no longer locks a leaf because a clip drives it: an animated constant has a
direct handle. Leaves *bound* to an animated leaf stay locked (inverting through a clip is
phase 5's).

**Auto-keyframe** is session state, the clock's `autokey` (never saved, never undone),
toggled by the transport's button and Shift+K. While it is on, every 2D view other than
the timeline wears a coloured frame and an "Auto-keyframe" chip, so the mode is visible
wherever a drag happens (vision 05's corollary). The "Animate change" button, `K` and
`animate_change` go away: auto-keyframe and the rows' ◇ cover them, without a duration.

### 12.3 The view: a hybrid of DOM and canvas

Figma Motion draws its timeline's left column with DOM and its tracks on a canvas. Iron
does the same, which makes the timeline the first **advanced hybrid view**: one view, its
HTML part and its canvas part laid out from one plan.

- **The canvas part** (the generated composition, as now) draws what needs the renderer:
  the ruler, the bars, the keys, the playhead, the grid, with bindings, caching and
  picking. Time starts at `label_w`; the canvas draws only a plain backdrop under the
  left column.
- **The DOM part** (`TimelinePanel.svelte`, replacing the floating transport) draws the
  left column, `label_w` wide: the transport in the corner over the ruler, then one row
  per object and per property, at the y the plan gives it. Object rows name what the
  animation applies to (a shape, a component instance, an iterator's targets as one
  group); property rows name the leaf (`x`, `y`, `rot`, a colour) and carry **‹ ◇ ›**:
  the previous key, add or remove a key at the playhead (the value as it is now), the
  next key. Text, menus and editing are Svelte's; nothing there is drawn twice.
- **One layout, one scroll.** The plan's rows (key, kind, label, y, height, the clip and
  target) are served by an engine call, `timeline_rows`, with the view's vertical pan;
  the DOM column positions its rows from them, and its wheel scrolls the canvas camera
  (`view_pan`), so both halves move together.

On the canvas: per property row, a **bar** spanning the clip's keys (drag: retime the clip;
Delete: delete the clip) and a **diamond per key** (drag: the key's time; Delete: the key).
Per object row, a **summary bar** spanning all its keys (display only for now). A diamond's
x reads both the clip's start and its own `t`; the handle rule prefers the leaf of the node
the dragged piece `projects` (a new rule in `handle.rs`, generally: a view of something
edits that thing), so a diamond drag moves the key, a bar drag the clip.

### 12.4 Not in this step

Dragging an object's summary bar (it would move several clips); keys' values in the
inspector (selection through `projects`); the graph editor; snapping keys to each other and
to the playhead; phase 5's seekable keys.

### 12.5 Trim cursors (2026-11-03; wired the same day, §11)

The user captured Figma's two cursors for dragging the start and end of an object's
timeline duration. Use them for a bar's `ends` grips (and any later begin/end trim in the
timeline creation flow) in place of the resize arrow those grips show today.

| File | Look | Use |
|---|---|---|
| `src-ui/cursors/trim-start.svg` | ‹ `(` › | over or dragging a bar's **left** end (`start`; the end stays) |
| `src-ui/cursors/trim-end.svg` | ‹ `)` › | over or dragging a bar's **right** end (`dur`; `start` stays) |

- **They are SVGs, not 8× PNGs** like the other cursors: 32×32 viewBox in CSS px, with
  their own drop shadow (`feGaussianBlur` 1.5, 1 px down). Import them as URLs and use
  them directly; they are sharp at any pixel ratio without `image-set`. They don't turn,
  so they belong in `fixed` in `src-ui/cursors/index.ts`, not `turned`. Check the native
  (WKWebView) path, which re-rasterises `fixed` entries to 2× PNGs.
- **Hotspot not captured**: the capture held only the image. Use the arrows' centre,
  about (16, 15.5), and fall back to `ew-resize`.
- **Where the name comes from:** `select.rs` maps an `ends` hit to `Grip::Resize(u, 0.5)`
  (`u` 0 = left, 1 = right; see `ends_at`), so `cursor_for` gives `resize:<deg>`. Name
  `trim-start` / `trim-end` there instead, for both hover and the live `Ends` gesture.
- The raw captures were removed; the SVG files above are the decoded captures, unchanged.

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

### Phase 2 (2026-11-03)

- **The generator.** `iron_document::timeline_view`: `plan(store, clock, comp)` says
  what the timeline shows (rows by object then property, a segment per clip and target
  with its stagger offset, the ruler's extent in 5 s steps past the last clip, the tick
  spacing for the axis's `pps`, the clips in error); `reconcile` turns it into session
  ops, keyed by node name (`segment <clip> <target>`, `object <target>`, `tick 3`, …),
  so kept nodes keep their ids; `sync` runs both when the plan changed. The bridge calls
  it from `sync_document`, after the Library and Stats compositions.
- **The composition.** A session `composition` named `timeline`, notation `timeline`,
  with an `axis`. Everything is bound: segments to `label_w + (start + offset − t0)·pps`
  and `w = dur·pps`, their labels likewise; the playhead and its grip to `clock.t`; the
  background, ruler, ticks, grid lines and row headers to expressions reading no free
  leaf, so a drag cannot move them. Each segment and its label `projects` its clip (a
  new relation kind, session to content).
- **Behaviour.** `std:timeline`: rects and text select and drag; the palette is Select
  only. The select tool's session-composition branch was the Library's for every
  session composition; it is now the Library's only. A drag on a segment is named
  "retime <clip>".
- **The transport.** `TimelineTransport.svelte` over the view (from
  `CompositionTools`, which hides its tool palette for the timeline): back to 0, play
  and pause, the time (click to type a time and seek), the end. It polls `clock_read`
  every 100 ms.
- **What went away.** `bevy_app/timeline.rs` (`TimelinePlugin`, `TimelineState`),
  `panels::TIMELINE_PANEL`, `order::TIMELINE_*`, the `timeline` notation in `views.ts`
  and its tab icon. The layout key is `iron.workspace.v8`.
- **Tests.** `tests/timeline_view.rs` (4); `timeline.txt`; the smoke's scrub step drags
  the grip to 3 s (the torus reads `rot="0,4.88…,0"`).

### Phase 3, first part (2026-11-03)

- **Trim.** `std:timeline` gives rects `affords.grips="box"`: a segment's width is
  `dur·pps` and its height a literal, so the right edge changes `dur` and the left edge
  `start` and `dur` by inversion ("trim slide"). The spec's `affords.grips="ends"` (only
  the two horizontal edges) is not built; the box stands in, its vertical grips inert.
- **End values.** A numeric clip shows `fmt(from, 0)` right-aligned before its segment
  and `fmt(to, 0)` after it, text with an inline `affords` of `edit: text`. Readouts
  invert through the number (`handle.rs`), so typing edits the clip. Select one and
  press Enter; a double-click does not yet enter it (the text tool's double-click test
  misses fixed-size text here; not investigated).
- **Errors.** An object whose clips are in error shows `⚠` and the error colour in its
  header row, as its segments already did.
- **The loop band**, on the ruler, bound to `loop_in`/`loop_out` (no width without a
  loop). The transport's loop button loops every clip or turns the loop off; I and O
  set its ends at the playhead.
- **Keys.** The transport listens in the capture phase while the pointer is over the
  timeline and no text field has focus: Space, Home, End, ← and → (1/30 s; Shift, 1 s),
  I, O, F. It applies clock writes locally first, so keys faster than a round trip
  compose. This stands in for `std:timeline`'s `keys.tools`, which name tools, not
  actions.
- **Navigation.** `timeline_view::navigate` (pan by px, zoom about a point) and `fit`,
  as session ops on the composition's axis; `navigate_views` routes the wheel there
  when the view shows the timeline (horizontal pans time, vertical scrolls the rows,
  Ctrl/Cmd zooms; Shift+1 fits), and the `timeline_axis` engine call serves the
  transport's F and fit button. This is keyed on the view showing the timeline
  composition, not yet a `view.nav` context field. Ticks follow the zoom
  (regenerated), and the ruler reaches a wide panel past `t0`.
- **Cross-view highlighting.** `highlight::lit` follows `projects`: a selected or
  hovered segment lights what its clip animates in every view, and a selected
  animated node lights its segments.
- **Tests.** `tests/timeline_view.rs` 7 (an end value typed into the clip; pan, zoom
  and fit without history; the loop band); `timeline.txt` extended to trim, type,
  keys, fit and pan.
- **Not yet:** snapping (`snap.targets`), easing curves in segments, a press on the
  ruler jumping the playhead (`affords.press="jump"`), selection following `projects`
  to the clip in the inspector, the error reason in the probe, and the double-click
  above. Phases 4 (Animate change, editing an end at the playhead) and 5 (seekable
  endpoints) are not started.

### Phase 4, first part: Animate change (2026-11-03)

- **`timeline_view::animate_change_ops(store, nodes, at)`**: the last transaction's
  constant `Set`s of number, quantity, vector and colour leaves on `nodes` (on every
  content node it touched when `nodes` is empty) become clips from the old value (its
  inverse's first write to the leaf) to the new, starting at `at`, 1 s, `ease_in_out`,
  named "<node> <field>", each linked by `animates`; the authored value is set back to
  the old one, all in one transaction. A `timeline` root is made when the document has
  none. `Err` says why there is nothing to animate.
- **Engine call** `animate_change(actor)`: the selection (session nodes, such as the
  timeline's own readouts, ignored) at the playhead; `{ok, version, clips, at}`.
- **UI.** The transport's Animate change button and `K` ("key it", captured anywhere
  but a text field; no tool uses K). A note over the transport reports the clips made,
  or the reason.
- **Tests.** `tests/timeline_view.rs` 8 (animate change: the clip samples old then new,
  the authored value goes back, the timeline gains a row, one undo, nothing to animate);
  `timeline.txt` moves the ellipse at 1 s, presses K, checks 1 s and 2 s, and undoes.
- **Not yet:** editing an end at the playhead (§6, way 2), the canvas context menu
  entry, phase 5 (seekable endpoints).

### Press semantics as data, and the ruler (2026-11-03)

Asked for by the user: static pieces were selectable and draggable, with selection
chrome; the ruler should be taller with bigger labels and, as in After Effects, a
press and drag anywhere on it should move the playhead, while below it presses reach
the clips.

- **Role groups.** The generator puts every piece in one of six groups, bottom to
  top: `backdrop` (background, grid), `tracks` (segments and their labels), `values`
  (end readouts), `headers` (header column, row names), `ruler` (bar, loop band,
  ticks, tick labels, the grip) and `overlay` (the playhead's line). Each group has a
  `context` facet naming a built-in context, and those contexts' `children` rules are
  the whole of the timeline's press behaviour: `std:timeline-static` (no target),
  `std:timeline-tracks` (segments select, drag and trim; labels inert),
  `std:timeline-values` (select and edit as text, no drag), `std:timeline-ruler` (the
  scrub control) and `std:timeline-overlay` (inert). The groups are `window`s, not
  `through`: a `through` node is passed over when a node's container context is found,
  so its `children` rules would never apply. `std:timeline` keeps only the view's
  offers. Reconcile now keys nodes at any depth and reparents a kept node whose group
  changed.
- **Two pointer values made real.** `affords.select="inert"` was declared and unused:
  the picker now drops inert hits, so a press or a hover reaches what is under it.
  `affords.select="none"` *declared by a rule* (not merely unlisted) now blocks a
  press and only clears the selection, so the header column also hides segments
  scrolled under it. No other context declares either value.
- **The `track` facet** (`value`, `origin`, `scale`): with `affords.drag="track"`, a
  node that has one maps the pointer absolutely, `origin + x · scale` along
  `affords.axis`, so the press itself sets the value and a drag scrubs (a `Drive` with
  `jump`). The ruler group's `value` is bound to `clock.t`, `origin` to
  `t0 − label_w / pps`, `scale` to `1 / pps`; inversion writes the clock as a session
  preview, so scrubbing is never an edit. This is the spec's `affords.press="jump"`
  (§7) expressed with existing fields plus one facet; a slider could become a node
  with a `track`.
- **The ruler**: 40 px tall (was 24), labels 13 px (were 10), minor ticks at a fifth
  of the major spacing, a 14 px grip.
- **Tests.** `tests/timeline_view.rs` 9 (each piece's affordances from its group's
  context; the ruler's track maps local x to time and writes the clock without
  history); `timeline.txt` presses the ruler away from the grip, checks that header
  clicks select nothing, a label click selects its segment and a tick-label click
  moves the playhead.

### Bars as one target; ends, delete; a bigger ruler (2026-11-03)

Asked for by the user: the parts of a bar (rect, label, end values) were separately
selectable and deletable; a bar should select and delete as a whole, deleting its clip.

- **A bar is a group** (`bar <clip> <target>` in `tracks`), placed at the clip's start
  and sized `dur · pps` (its own `size` facet), holding its rect, label and end values,
  and `projects` its clip. `std:timeline-tracks` makes bars targets that drag, trim by
  `ends` and delete their `subject`; `std:timeline-bar` makes the rect a part (declared
  no target) and the label inert; the end values say inline that they are editable
  targets, reached by entering the bar. The `values` group is gone.
- **`affords.grips="ends"`** (new word): the selection shows two edge handles, and a drag
  on one writes the node's own `transform2d.x` and `size.w` (an `Ends` gesture), which
  invert to the clip's `start` and `dur`; the right end keeps `start`, the left keeps the
  end. A group with its own size and no layout now frames at that size.
- **Delete follows `projects`**: `parts::delete_subject_ops` deletes what a session node
  projects; the timeline regenerates without the bar, and undo brings both back.
  Reconcile deletes only the outermost of the nodes that go.
- **Press rules, generally**: a double-click enters only into a target, and a press on a
  declared non-target part of an entered target selects that target.
- **The ruler** is 80 px tall with 23 px labels (75% larger), longer ticks, and a 26 px
  playhead head low on it.
- **Tests.** `tests/timeline_view.rs` 10 (a bar trims by its ends and deleting it deletes
  the clip, then undo); `timeline.txt` selects the bar from its words, trims by the end,
  enters the bar to type a value, and deletes and restores the clip.

### Keyframes, auto-keyframe and the hybrid view: §12 (2026-11-03)

Built as §12.1–12.3 say, with §12.5's cursors.

- **Storage** (`facet.rs`, `registry.rs`, `time.rs`): a clip is `timing.start`, `clip.prop`
  and `key` children (`key` facet: `t`, `value` of any kind, `easing`); `time::keys`,
  `start`, `value_at`, `length`; sampling holds the authored value before the first key
  and the last value after the last. The demo's clips are keys (its op count 298 → 302).
- **A drag on an animated object** (`timeline_view::key_ops`, called first by
  `commit_gesture`): §12.2's table, each row a test. A leaf the gesture did not change
  writes nothing. `handle.rs`: a clip-driven constant is Direct, and a "projects"
  preference picks the source leaf of what the dragged piece projects (a diamond its
  key's `t`, a bar its clip's `start`).
- **Auto-keyframe**: the clock's session `autokey`; the transport's ◉ and Shift+K; a red
  frame and an "Auto-keyframe" chip on the other 2D views (`select.rs`,
  `draw_autokey`). Animate change, `K` and `animate_change` are gone.
- **The hybrid view**: the canvas composition draws the ruler, bars (one per clip and
  target, spanning its keys), diamonds (`keys` group, `std:timeline-keys`: target, drag,
  delete) and object summary bars; `TimelinePanel.svelte` draws the left column (the
  transport in the corner, a row per object and property, names that select and light
  their object, ‹ ◇ ›) from `timeline_rows(panel)`, and its wheel scrolls the canvas
  view (`view_pan`). `timeline_key(action, clip)` steps to and toggles keys.
- **Trim cursors** (§12.5): `affords.cursor` (new field: `auto`, `trim-start`,
  `trim-end`), which a clip's first and last diamonds carry inline, and the `ends` grips'
  hover and drag name the same; `src-ui/cursors/index.ts` serves the SVGs (hotspot
  16,15.5, `ew-resize` fallback; native rasterises them at 2×).
- **Tests.** 252 `iron_document` tests (`timeline_view` 13, rewritten: the §12.2 table,
  the row buttons, the cursor data). `timeline.txt` rewritten: the ruler scrub, a bar
  retimed, the last key's cursor and drag, the column's name, ‹ › and ◇, a drag on the
  square with auto-keyframe off (both keys move) and on (a key added), delete and undo.
  The full smoke passes; its expected ids after the demo moved by the four new keys.
- **Also:** `vite.config.ts` stops watching `.agents/`, `plans/` and `target/`: another
  agent's worktree edits had reloaded the app mid-smoke.

### CSS px views; the timeline's own scroll (2026-11-03)

Found by the user on a 2× display: the DOM column was half its intended width and the
transport did not fit, because a 2D view mapped 1 document px to 1 *device* px, so every
composition drew at half its CSS size on Retina while the DOM beside it did not.

- **A view's zoom is CSS px per document px** (`ViewCamera::zoom`; `PanelView.zoom` is
  that times the pixel ratio, `ViewCamera::scale`), for every 2D view: 100% now means 1
  document px per CSS px, Figma's 100%, and a hybrid view's DOM half places itself at
  `document px · zoom` with no `devicePixelRatio`. The zoom keys, fit, pinch and "Show
  in" work in it; 3D views (a field of view) and the gizmo (already scaled) are
  unchanged. At 1× nothing changes, so the smoke's expected output does not.
- **The ruler at CSS size**: 56 tall (the transport's height), 13 px labels, ticks and
  the playhead's head to match: what the user had on a 2× display, plus room for the
  transport.
- **The timeline never pans in 2D.** Its rows scroll on the axis (`axis.y0`, a session
  leaf, `timeline_view::scroll`): row pieces are bound to `y − y0` and pass under the
  ruler, which stays. The wheel, the column's wheel (`timeline_axis` `{"scroll": dy}`,
  replacing `view_pan`), middle-drag and Space-drag all scroll rows on y and pan time on
  x; a camera pan left on the timeline is reset.
- **Tests.** 253 `iron_document` tests (`timeline_view` 14: scrolling moves the rows
  under a ruler that stays, without history). `timeline.txt` (rows 24 px higher) and the
  full smoke pass; checked at 2× by screenshot, before and after a middle-drag.
- **The playhead never goes before 0** (found by the user): a `track` has a range
  (`track.min`, `track.max`, no upper end when `max` ≤ `min`) that a press or drag keeps
  its value in, and the ruler declares `min` 0; `clock::set` clamps `t` at 0 too.

