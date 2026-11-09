# Machines: component states, transitions, and how they meet the timeline and the narrative

**Proposed 2026-11-08.** Waits on the user. The first slice of the state machine that
affordances.md §8.4 promised ("a later spec builds vision 06's state machine … one
animation system, not two"), sized for its first user: [`plot.md`](plot.md)'s appear
and disappear. Built on the recorded decisions it cites; where it goes past them it says
so (§9).

## 0. What is already decided (and where)

| Decision | Source |
|---|---|
| A component's state machine is part of its design: states, and transitions that each hold a **sub-timeline**, Rive-style | narrative note §6.7 (`notes/narrative-layer-and-storage-primitives.md`), replacing vision 05 §4's `<play>` |
| Timelines say *how* a change looks, machines say *when* it plays; signals may be machine inputs | vision 05 §5.1 |
| A transition has a duration but no start: its start is an event | vision 05 §1–2 |
| The current state is an enum value rules and bindings read; progress is a scalar on a track; states are discrete (no dragging between), transitions are scrubbable | affordances.md §8.4, vision 06 §3.2 |
| A pinned state while authoring is editor-local, not a document edit, and loud | vision 06 §3.3 |
| A narrative never reaches inside a component: its move **sets the state**, and the component's transition is the derived sub-move, overridden by exception | narrative note §6.7 |
| State changes in a narrative are **held overrides** with an extent (SMIL `<set>`, Unreal Keep/Restore State); playing forward runs the transition, scrubbing snaps; "if in A, go to B" is not used | narrative note §6.7 |
| Moves' **ends** must be seekable; anything not seekable is event-driven and belongs to the machine | narrative note §6.4 |
| Pedagogical components (hint, quiz) are fine if their states live in the narrative's compositions; a model component's state is design | narrative note §6.7 |

## 1. Decisions at a glance (proposed)

| # | Decision |
|---|---|
| M1 | A machine is a `machine` child of a **definition**: `state`s, `transition`s, and `channel`s, the numbers its states set. Template parts bind to channels, never to "the state" directly for continuous values. |
| M2 | The **current state is an in port** of enum kind, so it is an argument (`args.vis="shown"`): the agent, the inspector, a binding, a clip key and a narrative override all set it the one way arguments are set. Saved and undone like any argument. |
| M3 | A state is a **pose**: a value for every channel. A transition with nothing written **derives** its motion: every channel that differs eases from the old pose to the new one with the transition's duration and easing (the state diff, narrative note §6.2). |
| M4 | A transition may refine that with **tweens** (a channel, a start and a length within the transition, an easing), and for real keyframes, clips. That is the sub-timeline. |
| M5 | Every transition has a **`progress` input**, 0 → 1. The transition's tweens are written against progress, not seconds: the component owns **what** happens; whoever drives progress owns **when and how fast**. (Decided 2026-11-08.) |
| M6 | **Who drives progress is the only difference between cases.** A clip on a clock (the document timeline, a lesson's narrative clock) drives it: the transition scrubs, forwards and backwards, for free. Nothing drives it (a click, the inspector, the agent): the machine plays 0 → 1 itself over the default duration, on presentation time; scrubbing past it shows the end state. Channel values are a **session producer** either way: never saved, never undone; picking and bounds use them. |
| M7 | **One writer per leaf.** A channel belongs to its machine: clips key the *state*, not the channels. A leaf is bound, animated, or machine-driven, never two. |
| M8 | Interrupting a transition **retargets from where things are drawn** (CSS transitions' rule), so reversing halfway is smooth. |

## 2. Markup

As built (phase 1; the names a document uses today):

```xml
<definition name="Plot">
  <port_spec name="vis" kind="value" dir="in" signal="shown"/>   <!-- M2 -->
  <channel name="fade"   value="1"/>        <!-- channels belong to the definition -->
  <channel name="reveal" value="1"/>
  <channel name="lift"   value="0"/>
  <machine name="vis">                      <!-- named for the input it follows -->
    <state name="hidden" fade="0" reveal="0" lift="12"/>
    <state name="shown"  fade="1" reveal="1" lift="0"/>
    <transition name="appear" from="hidden" to="shown" ms="900" ease="ease_out">
      <tween channel="fade"   at="0"   ms="250"/>
      <tween channel="lift"   at="0"   ms="350" ease="ease_out"/>
      <tween channel="reveal" at="250" ms="650" ease="easy_ease"/>
    </transition>
    <transition name="disappear" from="shown" to="hidden" ms="300" ease="ease_in"/>
  </machine>
  <template notation="free">
    <group name="body" appearance.opacity="{= #fade.channel.value}" y="{= #lift.channel.value}">
      <path name="series 1" d="{= 'M ' + points(…)}" trim.end="{= #reveal.channel.value}"/>
    </group>
  </template>
</definition>
```

- `kind="state"` is a port kind whose values are the machine's state names (an enum);
  `signal` is the default state. The port and the machine share a name.
- `disappear` writes no tweens, so it is derived (M3): all three channels ease together
  over 300 ms.
- `from="*"` matches any state; a change with no matching transition **snaps** (and the
  editor lists such pairs, so a missing transition is visible, not silent).
- Channels are numbers in phase 1. Colours and vectors ease the way the interpolator
  will (affordances.md §8.2 `color`), later.

As types: `machine` (`name`, `initial`), children `channel` (`name`, `channel.value`:
the resting value used before any state applies), `state` (`name`, then one field per
channel: an open record like `args`), `transition` (`from`, `to`, `ms`, `ease`), whose
children are `tween`s (`channel`, `at`, `ms`, `ease`) and optionally a `timeline` of
clips for keyframed motion (overshoot, a pulse). A clip in a transition's timeline
animates a channel, with key times in seconds from the transition's start; key values
may be omitted on the first and last key to mean "the from pose" and "the to pose".

Expansion copies a definition's machine for each linked instance as it copies ports:
`#plot1/fade` is plot 1's fade channel. Bindings in the template read
`#fade.channel.value`, which expansion maps to the instance's channel, as it maps port
signals today.

## 3. Running a transition

When an instance's state argument changes from A to B (resolved, so a bound state
argument works too: `args.vis="{= #s.slider.value > 0 ? "shown" : "hidden"}"`):

1. Find the transition A → B (or `*` → B, A → `*`). None: the channels take B's pose
   at once.
2. Find its progress driver. A clip animating `#plot1/appear.progress` (stored or
   derived, §4.2) drives it from the clock. Otherwise the machine's own player starts
   it: progress rises linearly from 0 to 1 over the transition's `ms`, on presentation
   time.
3. Each frame, the machine producer publishes every channel's value from progress `p`:
   each tween is sampled at `(p · ms − at) / tween.ms`, clamped and eased; channels with
   no tween ease by the transition's own `ease` over the whole of `p` (M3). At `p = 1`
   channels rest at B's pose.
4. While any transition runs, the views showing that instance render every frame
   (free-running.md's mechanism), then return to on demand.
5. **Interrupted** (the state changes again mid-way): the new transition's from-pose is
   the channels' **current** values, not A's pose (M8).

Two easings stack and are kept apart: a **tween's** easing is the shape of one channel
inside the transition (the component's), and the **progress clip's** easing is a time
warp of the whole transition at one occurrence (the timeline's). Linear progress plays
the transition exactly as designed.

The producer uses the same easing functions as clips (`time.rs::ease`), and the
interpolator of affordances.md §8.2 becomes a client of the same code when it gains
channels beyond placement: **one animation system**, as decided.

## 4. The timeline

### 4.1 What drives progress

| Source of the state change | Progress driven by | Seekable |
|---|---|---|
| A **clip key** on the document's clock | the derived (or edited) progress clip after that key | yes |
| A **narrative move** (narrative-panel.md, not built) | a progress clip on the narrative clock, by the same rule | yes |
| An **event**: the inspector, the agent, a click, a binding on a free input | the machine's own player | no: scrubbing shows the end pose |

This resolves vision 05's "a transition has no start" without a special case: it has
none of its own, and it is seekable exactly when a clock drives its progress.

### 4.2 The progress band: derived by default, overridden by exception

A state port is an ordinary leaf, so a clip may key `args.vis`, with `hold` easing (an
enum steps; Unreal and Unity key enums the same way). The timeline draws that clip as
**blocks of named state**. After each key that changes state, it **derives** a progress
clip for the transition that change selects: `0` at the key's time, `1` at the key's
time plus the transition's `ms`, linear. It is drawn as a sloped band under the state
row:

```
time            0s        1s        2s        3s        4s
vis             [ hidden ·············][ shown ···················]
appear.progress                       ╱‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾
                                    2.0  2.9
```

Scrubbing to 2.45 s gives progress 0.5: half faded, partly drawn. The derived band is
computed, never saved. **Editing it saves it** as an ordinary clip on
`#plot1/appear.progress`, owned by that key (`key.progress` names it), and changes this
one occurrence only:

| Edit | Effect here |
|---|---|
| drag its end from 2.9 s to 4 s | appears slowly, over 2 s |
| easing `ease_in_out` | starts and ends gently |
| a middle key 0.5 at 3 s, held to 3.5 s | pauses half-drawn while narration explains |
| keys 0 → 1 → 0.6 → 1 | overshoots and settles |

"Reset to default" deletes the stored clip and the derived band returns. If the key
moves, an edited band moves with it (its keys are relative to the key). If the key's
state changes so that another transition is selected, the edited band is kept as stale
and listed, as overrides are.

### 4.3 What stays disallowed (M7)

- A clip animating a channel directly. The channel's writer is its machine; key the
  state, edit the progress band, or override a tween.
- A state port both bound and keyed (the existing rule: a leaf is bound or animated).
- Autokey (timeline.md §12) while a machine is mid-transition writes the *state*, never
  a channel, so moving the playhead never freezes a half-drawn curve into the document.

## 5. The narrative and teaching layer

Recommendations, following the recorded decisions in §0:

1. **The plot's `vis` machine is design state** (the plot makes sense without a lesson),
   so it lives in `std:plot`, and by default a lesson **sets** `vis` and inherits the
   animation. A lesson that needs more (a changed part, a new state or transition) makes
   a **lesson variant** of the component and uses that
   ([`variants-and-imports.md`](variants-and-imports.md)); structure never lives in the
   narrative's text.
2. **A narrative move that shows a plot is a held override** of `args.vis = shown` with an
   extent ("from this paragraph on"); leaving the extent backwards restores `hidden` and,
   when played, runs `disappear`. Jumping (clicking a heading far away) snaps.
3. **Per-lesson timing** ("everything at half speed in this section") is a motion-domain
   context rule held by the section's mode; it needs a `transition:` selector beside
   `state:` (narrative note §6.7), which must pass contexts.md §4.1's admission test.
   Proposed selector: `match="transition:appear"`, fields `motion.scale` (a duration
   multiplier) and `motion.ease`. Not in phase 1.
4. **One change, a different motion** is an edited progress band (§4.2) on the narrative
   clock: the same object as on the document timeline, owned by the move. Retiming,
   pausing mid-transition and overshoot are all edits of that band.
5. **Pedagogical components** (hint, quiz, "try it") use the same `machine` markup, but
   their definitions belong to the narrative's compositions and their state is narrative
   state (§0). Nothing in this spec forks for them.
6. **Learner exploration** (narrative note's scratch layer): if a learner flips a plot to
   `hidden` mid-lesson, the scratch layer holds it; on continue, the rejoin is that
   state's transition, played. No new mechanism.
7. **Steps or moves.** ECM §10 and vision 09 still say "a lesson is steps"; the narrative
   note says steps dissolve into moves. This spec needs only "something sets the state at a
   position", which either supplies; it recommends the moves reading, and that ECM §10
   be amended when the narrative clock is specified, not here.

## 6. Editing and inspecting

- **Inspector, instance selected:** the state port shows as a segmented control of the
  machine's states. Changing it is an event (§4.1): the transition plays in the view and
  the change is one undo step. **Undo snaps** (no reverse animation), as Figma and Keynote
  do; replaying a transition on undo reads as the undo doing something else.
- **Entering a component** (editing its main) switches the timeline panel to the
  component's machines. **One panel, two layouts**, switchable, chosen automatically:

```
Strip (each state has at most one way in and one way out: a chain or a loop):
 vis:  ( hidden ) ──appear──▶ ( shown ) ──disappear──▶ ( hidden )      [graph]
 │ fade     ▬▬▬▬                                                        │
 │ reveal       ▬▬▬▬▬▬▬▬▬▬                                              │

Graph (it branches): a resizable pane left of the tracks
 ┌───────────────────┬──────────────────────────────────────────────────┐
 │ (hidden)          │ appear · 900 ms                        [strip]   │
 │   ↓       ↘       │ fade     ▬▬▬▬                                    │
 │ (shown) → (compare)│ reveal   ▬▬▬▬▬▬▬▬▬▬                              │
 └───────────────────┴──────────────────────────────────────────────────┘
```

  Several machines stack one strip each. In either layout, **clicking a state** pins the
  canvas to it (session-only, with a loud "Previewing: hidden" chip; vision 06 §3.3) and
  the panel lists its pose, editable; a state has no duration. **Clicking a transition**
  gives the tracks that transition's duration and one row per tween; the playhead there
  scrubs its progress on every instance in the view, session-only. Leaving the component
  returns the panel to the document's clock. Editing a part while pinned offers "set this
  channel in *hidden*" when the edited leaf is bound to a channel.
- Panel layout and the switch are workspace state (the session tier), not the document.

## 7. For the agent

```xml
<edit><set id="#plot1" args.vis="hidden"/></edit>        <!-- plays disappear in the user's view -->
<edit>
  <create id="$k" type="clip" parent="#timeline" prop="args.vis" start="0"/>
  <create type="key" parent="$k" t="0" value="hidden" easing="hold"/>
  <create type="key" parent="$k" t="2" value="shown" easing="hold"/>
  <link from="$k" to="#plot1" rel="animates"/>
</edit>                                                   <!-- appears at 2 s, scrubbable -->
```

The view prints a machine compactly, one line per state and transition, and an
instance's state port as `vis="shown"` with `transition="appear 0.4"` while one runs, so
the agent can tell it caught an instance mid-way.

## 8. Phases and exit criteria

| Phase | Builds | Exit |
|---|---|---|
| 1. Machines on events | §2–§3: types, expansion of machines and channels, the `state` port kind, the session producer, derived and tweened transitions, interruption | a test definition fades and draws on when `vis` flips in the inspector; flipping back mid-way reverses smoothly; undo snaps; nothing about channels reaches the saved file |
| 2. On the clock | §4 and §6: hold keys on a state port, the derived progress band, edited bands, the strip and graph layouts | scrubbing across a key shows the transition mid-way, forwards and backwards; dragging the band's end retimes one occurrence; reset restores the default; autokey never writes a channel |
| 3. `std:plot` gains `vis` | plot.md §8 | the plot appears (fade, lift, then draw-on) and disappears; works the same from the inspector, a clip key and the agent |
| 4. Narrative | §5 items 2–4, once narrative moves exist | a paragraph shows the plot on reading forward and hides it reading back |

## 9. Open questions (including where this goes past the recorded plans)

1. **Channels** are new. The notes' sketches animate the component's own leaves from the
   sub-timeline. Channels add one indirection so that template leaves can stay bound (a
   curve's `path.d` is an expression; a sub-timeline cannot key it). Acceptable, or should
   tweens also target ordinary constant template leaves directly?
2. **Per entity or per picture?** M2 makes state an argument, so one plot pictured in two
   compositions appears in both at once. That suits a model component. A picture-level
   override (show it in the overview, not in the detail view) would be a `use`-level
   argument override; leave it until someone needs it?
   **User, 2026-11-08:** per entity is fine for now, but views will need their own
   machines: a phase modulator's diagram animating through states in one view while its
   schematic stays constant in another. The likely form is that `use`-level override of
   the state argument (or of the channels), so one entity has a state per picture.
   Revisit after step 8.
3. **Triggers inside the component** (`on="press"`, `on="hover"`, `on="mount"` from
   vision 05 §4 and vision 06 §2): phase 1 changes state only from outside. A click that
   toggles a component's own state is `affords.activate="cycle"` driving the state port
   (affordances.md §5.3), which may already be enough; `on="mount"` (appear when first
   shown) waits on what "shown" means in a narrative or presentation.
4. **Machines without components.** Can a plain group in a composition have a machine,
   or only definitions? Proposed: definitions only for now; make-component is one step.
5. **Seekable events.** Should an agent's edit that sets a state be recorded with a
   time so a later scrub replays it? Proposed: no; that is what keying the state is for.
6. **Progress ids.** `#plot1/appear.progress` names a transition's progress on an
   instance. Is a transition a node with a `progress` leaf (simplest for clips), or is
   progress a port on the entity (`#plot1.appear`)? Proposed: a leaf on the expanded
   transition node, so clips animate it like any leaf.

7. **Tracks as timing values (direction agreed 2026-11-08).** Tweens and
   `ease(window(progress))` bindings are two spellings of one thing and should
   coalesce. Not by pattern-matching formulas in the timeline: a **timing** becomes a
   value kind, as a colour or path data is, built by `ease(curve)`, `window(a, b, t)`,
   `then(…)`, `keys(…)` and `custom(p => …)`. A transition holds one track per channel
   whose value is a timing; the player computes `from + (to − from) · timing(progress)`.
   The timeline edits a **constant** timing's structure directly (segments, handles);
   a **bound** timing (its window depends on an input) shows its formula, like every
   bound leaf in the inspector. The same rule as everywhere: constant typed values get
   an editor, bindings get a formula. Lambdas and timings are one-way, as tweens were:
   the player writes channels, templates read them, nothing flows back, so no inverse
   is needed. Built with step 8.

8. **Authoring (council 2026-11-08, both members).** §6 covers editing but not
   *creating* a machine; today only markup can. Add to step 8: on a main, **Add state**
   records the current pose (Figma variants, Keynote Magic Move); edits made while a
   state is pinned make a channel for each changed leaf and bind it; transitions are
   derived and their band edited in the strip panel; a loud chip says which state is
   being previewed. Exit: a person makes a two-state draw-on by hand, scrubs it
   backwards, retimes one occurrence, and undoes.
9. **Tweens in progress fractions, not ms** (council): M5 writes tweens against
   `progress`, the code writes `at`/`ms`. With timing values (Q7) a tween is a window of
   progress, so step 8's band is the same object as a clip.
10. **Channels as out ports?** (council, Opus) A channel is a fourth kind of component
    variable beside in, out and port. As an out port driven by the machine, pins, wires
    and the inspector would show it with nothing new. Decide in step 8.
11. **The plot's `reveal`** duplicates `vis` (council, Opus): two ways to draw the
    series on. Proposed: drop `reveal` once the timeline keys state (step 8).

12. **Customising timing from inside and outside (agreed 2026-11-08, step 6).** Four
    layers, like arguments, overrides and clips: (a) the author's default, in the
    definition; (b) timing the author exposes as an input (`ms="{= $self.appear_time}"`:
    the player evaluates transition and tween timing per instance instead of reading
    constants); (c) a per-instance override of any transition, without the author
    exposing it (the inspector lists an instance's transitions with Override and Reset);
    (d) a per-occurrence retime on the timeline (§4.2's band). Poses (each state's
    channel values) take (b) and (c) the same way. None of (b)–(d) is built yet.

## 10. Done

**2026-11-08: phase 1 (machines on events) and phase 3's plot.**

- Node types (registry.rs): `channel` (a definition child, `channel.value` its rest
  value, copied onto each instance like a port: expand.rs `is_body`), `machine` (named
  for the `in` port whose value is the state), `state` (its pose as `args`, one per
  channel), `transition` (`from`, `to` with `*`, `ms`, `ease`) and `tween` (`channel`,
  `at`, `ms`, `ease`). §2's `<machine>`/`<channel>` markup is these nodes; the state
  port is an ordinary `value` input whose default is the initial state.
- The player (`machine.rs`, `Store::advance_machines(now)`, a PreUpdate system beside
  the motion player that keeps picturing compositions rendering while it runs): a
  changed state argument starts the transition (exact match, then `*`), progress runs
  0 → 1 over `ms` on the host clock, and each channel is `from + (to − from) ·
  ease(curve, window(progress, at/ms, (at+ms)/ms))` (wires-and-formulas.md §5). An
  interruption starts from the current values (M8); a change with no transition, an
  instance first seen, and undo or redo snap. Values publish as
  `ProducerId::MACHINES` only where they differ from rest, so resting instances stay
  static to the renderer and nothing is saved.
- An enum input takes text (`args.vis="hidden"` from an edit; `definition::check_kind`).
- `std:plot` gains `vis` (`shown`/`hidden`) and channels `frame`, `axes`, `ticks`,
  `series`: appear (1.4 s) fades the frame and titles in, draws the axes, then ticks,
  grid and labels, then the curve, with Easy Ease; disappear (0.45 s) reverses it. The
  series' `trim.end` is `min(reveal, series)`.
- `tests/machines.rs` (transition, tween slice, interruption, undo snap, nothing
  saved, no transition snaps) and `tests/plot.rs` (the plot's order). Seen in the app
  through `<edit>`.
- Not yet: the `progress` leaf a clip can drive (phase 2, with keys on the timeline,
  the derived band and the strip/graph panel), the inspector's segmented state
  control, a pinned preview state on a main, a machine on a main's own picture (mains
  show channels' rest values). Tweens stay their own nodes; a tween is the binding
  `ease(window(progress))` in meaning, and phase 2's progress leaf lets either form be
  written. In the area style the fill fades in whole while the line draws; clipping
  the fill to the drawn length would read better.


**2026-11-08: step 6, machines on the timeline and made by hand.**

- **Per instance (Q12 b, c).** Each instance gets copies of its component's machines
  (expand.rs, after its ports), so a transition's `ms` may read an input (each
  instance its own length) and an instance may override any transition, tween or pose
  through the existing override mechanism. The player reads the resolved copies.
- **Timing (Q7, Q9).** Tweens are fractions of their transition (`start`, `end`), so
  retiming keeps proportions, and `shape` takes any function of progress
  (`{= p => …}`). One easing default, Easy Ease. Not built as a separate value kind:
  the tween's fields are the timing's structure, and `shape` is its formula form.
- **On the clock (§4, phase 2).** A clip keying a state (hold keys) drives the
  transitions it selects from the clock, from the previous state's pose, so any
  playhead time shows the same picture, forwards or backwards. `key.span` retimes one
  occurrence (auto: the transition's length). The timeline draws a progress band from
  each such key. The inspector keys a choice input with ◇, and a change to an
  animated input keys the playhead (After Effects).
- **By hand (Q8).** On a main, **+ State** makes the machine (a `state` input typed as
  the choice of its states, `default` and `state_2`, one any-to-any transition of
  300 ms Easy Ease) and pins the new state; clicking a state pins it (session only,
  the main shows its pose, a banner says so, a field renames it); number edits made
  while pinned become its pose (`machine::record`: a new channel resting at the old
  value, the part bound to it, the state's value; one undo step). Transitions show
  their length and an **easing field** (curve previews, a cubic-bezier editor).
- Exit met in the app: a line made a component; + State; Trim End 0 recorded into
  `hidden`; an instance's state keyed hidden at 0 s and default at 1 s by hand;
  scrubbing 0.9 / 1.15 / 1.6 s shows nothing, half and all; a 2 s span retimes that
  occurrence; undo restores it. `tests/machines.rs`, `tests/machine_authoring.rs`.
- Not yet: dragging the band's end (the drag's handle is ambiguous between the key's
  time and its span; set Span in the key's inspector instead); the strip/graph
  panel; a "Previewing" chip on the canvas (the inspector shows it); colour channels
  (channels are numbers); double-click does not enter a main whose only part is an
  unfilled path (Enter does); per-view machines (Q2); per-lesson timing (§5.3).
